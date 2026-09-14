# 代码冗余审计报告

**日期**：2026-09-10　**范围**：`src/`（TS/TSX）+ `src-tauri/src/`（Rust）+ `src-tauri/tests/`
**规模**：Rust 约 47k 行 / 69 文件；前端约 20k 行 / 90 文件
**方法**：跨文件滑动窗口重复块检测（10 行窗口 + 归并极大区间）＋ 函数名/函数体指纹比对 ＋ 导出引用计数 ＋ cargo 依赖使用计数

## 总体判断

冗余**真实存在但未失控**，不存在重复组件或环形依赖。问题集中在两类：

| 类别 | 表现 | 影响 |
|---|---|---|
| **实现分叉** | 同一个能力被复制成 2 份，副本后续被独立修改 | 功能/安全不一致（最危险） |
| **样板复制** | 工具函数、网关前置检查、表格渲染逐字复制 | 维护成本、改一处漏一处 |

最高价值的三件事：① 收敛 `restrict_file_to_owner` 与同步文件清单两份分叉实现；② 把 `expand_tilde` / `resolve_host` / `write_all_*` / `glob_match` 提到公共模块；③ 清理死代码与死导出。

---

## P0 — 实现分叉，已产生行为不一致

| # | 位置 | 问题 |
|---|---|---|
| P0#1 | `src-tauri/src/copy_redact.rs:161` vs `src-tauri/src/store.rs:172` | 同名 `restrict_file_to_owner` 两份实现。store.rs 版含 **Windows `icacls` ACL 收紧**；copy_redact.rs 版是 `#[cfg(unix)]` only，Windows 下 `let _ = path;` **静默无操作**。`save_copy_redact_rules`（copy_redact.rs:121）调的是弱化版。应删除副本、改调 `store::restrict_file_to_owner`。 |
| P0#2 | `src-tauri/src/webdav_sync.rs:27` `SYNCABLE_FILES` vs `src-tauri/src/tauri_commands.rs:2096` `WEBDAV_SYNC_FILES` | 两份同步文件清单已**实质分叉**：前者含 `secrets.enc` / `approval_policies.toml` / `snippets.json` 但缺 `webhook.toml` / `app_preferences.json`；后者反之。同一份云同步走两条路径时结果不同 → 文件静默丢失。应收敛为单一常量。 |

---

## P1 — 明显重复，可安全收敛

### 工具函数（逐字或近逐字相同）

| # | 位置 | 重复情况 |
|---|---|---|
| P1#1 | `core.rs:1642` / `embedded_ssh.rs:132` / `keys.rs:352` / `ssh_config.rs:237` | `expand_tilde()` **4 份**逐字相同（~14 行/份）。同时 `anyhow!("cannot locate home directory")` 出现 4 次 → 缺一个 `home_dir() -> Result<PathBuf>` 公共助手。 |
| P1#2 | `connection.rs:100` / `core.rs:1627` / `embedded_ssh.rs:269`(`resolve_host_profile`) / `forward.rs:307` / `session.rs:57` | 主机名 → `HostProfile` 查询 **5 份**，逻辑完全一致（`load_config().hosts.into_iter().find(...)`），错误文案同为 `unknown host profile`。 |
| P1#3 | `embedded_ssh.rs:977,991` vs `forward.rs:749,763` | `write_all_tcp()` 与 `write_all_channel()` 各 **2 份**逐字相同（~40 行）。二分文件各持一份 TCP↔SSH channel 泵送工具。 |
| P1#4 | `approval.rs:747` / `ssh_config.rs:137` / `store.rs:1276` | `glob_match()` **3 套独立实现**（递归 char 版 / byte 版 / store 版）。语义应一致，实际是三份代码。 |
| P1#5 | `tauri_commands.rs:100` vs `bin/agent2ssh-daemon.rs:317` | `append_operation_audit()` **2 份**逐字相同（~22 行）。 |
| P1#6 | `bin/agent2ssh.rs` / `bin/agent2ssh_mcp/auth.rs` / `tauri_commands.rs` | `authorize_command_with_approval(...)` 调用样板 **8 处**：`CommandAuthorizationInput` 字段逐个填充 + "approval required but no … handler is available" 拒绝闭包（含 `append_rejected_exec_audit`）。唯一差异是默认 source 字符串（`cli`/`mcp`/空）。 |
| P1#7 | `bin/agent2ssh.rs:977` vs `bin/agent2ssh_mcp/auth.rs:10` | `authorize_local_exec_request()` 与 `authorize_local_mcp_exec_request()` 近逐字相同，差异仅默认 source 与错误类型包装。 |

### 前端

| # | 位置 | 重复情况 |
|---|---|---|
| P1#8 | `AuditPanel.tsx:352-378` vs `HostList.tsx:513-539` | 表格 `thead`（排序按钮 + `flexRender`）**23 行逐字相同**；选择列 checkbox 定义（`AuditPanel.tsx:112-128` vs `HostList.tsx:173-189`）**17 行相同**。`components/ui/data-table.tsx` 只抽出了 `SortIcon` + `ColumnVisibilityMenu`，未抽 `<DataTable>` 骨架。 |
| P1#9 | `ApprovalDetail.tsx:28` / `ApprovalQueue.tsx:28` / `ConfigSnapshotsPanel.tsx:15` / `LiveActivityPanel.tsx:97` / `SyncPanel.tsx:31` | `formatTime()` **5 份**（`toLocaleString` / `toLocaleTimeString` 混杂），另有 7 处内联日期格式化。 |
| P1#10 | `ForwardPanel.tsx:267` / `RecordingsPanel.tsx:13` / `SyncPanel.tsx:38` | `formatBytes()` **3 份**且**输出单位已分叉**：前两者输出 `KiB/MiB`，SyncPanel 输出 `KB/MB`。 |
| P1#11 | 8 个面板 | `ui/state.tsx` 的 `ErrorState`/`LoadingState` 被绕过：`text-destructive` 手写错误块散落 14+ 文件，`animate-spin` 手写 spinner **12 处**。 |
| P1#12 | `KeysPanel.tsx:79` / `McpAgentsPanel.tsx:85,118` / `PlaybooksPanel.tsx:284` / `RecordingsPanel.tsx:169` / `SnippetsDialog.tsx:138` | `window.confirm` **6 处**，无共享确认弹窗；且 `ui/dialog.tsx` 已存在。 |

### 二进制层样板

| # | 位置 | 重复情况 |
|---|---|---|
| P1#13 | `bin/agent2ssh-daemon.rs` | 每个 HTTP handler 重复同一条前置链：`REQUEST_COUNT.fetch_add`（68 次）→ `source_from_transport`（13）→ `reject_if_gate_paused`（26）→ `reject_if_rate_limited`（25）→ `authorize_command`（27）→ `append_operation_audit`（31）。可用 axum `middleware`/`FromRequest` 收敛掉大部分。 |

---

## P2 — 死代码与清理项

### 完全无引用的 Rust 函数

> ⚠️ 下表是 2026-09-11 首次审计时的**原始快照**，行号与「死分支」判断都是当时的状态。
> 其中已接线的项不再无引用，已删除的项在源码里已不存在。**不要**把它当作当前代码状态的清单；
> 处置结果见下方「解决状态」表与文末的复查小节。

| 位置 | 说明 |
|---|---|
| `core.rs:2293/2308/2339/2347/2372/2380/2406/2419` | **8 个 `sftp_*_core` 函数**（`sftp_rename_core(+_with_source)`、`sftp_remove_file/dir/dir_all_core(+_with_source)`），共约 180 行。既未注册为 tauri command，前端也无调用 —— 整条 SFTP rename/remove 能力是死分支。 |
| `app_state.rs:313/335/340` | `emit_bus` / `is_headless` / `is_cli` |
| `embedded_ssh.rs:521` | `remove_system_known_host()`（约 70 行，注释标 Finding 18，无调用点） |
| `policy.rs:112` | `policy_risk_rules()` |
| `ssh_algo.rs:138` | `save_algo_prefs()` —— 与 `load_algo_prefs()` **不对称**：算法偏好只被读取、从不写入。 |

### 死导出（TS）

| 位置 | 说明 |
|---|---|
| `src/api.ts:76` | `getSessionTraceId()` 声明后无任何引用 |
| `src/api.ts:122` | `setDaemonUrl()` 声明后无任何引用（`getDaemonUrl` 有引用） |
| `src/components/ui/card.tsx:36,50` | `CardDescription` / `CardFooter` 全仓库无使用 |
| `src/components/ui/badge.tsx:29`、`button.tsx:47` | `badgeVariants` / `buttonVariants` 的 `export` 无外部消费者（仅内部使用） |
| `src/i18n.tsx` | 约 **53 条** zh 词条在 `src/` 中找不到任何字面引用。需人工确认（部分可能由动态 key 拼装，勿直接删）。 |

### 其他轻微冗余

| 位置 | 说明 |
|---|---|
| 8 个组件文件 | `const labelCls = ...` 逐字复制（AddHostForm / AuditPanel / ExecPanel / ForwardPanel / HostSelector / MultiExecPanel / ProxyPanel / SFTPPanel），共 47 处引用同一常量 |
| Rust 全仓 | `"AGENT2SSH_CONFIG_DIR"` 作为**裸字面量出现 71 次**，应为 `pub const`（✅ 2026-09-11 已处理：新增 `store::CONFIG_DIR_ENV`，98 处调用点收敛，详见下） |
| `src-tauri/Cargo.toml` | `rand = "0.8"` 与 `getrandom = "0.4"` 双 RNG 入口（各 3 处使用）；`rand` 内部本就走 `getrandom`，可统一 |
| `store.rs:2155` / `playbook.rs:853` | 测试中重复构造 `AuditEntry`（两处注释均自认 "Mirror append_audit"）→ 测试 fixture 未复用 |
| `core.rs:397` `normalize_proxy()` vs `store.rs:567-580` | 代理字段 trim/过滤逻辑两处重复 |
| `tauri_commands.rs` | `resolve_host` 内联重复一份（未走 session.rs 的公共版） |

---

## 未发现问题的部分

- **无 0 引用组件**：40 个组件全部被引用（`FilePreview.tsx` 经 `SFTPPanel.tsx:33` 的 `lazy(() => import(...))` 动态加载，非孤儿；Monaco 依赖链因此是活的，非死重）。
- `src/lib/terminal/` 各模块（`folds` / `highlight` / `osc52` / `prompt` / `paint-scheduler` / `asciicast` / `block-*`）均有测试与调用方，无重复实现。
- `RawTerminal` / `terminal_size` 在 `bin/agent2ssh.rs` 出现两次属 `#[cfg(unix)]` / `#[cfg(windows)]` 门控，**不是冗余**。
- 前端无绕过 `api.ts` 直接 `invoke()` 的调用（0 处）。

---

## 建议处理顺序

1. **P0#1 / P0#2** —— 涉及安全收紧失效与同步丢文件，先确认再改。
2. **P1#1~#7** —— 每项都是「删副本 + 改引用」，单次改动小、收益明确，建议合并为一个 commit。
3. **P2 死代码** —— `core.rs` 的 8 个 `sftp_*_core` 与 `remove_system_known_host` 删前需确认是否为计划中的未接线功能。
4. **P1#13 daemon 中间件** —— 收益最大但改动面最广，建议单独排期。

---

# 修复后状态（2026-09-10 收尾）

> 上表 P0 / P1 / P2 项已按「建议处理顺序」执行完毕（改动**未提交**）。
> 规模：**51 个已跟踪文件 +860 / −1077**，另新增 3 个文件（`docs/reports/redundancy-audit.md`、`src/lib/format.ts`、`src/lib/ui-classes.ts`，共 146 行）。
> 分层：`src-tauri/` 19 文件 +522 / −738；`src/` 32 文件 +338 / −339。

## 已完成的收敛

| 条目 | 处置 | 关键落地 |
|---|---|---|
| P0#1 `restrict_file_to_owner` 分叉 | ✅ 删弱化副本 | `copy_redact.rs` 改调 `store::restrict_file_to_owner`，Windows `icacls` ACL 收紧恢复生效 |
| P0#2 同步清单分叉 | ✅ 收敛为单一常量 | 删 `tauri_commands.rs::WEBDAV_SYNC_FILES`，统一走 `webdav_sync::SYNCABLE_FILES`；`validate_remote_file` 改严格白名单 |
| P1#1 `expand_tilde` ×4 | ✅ →1 | 新建 `path_resolver::expand_tilde` |
| P1#2 `resolve_host` ×5 | ✅ →1 | 统一到 `session::resolve_host`（含 `tauri_commands.rs` 的内联副本） |
| P1#3 `write_all_tcp` / `write_all_channel` ×2 | ✅ →1 | `embedded_ssh` 版本提升为 `pub(crate)`，`forward.rs` 改引 |
| P1#4 `glob_match` ×3 | ✅ →1 | `approval.rs` 改调 `store::glob_match` |
| P1#5 `append_operation_audit` ×2 | ✅ →1 | `tauri_commands.rs` 与 `bin/agent2ssh-daemon.rs` 均改调 `store::append_operation_audit` |
| P1#6/#7 授权样板 12 处 | ✅ →1 个助手 | 新增 `execution_control::authorize_command_without_approval_handler`，消费方 `bin/agent2ssh.rs` / `bin/agent2ssh_mcp/auth.rs` / `tauri_commands.rs` 各 4 处；净 **−159 行** |
| P1#8 表格骨架 / 选择列 | ✅ 抽取 | `ui/data-table.tsx` 新增 `selectColumn<T>` 与 `<DataTable<T>>` |
| P1#9 时间格式化 ×5 + 7 处内联 | ✅ →1 | 新建 `src/lib/format.ts`，12 个组件改为引用 |
| P1#10 `formatBytes` ×3（单位已分叉） | ✅ →1，**统一 `KiB/MiB`** | ⚠️ SyncPanel 显示标签由 `KB/MB` 变为 `KiB/MiB`（用户可见，见下） |
| P1#11 三态组件被绕过 | ⏸ 保留 | 逐处核对后确认均为解构 / 内联用法，**无安全的 panel 级替换点**，强改会引入行为差异 |
| P1#12 `window.confirm` ×6 | ✅ 统一 | `ui/dialog.tsx` 新增 `ConfirmDialog` / `confirmDialog()` / `ConfirmHost`，`App.tsx` 挂载 |
| P1#13 daemon 前置链样板 | ✅ 收敛 9 个 handler | 新增 `preflight_guarded_request`（纯 gate 链：gate_paused → rate_limit → authorize） |
| P2 Rust 死代码 | ✅ 接线 + 清理（2026-09-11 / 09-12） | 8 个 `sftp_*_core`、`remove_system_known_host`、`save_algo_prefs` 已全部接线；`emit_bus` / `is_headless` / `is_cli` / `policy_risk_rules` 定性为冗余后**已删除**（见文末「第二类」）。删除过程中连带发现并修复了一个真缺陷：桌面端从不 `set_host`，`Host` 恒停在 `Host::Cli` |
| P2 TS 死导出 | ✅ 删除 | `getSessionTraceId` / `setDaemonUrl` / `CardDescription` / `CardFooter` / `badgeVariants` / `buttonVariants` 的 `export` |
| P2 i18n 死词条 | ✅ 删除 **49 条** | 逐条核验 `src/` 零引用（报告原估 ~53，实际确认 49，未误删） |
| P2 `labelCls` 复制 ×8 | ✅ →1 | 新建 `src/lib/ui-classes.ts`，1 定义 / 8 引用 |

## P1#13 的收敛边界（有意保留）

助手只覆盖「单主机 + 源解析不 fallible + gate 与 authorize 之间无额外校验」的链。以下 **有意保留内联**，非遗漏：

- **多主机目标**：`exec_multi`、`exec_compare`（`targets_for_exec_multi`）。
- **夹了 `check_daemon_scope`**：`forward_remove` / `forward_stop` / `forward_start` / `connect` / `disconnect`。
- **多步 / 非单次授权**：`session_open`（含 `register_limited_session_for_command` + 失败回滚）、`session_write`（逐条 `completed_commands` 循环）、`run_playbook`（按 step 循环 + `playbook_risk_override`）。
- **错误不走 `?`**：`ws`（terminal over websocket，错误需写回 socket 后 `return`，套用助手会改变错误语义）。

## 未处理（有意搁置）

| 条目 | 原因 |
|---|---|
| `"AGENT2SSH_CONFIG_DIR"` 裸字面量 71 处 | ✅ 2026-09-11 已处理（见文末）；其余项不在「建议处理顺序」内 |
| `rand` + `getrandom` 双 RNG 入口 | 需评估版本兼容与行为差异，收益低 |
| `normalize_proxy` / 测试 `AuditEntry` fixture 重复 | 低收益 |
| P1#11 三态组件 | 见上，无安全替换点 |

## 需知晓的行为差异（有意收敛，非回归）

1. **`formatBytes` 单位**：SyncPanel 由 `KB/MB` → `KiB/MiB`，与另两处对齐。1024 进制下 `KiB` 更准确，但属**用户可见文案变化**。
2. **`secrets.enc` / `snippets.json` / `approval_policies.toml` 不再跨机同步**：P0#2 收敛后同步集合以 `SYNCABLE_FILES` 为唯一来源，这三个不再入集。**属产品面缺口**：依赖云同步在多机间共享密钥的用户需手动迁移，需单独决策是否补回。**（2026-09-11 补记）** 收敛时把 `validate_remote_file` 改成了严格白名单，顺带删掉了 `known_hosts.json` 的遗留容忍逻辑，导致旧 marker 列出这些文件时 `pull` 直接硬失败——已恢复「容忍并跳过」语义（`applied_sync_files` 过滤），同步集合成员关系不变；见 `CHANGELOG.md` 的 `[Unreleased]` 段。
3. **daemon `requests_total` 口径**：计数器最终实现放在每个 handler **入口首行**（在 source 解析之前），与改前 68 处调用点的语义完全一致——非法 / 无 source 请求仍计入。
4. **`test-setup.ts` 新增 localStorage shim**：Node 26 的实验性 `localStorage` 全局会遮蔽 jsdom 的实现，导致 `i18n.tsx` 抛 `localStorage is not defined`。已用 `git show HEAD:src/i18n.tsx` 证实用法早于本次改动存在。**建议此文件独立成一个 commit**，与去冗余改动分开。

## 回归验证（收尾实测）

| 命令 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo check --no-default-features --features daemon --bin agent2ssh-daemon` | 0 warning / 0 error |
| `cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp` | 通过 |
| `cargo check`（default / tauri） | 通过 |
| `cargo test --no-default-features --lib` | **579 passed / 0 failed** |
| `cargo test --no-default-features --features daemon --test daemon_integration` | **57 passed / 0 failed** |
| `cargo test --no-default-features --test cli_smoke` | **33 passed / 0 failed** |
| `npm test` | **73 passed / 0 failed**（12 文件） |
| `npm run build` | 通过 |

> 收尾时发现并修复了两处由本次重构引入的未用导入 warning：`forward.rs` 的 `Write`（原被已删的本地 `write_all_*` 使用）与 `connection.rs` 的 `HostProfile`（原被已删的本地 `resolve_host` 使用）。修后全 feature 矩阵 0 warning。

---

# 后续补处理（2026-09-11）

## `"AGENT2SSH_CONFIG_DIR"` 裸字面量收敛

上表「未处理（有意搁置）」中的最后一项。新增 `store::CONFIG_DIR_ENV`（声明紧邻读取它的 `store::config_dir()`），替换 **98 处**调用点，涉及 15 个文件：

| 分层 | 文件 | 处数 | 导入位置 |
|---|---|---|---|
| 库 | `webdav_sync.rs` | 20 | `mod tests` 内 `use crate::store::CONFIG_DIR_ENV;` |
| 库 | `store.rs` | 17 | 同模块，无需导入（含 `config_dir()` 生产路径 1 处） |
| 库 | `core.rs` | 14 | 同上 |
| 库 | `embedded_ssh.rs` / `diagnostics.rs` | 6 / 6 | 同上 |
| 库 | `keys.rs` | 4 | 同上 |
| 库 | `telemetry.rs` / `policy.rs` / `mcp_binding.rs` / `anomaly.rs` | 各 2 | 同上 |
| 二进制 | `bin/agent2ssh-daemon.rs` | 2 | 无需导入（`use agent2ssh::store::*;` 已在作用域） |
| 集成测试 | `tests/cli_smoke.rs` | 16 | 文件顶 `use agent2ssh::store::CONFIG_DIR_ENV;` |
| 集成测试 | `tests/exec_fixture.rs` / `connect_deadline.rs` | 各 2 | 同上 |
| 集成测试 | `tests/cli_contract.rs` | 1 | 同上 |

**关键取舍**：全部 98 处都在测试代码里（`src/` 侧仅 `config_dir()` 自身 1 处在生产路径）。因此 `src/` 各文件的导入必须放进 `mod tests` 内 —— 放到文件顶会让非测试构建报 `unused_imports`，而 clippy 门是 `-D warnings`。集成测试是独立 crate，导入放文件顶（整体编译单元即测试）不会有该问题。

**未纳入**：`scripts/e2-scale-plan-smoke.py` 与文档中的同名字面量（非 Rust，不在本项范围）。

**验证**：`cargo fmt` 后 clippy 四目标（lib 非默认 / lib 默认 tauri / CLI+MCP / daemon）与 `cargo check --tests`（两种特性）全部 exit=0、零 warning；`592`（非默认）/ `600`（默认）lib、`cli_smoke` 33、`cli_contract` 27、`connect_deadline` 2、`daemon_integration` 57 全绿，测试计数与改动前一致。

---

# P2 死代码定性（2026-09-11）

上一轮只做了「保留 + `TODO(unwired)` 标注」，没有区分「设计过但没实现」与「本来就多余」。逐项定性后分三类：

## 第一类：设计了、实现了一半 → 已接线

| 项 | 差距 | 落地 |
|---|---|---|
| `sftp_rename_core` / `sftp_remove_file_core` / `sftp_remove_dir_core` / `sftp_remove_dir_all_core` | `core.rs` 里实现完整（`remove_dir_all` 是 BFS 递归删除）且带单测；`store.rs` 的分类器**已为 `sftp_rename` / `sftp_remove` 预留分支**，`types.rs` 的文档也列了这两个 action label。但没有 CLI 子命令、没有 MCP 工具、没有 daemon 路由、没有 Tauri 命令、前端无 UI —— 五个入口全部不可达 | CLI `sftp rename\|rm\|rmdir\|rm-rf`；MCP `ssh_sftp_rename` / `_rm` / `_rmdir` / `_rm_rf`（工具数 54 → 58）；daemon `POST /sftp/rename\|rm\|rmdir\|rm-rf`；桌面命令 ×4 + SFTP 面板远程行右键菜单 |
| `remove_system_known_host` | 是 G13 `import_known_hosts_from_ssh` 的对偶（`ssh-keygen -R`）。⚠️ **本轮初稿在此写错过**：当时写「CLI 与 daemon 已可达」，事后 `git grep` 实测确认**全仓零调用点**——CLI 侧只有 `known-hosts import`，daemon/MCP 两侧从来没有 known_hosts 面 | 桌面新增 `forget_system_known_host` 命令 + Host Management 每行「从 OpenSSH known_hosts 移除」；又补 CLI `agent2ssh known-hosts forget <host> [--port] [--json]`，使与导入侧的三个面（Tauri 命令 / CLI / 前端 api）一一对应 |
| `save_algo_prefs` | 读侧是真跑的（`embedded_ssh.rs:266` / `:1416` 每次握手前 `load_algo_prefs`），但没有任何代码写 `ssh_algos.json`，该文件名在所有文档里都**未出现过** —— 等于一个只能手改、且未文档化的隐形配置 | Settings 新增「SSH algorithms」分区 + 编辑对话框（8 个字段、与内置默认值差异标记、一键恢复）；保存前校验 |

> **接线前的安全前提**：SFTP 这四条在任何 surface 都不执行 shell，只构造 `sftp <op> <path>` 字符串用于风险分类 / 审批策略 / 审计 action。`classify_risk_single` 只认 shell 动词，canonical head 恒为 `sftp`，于是 `rm` 规则一条都匹配不到、全部落到默认 verdict —— **实测 `sftp rm-rf /` = low**。而审批是 opt-in（无策略命中即自动批准），所以直接接线会让「递归删远端 `/`」被静默放行。已先在 `classify_risk_single` 加 `sftp` 分支镜像 shell 表（`rm-rf /`、`/*`、`/.` → blocked；其它 `rm-rf` → high；`rename` → medium；`rm`/`rmdir` 保持 low），并补 2 个单测（含一个断言路径里夹 `;` 的链式载荷仍被链式检测抓住）。

## 第二类：不是需求缺失，是冗余 → 已删除（2026-09-12）

| 项 | 定性依据 | 处置 |
|---|---|---|
| `emit_bus` | 只是 `events::publish_event` 的**转发壳**，而 `publish_event` 有 15+ 个直接调用点（`anomaly.rs`、`approval.rs`、daemon 各处）。绕经 `AppState` 再转发不产生任何行为差异，只多一层 | **已删** |
| `is_headless` / `is_cli` | 与 `transport_name()` 重复的谓词（`matches!(self, Variant)` vs `transport_name()` 的比较），零调用。`is_desktop()` 有真实调用点（`diagnostics.rs:545`）故保留 | **已删** |
| `policy_risk_rules` | 不是「没接线的功能」，而是 `risk_config::load_risk_rules()` 的**劣化副本**：它同步只读 `policy.toml` 的 `risk` 字段，**忽略 `risk_rules.toml` 回退**、也没有 mtime 缓存；而 `load_risk_rules()` 两者都做。所以「把它接上线」反而会**退步**——读取会漏掉独立的 `risk_rules.toml` | **已删**（接线才是错的） |

`Host::emit` **保留**：它有单测覆盖，且是 `Host` 枚举文档里写明的 `app.emit()` 迁移目标（桌面端补上 `set_host` 后，它的 `Host::Tauri` 分支已可达）。它不是同形重复入口，只是尚未被生产代码采用。

### 删除时暴露的根因：桌面端从不声明自己是 desktop

`TODO(unwired)` 把 `is_headless` / `is_cli` 归类为「冗余谓词」，但底下真正的问题更严重：**这对谓词（以及 `transport_name()`）赖以判断的 `Host` 值，在桌面端永远是默认值 `Host::Cli`。**

- `tauri_commands.rs` 的 `run_tauri` 原注释写着「`is_desktop()` returns true under `#[cfg(feature = "tauri")]`」，据此认为不必调用 `set_host`。
- 但实现是 `#[cfg(feature = "tauri")] pub fn is_desktop(&self) -> bool { matches!(self, Host::Tauri(..)) }` —— **判的是值，不是 feature**。
- 而 `set_host` 全仓只有一个调用点：daemon 启动时设 `Host::Headless`。桌面端从不构造 `Host::Tauri`。
- 后果：桌面应用的诊断报告里 `"transport": "cli"`、`"is_desktop": false`。

已在 `run_tauri` 的 `.setup()` 钩子里补 `set_host(Host::Tauri(app.handle().clone()))`，并把那段错误注释改为描述真实机制；`app_state.rs` 补了两个单测锁住「按值判断」的语义。详见 `CHANGELOG.md` 的 `Fixed` 条目。

## 第三类：文档承诺实际已实现

原始审计怀疑 `policy.toml` 的 `risk` 字段没人读。核实结论：`risk_config::load_risk_rules()` 在统一 policy 文件存在时正是 `load_policy_from_path(&path)?.risk`，所以 `docs/architecture.md` 承诺的「policy.toml 优先」成立——只是入口是 `load_risk_rules()`（async + 带缓存 + 含 `risk_rules.toml` 回退），而不是那个同形但劣化的 `policy_risk_rules()`。后者已删（见上）。

## 本轮的验证

`cargo fmt --check` clean；clippy 四目标（CLI+MCP / daemon / lib 非默认 / lib 默认 tauri）各 0 warning；`cargo test --lib` 607、`--no-default-features --lib` 599、`cli_contract` 29（含本轮新增 2 个 `known-hosts` 契约用例）、`cli_smoke` 33、`daemon_integration` 57、`connect_deadline` 2 全绿；`npm test` 79、`npm run lint` 96 文件 clean、`npm run build` 通过、`check-i18n.mjs` 无缺失键。

**顺带修掉的三个既有缺陷**（见 `CHANGELOG.md`）：① 上面那个 `sftp` 风险洞；② 工具数 54 → 58 后 2 处测试断言 + 8 个文档文件 14 处计数未同步；③ `test_timed_out_approval_cannot_be_approved` 的墙钟竞态（TTL 判定用 `Utc::now()` 而 `tokio::time::sleep` 走单调钟，单发 sleep 在 6 次整轮运行里挂了 1 次）。

**同时发现并已规避的工程陷阱**：并行发出的多个 `Edit` 调用若打在同一文件上会互相覆盖（各读原文件、后写者胜），本轮因此丢过 2 处写入（`api.ts` 的类型导入、`AlgoPrefsDialog` 的一处 `t()` 字面量），均已补回。同文件多改必须串行。

## 复查（2026-09-12）：接线是否真的完成

用户追问「该实现的实现了吗？」，于是重做一次**功能级**核查（不是只看 diff，因为「不可达」正是 diff 看不出的失败模式）。结果分三类：

### 已确认接通（用编译出来的二进制实测）

| 项 | 证据 |
|---|---|
| SFTP 四操作 | `agent2ssh sftp --help` 列出 `rename`/`rm`/`rmdir`/`rm-rf`；`risk` 实测 `sftp rm-rf /` → blocked、`sftp rm-rf /home` → high、`sftp rename` → medium、`sftp rm`/`sftp rmdir` → low，与 `rm -rf /` / `rm -rf /home` / `mv` 逐条一致 |
| MCP 四工具 | `mcp_stdio_end_to_end_initialize_tools_and_risk` 走真实 stdio 握手并断言 `tools/list` = 58 |
| 算法偏好 | `get_algo_prefs` 读侧本就在 `embedded_ssh.rs:266`/`:1416` 每次握手前跑；写侧新增三命令 + 前端对话框 |

四个符号（`sftp_*_core_with_source` 各变体、`remove_system_known_host`）现在都有**自身模块之外的调用点**，由 `git grep` 逐个确认。

### 复查才发现的遗漏与错误（已修）

1. **CLI 少一个对偶**：CLI 只有 `known-hosts import`，而 `KnownHostsCommands` 枚举**只挂了一个 `Import` 变体**——本就是为对偶预留的形状。G13 的三面约定是「Tauri 命令 + CLI + 前端 api」，desktop 与前端都有了两向，只有 CLI 缺。已补 `known-hosts forget <host> [--port] [--json]`，并用临时 `HOME` + 四行伪 `known_hosts` 实测（裸主机名 / `[host]:port` / 多别名行 / hashed `|1|` 行 / 注释保留 / `.bak` 生成 / miss 不报错）。
2. **两处 `TODO(unwired)` 注释在代码接线后没删**：`core.rs` 仍说 SFTP 簇「八函数无任何仓库内调用者」，`embedded_ssh.rs` 仍说 `remove_system_known_host`「no caller in the repository」——两者在写下时成立、接线后为假。**这种注释最坏的形态就是这个**：想知道「还有什么没接线」的直觉做法正是 grep 这个字符串，而它会指向两处已经上线的东西。已改为描述真实调用点。
3. **CHANGELOG 与审计报告里一句事实错误**：初稿称 `remove_system_known_host`「CLI 与 daemon 已可达」，实测为假（全仓零调用点）。已修正。
4. **G13 设计文档的结论下得过早**：`docs/rssh-design-analysis.md` 写「导出回写系统 known_hosts 不可行——只存 SHA256 指纹、无完整 public key，故只做单向导入」。前半句只否定了**写**；**删**按 hostname 匹配，不需要 key 材料。已修正为「导出不可行、移除可行」，状态行由「双向导入导出」改为「导入 + 移除」。

### 冗余项的处置：已删（2026-09-12）

`emit_bus`、`is_headless`、`is_cli`、`policy_risk_rules` 均零调用，已全部删除，连带删掉它们的 `TODO(unwired)` 注释（详情见上面「第二类」）。删除时发现一个真缺陷：桌面端从不调用 `set_host`，`Host` 恒为默认的 `Host::Cli`，于是诊断报告把桌面应用报成 `"transport": "cli"` / `"is_desktop": false` —— 已在 `run_tauri` 的 setup 阶段一并修复。

## 本轮（复查）的验证

`cargo fmt --check` clean；`cargo build --no-default-features --bin agent2ssh` 通过；CLI `sftp --help` / `known-hosts --help` / `risk` 输出已实测（见上）。

**方法论教训**：只 grep 调用点会漏掉「有调用点但路径根本跑不通」的情形，只读 diff 则会漏掉「没人调用」——`TODO(unwired)` 这类标注**在接线时必须同步删除**，否则它会反过来成为下一次审计的错误信源。

## 清理轮（2026-09-12）的验证

删除四个冗余项 + 修复桌面端 transport 之后重跑全量矩阵，全绿：`cargo fmt --check` clean；clippy 四目标（CLI+MCP / daemon / lib 非默认 / lib 默认 tauri）各 0 warning；`cargo test --lib` **609**、`--no-default-features --lib` **601**（各比上轮 **+2**，正是新增的两个 `Host` 单测）、`cli_contract` 29、`cli_smoke` 33、`daemon_integration` 57、`connect_deadline` 2。删除的验证是双向的：`git grep` 确认四个符号在 Rust 源码与 `src/`（前端从未引用过它们）都无残留，同时 `policy.rs` 的 `RiskRules` 导入因 `AgentPolicyFile` 字段仍在用而**保留**、`Host::emit` 因有单测而**保留**。

**方法论教训（本轮新增一条）**：判断某段代码「多余」之前，先确认它引用的运行时状态是不是真的被设置了。`TODO` 把 `is_headless` / `is_cli` 说成「冗余谓词」，而真相是它们依赖的 `Host` 值在桌面端**从未被安装**——照注释直接删掉就永远发现不了那个真 bug。

---

# 增量扫描（2026-09-14）

用户问「针对冗余代码修复，还有哪些需要处理的」。重跑一轮**机械扫描 + 逐项实测**（不依赖上面各节的自述），
方法固化为脚本 `~/.workbuddy/skills/agent2ssh-add-operation/scripts/redundancy_scan.py`（五类检查）。

## 机械可查的三类已清零

| 检查 | 结果 |
|---|---|
| Rust `pub fn` 全仓仅出现一次（死函数） | **0** |
| TS `export` 全仓仅出现一次（死导出） | **0** |
| i18n 未使用键 | **0**（4 条疑似键经核实均为模板串动态键 `t(\`Highlight validation: ${v}\`)`） |

`TODO(unwired)` 标注也已归零。所以上一轮的清理是**彻底**的；剩下的不是「没人调用」，而是「重复 / 形态问题」。

## 待处理清单（按建议优先级）

| # | 项 | 位置 / 规模 | 证据 | 建议 |
|---|---|---|---|---|
| **R1** | `split_completed_session_commands` **整函数逐字复制** | `tauri_commands.rs:307` + `bin/agent2ssh-daemon.rs:375`，各 19 行 | 归一化后 **17 行完全一致** | 提到公共模块（`session.rs`），两处改引。**本轮最干净的一处** |
| **R2** | 多主机授权循环 **~30 行 × 3** | `tauri_commands.rs:466` / `bin/agent2ssh.rs:1055` / `agent2ssh_mcp/auth.rs:40` | 仅「拒绝文案 + 错误映射」不同（`command_authorization_error` 在前两处相同，MCP 用 `mcp_authorization_error`） | 加 `authorize_targets_without_approval_handler(rejection_message, rejection_hint)`；这是 **P1#6/#7 收敛时漏掉的多主机分支** |
| **R3** | `normalize_proxy` 重复 —— **报告称已收敛，实测未收敛** | `core.rs:397`（fn） + `store.rs:574`（内联块） | 重复块检测命中 `core.rs:398-405` / `store.rs:574-581` | 让 `store` 的 `normalize_config` 改调 `core::normalize_proxy`（先确认「循环 + retain + sort」组合语义等价）。**注意它不再同名，按名字搜不到** |
| **R4** | `ssh_config.rs` 的**测试专用 glob 镜像** | 3 个 `#[cfg(test)]` 函数（`glob_match` / `glob_match_bytes` / `char_class_match`）+ 约 10 个只为测它们的单测 | 生产侧 `splice_includes` 用的是 **`glob` crate**；这组镜像全仓仅被自己的测试引用 | 删除镜像与其单测（生产路径已有 `include_splices_glob_matches` 等真实覆盖），或明确它要保护什么语义 |
| **R5** | `AuditEntry` 测试 fixture 重复 | `playbook.rs:853` + `store.rs:2194` | 两处注释均自认「Mirror append_audit」；重复块检测命中 | 抽 `#[cfg(test)] pub(crate) fn test_audit_entry(...)` 复用 |
| **R6** | P1#11 手写 spinner / 错误块 | `animate-spin` **25 处 / 12 文件**；`text-destructive` **21 文件**；`ErrorState` 仅 `SFTPPanel` 1 个组件在用 | 逐文件统计 | 体量最大、且属用户可见一致性；报告判定「无安全 panel 级替换点」，需逐处确认后再动 |
| **R7** | `redaction.rs` / `highlight.rs` 平行规则库 | 各含 `seed_default_rules()` + `load_rules_from_json()`，同职责、不同规则类型 | 同名 `pub fn` 扫描 | 可抽泛型规则库；中等收益、中等风险（原报告未列） |
| **R8** | `rand` + `getrandom` 双 RNG 入口 | `Cargo.toml:65-66`；`rand` 仅在 `backup_crypto.rs:78-79` 用了 2 次 | `getrandom::fill` 已在 `keys.rs` / `mcp_binding.rs` / `secrets.rs` 使用 | 这 2 处改为 `getrandom::fill` 后删 `rand` 依赖（先 `cargo tree -i rand` 确认无其他消费者） |
| **R9** | 测试助手 `unique_dir` ×2 | `copy_redact.rs:153` + `snippets.rs:198` | `#[cfg(test)]` 扫描 | 低收益 |
| **R10** | 前端 2 处 8 行 UI 惯用法 | 「Refresh IconButton + `animate-spin`」`SnippetsDialog:170` / `PlaybooksPanel:339`；「host `<Select>`」`TerminalPanel:445` / `PlaybooksPanel:576` | 重复块检测 | 低收益；与 R6 同源，做 R6 可顺带覆盖 |

## 已排除：形似但**非**冗余（避免下一轮误删）

| 形似之处 | 为什么保留 |
|---|---|
| `daemon::authorize_command` vs `execution_control::authorize_command_without_approval_handler`（同为 8 参） | daemon 版多一个 daemon 专属 `auth_scope`、错误类型是 `(StatusCode, Json<ErrorBody>)`；`execution_control` 的注释明确写了「传 struct 会把 `auth_scope` 默认值推给全部 12 个调用方」 |
| `ssh_config::glob_match` vs `store::glob_match` | 前者字节级、**大小写敏感**、支持 `[...]` 字符类；后者**大小写不敏感**、只支持 `*`/`?`。**两种语义**。（前者是死代码，见 R4，但「同名」不是它该删的理由） |
| 同名 `#[tauri::command]` 与 `diagnostics.rs` / `recording.rs` 同名的实现 | 薄封装 + 实现分层，本仓既定模式 |
| `Host::is_desktop` / `activate` / `terminal_size` 等同名双份 | `#[cfg]` 门控，非副本 |

## 本轮（增量扫描）的验证

无代码改动，仅扫描 + 只读核实。所有结论均可用脚本复现：

```sh
python3 ~/.workbuddy/skills/agent2ssh-add-operation/scripts/redundancy_scan.py /Users/yuqu/Vbercodeing/agent2ssh
```

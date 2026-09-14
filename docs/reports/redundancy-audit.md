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
| **R7** | `redaction.rs` / `highlight.rs` 平行规则库 | 各含 `seed_default_rules()` + `load_rules_from_json()`，同职责、不同规则类型 | 同名 `pub fn` 扫描 | 可抽泛型规则库；中等收益、中等风险（原报告未列）。**⚠ 本行的范围与性质均已被证伪：实测是三方平行（漏了 `copy_redact.rs`）、其中一方是死代码、真正缺陷是权限加固缺失而非重复。见「对本报告自身结论的四处订正」** |
| **R8** | `rand` + `getrandom` 双 RNG 入口 | `Cargo.toml:65-66`；`rand` 仅在 `backup_crypto.rs:78-79` 用了 2 次 | `getrandom::fill` 已在 `keys.rs` / `mcp_binding.rs` / `secrets.rs` 使用 | 这 2 处改为 `getrandom::fill` 后删 `rand` 依赖（先 `cargo tree -i rand` 确认无其他消费者） |
| **R9** | 测试助手 `unique_dir` ×2 | `copy_redact.rs:153` + `snippets.rs:198` | `#[cfg(test)]` 扫描 | 低收益 |
| **R10** | 前端 2 处 8 行 UI 惯用法 | 「Refresh IconButton + `animate-spin`」`SnippetsDialog:170` / `PlaybooksPanel:339`；「host `<Select>`」`TerminalPanel:445` / `PlaybooksPanel:576` | 重复块检测 | 低收益；与 R6 同源，做 R6 可顺带覆盖。**⚠ 本行的范围被低估：检测口径是「重复块」，所以它只圈到了 busy 惯用法 11 处里的 2 处（另 9 处不是逐字重复块）。且「给 `IconButton` 加 `busy` 就收拢」已被证伪。见「R10 重新取证」的补测段** |

## 已排除：形似但**非**冗余（避免下一轮误删）

| 形似之处 | 为什么保留 |
|---|---|
| `daemon::authorize_command` vs `execution_control::authorize_command_without_approval_handler`（同为 8 参） | daemon 版多一个 daemon 专属 `auth_scope`、错误类型是 `(StatusCode, Json<ErrorBody>)`；`execution_control` 的注释明确写了「传 struct 会把 `auth_scope` 默认值推给全部 12 个调用方」 |
| `ssh_config::glob_match` vs `store::glob_match` | 前者字节级、**大小写敏感**、支持 `[...]` 字符类；后者**大小写不敏感**、只支持 `*`/`?`。**两种语义**。（前者是死代码，见 R4，但「同名」不是它该删的理由） |
| 同名 `#[tauri::command]` 与 `diagnostics.rs` / `recording.rs` 同名的实现 | 薄封装 + 实现分层，本仓既定模式 |
| `Host::is_desktop` / `activate` / `terminal_size` 等同名双份 | `#[cfg]` 门控，非副本 |

## 处理结果（本轮落地）

按「R1 → R3 → R2 → 低风险项」推进，十项已收敛，各自独立成提交：

| # | 提交 | 结果 |
|---|---|---|
| R1 | `7455e12` | `split_completed_session_commands` 提到 `session.rs`，两侧改引；daemon 因已 `use agent2ssh::session::*` 无需改 import |
| R3 | `dce8809` | 落成 `ProxyProfile::trim_fields`（纯 trim），**校验与失败策略留在各自侧** |
| R2 | `a268c76` | 新增 `execution_control::authorize_targets_without_approval_handler`，三处各缩为转发 |
| R4 | `3aceca7` | 删除 `ssh_config.rs` 的测试专用 glob 镜像（3 函数 + 6 单测，-138 行） |
| R5 | `eb6f6aa` | 新增 `AuditEntry::test_fixture()`，15 处字面量改为「只覆写断言字段」 |
| R9 | `07d3c64` | 新增 `store::TestConfigDir`（RAII），六个模块的临时配置目录统一 |
| R8 | `e0a539f` | `backup_crypto` 改用 `getrandom::fill`，`rand` 依赖已删除 |
| R6 | `627937a` | `ui/state.tsx` 新增 `Spinner` 与 `InlineAlert`，收拢 12 处手写 spinner、13 处手着色提示盒；补 `ui/state.test.tsx`（+9 测试） |
| R11 | `29ea30c` | `ConfirmDialog` 补 `note` / `confirmDisabled` 两个槽位，7 处手搓确认框收拢；顺带修掉「Cancel 未走 i18n」缺陷；补 `ui/dialog.test.tsx`（+9 测试） |
| R7 | `e7054a2` | `store::write_private_json` 成为三个规则文件的唯一写入路径；**修掉 `highlight_rules.json` 缺权限加固的真缺陷**；+2 测试 |
| R7 余项 | `51522a0` | A24 生命周期接线：`redact_rules.json` 成为**实时的规则来源**（core 切换 + 桌面命令 + 设置面板）；错误类型补 `IoError`/`ParseError`；写入改走 `write_private_json`（0600）；+4 测试 |
| R7 余项（第二步） | `9c843ed` | 桌面给出完整 CRUD（`add` / `update` / `delete`），推翻上一轮「只暴露安全方向」的判据；护栏改为确认框 + `Built-in` 徽章 + `Duplicate`/`NotFound` 写入前校验 + 空集警示；`BUILTIN_RULES` 提成常量；删掉死镜像 `RedactRuleConfig`；+8 Rust 测试、+9 前端测试（`RedactionSettings.test.tsx`） |

### 对本报告自身结论的四处订正

- **R3 的建议是错的**。原建议写「让 `store` 的 `normalize_config` 改调 `core::normalize_proxy`」。实际执行时发现两者的**失败策略有意不同**：`core` 对单个提交报错并把字段名回给用户（`proxy id is required`），`store` 对整文件静默 `retain` 丢弃。合并会把其中一种强加给另一种。因此只收敛了 trim（两种策略下逐字相同的那部分），校验各自保留。**这是本轮唯一一处「按原建议做反而会引入缺陷」的地方。**
- **R9 的收益被低估**。它不只是「低收益」的去重：`highlight.rs` 的 teardown 只清 override、**从不删目录**（每次跑测试泄漏一个临时目录），`forward.rs` 三处用 `std::process::id()` 造「唯一」目录，**同进程内测试实际共用一个目录**。两者都是真实缺陷，随 R9 一并修掉。
- **R7 的形态与范围都写错了**。原始扫描表把它记为「`redaction.rs` / `highlight.rs` 平行规则库……可抽泛型规则库」，
  检测方式是「同名 `pub fn` 扫描」。实际是**三方**平行（漏了 `src/copy_redact.rs`），其中 `redaction` 那一方**整个是死代码**，
  而唯一真正的缺陷是**两个活写入者里有一个漏了全仓统一的 `0600` 权限加固**——「抽泛型规则库」既不是唯一解法、也不是重点。
  详见下面「R7 落地」一节。**教训：`同名 pub fn 扫描` 这种机械检测会同时误判范围（漏文件）和误判性质（把「写入路径缺一个横切步骤」当成「重复代码」）。**
- **R7 余项的「只暴露安全方向」同样站不住**（第四处，`9c843ed` 反转）。上一轮在本报告里论证：
  `reset`（恢复内置规则）只会让脱敏**变强**，而 `delete` 会让某类秘密在应用写入的所有位置不再被脱敏，
  因此只暴露 `list` + `reset`，`add` / `delete` / `update` 留在手改文件；佐证是
  「`copy_redact_rules.json` 从来没有 CRUD UI」。
  **事实成立，论证不成立**：`copy_redact` 恰恰是同族里**唯一**一个没有编辑面的成员，
  而三十行之外的 `HighlightSettings` 四个操作俱全（core + 5 个桌面命令 + `api.ts` + 面板）。
  只举一个同向的兄弟，就等于让它替结论说话。护栏因此从「不暴露这个操作」换成「给这个操作定性」，
  细节见「R7 余项（第二步）」一节。
  **教训：以「兄弟模块的既有契约」为论据时，必须先把兄弟逐个列出并注明各自形态，否则该论据只反映被选中的那个样本。**

### 本轮新踩到的坑（供下一轮参考）

- **`..base,` 不能带尾逗号**：Rust 报 `cannot use a comma after the base struct`，struct update 语法的展开项必须是最后一个且无逗号。
- **`let _x = Guard::new();` 与裸 `Guard::new();` 不等价**：后者是临时值，**当行即析构**。R9 里 5 个 highlight 测试因此失败——旧 helper 的 override 是副作用、返回值被丢弃，机械替换成裸语句后 override 在测试体执行前就被清掉了。
- **`fn f(..) -> AuditEntry {` 也会匹配 `AuditEntry {`**：按「含 `AuditEntry {` 的行」定位字面量会把函数签名当成字面量起点，导致花括号配对整体错位。

### R6 落地时的取舍（用户可见变更已在此列明）

- **改了 2 处外观，均朝统一规格**：`AlgoPrefsDialog` 的错误框 `text-xs → text-sm`、`px-2.5 → px-3`；
  `SyncPanel` 的 warning 提示条去掉一次性的 `rounded-lg`/`p-3`（改回 `rounded-md`/`px-3 py-2`）。其余 10 处渲染结果不变。
- **未并入的一处**：`SFTPPanel:1157` 的传输进度条同为着色边框盒，但它是**进度指示**（spinner 打头、`items-center`、
  内容为内联文本 + 条件 `<span>`），并入会强行套用 alert 布局（`items-start` + `break-words` 包裹层会吃掉文本与
  `<span>` 之间的 `gap`）。故保留，并在组件注释里说明 `InlineAlert` 是 alert、不是 progress。
- **未动 `"animate-spin" : ""` 切换写法**（11 处）。抽 `BusyIcon` 后调用点反而更长
  （`<BusyIcon busy={loading} icon={RefreshCw} size={14} />` vs `<RefreshCw size={14} className={loading ? "animate-spin" : ""} />`），
  属改而不善。`Spinner` 只统一了「spinner 长什么样」这一件事。

### R10 重新取证：两项都是设计决策，不是去重

**R10-1 刷新按钮**。`title={t("Refresh")}` 的 `IconButton` 有 **7 处**（订正：第二轮记 5 处，漏了
`AuditPanel:238` 与 `HostList:325`）—— 5 处静态（`AuditPanel:238`、`ConfigSnapshotsPanel:150`、
`ForwardPanel:179`、`HostList:325`、`ConnectionTopology:234`，全部 `size={15}`）与 2 处会转的
（`SnippetsDialog:176` / `PlaybooksPanel:344`，`size={14}`，这两处是逐字重复的 6 行块）。
而 `className={busy ? "animate-spin" : ""}` 切换全仓有 **11 处**（`RefreshCw` 10 + `RotateCcw` 1），
落在 6 个文件、3 种显式尺寸（14 / 15 / 16）加 1 处无尺寸，
且 busy 表达式有 7 组不同写法 —— 仅 `SettingsMenu` 一个文件就占了 5 组（`refreshBusy` / `healthBusy` /
`updateBusy === "check"` / `daemonActionBusy === "restart"` / `diagnosticBusy === "refresh"`）。
要收就得定「busy 怎么画」这个契约 —— 需先定契约，不能顺手改。
**（2026-09-14 第二轮补测：报告建议的「给 `IconButton` 加 `busy`」这条路径只覆盖 11 处里的 2 处，见下。）**

**第三轮补测：仓里早就有了「busy 怎么画」的答案，而且已经用了 11 次**

11 处手写者不是全仓唯一的 busy 画法。另有 **11 处调用点**采用「忙碌时把按钮自己的图标**换成** `Spinner`」：

| 站点 | 形态 |
|---|---|
| `SyncPanel:308 / 316 / 324 / 345` | `{busy === "…" ? <Spinner size={14} /> : <Save \| RefreshCw \| UploadCloud size={14} />}` |
| `PlaybooksPanel:499 / 598` | `<Save>` / `<Play>` 同上 |
| `SnippetsDialog:238` | `<Save>` 同上 |
| `ForwardPanel:166 / 234` | `<Plus>` / `<Trash2>` 同上 |
| `App.tsx:980`、`SFTPPanel:1158` | 非按钮：连接进度行 / 传输提示条 |

9 处在按钮内（8 处是共享 `<Button>`，1 处 `ForwardPanel:234` 是 `IconButton`），尺寸一律 14。

所以「busy 怎么画」在本仓是**两种并存的语言**：

| | (A) 换成 `Spinner` | (B) 原图标 `animate-spin` |
|---|---|---|
| 调用点 | **11**（9 按钮 + 2 非按钮） | **11** |
| 是否已有统一出口 | **是** —— `Spinner`（`state.tsx`，带 `aria-hidden`、3 个测试） | 否，字面量散在 6 个文件 |
| 尺寸 | 一律 14 | 14×4 / 15×1 / 16×5 / 无×1 |
| 忙碌时图标形状 | 变（→ 加载圈） | 不变 |

**两个方向都能收，但 (A) 的边际成本更低**：它不需要新造组件，且收完只剩一种语言；
(B) 若只覆盖这 11 处，另 11 处仍在走 `Spinner` —— 全仓仍是两种语言。

**11 处手写者按「外层包装」拆开**：

| 外层 | 处数 | 位置 |
|---|---|---|
| `IconButton` | **2** | `SnippetsDialog:176`、`PlaybooksPanel:344` |
| 共享 `<Button>` + 文字标签 | **4** | `McpAgentsPanel:159`、`RecordingsPanel:230`、`AlgoPrefsDialog:206`（`RotateCcw`）、`SFTPPanel:942`（无标签） |
| **裸 `<button>`** | **5** | `SettingsMenu:484 / 512 / 533 / 664 / 769`（全部 `size={16}` + 标签） |

所以「给 `IconButton` 加 `busy`」只解决 2/11；**最大的一块（5 处）是 `SettingsMenu` 的裸按钮**，
报告正文没提到它们是裸按钮。另外 `SettingsMenu:664` 忙碌时还会把标签从 `Restart daemon` 切成 `Restarting...`，
说明「busy」在这些站点上影响的**不只图标**。

**同一族的尺寸不一致，但只有一半是真的（第三轮订正）**：7 处「卡片头 / 工具栏刷新 `IconButton`」
被画成两种尺寸 —— 5 处静态是 `size={15}`、2 处会转的是 `size={14}`，差异**刚好沿着「会不会转」切开**。
但**只有 `IconButton` 与裸 `<button>` 内的尺寸才是真实渲染差异**，因为共享 `Button` 的基础类含
`[&_svg]:size-4`，构建产物里确实生成了：

```css
.\[\&_svg\]\:size-4 svg{width:calc(var(--spacing) * 4);height:calc(var(--spacing) * 4)}
```

它作用于 `svg` 元素本身，而 `<RefreshCw size={14} />` 产生的 `width="14"` 只是 presentation
attribute —— 级联上低于任何作者样式表声明。**所以 4 处 `Button` 内传的 `size`（14 / 14 / 15 / 无）
全是死参数，实际一律渲染 16px**；只有 `IconButton`（仅 `[&_svg]:shrink-0`，不覆盖尺寸）与裸 `<button>`
让 `size` 生效。这条**不限于 R10-1**：它意味着「在 `Button` 里写 `size`」是一类静默失效的写法，
可在收口时一并清掉（零视觉变化）。

**结论（`dd59051`）：已收口，取方向 A。** 11 处全部改为 `{busy ? <Spinner size={n} /> : <原图标 size={n} />}`。
另收一处**断言时才发现**的同类站点：`ui/toast.tsx` 的 `progress` 变体把 `animate-spin` 写在类名表里
（`VARIANT_ICON_CLS.progress`），是第三种「手写旋转」的写法。该变体**当前零调用点**（单行调用 90 处
全是 error / success / warning，多行调用亦然），而换成 `Spinner` 后渲染出的类集合完全相同，
故属零视觉变化——收它是为了让 `Spinner` 自己的文档注释（「`Spinner` is the only thing in the app
that turns」）不再是一句假话，而不是修一个可见问题。

按站点保留的差异：`SettingsMenu` 5 处保持 `size={16}`、`RecordingsPanel` 保持行内标签，两者都不属重复。
**唯一可见变化**：`IconButton` 内 2 处忙碌 `RefreshCw` 由 14 → 15，与同族 5 处静态对齐（这 2 处尺寸真实生效，
所以点击瞬间图标曾缩 1px）；`Button` 内 4 处死 `size` 一并删除，零视觉变化。
收口后全仓手写 `animate-spin` 归零（只剩 `Spinner` 定义本身与两处注释），`Spinner` 调用点 13 → 25，
且「忙碌时图标变成加载圈」是本仓既有主流形态，不是本次新引入的语言。

**本轮新发现（未编号，待拍板是否立项）**：`SettingsMenu.tsx` 有 **23 个裸 `<button>`、0 个 `<Button>`**，
是全仓最大的裸按钮集中点（第二名 `SFTPPanel` 8 处，其余 ≤ 6；全仓 `<Button>` 共 **85** 处）。
它与 R10-1 **相邻但不同源**：R10-1 是「busy 态怎么画」，这个是「整个面板不用共享按钮组件」。
本轮不立项——立项前需要先确认 `Button` 是否存在能覆盖设置面板那种密排小按钮的尺寸档
（否则「替换」会改变面板密度，属可见变化）。

**R10-2 host 选择器**。**`src/components/HostSelector.tsx` 已经存在**，而 `TerminalPanel:441` 与
`PlaybooksPanel:571` 仍在手搓同一段 8 行 `<Select>`（`{hosts.length === 0 && <option value="">{t("No hosts")}</option>}` +
`hosts.map(...)`），选项文案为 `host.name`。
但两者**契约不匹配**，不是 drop-in：

| | `HostSelector` | 两处手搓 |
|---|---|---|
| 外层 | `<label>` 包裹 + 图标 + 「Target server」标题 | 裸 `<Select>`，嵌在 `flex flex-wrap items-center gap-2` 工具条里 |
| 选项文案 | `name - user@host:port`（`describeHost`） | 仅 `name` |
| 空值时 | **自动选中第一个 host**（`useEffect`） | 允许空选，靠提交按钮 `disabled={!newHost}` 兜底 |

自动选中会改变这两处工具栏的行为（用户原本可以看到「未选择」状态）。故需先决定：是给 `HostSelector` 加一个
`compact` / 无 label 变体并接受自动选中，还是让两处显式选择「不自动选」。**属产品决策，不宜由去重驱动。**

### R11 落地：两个槽位，而不是一类字符串

上一轮把 R11 记为「7 处手搓确认框绕过 `ConfirmDialog`」（那个数在上一轮已由 6 订正为 7）。**这次实测复核确认为 7 处，
但形态不是「类名字符串重复」，而是「共享组件缺两个槽位」**：

| 站点 | 标题 | 警示条 | 确认按钮 |
|---|---|---|---|
| `HostList:530` 删单主机 | `Remove host {name}?` | `<InlineAlert>` 孤立资源 | destructive |
| `HostList:553` 批量删 | `Remove {count} selected hosts?` | `<InlineAlert>` 孤立资源 | destructive |
| `HostList:579` 删分组 | `Delete group {name}?` | `<InlineAlert>` 移入 Default | destructive |
| `ConfigSnapshotsPanel:209` 应用模板 | `Apply the {name} template?` | `<InlineAlert>` 覆盖 policy.toml | default + `disabled={busy}` |
| `ConfigSnapshotsPanel:228` 恢复快照 | `Restore snapshot {label}?` | `<InlineAlert>` 覆盖当前配置 | destructive + `disabled={busy}` |
| `ConfigSnapshotsPanel:249` 删快照 | `Delete snapshot {label}?` | **无** | destructive + `disabled={busy}` |
| `ProxyPanel:260` 删代理 | `Delete proxy {name}?` | `<InlineAlert>` 回退直连 | destructive |

所以 `ConfirmDialog` 只缺两样东西，且两样都是**纯增量、向后兼容**的：

- `note?: React.ReactNode` —— 6/7 处的内容；渲染在 `description` 与按钮行之间。**「后果」用 `note`（着色警示盒），
  「中性说明」用 `description`（灰色小字）**，这条分工此前没有写下来，现在写进类型注释：`SFTPPanel:519`
  的「删除后不可撤销」就该是 `description`，而 `HostList` 的「会话会变成孤立资源」就该是 `note`。
- `confirmDisabled?: boolean` —— 3 处快照对话框在飞行中需要它。

### R11 的三处可见变更（全部落在已使用 `ConfirmDialog` 的 7 处）

按钮尺寸是唯一需要判断的地方：**迁移前**全仓 `flex justify-end gap-2.5` 这个 footer 惯用法共 9 处，
其中 **8 处是手搓确认框、都用默认尺寸**（含最权威的 `ApprovalDialog` 风险闸门），**只有 `ConfirmDialog` 一个用 `size="sm"`**。
而应用基准字号是 **14px（`text-sm`）**，`size="sm"` 是 `h-8 text-xs` —— **模态框的按钮比它自己的正文还小**，
像是从面板工具栏抄来的。故**去掉 `size="sm"`**，统一到默认尺寸。

- 7 处**已使用** `ConfirmDialog` 的确认框：按钮 `h-8 text-xs` → `h-9 text-sm`（变大）。
- 7 处**新迁移**的确认框：按钮尺寸**不变**（它们本来就是默认尺寸）；标题由常规字重 → `font-medium`。
- 标题颜色：原先继承弹层的 `popover-foreground`，现在被 `ConfirmDialog` 显式写成 `text-foreground`。
  6 套主题里 4 套两个 token 同色，**dark 与 nord 两套不同**，这两套下标题比原先略暗。

### R11 顺带修掉的一个真缺陷：确认框的 Cancel 从未走 i18n

`ConfirmDialog` 把 `cancelLabel` / `confirmLabel` 默认成**字面量** `"Cancel"` / `"Confirm"`，
而 7 个 `confirmDialog()` 调用点里**有 6 个省略了 `cancelLabel`** —— 于是这 6 处在中文界面上渲染英文 "Cancel"，
而 `"Cancel": "取消"` 就在 zh 表里、另外 14 处调用点都规规矩矩写着 `t("Cancel")`。

**修法是让默认值走 `t()`**，而不是让 7 个迁移点各自补一个 `cancelLabel={t("Cancel")}`：
后者只能修好当下这 7 处，下一个写 `confirmDialog({...})` 的人照样会漏。**默认值正确 = 由构造保证，而非靠纪律。**
代价是 `ui/dialog.tsx` 从此依赖 `src/i18n`（`ui/` 此前零 i18n 依赖），这是**有意为之**：
`useI18n()` 在任何渲染路径上都可用（`main.tsx` 的 `I18nProvider` 包住 `App`，`ConfirmHost` 在其中），
且 `ui/` 并非独立发包，用本仓的 i18n 不构成层次违规。**未来若有人想把 i18n 从这里摘出去，请先读这段。**
`"Confirm"` 也补了 zh 条目 —— 它此前从未真正渲染过，所以一直不必存在。

### 留待决定的一处 token 错配

`ConfirmDialog` 的标题写 `text-sm font-medium text-foreground`，但 `ConfirmDialog` 永远渲染在 `<Dialog>` 里，
而 `<Dialog>` 的卡片是 `bg-popover text-popover-foreground`。**弹层表面上的文字本该用 `popover-foreground`。**
在 dark（`#c7d0d8` vs `#d1dae2`）与 nord（`#d8dee9` vs `#eceff4`）两套主题下这两个 token 不同色，
标题因此比弹层里的其他文字略暗。

**已取证，本轮不改。** 理由：改法（删掉 `text-foreground` 让标题继承）会让**全部**确认框的标题在两套主题下一起变色，
而这两套主题无法在本机目视验证，属「读 CSS 变量推断出来的修改」，收益（颜色一致性）远小于风险。
留给有主题截图条件的一轮处理。**注意它与上面的「标题颜色」delta 是同一件事的两个方向**：
保持现状 → 7 个迁移点略暗；删掉 → 7 个原有调用点略暗。两边都不零成本。

**（2026-09-14 第二轮补测，两项结论一正一误）**：

- **主题差异的范围属实，已逐套复核**：全仓 **6 套主题**，只有 **dark**（`--foreground: #c7d0d8`
  vs `--popover-foreground: #d1dae2`）与 **nord**（`#d8dee9` vs `#eceff4`）两个 token 取值不同；
  其余四套（`:root` / `light` / `dracula` / `solarized-light`）两值相同。
  → 改动**只在两套主题下可见**，这也正是它无法在本机目视验证的原因。
- **但「14 个确认框」这个计数已过期**：`<ConfirmDialog>` / `confirmDialog()` 的调用点现在是 **16 个**
  （`HostList` 3、`ConfigSnapshotsPanel` 3、`RedactionSettings` **2**、`McpAgentsPanel` 2，
  其余 6 个文件各 1）。**其中 +2 来自本轮「R7 余项（第二步）」新增的删除确认与恢复默认确认**——
  这个数字是被我自己这一轮的改动推高的。**影响面由 14 变 16，风险随之后移；「暂不处理」的结论不变。**

### R7 落地：不是「两个平行规则库」，是三方平行 + 一个死生命周期 + 一个权限缺口

报告对 R7 的描述只有一行：「`redaction.rs` / `highlight.rs` 平行规则库，各含 `seed_default_rules()` + `load_rules_from_json()`，
同职责、不同规则类型」，检测方式写的是「同名 `pub fn` 扫描」。**勘察推翻了这个描述的三处分。**

**一、是三方，报告漏了第三方。** `src/copy_redact.rs` 有完整的一套同名生命周期：
`CopyRedactRuleConfig` + `From<&CopyRedactRule>`、`default_copy_rules()`、`load_copy_redact_rules()`、
`save_copy_redact_rules()`、`reset_copy_redact_rules()`。报告只数了两个文件，所以「抽泛型规则库」这个建议的方向对、范围错。

**二、其中一方整个是死代码。** `redaction.rs` 的 A24「用户可编辑规则文件」部分（`src-tauri/src/redaction.rs:278–390`，
**113 行**：`REDACT_RULES_FILE`、`redact_rules_path()`、`seed_default_rules()`、`load_user_rules()`、
`reset_default_rules()`、`redact_with_user_rules()`、`RedactRuleConfig` + 它的 `From`、`load_rules_from_json()`）
**在全仓没有任何调用者**：四个生命周期函数零调用，`load_rules_from_json` 虽从 `lib.rs` re-export 但也零调用。
前端零提及，`tauri_commands.rs` 里也没有对应命令。它还有 **5 个** `a24_*` 测试（`:688` 起）——测试全绿，
因为它们直接调用这些函数；绿的是函数，不是功能。**redaction 的引擎是活的**（`redact_default` ← `store.rs:1036`），
死的只是「文件化 + 用户可编辑」这一层。而 `copy_redact` 恰好是**同一想法的已接线版本**——
`redact_for_clipboard` 读它的规则文件，桌面端有命令。换言之：这个能力在剪贴板侧做完了，在日志侧停在了半途。

**三、真正的缺陷不在「重复」，在「写入路径漏了一个横切步骤」。**

| | `highlight.rs` | `copy_redact.rs` | `redaction.rs` |
|---|---|---|---|
| 接线状态 | **活**（`tauri_commands` 5 个命令） | **活**（`redact_for_clipboard` ← `tauri_commands:1502`） | **死**（零调用者）→ `51522a0` **已接线**，见「R7 余项落地」 |
| 默认规则 | `default_rules()` 私有 | `default_copy_rules()` 私有 | `default_rules()` pub |
| 配置 DTO | 不需要（`HighlightRule` 本身可序列化） | `CopyRedactRuleConfig` | `RedactRuleConfig` |
| 写入 | `save_rules()` | `save_copy_redact_rules()` | 内联在 `seed` 与 `reset` **两处** |
| **权限加固** | **❌ 缺** | ✅ `restrict_file_to_owner` | ❌ 缺 |
| 错误类型 | `HighlightError`（有 `IoError` / `ParseError`） | `anyhow::Result` | `RedactRuleError`（**只有 `InvalidRegex` / `ZeroWidth`**） |

`restrict_file_to_owner`（Unix `0600`，Windows `icacls` 收紧 ACL）是**全仓 16 个文件、约 40 处调用点**的惯例，
覆盖 `daemon.token`、`keys/`、`hosts.json`、`secrets.enc`、录屏、审批策略等等。
**三个规则文件里只有 `copy_redact` 做了这一步**，而且它带着注释说明自己修过一个缺陷：
「previous module-local helper was Unix-only and silently did nothing on Windows, leaving the rules file readable by other local accounts」。
`highlight` 两个平台都从来没做过这一步。

**实测后果**：`highlight_rules.json` 在本机（umask `022`）落成 `0644` / `-rw-r--r--`——**任何本地账户可读**。
对本轮两个文件来说敏感性有限（高亮关键词不是秘密），但这是与仓库自身惯例的背离，而修法零风险。
另有一处内部不对称：`redaction` 的 `seed` 建父目录、`reset` **不建**（同一段写入逻辑抄了两遍、只改了一半）。

**落地方案**：在 `store.rs` 新增 `write_private_json<T: Serialize>(path, value)`——
建父目录 → `to_string_pretty` → 写入 → `restrict_file_to_owner`，**加固放在函数内部，让「忘记加固」在结构上不可能发生**
（与 R6 的 `InlineAlert`、R11 的 `ConfirmDialog` 默认标签同一条思路：由构造保证，而非靠纪律）。
两个**活**的写入者（`highlight::save_rules`、`copy_redact::save_copy_redact_rules`）改走它。
它刻意不加锁、不做备份——那是 `save_config` 对 `hosts.json` 的职责（多进程写 + 需要可回退的上一版），规则文件一次只有一个用户动作在写。
顺带 `highlight::save_rules` 不再吞掉 `create_dir_all` 的失败。

**`redaction.rs` 的死生命周期本轮故意不动。**（**下一轮已接上，见下面「R7 余项落地」——本节保留当时的判断过程。**）
理由：它是「未完成的功能」而不是「更差的副本」，
删掉它和接上它都是产品决策（仓库既有先例两种都有——`policy_risk_rules` 因为只是活路径的更差副本而被删，
`ssh_algo` 的半成品则被接上）。所以它保留原样、保留那两处内联写入（因而也保留缺少的加固），
并在下面「仍未处理」里作为一项**需要决策**列出。

**验证**：`cargo fmt --check` 干净；clippy `-D warnings` 四目标 0 告警；
`cargo test --no-default-features --lib` **598**（+2）、`cargo test --lib` **606**（+2）；
cli_smoke 33 / connect_deadline 2 / daemon_integration 57 全绿。
两个新测试：`test_write_private_json_hardens_and_creates_parents`（0600 + 自动建父目录）、
`seed_restricts_the_rules_file_to_its_owner`（**这是那个被暴露文件的回归测试**）。

### R7 余项落地：把 A24 接到实时路径上（`51522a0`）

上一轮把 A24 列为「删与接都是产品决策」。本轮决定**接上**，但勘察让原计划的三处前提都不成立：

**一、「五面贯通」这个说法本身要先改。** 动手前重测了它的模板——**`highlight`（B24）和 `copy_redact` 自己都不是五面贯通的**：
`highlight` 是 core + 桌面 5 个命令 + `api.ts` + UI，CLI / MCP / daemon 一个都没有；
`copy_redact` 更少，只有一个桌面命令，规则文件靠手改。
所以「A24 比照 highlight 接线」= **core + 桌面 + UI**，而不是四五个面。
**教训与 R7 同源：报告/计划里的范围描述，和它的计数一样要回测。**

**二、真正的交付不是「把死函数接上」，而是「换掉实时路径读谁」。**
`store::redact_sensitive_text` 原本调 `redact_default`（硬编码），因此文件即使被接上也只是个装饰。
现在它调 `redact_with_user_rules`，文件成为**审计记录 / webhook 通知 / playbook 结果 / 诊断导出**四条链路共同的规则来源。
这一条是整件事的重点——死函数接上但不改路径，就是「改而不善」。

**三、安全方向决定了暴露哪些操作。** 报告原文把 A24 描述为「用户可 customize、**disable**、add 规则」。
「disable」这条在实时路径上是**安全降级**：删掉一条规则 = 某类秘密在应用写的所有地方不再被脱敏。
所以本轮的切分是：

| 操作 | 方向 | 本轮暴露 | 下一轮 |
|---|---|---|---|
| `list` | 只读 | ✅ 桌面 + 设置面板 | — |
| `reset`（恢复内置） | **只会变强**（把用户删掉的规则加回来） | ✅ 桌面 + 设置面板（带确认框） | — |
| `add` / `delete` / `update` | 可能变弱 | ❌ 不做 UI；仍是手改 `redact_rules.json` | **已被推翻，见「R7 余项（第二步）」** |

手改文件这条路当时记作「不是妥协，而是**该文件既有兄弟的既有契约**」：`copy_redact_rules.json` 从来没有 CRUD UI。
**这句推论下一轮被推翻了——被推翻的不是事实（复测仍成立：`copy_redact` 至今只有一个桌面命令、零 CRUD 面），
而是这条推论本身：它选错了对比对象。**
面板因此是只读 + 一个「恢复默认」，并在组件注释里写明为什么不做编辑。

**顺带把上一轮留下的两处一并修掉**（都在这次必须碰的函数里）：
`seed` 建父目录而 `reset` 不建（同一段写入逻辑抄了两遍只改了一半）；
以及 `RedactRuleError` **只有 `InvalidRegex` / `ZeroWidth`**，导致「配置目录取不到」「文件不是 JSON」全被报成
`invalid regex pattern: cannot determine config directory` —— 现在补 `IoError` / `ParseError`。
上一轮记录过「它在全仓无穷举匹配、可安全扩充」，本轮正是那个时机。
写入改走 `store::write_private_json`（0600）：原实现是裸 `fs::write`，**实测 umask 022 下落成 `0644`**——
不过这是**潜在**缺陷而不是已发生的（函数此前不可达，文件从未被写出）。

**一处刻意不做的事：不给 `load_user_rules` 加缓存。** 它在实时路径上每次调用都读文件 + 重编译，
但**原来的 `redact_default` 在同一路径上本来就每次重编译 `default_rules()`**，所以这是增量而非新增量级；
而 mtime 缓存要处理**文件系统 mtime 粒度**——「刚写完立刻读回」并不保证失效，正是让测试 flaky 的那种形状。

**一处必须一起改的横切面：测试的配置目录隔离。** 实时路径改读文件后，
任何传递调用 `redact_sensitive_text` 的测试都会去读写开发者真实的 `~/.agent2ssh/redact_rules.json`。
逐点审计后隔离了 3 处：`store::test_redact_sensitive_text`（它**断言的是字面量期望值**，
删掉内置 hex 规则的开发者会让它失败）、`store::test_exec_multi_audit_entries_reason_and_change_id`、
`playbook::test_playbook_audit_entries_all_share_context`。
`notify.rs` / `diagnostics.rs` 的测试本来就用 `HOME` / `CONFIG_DIR_ENV` 隔离，未动。

**验证**：`cargo fmt --check` 干净；clippy `-D warnings` 四目标 0 告警；
`cargo test --no-default-features --lib` **602**（598 + 4）、`cargo test --lib` **610**（606 + 4），两处都恰好 +4；
tsc 干净、biome 99 文件、`check-i18n` 干净、vitest 97、`npm run build` 成功、`gen/schemas` 无噪声。
新测试 4 个，其中 `redact_sensitive_text_reads_the_rules_file` 是**接线本身的回归测试**：
它同时断言「只存在于文件里的规则被应用」与「被删掉的内置规则不再生效」，
即证明文件是**权威的**而不是叠加的——没有它，「接上」和「没接上」在测试上无法区分。
另 3 个：`a24_rules_file_is_owner_only`（0600）、
`a24_malformed_json_is_a_parse_error_not_a_regex_error`、
`redact_sensitive_text_falls_back_to_defaults_on_corrupt_file`。

### R7 余项（第二步）：add / update / delete 也做进 UI（`9c843ed`）

上一轮把 `add` / `delete` / `update` 留在手改文件，并把这个切分同时写进了 CHANGELOG、本报告、组件注释和工作记忆四处。
**用户直接推翻了它**（「add/delete/update 也应该有 ui」），本轮照办，
并把护栏从「不暴露这个操作」换成「给这个操作定性」。

**被推翻的不是事实，是那条论证。** 上一轮写下的是两句：一句是实测（`copy_redact` 确实没有 CRUD UI），
另一句是由此得出的推论（所以「手改文件」是本仓既有契约、不算妥协）。推论的问题在于**只列了一个同向的兄弟**：
同一个设置面板往上三十行就是 `HighlightSettings`，它 **add / update / delete 全都有**
（core + 5 个桌面命令 + `api.ts` + 面板）。拿唯一一个没有编辑面的规则库去代表另外两个，结论自然偏向它。

**教训：用「兄弟模块的既有契约」当论据时，必须先把兄弟全列出来、给出各自的形态，再下结论。**
只举一个例子就等于让例子替你选结论——这与 R7 本身的错误同源（范围描述没回测）。

**护栏从「没有这个操作」换成「给这个操作定性」：**

| 机制 | 替代了原先的什么 |
|---|---|
| 删除走 `ConfirmDialog` + `danger`，文案点名后果（此后写入的审计记录 / 通知 / 诊断导出不再脱敏） | 原先「你不该做这件事」 |
| 内置规则带 `Built-in` 徽章；删除内置规则时对话框再加一条 `InlineAlert`：「这是内置规则，删除后应用写入的所有位置都不再对这一类秘密脱敏」 | 徽章让警告能说清**具体丢的是哪一类**，而不是泛泛的「有风险」 |
| 空规则集渲染 `InlineAlert tone="destructive"`（「规则列表为空，审计记录会原样写出」） | 「空」原先会被读成中性的「还没配」 |
| 空 replacement 渲染成 `(removed)` | 原先是一个无名空白单元格 |
| `Duplicate` / `NotFound` 在写入前挡住歧义操作 | 原先靠用户不犯错 |

**`pattern` 是规则唯一的身份**（没有 id、没有 name），所以那两个错误变体不是防御性编程而是**必需**：
不改的话，「改名撞上另一条规则」会静默覆盖它，「改一个已不存在的 pattern」会静默变成新增。
`insert_rule` / `update_rule` 都**先校验正则再动文件**，坏 pattern 不会留下半写状态。
`update_rule` 的查重特意跳过「改的正是自己」（只在 `pattern != old_pattern` 时查），
否则只改替换文本会和自己撞车——这条有专门的测试。

**`BUILTIN_RULES` 提成一个常量数组**，`default_rules()` 与新增的 `is_builtin_pattern()` 都读它。
原先「是否内置」只能靠重编译内置正则来回答；提出来之后，**播种集合与徽章在结构上不可能漂移**——
而这正是 UI 那条「删除内置规则会让一整类秘密不再脱敏」的警告所依赖的性质。

**顺带删掉一个上一轮刚引入的死镜像**：`src/types.ts` 的 `RedactRuleConfig`。
它随只读面板一起进来，本轮面板改用 `RedactRuleInfo` 后全前端零引用——
前端从不读 `redact_rules.json`，这个同形状的第二类型只提供了让两者漂移的机会。

**一处必须一起改的措辞**：i18n 里恢复默认的说明原文是「恢复内置规则，并丢弃你**新增或删除**的规则」。
「丢弃你删除的规则」本身就不通（删除的规则正是被恢复的对象），CRUD 到位后这句会真的误导，
故改为「用内置规则替换当前列表：你新增的会被丢弃，你删除的会重新恢复」。

**验证**：`cargo fmt` 干净；clippy `-D warnings` 四目标 0 告警；
`cargo test --no-default-features --lib` **610**（602 + 8）、`cargo test --lib` **618**（610 + 8），两处都恰好 +8；
tsc 干净、biome **100** 文件、`check-i18n` 干净、vitest **106**（97 + 9）、`npm run build` 成功、`gen/schemas` 无噪声。
新增 Rust 测试 8 个，其中三个是**不变量测试**而非功能测试：
`a24_every_default_rule_is_reported_as_built_in`（徽章与播种集合不漂移）、
`a24_update_rule_allows_changing_only_the_replacement`（只改替换文本不和自己撞车）、
`a24_delete_rule_removes_by_pattern_and_can_empty_the_set`
（删空之后 `redact_with_user_rules` 真的不再脱敏——这条同时钉住「空文件不会被重新播种」）。
前端新增 `RedactionSettings.test.tsx` 单文件 9 个，覆盖四个操作各自的成功路径 + 拒绝路径 + 确认框门禁 + 空集警示。

## 仍未处理

| # | 项 | 为什么留到下一轮 |
|---|---|---|
| R7 余项（再度收窄） | A24 的 **CLI / MCP / daemon** 三个面尚未暴露 | `51522a0` 接线 + `9c843ed` 桌面完整 CRUD，已等于 B24 highlight 的同一个面集合。剩下三个面按 `agent2ssh-add-operation` 走，但**该暴露哪些必须重新论证**：上一轮「只应暴露 list + reset」的判据（把安全方向当成唯一筛选条件）已被用户推翻，不能直接沿用。可考虑的中间形态是「`list` + `reset` 上自动化面，`add` / `update` / `delete` 仅桌面」。同步时须改 `docs/skills.md` 与其余 ~10 处 MCP 工具计数 |
| R10-2（host 选择器） | `HostSelector` 已存在，`TerminalPanel:446` / `PlaybooksPanel:576` 仍手搓同一个 `<Select>` | 补测确认：合并会改**下拉项文案**（`name` → `name - user@host:port`）并引入自动选中第一个 host。可加 `compact` / `optionLabel` 两个开关保住现有行为，但**文案变化无法避免**。**属产品决策，不宜由去重驱动** |
| 新发现（未编号） | `SettingsMenu`：23 个裸 `<button>` / 0 个 `<Button>`，全仓最大集中点 | 见上「R10 重新取证」的补测段。**未立项**：立项前需先确认 `Button` 是否存在能覆盖设置面板那种密排小按钮的尺寸档，否则「替换」会改变面板密度 |
| R11 余项 | `ConfirmDialog` 标题的 `text-foreground` 与它所在的 `bg-popover` 表面不匹配 | 见上「留待决定的一处 token 错配」。**已取证、未改**：补测确认 6 套主题里**只有 dark / nord** 两套这两个 token 不同色，但这两套无法在本机目视验证，属「读 CSS 变量推断出来的修改」。影响面已由 14 涨到 **16** 个确认框（本轮 +2）。不是冗余项，故不占 R 编号 |

## 本轮（增量扫描）的验证

无代码改动，仅扫描 + 只读核实。所有结论均可用脚本复现：

```sh
python3 ~/.workbuddy/skills/agent2ssh-add-operation/scripts/redundancy_scan.py /Users/yuqu/Vbercodeing/agent2ssh
```

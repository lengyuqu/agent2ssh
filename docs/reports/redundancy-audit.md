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
| P2 Rust 死代码 | ✅ 保留 + `TODO(unwired)` 标注 | 8 个 `sftp_*_core`、`remove_system_known_host`、`emit_bus`/`is_headless`/`is_cli`、`policy_risk_rules`；`save_algo_prefs` 标注与 `load_algo_prefs` 的不对称 |
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

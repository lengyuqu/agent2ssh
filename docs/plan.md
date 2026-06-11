# Agent2SSH 开发任务计划

## 当前状态（基线）

MVP 已完整实现，包含 21 个 MCP 工具、CLI、桌面 UI。详见 [README](../README.md)。

核心库 (`src-tauri/src/`) 已拆分为独立模块：
- `types.rs` — 所有共享类型
- `store.rs` — 文件持久化（hosts.json、audit.jsonl）
- `core.rs` — SSH 执行、风险评分、SFTP、ping
- `session.rs` — PTY 会话（进程内 in-memory）
- `forward.rs` — 端口转发（进程内 in-memory）
- `connection.rs` — SSH ControlMaster socket 管理、ssh_config 解析

---

## 里程碑总览

目标平台：**Windows / Linux / macOS**（Tauri 桌面 App 三端均支持）。

| 里程碑 | 主题 | 依赖 | 优先级 |
|--------|------|------|--------|
| **M1** | HTTP Daemon API | — | 高 |
| **M2** | 审批门禁（Approval Gates） | M1 | 高 |
| **M3** | 风险规则可配置化 | — | 中 |
| **M4** | Web 控制台 | M1 | 中 |

---

## M1 · HTTP Daemon API

> 目标：让 Web、移动端、其他本地进程都能连接同一个本地核心，并共享 session/forward 状态。

### M1-1 · Daemon API 规范设计

**内容**
- 确定监听地址：`127.0.0.1:7722`（可通过 `~/.agent2ssh/config.toml` 覆盖）
- 认证：启动时生成 bearer token 写入 `~/.agent2ssh/daemon.token`，每次请求 Header 带 `Authorization: Bearer <token>`
- 端点列表（见下表）
- WebSocket 端点用于流式 exec 输出

**端点设计**

| 方法 | 路径 | 对应 MCP 工具 |
|------|------|---------------|
| GET | `/health` | — |
| GET | `/hosts` | ssh_list_hosts |
| POST | `/hosts` | ssh_add_host |
| DELETE | `/hosts/:name` | ssh_remove_host |
| POST | `/hosts/import` | ssh_import_config |
| POST | `/ping` | ssh_ping |
| POST | `/exec` | ssh_exec |
| POST | `/exec-multi` | ssh_exec_multi |
| WS | `/exec/stream` | ssh_exec（流式） |
| GET | `/audit` | ssh_audit |
| POST | `/sftp/upload` | ssh_sftp_upload |
| POST | `/sftp/download` | ssh_sftp_download |
| POST | `/sftp/ls` | ssh_sftp_ls |
| POST | `/sftp/stat` | ssh_sftp_stat |
| POST | `/sftp/mkdir` | ssh_sftp_mkdir |
| POST | `/sessions` | ssh_session_open |
| POST | `/sessions/:id/write` | ssh_session_write |
| GET | `/sessions/:id/read` | ssh_session_read |
| DELETE | `/sessions/:id` | ssh_session_close |
| GET | `/sessions` | ssh_session_list |
| POST | `/forwards` | ssh_forward_add |
| GET | `/forwards` | ssh_forward_list |
| DELETE | `/forwards/:id` | ssh_forward_remove |

**验收标准**
- [ ] API 规范以 OpenAPI 3.1 YAML 形式落地到 `docs/api.yaml`
- [ ] 所有端点入参/出参与现有 MCP 工具对齐

---

### M1-2 · `agent2ssh-daemon` 二进制

**内容**
- 新建 `src-tauri/src/bin/agent2ssh-daemon.rs`
- 使用 `axum` 作为 HTTP 框架（已有 tokio，无额外运行时引入）
- 静态 `OnceLock<Arc<Mutex<SessionMap>>>` 和 `ForwardMap` 迁移到 daemon 进程级别（与 session.rs/forward.rs 共享，不需改接口）

**Cargo.toml 新增依赖**
```toml
axum = "0.7"
tower = "0.4"
tower-http = { version = "0.5", features = ["cors"] }
```

**验收标准**
- [ ] `cargo run --bin agent2ssh-daemon` 正常启动，`GET /health` 返回 `{"ok":true}`
- [ ] Token 写入 `~/.agent2ssh/daemon.token`，无 token 请求返回 401
- [ ] 请求体 / 响应结构与 `docs/api.yaml` 一致

---

### M1-3 · 核心 REST 端点实现

**内容**
- 每个 handler 调用现有 `*_core()` 函数，不重复业务逻辑
- 错误统一返回 `{"error": "..."}` + 合适的 HTTP 状态码（400/404/500）

**验收标准**
- [ ] 所有非流式端点（见 M1-1 表格）通过 curl/httpie 手动测试
- [ ] 错误路径（未知 host、blocked 命令）返回正确状态码

---

### M1-4 · WebSocket 流式 Exec

**内容**
- `WS /exec/stream`：升级为 WebSocket 后，客户端发送 `ExecRequest` JSON
- 服务端逐行推送 `{"type":"stdout","data":"..."}` 和 `{"type":"stderr","data":"..."}`
- 命令结束推送 `{"type":"exit","code":0,"duration_ms":1234}`
- 需要改造 `exec_ssh_core` 或新增 `exec_ssh_stream_core`，通过 `tokio::sync::mpsc` 传递 chunk

**验收标准**
- [ ] `wscat -c ws://127.0.0.1:7722/exec/stream` 能实时收到输出
- [ ] 超时、blocked、force 校验行为与现有 exec 一致

---

### M1-5 · Daemon 生命周期管理

**内容**
- PID 文件：`~/.agent2ssh/daemon.pid`（启动写入，退出删除，启动时检测是否已运行）
- CLI 子命令扩展：`agent2ssh daemon start | stop | status | restart`
  - `start`：fork 子进程（或 `std::process::Command::new(self)` 启动 daemon）
  - `stop`：读 PID 文件发送 SIGTERM
  - `status`：检查进程存活 + `GET /health`
- macOS launchd plist 模板（`scripts/com.agent2ssh.daemon.plist`）

**验收标准**
- [ ] `agent2ssh daemon start` 后 daemon 在后台运行，`status` 正确输出
- [ ] `agent2ssh daemon stop` 干净退出（PTY 进程、forward 进程也随之终止）
- [ ] 双重启动时报错 `daemon is already running (pid=XXX)`

---

## M2 · 审批门禁（Approval Gates）

> 目标：high-risk 命令不再直接拒绝，改为挂起等待人工审批，桌面/移动端弹出确认框。
> **依赖 M1**（daemon 提供共享审批队列）。

### M2-1 · 审批请求类型与队列

**新增类型（`types.rs`）**
```rust
pub struct ApprovalRequest {
    pub id: Uuid,
    pub host: String,
    pub command: String,
    pub risk_level: RiskLevel,
    pub requested_at: DateTime<Utc>,
    pub ttl_secs: u64,      // 超时后自动拒绝，默认 120s
}

pub enum ApprovalStatus { Pending, Approved, Rejected, TimedOut }
```

**新增模块 `src-tauri/src/approval.rs`**
- `static APPROVALS: OnceLock<...>` 存储队列
- `approval_request(req) -> Uuid`：入队
- `approval_poll(id) -> ApprovalStatus`：轮询状态
- `approval_respond(id, approved: bool)`：审批人回应
- 后台任务每秒扫描超时条目并标记 `TimedOut`

**验收标准**
- [ ] 单元测试覆盖：入队 → 等待 → 批准 / 拒绝 / 超时三条路径

---

### M2-2 · Exec 流程集成审批

**内容**
- 在 `exec_ssh_core` 中，当 `risk == High && !force` 时：
  - 若 daemon 模式：入审批队列，返回错误 `{"error":"approval_required","approval_id":"..."}`
  - 若 CLI 直接模式：保持现有行为（拒绝并提示用户加 `--force`）
- MCP server 新增两个工具：

| 工具 | 说明 |
|------|------|
| `ssh_approval_list` | 列出所有 pending 审批 |
| `ssh_approval_respond` | 批准或拒绝一个审批（参数：`approval_id`, `approved: bool`） |

**验收标准**
- [ ] MCP 调用 `ssh_exec` high-risk 命令，返回 `approval_required` 错误和 ID
- [ ] 通过 `ssh_approval_respond` 批准后，命令实际执行并返回结果

---

### M2-3 · 桌面审批弹窗

**内容**
- Tauri app 通过轮询 `GET /approvals`（或 WebSocket 推送）感知新审批请求
- 收到请求时弹出系统通知 + 自定义审批窗口（`tauri::WebviewWindowBuilder`）
- 窗口展示：主机名、完整命令、风险等级、剩余倒计时
- 按钮：「批准」 / 「拒绝」，调用 `POST /approvals/:id/approve|reject`

**验收标准**
- [ ] 从 MCP 触发 high-risk exec → 桌面弹窗出现
- [ ] 点击批准后命令执行，点击拒绝后 MCP 收到拒绝错误
- [ ] TTL 超时后弹窗自动关闭，命令返回超时拒绝

---

### M2-4 · Daemon 审批端点

| 方法 | 路径 |
|------|------|
| GET | `/approvals` |
| POST | `/approvals/:id/approve` |
| POST | `/approvals/:id/reject` |

**验收标准**
- [ ] 端点与 `docs/api.yaml` 同步更新
- [ ] 无 token 访问返回 401

---

## M3 · 风险规则可配置化

> 目标：用户可自定义 blocked/high/medium 规则，无需修改代码。

### M3-1 · 风险规则配置文件

**路径**：`~/.agent2ssh/risk_rules.toml`

**格式**
```toml
[blocked]
patterns = [
  "kubectl delete namespace",
  "terraform destroy",
]

[high]
patterns = [
  "docker system prune",
  "git push --force",
]

[medium]
patterns = []
```

- 支持前缀匹配和 glob（`*`）
- 用户规则优先级高于内置规则
- `classify_risk()` 先查用户规则，再走内置逻辑

**验收标准**
- [ ] 添加自定义 blocked 规则后，对应命令被拒绝
- [ ] 修改配置文件后无需重启（每次调用时重新读取，加文件修改时间缓存）

---

### M3-2 · Per-host 风险覆盖

**内容**
- `HostProfile` 新增字段：
  ```rust
  #[serde(default)]
  pub risk_override: Option<String>, // "low" | "medium" | "high" | "blocked"
  ```
  表示该 host 上所有命令的最低风险等级（例：测试机设为 `low` 则免确认）
- `classify_risk_for_host(cmd, host)` 合并 host override 和命令规则

**验收标准**
- [ ] host 设置 `risk_override: "low"` 后，high-risk 命令无需 force 即可执行
- [ ] `--json` 输出中 `risk_level` 仍为命令自身的评分（override 不影响记录）

---

### M3-3 · `agent2ssh risk check` 命令

**内容**
- `agent2ssh risk check "<command>" [--host <alias>]`
- 输出风险等级和命中的规则说明
- 方便开发者调试自定义规则

**验收标准**
- [ ] `agent2ssh risk check "rm -rf /tmp/test"` 输出 `high`
- [ ] `agent2ssh risk check "ls -la"` 输出 `low`

---

## M4 · Web 控制台

> 目标：团队成员通过浏览器访问本地 daemon，查看审计日志、管理主机。
> **依赖 M1**。

### M4-1 · Web App 骨架

- 独立 Vite + React 项目（或复用现有 `src/`，但去掉 Tauri IPC 依赖）
- daemon 启动时静态文件由 axum 托管，访问 `http://127.0.0.1:7722/`
- 登录页：输入 `~/.agent2ssh/daemon.token` 完成认证（token 存 localStorage）

**验收标准**
- [ ] 浏览器打开 `http://127.0.0.1:7722/` 显示登录页，输入正确 token 后进入主界面

---

### M4-2 · 主机管理 UI

- 主机列表表格：名称、地址、用户、端口、jump host
- 新增 / 编辑 / 删除操作
- 「从 ~/.ssh/config 导入」按钮

**验收标准**
- [ ] 增删改通过 Web UI 操作后，`~/.agent2ssh/hosts.json` 同步更新

---

### M4-3 · 命令执行面板

- 主机下拉、命令输入、timeout 配置
- 风险等级实时预览（前端复刻 `classify_risk` 逻辑，或调用 `GET /risk/check?cmd=...`）
- 执行结果通过 WebSocket 流式显示
- force 开关：high-risk 命令时自动展示

**验收标准**
- [ ] 执行 `uname -a` 能看到实时输出
- [ ] 执行 `sudo whoami` 时 UI 显示 high-risk 警告和 force 开关

---

### M4-4 · 审计日志查看器

- 分页表格：时间、主机、命令、退出码、耗时、风险等级
- 过滤：主机、风险等级、退出码、时间范围
- 导出 CSV

**验收标准**
- [ ] 过滤条件生效，分页正确
- [ ] 点击一条记录展开完整 stdout/stderr

---

### M4-5 · 会话管理 UI（可选）

- 列出所有 open session（session_id、主机、开启时间）
- 关闭 session 按钮

---

## 开发规范

### 分支策略

```
main          ← 稳定发布
feat/m1-daemon
feat/m1-websocket
feat/m2-approval
feat/m3-risk-config
feat/m4-web
feat/m5-mobile
```

每个任务建独立分支，完成后 PR 合入 main。

### 任务接单原则

- 每个任务最多一人负责，避免冲突
- 任务开始前在 PR/Issue 中 assign 自己
- M1 必须在 M2/M4 启动前完成；M2 必须在 M5 启动前完成

### 接口约定

- 所有新 Rust 公开函数遵循 `xxx_core()` 命名，放在对应模块
- HTTP handler 只做参数解析和错误转换，业务逻辑全在 `*_core()` 函数
- 新类型加到 `types.rs`，序列化字段名用 snake_case

### 测试要求

- 每个 `*_core()` 函数写单元测试（`src-tauri/src/` 内 `#[cfg(test)]` 块）
- HTTP 端点写集成测试（`axum::test`）
- 审批队列的三条路径（批准/拒绝/超时）必须有测试覆盖

---

## 任务状态速查

| 任务 | 状态 | 负责人 |
|------|------|--------|
| M1-1 API 规范 | 待认领 | — |
| M1-2 Daemon 二进制 | 待认领 | — |
| M1-3 REST 端点 | 待认领 | — |
| M1-4 WebSocket 流式 | 待认领 | — |
| M1-5 生命周期管理 | 待认领 | — |
| M2-1 审批队列 | 待认领 | — |
| M2-2 Exec 集成审批 | 待认领 | — |
| M2-3 桌面弹窗 | 待认领 | — |
| M2-4 Daemon 端点 | 待认领 | — |
| M3-1 规则配置文件 | 待认领 | — |
| M3-2 Per-host override | 待认领 | — |
| M3-3 risk check CLI | 待认领 | — |
| M4-1 Web App 骨架 | 待认领 | — |
| M4-2 主机管理 UI | 待认领 | — |
| M4-3 执行面板 | 待认领 | — |
| M4-4 审计日志 | 待认领 | — |

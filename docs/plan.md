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
| M1-1 API 规范 | ✅ 已完成 | — |
| M1-2 Daemon 二进制 | ✅ 已完成 | — |
| M1-3 REST 端点 | ✅ 已完成 | — |
| M1-4 WebSocket 流式 | ✅ 已完成（有缺陷，见 Fix-2/Fix-3） | — |
| M1-5 生命周期管理 | ✅ 已完成 | — |
| M2-1 审批队列 | ✅ 已完成 | — |
| M2-2 Exec 集成审批 | ✅ 已完成 | — |
| M2-3 桌面弹窗 | ⚠️ 组件已写未接入（见 Fix-1） | — |
| M2-4 Daemon 端点 | ✅ 已完成 | — |
| M3-1 规则配置文件 | ✅ 已完成 | — |
| M3-2 Per-host override | ✅ 已完成 | — |
| M3-3 risk check CLI | ✅ 已完成 | — |
| M4-1 Web App 骨架 | ✅ 已完成 | — |
| M4-2 主机管理 UI | ✅ 已完成 | — |
| M4-3 执行面板 | ✅ 已完成 | — |
| M4-4 审计日志 | ✅ 已完成 | — |

---

## 遗留缺陷修复

> 以下三项是在代码审查中发现的遗留问题，优先级高于新里程碑，建议最先认领。

### Fix-1 · 桌面审批弹窗未接入（M2-3 收尾）

**问题**
`src/components/ApprovalDialog.tsx` 已实现完整组件，但 `src/App.tsx` 既未导入该组件，也没有轮询待审批任务的逻辑，导致 Tauri 桌面 App 无法弹出审批窗口。

**需要做的事**
- 在 `App.tsx` 中每 2 秒轮询一次 `GET http://127.0.0.1:7722/approvals`（读取 `~/.agent2ssh/daemon.token` 作 Bearer token）
- 当存在 `status === "pending"` 的审批请求时，渲染 `ApprovalDialog`
- 用户点击"批准"调用 `POST /approvals/:id/approve`，点击"拒绝"调用 `POST /approvals/:id/reject`
- 注意：daemon 未运行时轮询应静默失败，不展示错误

**涉及文件**
- `src/App.tsx`（主改动）
- `src/components/ApprovalDialog.tsx`（只读，已实现好，直接使用）
- `src/api.ts`（可能需要补 approvalApprove / approvalReject 方法）

**验收标准**
- [ ] MCP 触发 high-risk exec → Tauri 桌面弹出审批窗口
- [ ] 点击批准 → 命令执行，MCP 收到结果
- [ ] 点击拒绝 → MCP 收到 403 错误
- [ ] TTL 超时后弹窗自动消失

---

### Fix-2 · `exec_stream` WebSocket 无鉴权（安全漏洞）

**问题**
`src-tauri/src/bin/agent2ssh-daemon.rs:269` 的 `exec_stream` handler 声明为：

```rust
async fn exec_stream(State(_s): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
```

`_s` 以下划线忽略了 state，从未调用 `check_auth()`。任何人连接 `ws://127.0.0.1:7722/exec/stream` 即可不带 token 执行命令。

**需要做的事**
- 将 `State(_s)` 改为 `State(s)`
- 在 `ws.on_upgrade` 回调之前检查 HTTP 握手阶段的 Authorization header；axum WebSocket 升级前可通过 `HeaderMap` 提取，示例：

```rust
async fn exec_stream(
    State(s): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&s, &headers) {
        return e.into_response();
    }
    ws.on_upgrade(|socket| async move { /* ... */ })
}
```

**涉及文件**
- `src-tauri/src/bin/agent2ssh-daemon.rs:269`

**验收标准**
- [ ] 无 token 的 WebSocket 连接收到 HTTP 401 响应
- [ ] 携带正确 token 的连接正常工作

---

### Fix-3 · `exec_stream` 只流 stdout，stderr 丢失

**问题**
`exec_stream` 中仅对 `child.stdout` 进行了流式读取，`child.stderr` 未被处理，SSH 命令的所有错误输出对客户端不可见。

**需要做的事**
- 同时消费 stdout 和 stderr，分别推送 `{"type":"stdout","data":"..."}` 和 `{"type":"stderr","data":"..."}`
- 两个流需要并发读取（使用 `tokio::select!` 或分两个 task），避免一方堵塞另一方
- 注意：WebSocket `send` 不是 `Clone` 的，需要用 `Arc<Mutex<>>` 或将 socket 传给 select! 循环

**涉及文件**
- `src-tauri/src/bin/agent2ssh-daemon.rs:267–329`

**验收标准**
- [ ] 执行会向 stderr 输出内容的命令（如 `ls /nonexistent`），客户端收到 `{"type":"stderr",...}` 帧
- [ ] stdout 和 stderr 能并发到达，不互相阻塞

---

## 后续里程碑

### M5 · 测试覆盖

> 目标：关键路径有自动化测试，防止重构破坏核心逻辑。

| 子任务 | 内容 | 验收标准 |
|--------|------|----------|
| M5-1 | `approval.rs` 单元测试：批准/拒绝/超时三条路径 | `cargo test` 全绿 |
| M5-2 | `risk_config.rs` 单元测试：glob 匹配、用户规则优先级 | `cargo test` 全绿 |
| M5-3 | `core.rs` 单元测试：`classify_risk`、per-host override 逻辑 | `cargo test` 全绿 |
| M5-4 | Daemon HTTP 集成测试（axum::test）：健康检查、鉴权、exec 拦截 | `cargo test` 全绿 |

---

### M6 · 打包发布

> 目标：用户可通过常规渠道安装，无需手动编译。

| 子任务 | 内容 |
|--------|------|
| M6-1 | Tauri bundle 配置：macOS .dmg、Linux AppImage、Windows .msi |
| M6-2 | CI 流水线（GitHub Actions）：PR 触发构建 + 测试，tag 触发发布到 GitHub Releases |
| M6-3 | Homebrew formula（macOS tap）：`brew install agent2ssh` |
| M6-4 | README 安装文档更新 |

**验收标准**
- [ ] 在全新 macOS / Linux / Windows 机器上按文档安装后能正常使用
- [ ] GitHub Releases 页面包含三平台产物

---

### M7 · SSH 密钥管理

> 目标：在 UI 中管理 SSH 密钥对，无需手动操作 `~/.ssh/`。

| 子任务 | 内容 |
|--------|------|
| M7-1 | 生成 Ed25519 密钥对，存入 `~/.agent2ssh/keys/` |
| M7-2 | 导入现有私钥文件 |
| M7-3 | Host profile 关联密钥（下拉选择） |
| M7-4 | 显示公钥，一键复制到剪贴板 |

**涉及文件（新增）**
- `src-tauri/src/keys.rs`
- `src/components/KeysPanel.tsx`

---

### M8 · 主机分组与批量操作

> 目标：对大量主机按标签分组，exec-multi 支持按组执行。

| 子任务 | 内容 |
|--------|------|
| M8-1 | `HostProfile` 新增 `tags: Vec<String>` 字段 |
| M8-2 | `ssh_exec_multi` 支持 `tags` 参数（替代手动枚举 hosts） |
| M8-3 | Web 控制台与桌面 UI 展示标签，支持按标签过滤 |

---

### M9 · MCP 审批工具

> 目标：AI agent 可通过 MCP 工具自助查询和响应审批，无需人工介入（适合自动化流水线）。

| 子任务 | 内容 |
|--------|------|
| M9-1 | MCP server 新增 `ssh_approval_list` 工具（列出 pending 审批） |
| M9-2 | MCP server 新增 `ssh_approval_respond` 工具（批准/拒绝，参数：`approval_id`, `approved: bool`） |
| M9-3 | `docs/skills.md` 更新工具列表（MCP 工具数从 21 升至 23） |

---

## 任务状态速查（续）

| 任务 | 状态 | 负责人 |
|------|------|--------|
| Fix-1 桌面审批弹窗接入 | 待认领 | — |
| Fix-2 exec_stream 鉴权 | 待认领 | — |
| Fix-3 exec_stream stderr | 待认领 | — |
| M5-1 approval 单元测试 | 待认领 | — |
| M5-2 risk_config 单元测试 | 待认领 | — |
| M5-3 core 单元测试 | 待认领 | — |
| M5-4 Daemon 集成测试 | 待认领 | — |
| M6-1 Tauri bundle | 待认领 | — |
| M6-2 CI/CD 流水线 | 待认领 | — |
| M6-3 Homebrew formula | 待认领 | — |
| M7-1 密钥生成 | 待认领 | — |
| M7-2 密钥导入 | 待认领 | — |
| M7-3 Host 关联密钥 | 待认领 | — |
| M8-1 Host tags 字段 | 待认领 | — |
| M8-2 exec-multi 按 tag | 待认领 | — |
| M9-1 MCP approval_list | 待认领 | — |
| M9-2 MCP approval_respond | 待认领 | — |

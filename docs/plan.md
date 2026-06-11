# Agent2SSH 开发任务计划

## 当前状态（基线）

MVP 已完整实现，包含 24 个 MCP 工具、CLI、桌面 UI、HTTP Daemon、Web Console。详见 [README](../README.md)。

核心库 (`src-tauri/src/`) 已拆分为独立模块：
- `types.rs` — 所有共享类型
- `store.rs` — 文件持久化（hosts.json、audit.jsonl）
- `core.rs` — SSH 执行、风险评分、SFTP、ping
- `session.rs` — PTY 会话（进程内 in-memory）
- `forward.rs` — 端口转发（进程内 in-memory）
- `connection.rs` — SSH ControlMaster socket 管理、ssh_config 解析
- `approval.rs` — 审批请求队列（进程内 in-memory）
- `risk_config.rs` — 用户自定义风险规则（risk_rules.toml）
- `keys.rs` — SSH 密钥管理（生成/导入/删除）

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
| M1-4 WebSocket 流式 | ✅ 已完成（Fix-2/3 已修复鉴权+stderr） | — |
| M1-5 生命周期管理 | ✅ 已完成 | — |
| M2-1 审批队列 | ✅ 已完成 | — |
| M2-2 Exec 集成审批 | ✅ 已完成 | — |
| M2-3 桌面弹窗 | ✅ 已完成（Fix-1 接入轮询+弹窗） | — |
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

### M8-3 详细说明 · Tags UI

**当前状态** ✅ 已完成
- `HostProfile.tags` 字段已实现（`src-tauri/src/types.rs:40`）
- `exec_multi_core` 支持按 tag 过滤（`src-tauri/src/core.rs:311-322`）
- 桌面 UI：`AddHostForm.tsx` 新增 tags 输入框，`HostList.tsx` 展示 tag 徽章，`MultiExecPanel.tsx` 支持按 tag 执行
- Web 控制台：`console.html` 新增 Tags 列与 tag 徽章展示，Add Host 表单支持 tags

**桌面 UI 需要做的事**

1. `AddHostForm.tsx` — 新增 tags 输入框（逗号分隔，如 `production, web`），保存时解析为 `string[]`
2. `HostList.tsx` — 主机条目下展示 tag 徽章（小色块）
3. `MultiExecPanel.tsx` — 新增"按标签执行"模式：输入 tag → 自动展示匹配主机 → 执行
4. `src/types.ts` — 确认 `HostProfile.tags` 字段已声明（如缺失需补加）

**Web 控制台需要做的事**

1. "Add Host"表单补 tags 输入行
2. 主机表格新增 Tags 列（多个 tag 用 `<span class="badge">` 展示）
3. "Execute"面板 MultiExec 区域补 "By tag" 单选，选中后显示 tag 输入框

**涉及文件**
- `src/components/AddHostForm.tsx`
- `src/components/HostList.tsx`
- `src/components/MultiExecPanel.tsx`
- `src/types.ts`
- `src-tauri/web/console.html`

**验收标准**
- [x] 在 AddHostForm 填写 tags 并保存，`hosts.json` 中该 host 包含正确的 tags 数组
- [x] HostList 展示 tag 徽章
- [x] MultiExecPanel 输入 tag "production" → 只向该 tag 下的主机发送命令
- [x] Web 控制台主机表格显示 Tags 列，执行面板支持 "By tag" 模式

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
| Fix-1 桌面审批弹窗接入 | ✅ 已完成 | — |
| Fix-2 exec_stream 鉴权 | ✅ 已完成 | — |
| Fix-3 exec_stream stderr | ✅ 已完成 | — |
| M5-1 approval 单元测试 | ✅ 已完成 | — |
| M5-2 risk_config 单元测试 | ✅ 已完成 | — |
| M5-3 core 单元测试 | ✅ 已完成 | — |
| M5-4 Daemon 集成测试 | ✅ 已完成（24 个 axum HTTP 测试全绿） | — |
| M6-1 Tauri bundle | ⚠️ 待认领（Tauri bundle 配置未完成） | — |
| M6-2 CI/CD 流水线 | ✅ 已完成 | — |
| M6-3 Homebrew formula | ✅ 已完成 | — |
| M7-1 密钥生成 | ✅ 已完成 | — |
| M7-2 密钥导入 | ✅ 已完成 | — |
| M7-3 Host 关联密钥 | ✅ 已完成（密钥下拉选择 + 手动路径输入） | — |
| M7-4 公钥展示 | ✅ 已完成 | — |
| M8-1 Host tags 字段 | ✅ 已完成 | — |
| M8-2 exec-multi 按 tag | ✅ 已完成 | — |
| M8-3 Tags UI（桌面+Web） | ✅ 已完成（AddHostForm tags 输入 + HostList 徽章 + Web Console tags 列） | — |
| M9-1 MCP approval_list | ✅ 已完成 | — |
| M9-2 MCP approval_respond | ✅ 已完成 | — |
| M9-3 MCP ssh_risk_check | ✅ 已完成 | — |

---

## M5-4 详细说明 · Daemon 集成测试 ✅ 已完成

**当前状态** ✅ 已完成
已在 `src-tauri/tests/daemon_integration.rs` 中实现 24 个 axum HTTP 集成测试，使用 `tower::ServiceExt::oneshot()` 测试所有端点。daemon `main()` 已重构为 `daemon_app()` 工厂函数。

**需要做的事**

在 `src-tauri/src/bin/agent2ssh-daemon.rs` 末尾（或新建 `src-tauri/tests/daemon_integration.rs`）添加 `#[cfg(test)]` 集成测试，使用 `axum::test` 辅助工具：

```rust
// 示例
#[tokio::test]
async fn test_health_no_auth() {
    let app = build_app("test-token");
    let resp = app.oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_exec_no_token_returns_401() {
    let app = build_app("test-token");
    let resp = app.oneshot(Request::builder().method("POST").uri("/exec")
        .body(Body::from(r#"{"host":"h","command":"ls"}"#)).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

**需要重构 `main()` 为 `build_app(token: &str) -> Router`**，以便测试可以构造 app 实例。

**验收标准** ✅ 全部通过
- [x] `cargo test` 包含以下集成测试并全绿：
  - `GET /health` 返回 200
  - 无 token 请求返回 401
  - `POST /exec` blocked 命令返回 400
  - `POST /exec` high-risk 命令（无 force）触发审批流程，返回相应状态
  - `GET /approvals` 返回正确列表
  - `POST /approvals/:id/approve` 正确更新状态

---

## M6-1 详细说明 · Tauri Bundle（待补）

**当前状态**
CI 已配置（`.github/workflows/ci.yml`），但 `src-tauri/tauri.conf.json` 中 bundle 产物尚未配置，无法通过 `tauri build` 生成可分发安装包。

**需要做的事**

1. `src-tauri/tauri.conf.json` 中补全 `bundle` 配置：

```json
"bundle": {
  "active": true,
  "targets": ["dmg", "app"],        // macOS
  "identifier": "com.agent2ssh.app",
  "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"],
  "resources": [],
  "externalBin": ["binaries/agent2ssh", "binaries/agent2ssh-daemon", "binaries/agent2ssh-mcp"]
}
```

2. 图标文件：在 `src-tauri/icons/` 目录下放置标准尺寸图标（可用 `tauri icon` 命令从单张 1024×1024 PNG 批量生成）

3. sidecar 配置：`agent2ssh`、`agent2ssh-daemon`、`agent2ssh-mcp` 三个二进制需要作为 sidecar 打包进安装包，更新 `tauri.conf.json` 的 `externalBin`

4. GitHub Actions 补充 `tauri build` 步骤，tag push 时上传产物到 Releases

**验收标准**
- [ ] `npm run tauri build` 在 macOS 上生成 `.dmg` 文件
- [ ] 安装后三个命令行工具（`agent2ssh`、`agent2ssh-daemon`、`agent2ssh-mcp`）可在系统 PATH 中找到
- [ ] CI tag 触发时自动发布到 GitHub Releases，包含 macOS `.dmg`、Linux `.AppImage`、Windows `.msi`

---

## M10 · SSH 连接池（ControlMaster Persistence）

> 目标：利用 OpenSSH ControlMaster 复用已认证连接，避免每次 exec 重新握手，将重复命令延迟从 ~500ms 降至 ~10ms。
>
> `connection.rs` 已有 ControlMaster socket 管理基础，本里程碑在此之上完成自动化管理。

### M10-1 · 自动建立 ControlMaster

**内容**
- 首次向某 host 执行 exec 时，检查 `~/.agent2ssh/cm/<host>_<port>_<user>` socket 是否存在
- 若不存在，后台 spawn `ssh -N -M -o ControlMaster=yes -o ControlPath=<socket> <target>` 进程
- 后续所有 exec/sftp/session 命令携带 `-o ControlMaster=no -o ControlPath=<socket>`
- ControlMaster 进程在 daemon 退出时一并关闭

**涉及文件**
- `src-tauri/src/connection.rs`（主改动）
- `src-tauri/src/core.rs`（exec_ssh_core 调用时传入 socket 路径）

**验收标准**
- [ ] 第二次向同一 host 执行 exec 耗时比首次减少 ≥ 200ms（有网络延迟的真实主机上测试）
- [ ] 执行 `ssh_list_hosts` 能看到哪些 host 有活跃 ControlMaster

---

### M10-2 · 连接状态 UI

**内容**
- MCP 新增工具 `ssh_connection_status`：返回每个 host 的连接状态（connected / disconnected）
- 桌面 UI `HostList.tsx` 在主机名旁显示绿/灰点
- Web 控制台主机表格新增 Status 列

**验收标准**
- [ ] ControlMaster 建立后 UI 显示绿点
- [ ] 手动关闭 ControlMaster 进程后 UI 更新为灰点（轮询间隔 ≤ 5s）

---

### M10-3 · 手动连接管理

**内容**
- MCP 新增 `ssh_connect` / `ssh_disconnect` 工具（参数：`host` alias）
- 桌面 UI 主机条目增加「连接」/「断开」按钮
- Web 控制台同步增加按钮

**验收标准**
- [ ] `ssh_connect` 预建立连接，后续 exec 无握手延迟
- [ ] `ssh_disconnect` 正确关闭 ControlMaster 进程和 socket 文件

---

## M11 · 通知与 Webhook

> 目标：审批请求、高风险命令拦截、exec 完成等关键事件，自动推送到外部系统（Slack、自定义 webhook）。

### M11-1 · Webhook 配置

**路径**：`~/.agent2ssh/config.toml`

**格式**
```toml
[webhook]
url = "https://hooks.slack.com/services/T.../B.../..."
events = ["approval_required", "exec_blocked", "exec_completed"]  # 可选，默认只推 approval_required
secret = ""  # 若设置，在 HTTP header X-Agent2SSH-Signature 里附 HMAC-SHA256 签名
```

**涉及文件**
- `src-tauri/src/bin/agent2ssh-daemon.rs`（读取配置、发送）
- 新增 `src-tauri/src/notify.rs`（封装 HTTP POST 逻辑）

**验收标准**
- [ ] 配置 webhook 后，审批入队时 daemon 在 1s 内 POST 到目标 URL
- [ ] Payload 包含 `{"event":"approval_required","host":"...","command":"...","approval_id":"..."}`
- [ ] 设置 secret 后，签名校验通过

---

### M11-2 · Slack 集成模板

**内容**
- 当 webhook url 为 Slack Incoming Webhook 时，自动格式化为 Slack Block Kit 消息（包含 Approve/Reject 按钮）
- 自动检测：url 包含 `hooks.slack.com` 时启用 Slack 模板

**验收标准**
- [ ] Slack channel 收到格式化消息，包含主机名、命令、风险等级
- [ ] 按钮点击（通过 Slack 交互端点）触发审批（需额外配置公网可达的 callback URL，文档说明）

---

## M12 · 命令模板（Playbooks）

> 目标：预定义可复用的命令序列，AI agent 和用户可按名称调用，减少重复输入并降低人为出错风险。

### M12-1 · Playbook 配置文件

**路径**：`~/.agent2ssh/playbooks.toml`

**格式**
```toml
[[playbooks]]
name = "deploy-web"
description = "拉取最新代码并重启 web 服务"
steps = [
  "cd /opt/app && git pull",
  "systemctl restart nginx",
]
tags = ["production", "web"]     # 只允许在带这些 tag 的 host 上执行（可选）
risk_override = "high"           # 整体风险等级（可选，默认按每步命令评估）

[[playbooks]]
name = "disk-check"
description = "检查磁盘使用率"
steps = ["df -h", "du -sh /var/log/*"]
```

### M12-2 · MCP 工具

| 工具 | 说明 |
|------|------|
| `ssh_playbook_list` | 列出所有 playbook（name、description、step 数量、tags） |
| `ssh_playbook_run` | 在指定 host 上按顺序执行 playbook 各步骤，返回每步 `ExecResult` |

**验收标准**
- [ ] `ssh_playbook_run` 每步失败时中止后续步骤，返回已执行步骤结果和失败原因
- [ ] 整体风险等级按 `risk_override` 或各步骤中最高级计算

---

### M12-3 · Web 控制台 Playbooks 页签

**内容**
- 新增 "Playbooks" tab，列出所有 playbook
- 点击 "Run" 选择目标主机，实时展示每步输出（WebSocket 逐步流式）
- 允许查看 playbook 定义（只读）

**验收标准**
- [ ] Web 控制台能完整运行一个多步 playbook 并展示每步输出

---

## M13 · 远程 Daemon 支持

> 目标：不仅支持 localhost，还能连接运行在其他机器上的 agent2ssh-daemon（团队共享场景、CI 服务器场景）。

### M13-1 · 远程 Daemon 配置

**路径**：`~/.agent2ssh/remotes.toml`

**格式**
```toml
[[remotes]]
alias = "ci-server"
url = "http://192.168.1.100:7722"
token = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"

[[remotes]]
alias = "prod-gateway"
url = "https://agent2ssh.internal.example.com"
token_env = "AGENT2SSH_PROD_TOKEN"   # 从环境变量读取 token
```

**验收标准**
- [ ] 配置文件解析正确，`token_env` 优先于 `token` 字段

---

### M13-2 · CLI 与 MCP 路由

**内容**
- CLI 所有子命令支持 `--daemon <alias>` 参数，将请求路由到对应远程 daemon
- MCP server 新增工具 `ssh_list_daemons`（列出已配置的 daemon alias 和连接状态）
- MCP 工具新增可选参数 `daemon_alias`，未指定时默认 localhost

**验收标准**
- [ ] `agent2ssh --daemon ci-server exec --host web1 "uptime"` 通过远程 daemon 执行命令
- [ ] MCP `ssh_exec` 携带 `daemon_alias: "ci-server"` 路由正确

---

### M13-3 · Web 控制台 Daemon 切换器

**内容**
- 顶部 header 增加 daemon 下拉选择器（localhost + 已配置 remote）
- 切换时 token 和 url 自动更新，页面数据重新加载

**验收标准**
- [ ] 切换到远程 daemon 后，所有 API 调用路由到新 url
- [ ] 远程 daemon 不可达时显示错误提示，不影响本地 daemon 使用

---

## 里程碑总览（更新）

| 里程碑 | 主题 | 依赖 | 优先级 | 状态 |
|--------|------|------|--------|------|
| M1 | HTTP Daemon API | — | 高 | ✅ 完成 |
| M2 | 审批门禁 | M1 | 高 | ✅ 完成 |
| M3 | 风险规则可配置化 | — | 中 | ✅ 完成 |
| M4 | Web 控制台 | M1 | 中 | ✅ 完成 |
| M5 | 测试覆盖 | — | 高 | ✅ 完成（20 单元 + 24 集成） |
| M6 | 打包发布 | M5 | 高 | ⚠️ M6-1 待完成 |
| M7 | SSH 密钥管理 | — | 中 | ✅ 完成 |
| M8 | 主机分组与批量操作 | — | 中 | ✅ 完成 |
| M9 | MCP 审批工具 | M2 | 中 | ✅ 完成 |
| **M10** | **SSH 连接池** | — | 中 | 待认领 |
| **M11** | **通知与 Webhook** | M2 | 低 | 待认领 |
| **M12** | **命令模板（Playbooks）** | — | 低 | 待认领 |
| **M13** | **远程 Daemon** | M1 | 低 | 待认领 |

## 任务状态速查（M10–M13）

| 任务 | 状态 | 负责人 |
|------|------|--------|
| M8-3 Tags UI（桌面+Web） | ✅ 已完成 | — |
| M5-4 Daemon 集成测试 | ✅ 已完成（24 个 axum HTTP 测试全绿） | — |
| M6-1 Tauri bundle | 待认领 | — |
| M10-1 ControlMaster 自动建立 | 待认领 | — |
| M10-2 连接状态 UI | 待认领 | — |
| M10-3 手动连接管理 | 待认领 | — |
| M11-1 Webhook 配置与发送 | 待认领 | — |
| M11-2 Slack 集成模板 | 待认领 | — |
| M12-1 Playbook 配置文件 | 待认领 | — |
| M12-2 MCP Playbook 工具 | 待认领 | — |
| M12-3 Web 控制台 Playbooks 页签 | 待认领 | — |
| M13-1 远程 Daemon 配置 | 待认领 | — |
| M13-2 CLI + MCP 路由 | 待认领 | — |
| M13-3 Web 控制台 Daemon 切换器 | 待认领 | — |

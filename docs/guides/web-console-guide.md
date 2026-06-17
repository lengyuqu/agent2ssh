# Web 控制台指南

Agent2SSH Web 控制台是一个内嵌在 Daemon 中的浏览器界面，提供可视化的 SSH 主机管理、命令执行、审计日志查看、审批管理等功能。

---

## 访问与认证

### 启动守护进程

Web 控制台由 Daemon 提供服务，需要先启动守护进程：

```bash
agent2ssh daemon start
```

### 打开控制台

在浏览器中访问：

```
http://127.0.0.1:7722/console
```

或在终端中执行：

```bash
open http://127.0.0.1:7722/console
```

也可以从桌面 App 打开：

1. 点击右上角 **Settings**。
2. 在 **Daemon console** 区域点击 **Open Web Console**。
3. 如需在其他浏览器或终端中使用，点击 **Copy console URL** 复制地址。

桌面设置菜单显示当前 daemon console URL，默认是 `http://127.0.0.1:7722/console`。如果本地 daemon 不可达，桌面端的 execution gate 会显示为不可用；启动 daemon 后可在设置菜单点击 **Refresh gate status** 重新检查。

### 认证连接

控制台顶部 Header 区域包含认证控件：

1. **Token 输入框** -- 输入 Bearer Token（从 `~/.agent2ssh/daemon.token` 获取）：
   ```bash
   cat ~/.agent2ssh/daemon.token
   ```
2. **Connect 按钮** -- 点击后验证 Token 并建立连接
3. **健康状态指示灯** -- 绿色圆点表示已连接，红色表示连接失败

获取 Token 的快捷方式：

```bash
# 复制 Token 到剪贴板（macOS）
cat ~/.agent2ssh/daemon.token | pbcopy
```

---

## Header 区域

Header 横跨页面顶部，包含以下元素：

### 标题

显示 "Agent2SSH Console"。

### Daemon 切换器

位于标题右侧的下拉选择器，用于在本地和远程守护进程之间切换：

- **localhost** -- 本地守护进程（默认）
- **远程别名** -- 在 `~/.agent2ssh/remotes.toml` 中配置的远程守护进程

切换时：

1. 下拉列表显示所有已配置的守护进程别名
2. 选中后自动更新连接 URL
3. 状态指示灯显示目标守护进程的连通性（绿色 = 已连接，红色 = 不可达）
4. URL 文本显示当前连接的目标地址

切换守护进程后，所有 Tab 中的操作将路由到选中的守护进程。

---

## Tab 概览

控制台包含 6 个标签页：

| Tab | 功能 |
|-----|------|
| **Hosts** | 主机管理：查看、添加、删除、导入、Ping |
| **Execute** | 命令执行：单主机执行、风险检查 |
| **Audit** | 审计日志：查询和过滤执行历史 |
| **Approvals** | 审批队列：查看和处理高风险命令审批 |
| **Playbooks** | Playbook：查看和执行预定义的命令序列 |
| **Settings** | 设置：Webhook 通知配置 |

---

## Hosts Tab（主机管理）

Hosts 是默认激活的标签页，分为两个区域。

### 已配置主机列表

顶部工具栏按钮：

| 按钮 | 功能 |
|------|------|
| Refresh | 刷新主机列表 |
| Import SSH Config | 从 `~/.ssh/config` 导入主机 |
| Ping All | 检测所有主机的连通性 |
| Refresh Connections | 刷新 ControlMaster 连接状态 |

主机表格列：

| 列 | 说明 |
|----|------|
| Status | ControlMaster 连接状态（已连接/未连接） |
| Name | 主机别名 |
| Host | 主机地址 |
| User | SSH 用户名 |
| Port | SSH 端口 |
| Jump Host | 跳板机别名 |
| Risk Override | 风险覆盖等级 |
| Tags | 标签列表 |
| Ping | 延迟（毫秒）或不可达 |
| Actions | 操作按钮（删除） |

### 添加主机表单

表单字段：

| 字段 | 必填 | 说明 |
|------|------|------|
| Name | 是 | 主机别名 |
| Host | 是 | 主机地址 |
| User | 否 | SSH 用户名 |
| Port | 否 | SSH 端口（默认 22） |
| Key Path | 否 | SSH 私钥路径 |
| Jump Host | 否 | 跳板机别名 |
| Risk Override | 否 | 风险覆盖（None/Low/Medium/High/Blocked） |
| Tags | 否 | 标签（逗号分隔） |

填写完毕后点击 **Add Host** 按钮添加。

---

## Execute Tab（命令执行）

### 执行表单

| 控件 | 说明 |
|------|------|
| Host | 下拉选择目标主机 |
| Command | 输入远程命令 |
| Timeout (s) | 超时秒数，默认 60 |
| Force (high-risk) | 勾选后允许执行高风险命令 |
| Pipe stdin | 勾选后展开 stdin 数据输入框 |
| Check Risk | 检查命令风险等级（不执行） |
| Run | 执行命令 |

### 风险检查

点击 **Check Risk** 按钮可以在不执行命令的情况下查看风险等级。返回结果包含：

- 风险等级（low/medium/high/blocked）
- 是否匹配了用户自定义规则

### 命令输出

执行结果展示在 Output 区域，包含：

- 退出码（exit code）
- 执行耗时（duration_ms）
- stdout 输出
- stderr 输出
- 风险等级标记

### Stdin 传递

勾选 **Pipe stdin** 复选框后，会展开一个文本框用于输入要传递到远程命令 stdin 的数据。

---

## Audit Tab（审计日志）

### 过滤条件

| 控件 | 说明 |
|------|------|
| Host | 下拉选择主机过滤（All 表示不过滤） |
| Risk Level | 下拉选择风险等级过滤（All/Low/Medium/High/Blocked） |
| Limit | 返回条数上限，默认 20 |
| Refresh | 刷新查询 |

### 审计表格

| 列 | 说明 |
|----|------|
| Timestamp | 执行时间（ISO-8601 格式） |
| Host | 主机别名 |
| Command | 执行的命令 |
| Exit Code | 命令退出码 |
| Duration | 执行耗时 |
| Risk | 风险等级标签 |

审计记录按时间倒序排列（最新在前）。

---

## Approvals Tab（审批队列）

### 待审批列表

显示所有待处理的高风险命令审批请求。页面自动定时刷新审批状态。

每个审批请求显示为一张卡片，包含：

| 信息 | 说明 |
|------|------|
| Host | 目标主机 |
| Command | 待执行的命令 |
| Risk Level | 风险等级 |
| Requested At | 请求时间 |
| TTL | 剩余超时时间 |
| Status | 当前状态（pending/approved/rejected/timed_out） |

### 操作按钮

每张卡片包含两个操作按钮：

- **Approve** (绿色) -- 批准执行，命令将自动运行
- **Reject** (红色) -- 拒绝执行，返回 403 错误

### 超时行为

审批请求默认超时 300 秒（5 分钟）。超时后状态自动变为 `timed_out`，无法再批准或拒绝。

---

## Playbooks Tab（Playbook 执行）

### Playbook 列表

显示所有在 `~/.agent2ssh/playbooks.toml` 中定义的 Playbook。

| 列 | 说明 |
|----|------|
| Name | Playbook 名称 |
| Description | 描述 |
| Steps | 步骤数量 |
| Tags | 标签列表 |
| Risk Override | 风险覆盖等级 |
| Actions | Run 按钮 |

### 执行 Playbook

点击 Playbook 行的 **Run** 按钮后，展开执行表单：

| 控件 | 说明 |
|------|------|
| Target Host | 下拉选择目标主机 |
| Force (high-risk) | 勾选后允许执行高风险步骤 |
| Run | 执行 Playbook |
| Cancel | 取消 |

### 执行结果

执行完成后显示结果区域：

- 每个步骤的执行状态、stdout/stderr 输出、退出码、耗时
- 总体状态（success/failure）和总耗时
- 步骤失败时执行停止，显示已完成的步骤结果

---

## Settings Tab（Webhook 设置）

### Webhook 通知配置

用于配置 Webhook，在特定事件发生时向外部服务发送通知。

| 控件 | 说明 |
|------|------|
| Webhook URL | Webhook 接收端点（支持 Slack、Discord 等） |
| Events | 事件订阅复选框 |
| HMAC-SHA256 Secret | 可选，用于签名验证 |
| Save | 保存配置 |
| Reload | 重新加载当前配置 |

### 支持的事件

| 复选框 | 事件名称 | 触发时机 |
|--------|----------|----------|
| Approval Required | `approval_required` | 高风险命令需要审批 |
| Command Blocked | `exec_blocked` | 命令被用户规则阻止 |
| Command Completed | `exec_completed` | 命令执行完成 |

### Slack 集成

当 URL 包含 `hooks.slack.com` 时，自动使用 Slack Block Kit 格式发送消息：

- 标题显示事件类型
- 字段显示主机、命令、风险等级、退出码
- 审批请求消息包含打开本地 Approvals 控制台的按钮；实际批准或拒绝仍通过已认证的控制台/API 完成

### HMAC 签名

配置 Secret 后，每个 Webhook 请求头包含：

```
X-Agent2SSH-Signature: sha256=<hex-encoded-hmac-sha256>
```

接收方可以使用相同的 Secret 验证签名，确保请求来源可信。

---

## 使用提示

### 快速开始

1. 启动守护进程：`agent2ssh daemon start`
2. 获取 Token：`cat ~/.agent2ssh/daemon.token`
3. 打开控制台：`http://127.0.0.1:7722/console`
4. 粘贴 Token 并点击 Connect
5. 在 Hosts Tab 添加或导入主机
6. 切换到 Execute Tab 执行命令

### 安全建议

- Token 输入框使用 `password` 类型，输入内容不可见
- 不要将 Token 分享给他人或提交到版本控制
- 生产环境中使用 HTTPS（通过反向代理实现 TLS 终止）
- 定期轮换 Token：停止守护进程 -> 删除 `daemon.token` -> 重启守护进程

### 快捷键

控制台为单页面应用，Tab 切换通过点击标签页标题完成。表单提交通过点击对应按钮触发。

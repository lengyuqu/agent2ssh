# MCP 集成快速入门

## 概述

Agent2SSH 通过 MCP (Model Context Protocol) stdio 协议暴露 **51 个工具**，使任何兼容 MCP 的 AI Agent 都能直接管理 SSH 主机、执行远程命令、传输文件、管理会话、端口转发、Playbook、审计、健康检查、指标和远程 daemon。

MCP 服务器以 `agent2ssh-mcp` 二进制运行，通过标准输入/输出与 Agent 通信，遵循 JSON-RPC 2.0 协议，无需网络端口或 HTTP 服务。

---

## Agent 配置

在你的 Agent 的 MCP 配置文件中添加 Agent2SSH 服务器：

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "agent2ssh-mcp",
      "args": []
    }
  }
}
```

如果 `agent2ssh-mcp` 不在系统 PATH 中，使用完整路径：

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "/usr/local/bin/agent2ssh-mcp",
      "args": []
    }
  }
}
```

配置完成后，Agent 将自动发现并调用所有 51 个 SSH 工具。

---

## 工具分类与示例

### 主机管理

**ssh_list_hosts** -- 列出所有已配置的 SSH 主机

```json
{
  "name": "ssh_list_hosts",
  "arguments": {}
}
```

**ssh_add_host** -- 创建或更新主机配置

```json
{
  "name": "ssh_add_host",
  "arguments": {
    "name": "web1",
    "host": "192.168.1.10",
    "user": "deploy",
    "port": 22,
    "key_path": "~/.ssh/id_ed25519",
    "jump_host": "bastion",
    "tags": ["production", "web"],
    "env": "prod",
    "role": "web",
    "owner": "platform"
  }
}
```

**ssh_remove_host** -- 删除主机配置

```json
{
  "name": "ssh_remove_host",
  "arguments": {
    "name": "web1"
  }
}
```

**ssh_import_config** -- 从 `~/.ssh/config` 导入主机

```json
{
  "name": "ssh_import_config",
  "arguments": {
    "path": "~/.ssh/config"
  }
}
```

`path` 参数可选，默认读取 `~/.ssh/config`。已存在的别名会被跳过。

---

### 命令执行

**ssh_exec** -- 在远程主机上执行命令

```json
{
  "name": "ssh_exec",
  "arguments": {
    "host": "web1",
    "command": "uptime",
    "force": false,
    "timeout_secs": 60,
    "stdin": null,
    "max_output_bytes": 4194304
  }
}
```

参数说明：

| 参数 | 必填 | 说明 |
|------|------|------|
| `host` | 是 | 主机别名 |
| `command` | 是 | 远程命令 |
| `force` | 否 | 高风险命令需要设为 `true` |
| `timeout_secs` | 否 | 超时秒数，默认 60 |
| `stdin` | 否 | 传递到远程命令 stdin 的字符串 |
| `max_output_bytes` | 否 | 输出截断阈值，默认 4 MiB |
| `daemon_alias` | 否 | 路由到远程守护进程（见下方说明） |

向 stdin 传递数据：

```json
{
  "name": "ssh_exec",
  "arguments": {
    "host": "web1",
    "command": "cat > /tmp/data.txt",
    "stdin": "hello world"
  }
}
```

**ssh_exec_multi** -- 多主机并发执行

```json
{
  "name": "ssh_exec_multi",
  "arguments": {
    "hosts": ["web1", "web2", "web3"],
    "command": "systemctl status nginx",
    "force": false,
    "timeout_secs": 30
  }
}
```

**ssh_ping** -- 检测主机连通性和延迟

```json
{
  "name": "ssh_ping",
  "arguments": {
    "hosts": ["web1", "web2"],
    "timeout_secs": 5
  }
}
```

---

### 文件传输

**ssh_sftp_upload** -- 上传文件

```json
{
  "name": "ssh_sftp_upload",
  "arguments": {
    "host": "web1",
    "local_path": "/local/app.tar.gz",
    "remote_path": "/opt/releases/app.tar.gz"
  }
}
```

**ssh_sftp_download** -- 下载文件

```json
{
  "name": "ssh_sftp_download",
  "arguments": {
    "host": "web1",
    "remote_path": "/var/log/nginx/error.log",
    "local_path": "/local/error.log"
  }
}
```

**ssh_sftp_ls** -- 列出远程目录

```json
{
  "name": "ssh_sftp_ls",
  "arguments": {
    "host": "web1",
    "path": "/etc/nginx"
  }
}
```

**ssh_sftp_stat** -- 查看远程文件信息

```json
{
  "name": "ssh_sftp_stat",
  "arguments": {
    "host": "web1",
    "path": "/etc/nginx/nginx.conf"
  }
}
```

**ssh_sftp_mkdir** -- 创建远程目录

```json
{
  "name": "ssh_sftp_mkdir",
  "arguments": {
    "host": "web1",
    "path": "/opt/app/releases/2025"
  }
}
```

---

### 会话管理

持久化 PTY 会话，适合需要多次交互的场景。

**ssh_session_open** -- 打开会话

```json
{
  "name": "ssh_session_open",
  "arguments": { "host": "web1" }
}
```

返回 `session_id`（UUID），后续操作使用此 ID。

**ssh_session_write** -- 向会话写入输入

```json
{
  "name": "ssh_session_write",
  "arguments": {
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "input": "ls -la\n"
  }
}
```

**ssh_session_read** -- 读取会话输出

```json
{
  "name": "ssh_session_read",
  "arguments": {
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "timeout_ms": 2000
  }
}
```

**ssh_session_list** -- 列出所有会话

```json
{
  "name": "ssh_session_list",
  "arguments": {}
}
```

**ssh_session_close** -- 关闭会话

```json
{
  "name": "ssh_session_close",
  "arguments": {
    "session_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

---

### 端口转发

**ssh_forward_add** -- 启动端口转发隧道

```json
{
  "name": "ssh_forward_add",
  "arguments": {
    "host": "web1",
    "direction": "local",
    "bind_port": 8080,
    "target_host": "192.168.1.100",
    "target_port": 80
  }
}
```

**ssh_forward_list** -- 列出活跃的转发隧道

```json
{
  "name": "ssh_forward_list",
  "arguments": {}
}
```

**ssh_forward_remove** -- 停止转发隧道

```json
{
  "name": "ssh_forward_remove",
  "arguments": {
    "forward_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

---

### 安全与审批

**ssh_risk_check** -- 检查命令风险等级（不执行命令）

```json
{
  "name": "ssh_risk_check",
  "arguments": {
    "command": "sudo rm -rf /tmp/cache",
    "host": "web1"
  }
}
```

返回风险等级和是否匹配了用户自定义规则：

```json
{
  "command": "sudo rm -rf /tmp/cache",
  "risk_level": "high",
  "matched_user_rule": false
}
```

**ssh_approval_list** -- 列出所有待处理的审批请求

```json
{
  "name": "ssh_approval_list",
  "arguments": {}
}
```

**ssh_approval_respond** -- 批准或拒绝审批请求

```json
{
  "name": "ssh_approval_respond",
  "arguments": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "approved": true
  }
}
```

---

### 连接池

管理 SSH ControlMaster 连接池，复用 SSH 连接以降低延迟。

**ssh_connection_status** -- 查看所有连接状态

```json
{
  "name": "ssh_connection_status",
  "arguments": {}
}
```

**ssh_connect** -- 手动建立 ControlMaster 连接

```json
{
  "name": "ssh_connect",
  "arguments": { "host": "web1" }
}
```

**ssh_disconnect** -- 关闭 ControlMaster 连接

```json
{
  "name": "ssh_disconnect",
  "arguments": { "host": "web1" }
}
```

---

### Playbooks

**ssh_playbook_list** -- 列出所有已配置的 Playbook

```json
{
  "name": "ssh_playbook_list",
  "arguments": {}
}
```

返回 Playbook 摘要列表（名称、描述、步骤数、标签）。

**ssh_playbook_run** -- 执行 Playbook

```json
{
  "name": "ssh_playbook_run",
  "arguments": {
    "playbook": "deploy-web",
    "host": "web1",
    "force": false
  }
}
```

Playbook 在 `~/.agent2ssh/playbooks.toml` 中定义，详见 [配置指南](./configuration-guide.md)。

---

### 远程 Daemon

**ssh_list_daemons** -- 列出所有守护进程实例

```json
{
  "name": "ssh_list_daemons",
  "arguments": {}
}
```

返回本地和远程守护进程的别名、URL 和连接状态。

在 `ssh_exec` 中使用 `daemon_alias` 参数将命令路由到远程守护进程，详见下方 "远程 Daemon 路由" 一节。

---

### Webhook

**ssh_webhook_config** -- 获取或设置 Webhook 通知配置

获取当前配置：

```json
{
  "name": "ssh_webhook_config",
  "arguments": {
    "action": "get"
  }
}
```

更新配置：

```json
{
  "name": "ssh_webhook_config",
  "arguments": {
    "action": "set",
    "url": "https://hooks.slack.com/services/T.../B.../xxx",
    "events": ["approval_required", "exec_blocked", "exec_completed"],
    "secret": "my-hmac-secret"
  }
}
```

支持的事件类型：`approval_required`、`exec_blocked`、`exec_completed`。

---

## Force 与审批行为

Agent2SSH 对每条命令进行风险分级，并根据分级结果决定是否直接执行、需要审批或直接拒绝。

| 风险等级 | CLI 行为 | MCP 行为 | Daemon API 行为 |
|----------|----------|----------|-----------------|
| **low** | 直接执行 | 直接执行 | 直接执行 |
| **medium** | 直接执行 | 直接执行 | 直接执行 |
| **high** | 需要 `--force` | 需要 `force: true` | 需要 `force: true`，否则进入审批队列 |
| **blocked** | 拒绝执行 | 拒绝执行 | 拒绝执行，触发 `exec_blocked` Webhook |

**MCP 中的处理方式：** MCP 服务器不实现 Daemon 审批队列。在 MCP 层面，高风险命令必须携带 `force: true` 才能执行，否则会返回包含 `risk_level` 的错误信息。

**Daemon API 审批流程：**

1. Agent 提交高风险命令且未携带 `force: true`
2. Daemon 创建审批请求，触发 `approval_required` Webhook
3. Daemon 阻塞等待审批结果（默认超时 300 秒）
4. 管理员通过 API、Web 控制台或 MCP 工具批准或拒绝
5. 批准后自动执行命令；拒绝后返回 403 错误；超时返回 408 错误

**用户自定义规则：** 在 `~/.agent2ssh/risk_rules.toml` 中用 glob 模式定义额外规则，优先级高于内置分级。详见 [配置指南](./configuration-guide.md)。

---

## 远程 Daemon 路由

通过 `ssh_exec` 的 `daemon_alias` 参数，可以将命令路由到远程守护进程执行：

```json
{
  "name": "ssh_exec",
  "arguments": {
    "host": "web1",
    "command": "uptime",
    "daemon_alias": "ci-server"
  }
}
```

工作原理：

1. MCP 服务器在 `~/.agent2ssh/remotes.toml` 中查找 `ci-server` 的 URL 和 token
2. 将 `ExecRequest` 通过 HTTP POST 转发到远程守护进程的 `/exec` 端点
3. 远程守护进程执行命令并返回结果
4. MCP 服务器将结果返回给 Agent

`daemon_alias` 设为 `"localhost"` 或省略时，命令在本地执行。

远程守护进程配置示例（`~/.agent2ssh/remotes.toml`）：

```toml
[[remotes]]
alias = "ci-server"
url = "https://daemon.example.com:7722"
token_env = "AGENT2SSH_CI_TOKEN"
```

---

## 审计日志

**ssh_audit** -- 查询执行审计日志

```json
{
  "name": "ssh_audit",
  "arguments": {
    "limit": 50,
    "host": "web1",
    "risk_level": "high",
    "exit_code": 0,
    "since": "2025-01-01T00:00:00Z",
    "until": "2025-06-01T00:00:00Z"
  }
}
```

所有参数均可选，`limit` 默认 20。

---

## 常用工具摘录

完整 51 个工具列表以 [MCP Tools Reference](../skills.md) 为准。下表只列出最常用的基础入口。

| # | 工具名称 | 说明 |
|---|----------|------|
| 1 | `ssh_list_hosts` | 列出已配置的 SSH 主机 |
| 2 | `ssh_add_host` | 创建或更新主机配置 |
| 3 | `ssh_remove_host` | 删除主机配置 |
| 4 | `ssh_import_config` | 从 `~/.ssh/config` 导入 |
| 5 | `ssh_exec` | 执行远程命令（支持 daemon 路由） |
| 6 | `ssh_exec_multi` | 多主机并发执行 |
| 7 | `ssh_ping` | 检测连通性和延迟 |
| 8 | `ssh_audit` | 查询审计日志 |
| 9 | `ssh_sftp_ls` | 列出远程目录 |
| 10 | `ssh_sftp_stat` | 查看远程文件信息 |
| 11 | `ssh_sftp_mkdir` | 创建远程目录 |
| 12 | `ssh_sftp_upload` | 上传文件 |
| 13 | `ssh_sftp_download` | 下载文件 |
| 14 | `ssh_session_open` | 打开 PTY 会话 |
| 15 | `ssh_session_write` | 向会话写入输入 |
| 16 | `ssh_session_read` | 读取会话输出 |
| 17 | `ssh_session_close` | 关闭会话 |
| 18 | `ssh_session_list` | 列出所有会话 |
| 19 | `ssh_forward_add` | 添加端口转发 |
| 20 | `ssh_forward_list` | 列出转发隧道 |
| 21 | `ssh_forward_remove` | 删除转发隧道 |
| 22 | `ssh_risk_check` | 检查命令风险等级 |
| 23 | `ssh_approval_list` | 列出审批请求 |
| 24 | `ssh_approval_respond` | 批准或拒绝审批 |
| 25 | `ssh_connection_status` | 查看连接池状态 |
| 26 | `ssh_connect` | 建立 ControlMaster 连接 |
| 27 | `ssh_disconnect` | 关闭 ControlMaster 连接 |
| 28 | `ssh_webhook_config` | 获取或设置 Webhook 配置 |
| 29 | `ssh_playbook_list` | 列出 Playbook |
| 30 | `ssh_playbook_run` | 执行 Playbook |
| 31 | `ssh_list_daemons` | 列出守护进程实例 |

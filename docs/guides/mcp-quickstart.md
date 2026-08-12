# MCP 集成快速入门

## 概述

Agent2SSH 通过 MCP (Model Context Protocol) stdio 协议暴露 **54 个工具**，使任何兼容 MCP 的 AI Agent 都能直接管理 SSH 主机、命令片段、执行远程命令、传输文件、管理会话、端口转发、Playbook、审计、健康检查、指标和远程 daemon。

MCP 服务器以 `agent2ssh-mcp` 二进制运行，通过标准输入/输出与 Agent 通信，遵循 JSON-RPC 2.0 协议，无需网络端口或 HTTP 服务。

---

## Agent 配置

在你的 Agent 的 MCP 配置文件中添加 Agent2SSH 服务器：

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "agent2ssh-mcp",
      "args": [],
      "env": {
        "AGENT2SSH_SOURCE": "workbuddy"
      }
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
      "args": [],
      "env": {
        "AGENT2SSH_SOURCE": "workbuddy"
      }
    }
  }
}
```

`AGENT2SSH_SOURCE` 会写入 audit 和 Live Activity 的来源字段。不同客户端应使用不同值，例如 `workbuddy`、`qoder_work`、`trae`、`codex` 或 `claude_desktop`。

配置完成后，Agent 将自动发现并调用所有 54 个 SSH 工具。

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
| `force` | 否 | 在没有 daemon 审批流时执行高风险命令；仍受策略限制 |
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

MCP 的 exec、exec-multi、playbook、SFTP、session open/write/close、forward add/remove、connect 和 disconnect 等 mutation 操作会复用 Agent2SSH 的统一授权路径。daemon 或远程 token scope 会在审批前检查；用户风险规则只能升级内置风险；host/playbook `risk_override` 只能调整非 `blocked` 命令。PTY session 写入按完成的输入行做授权和操作审计；session/forward 的 read/list 类观察操作不默认写入 `audit.jsonl`。

MCP 的 exec、SFTP、session、terminal 相关 daemon 路径、jump-host、端口转发和连接保留都使用内置 SSH 传输，不依赖系统 `ssh`、`scp` 或 `sshpass`。SSH 主机指纹首次连接时会自动写入 `~/.agent2ssh/known_hosts.json`，后续算法或指纹变化会被拒绝。

高风险命令可通过 daemon 审批流处理。未路由到 daemon、且没有本地审批处理器时，MCP 会失败关闭；此时应改用 daemon 路由，或在策略允许时传入 `force: true`。

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

返回风险等级和是否匹配了用户自定义规则；传入 `host` 时会考虑该主机的 `risk_override`：

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

### 连接保留

管理内置 SSH 连接保留，提前建立连接并查看当前保留状态。

**ssh_connection_status** -- 查看所有连接状态

```json
{
  "name": "ssh_connection_status",
  "arguments": {}
}
```

**ssh_connect** -- 手动建立内置 SSH 连接

```json
{
  "name": "ssh_connect",
  "arguments": { "host": "web1" }
}
```

**ssh_disconnect** -- 关闭内置 SSH 连接

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

返回本地和远程守护进程的别名、URL、连接状态和已配置的客户端 scope。

在 `ssh_exec` 中使用 `daemon_alias` 参数将命令路由到远程守护进程，详见下方 "远程 Daemon 路由" 一节。`remotes.toml` 的 scope 会在本地转发前执行，远程 daemon 还可以通过 `daemon_tokens.toml` 再做服务端限制。

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
    "url": "https://example.com/agent2ssh-webhook",
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
| **high** | 本地执行需要 `--force`；daemon 路由可走审批 | 本地路径需要 `force: true`；daemon 路由可走审批 | 可走审批，或在策略允许时使用 `force: true` |
| **blocked** | 拒绝执行 | 拒绝执行 | 拒绝执行，并写入 blocked audit |

**MCP 中的处理方式：** MCP 本地路径不持有 Daemon 审批队列。高风险命令未携带 `force: true` 且没有 daemon 审批处理器时会失败关闭；需要审批的场景应通过本地或远程 daemon 路由执行。

**Daemon API 审批流程：**

1. Agent 提交高风险命令且未携带 `force: true`
2. Daemon 创建审批请求，触发 `approval_required` Webhook
3. Daemon 阻塞等待审批结果（默认超时 300 秒）
4. 管理员通过 API、Web 控制台或 MCP 工具批准或拒绝
5. 批准后自动执行命令；拒绝后返回 403 错误；超时返回 408 错误

**用户自定义规则：** 推荐在 `~/.agent2ssh/policy.toml` / `policy.json` 的 `[risk.*]` 区块中定义额外规则；旧版 `risk_rules.toml` 仍兼容。用户规则只能升级内置风险，降级非 `blocked` 命令需使用 host/playbook `risk_override`。详见 [配置指南](./configuration-guide.md)。

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

完整 54 个工具列表与权威描述以 [MCP Tools Reference](../skills.md) 为准。下表只列出最常用的基础入口，编号对应 `tools/list` 返回顺序的前 31 个。

| # | 工具名称 | 说明 |
|---|----------|------|
| 1 | `ssh_list_hosts` | 列出已配置的 SSH 主机 |
| 2 | `ssh_list_daemons` | 列出本地 + 远程守护进程（含连通性） |
| 3 | `ssh_import_config` | 从 `~/.ssh/config` 导入 |
| 4 | `ssh_add_host` | 创建或更新主机配置 |
| 5 | `ssh_remove_host` | 删除主机配置 |
| 6 | `ssh_exec` | 执行远程命令（支持 `daemon_alias` 路由到远程 daemon） |
| 7 | `ssh_ping` | 检测连通性和延迟 |
| 8 | `ssh_exec_multi` | 多主机并发执行（支持批量策略） |
| 9 | `ssh_exec_compare` | 跨主机比较执行结果 |
| 10 | `ssh_audit` | 查询审计日志 |
| 11 | `ssh_audit_export` | 导出审计日志（JSONL / CSV） |
| 12 | `ssh_sftp_ls` | 列出远程目录 |
| 13 | `ssh_sftp_stat` | 查看远程文件信息 |
| 14 | `ssh_sftp_mkdir` | 创建远程目录 |
| 15 | `ssh_sftp_upload` | 上传文件 |
| 16 | `ssh_sftp_download` | 下载文件 |
| 17 | `ssh_session_open` | 打开 PTY 会话 |
| 18 | `ssh_session_write` | 向会话写入输入 |
| 19 | `ssh_session_read` | 读取会话输出 |
| 20 | `ssh_session_close` | 关闭会话 |
| 21 | `ssh_session_list` | 列出所有会话 |
| 22 | `ssh_forward_add` | 添加端口转发 |
| 23 | `ssh_forward_list` | 列出转发隧道 |
| 24 | `ssh_forward_remove` | 删除端口转发 |
| 25 | `ssh_risk_check` | 检查命令风险等级 |
| 26 | `ssh_gate_status` | 读取本地 daemon 执行门状态（active / paused） |
| 27 | `ssh_approval_list` | 列出审批请求 |
| 28 | `ssh_approval_respond` | 批准或拒绝审批 |
| 29 | `ssh_playbook_list` | 列出 Playbook |
| 30 | `ssh_playbook_run` | 执行 Playbook |
| 31 | `ssh_playbook_dry_run` | 预览 Playbook 步骤（不执行） |

其余 20 个工具（`ssh_connection_status` 到 `ssh_sync_export`）涵盖连接保留、Webhook、配置导入/导出/预览、Doctor、Metrics、Preview、审批策略、健康快照、远程 Daemon 诊断、Metrics 趋势、事件订阅、与 `~/.ssh/config` 同步等。请直接阅读 [MCP Tools Reference](../skills.md) 了解参数与返回。

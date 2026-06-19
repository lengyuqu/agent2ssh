# Daemon API 快速入门

Agent2SSH Daemon 提供本地 HTTP API 和 WebSocket 接口，允许通过 REST 调用所有 SSH 操作。守护进程监听 `127.0.0.1:7722`，使用 Bearer Token 认证。

---

## 启动与认证

### 启动守护进程

```bash
agent2ssh daemon start
```

验证运行状态：

```bash
agent2ssh daemon status
```

### 获取认证令牌

守护进程使用 Bearer Token 认证，令牌存储在 `~/.agent2ssh/daemon.token`：

```bash
TOKEN=$(cat ~/.agent2ssh/daemon.token)
AUTH="Authorization: Bearer $TOKEN"
```

后续所有 curl 示例中均使用 `$AUTH` 变量。`/health` 端点无需认证。

### Scoped Token

默认 `daemon.token` 是 unrestricted admin token。需要给 CI、远程 agent 或只读巡检客户端发放最小权限时，在 `~/.agent2ssh/daemon_tokens.toml` 中添加 scoped token：

```toml
[[tokens]]
name = "prod-readonly"
token_env = "AGENT2SSH_PROD_READONLY_TOKEN"

[tokens.scope]
allowed_hosts = ["prod-web-1"]
allowed_tags = ["production"]
allowed_commands = ["uptime", "df *", "journalctl -n *"]
denied_commands = ["rm *", "sudo *"]
```

客户端仍然使用同一个 `Authorization: Bearer <token>` 请求头。scope 会在审批前执行，拒绝项不会进入审批队列。

---

## 端点总览

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查（无需认证） |
| GET | `/hosts` | 列出主机 |
| POST | `/hosts` | 添加主机 |
| POST | `/hosts/import` | 导入 SSH 配置 |
| DELETE | `/hosts/:name` | 删除主机 |
| POST | `/ping` | 检测连通性 |
| POST | `/exec` | 执行命令 |
| POST | `/exec-multi` | 多主机并发执行 |
| GET | `/exec/stream` | WebSocket 流式执行 |
| GET | `/terminal` | WebSocket 交互式终端 |
| GET | `/gate` | 查询全局执行 gate |
| POST | `/gate/pause` | 暂停非桌面来源执行 |
| POST | `/gate/resume` | 恢复执行 |
| GET | `/audit` | 查询审计日志 |
| POST | `/sftp/upload` | 上传文件 |
| POST | `/sftp/download` | 下载文件 |
| POST | `/sftp/ls` | 列出远程目录 |
| POST | `/sftp/stat` | 查看远程文件信息 |
| POST | `/sftp/mkdir` | 创建远程目录 |
| POST | `/sessions` | 打开会话 |
| GET | `/sessions` | 列出会话 |
| POST | `/sessions/:id/write` | 写入会话 |
| GET | `/sessions/:id/read` | 读取会话 |
| DELETE | `/sessions/:id` | 关闭会话 |
| POST | `/forwards` | 添加端口转发 |
| GET | `/forwards` | 列出转发 |
| DELETE | `/forwards/:id` | 删除转发 |
| GET | `/approvals` | 列出审批请求 |
| POST | `/approvals/:id/approve` | 批准审批 |
| POST | `/approvals/:id/reject` | 拒绝审批 |
| POST | `/risk/check` | 风险检查 |
| GET | `/connections` | 连接状态 |
| POST | `/connections/:host/connect` | 建立连接 |
| POST | `/connections/:host/disconnect` | 关闭连接 |
| GET | `/playbooks` | 列出 Playbook |
| POST | `/playbooks/run` | 执行 Playbook |
| GET | `/daemons` | 列出守护进程 |
| POST | `/daemons/:alias/exec` | 代理执行 |
| GET | `/webhook/config` | 获取 Webhook 配置 |
| PUT | `/webhook/config` | 更新 Webhook 配置 |
| GET | `/console` | Web 控制台 |

---

## Health

健康检查无需认证：

```bash
curl http://127.0.0.1:7722/health
```

响应：

```json
{"ok": true}
```

---

## Hosts

### 列出所有主机

```bash
curl -H "$AUTH" http://127.0.0.1:7722/hosts
```

### 添加主机

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "web1",
    "host": "192.168.1.10",
    "user": "deploy",
    "port": 22,
    "key_path": "~/.ssh/id_ed25519",
    "jump_host": null,
    "risk_override": null,
    "tags": ["production", "web"],
    "env": "prod",
    "role": "web",
    "owner": "platform"
  }' \
  http://127.0.0.1:7722/hosts
```

### 删除主机

```bash
curl -X DELETE -H "$AUTH" http://127.0.0.1:7722/hosts/web1
```

### 导入 SSH 配置

```bash
curl -X POST -H "$AUTH" http://127.0.0.1:7722/hosts/import
```

---

## Ping

检测主机连通性和延迟：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "hosts": ["web1", "web2"],
    "timeout_secs": 5
  }' \
  http://127.0.0.1:7722/ping
```

响应示例：

```json
[
  {"host": "web1", "reachable": true, "latency_ms": 23, "error": null},
  {"host": "web2", "reachable": false, "latency_ms": null, "error": "connection timed out"}
]
```

---

## Exec

### 单主机执行

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "command": "uptime",
    "force": false,
    "timeout_secs": 60
  }' \
  http://127.0.0.1:7722/exec
```

响应：

```json
{
  "host": "web1",
  "command": "uptime",
  "exit_code": 0,
  "stdout": " 10:30:15 up 45 days,  3:22,  1 user,  load average: 0.08, 0.12, 0.10\n",
  "stderr": "",
  "duration_ms": 156,
  "risk_level": "low",
  "truncated": false
}
```

### 高风险命令（强制模式）

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "command": "sudo systemctl restart nginx",
    "force": true
  }' \
  http://127.0.0.1:7722/exec
```

### 高风险命令（审批模式）

不携带 `force: true` 时，Daemon 会创建审批请求并阻塞等待（默认超时 300 秒）：

```bash
# 此请求会阻塞，等待审批
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "command": "sudo rm -rf /tmp/cache"
  }' \
  http://127.0.0.1:7722/exec
```

在另一个终端批准审批：

```bash
# 1. 查看待审批列表
curl -H "$AUTH" http://127.0.0.1:7722/approvals

# 2. 批准（替换为实际的 approval-id）
curl -X POST -H "$AUTH" \
  http://127.0.0.1:7722/approvals/<approval-id>/approve

# 3. 或拒绝
curl -X POST -H "$AUTH" \
  http://127.0.0.1:7722/approvals/<approval-id>/reject
```

### 向 stdin 传递数据

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "command": "cat > /tmp/data.txt",
    "stdin": "hello world"
  }' \
  http://127.0.0.1:7722/exec
```

### Execution Gate

全局 execution gate 可在紧急情况下暂停非 `desktop` 来源的 daemon mutation/execution 入口。`paused` 状态下，`/exec`、`/exec-multi`、`/exec/compare`、`/playbooks/run`、SFTP 操作、session open/write、forward add、connection connect、WebSocket exec、WebSocket terminal 和 `/daemons/localhost/exec` 会被拒绝；HTTP 入口返回 423，并写入 `blocked` audit。

查询状态：

```bash
curl -H "$AUTH" http://127.0.0.1:7722/gate
```

暂停执行：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"source":"cli","reason":"maintenance window"}' \
  http://127.0.0.1:7722/gate/pause
```

恢复执行：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"source":"desktop","reason":"operator resumed"}' \
  http://127.0.0.1:7722/gate/resume
```

响应示例：

```json
{
  "mode": "paused",
  "updated_at": "2026-06-16T10:30:00Z",
  "updated_by": "cli",
  "reason": "maintenance window"
}
```

### Execution Limits

daemon 会在进程内维护滑动窗口限额和 session 并发计数。配置文件为：

```text
~/.agent2ssh/execution_limits.toml
```

缺省配置启用限额；如需覆盖，可写入：

```toml
enabled = true
window_secs = 60
default_source_per_minute = 30
default_host_per_minute = 20
default_tag_per_minute = 60
default_source_max_sessions = 4
default_host_max_sessions = 4
default_tag_max_sessions = 8

[source.mcp]
per_minute = 10
max_sessions = 2

[host.web1]
per_minute = 5
max_sessions = 1

[tag.production]
per_minute = 10
max_sessions = 2
```

`per_minute = 0` 或 `max_sessions = 0` 表示该维度不限额。速率限额覆盖 `/exec`、`/exec-multi`、`/exec/compare`、SFTP 操作、session write、forward add、`/playbooks/run`、WebSocket exec、WebSocket terminal completed-line input 和 `/daemons/localhost/exec`；session 并发限额覆盖 session open 和 WebSocket terminal open。超限时 HTTP 入口返回 429，并写入 `blocked` audit；事件流发布 `limit_rejected`。本地代理执行路径 `/daemons/localhost/exec` 同样受本地 daemon 限额约束。

---

## Exec-Multi

多主机并发执行：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "hosts": ["web1", "web2", "web3"],
    "command": "df -h",
    "force": false,
    "timeout_secs": 30
  }' \
  http://127.0.0.1:7722/exec-multi
```

按标签批量执行：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "hosts": [],
    "command": "systemctl status nginx",
    "tags": ["production"]
  }' \
  http://127.0.0.1:7722/exec-multi
```

---

## WebSocket Exec/Stream

通过 WebSocket 实时流式获取命令输出。

使用 `wscat` 连接（需在 HTTP Header 中携带认证）：

```bash
wscat -c "ws://127.0.0.1:7722/exec/stream" \
  -H "Authorization: Bearer $TOKEN"
```

连接后发送 ExecRequest（JSON 格式）：

```json
{"host": "web1", "command": "tail -f /var/log/nginx/access.log", "timeout_secs": 30}
```

服务器推送流式消息：

```json
{"type": "stdout", "data": "192.168.1.1 - - [12/Jun/2025:10:30:15 +0800] \"GET / HTTP/1.1\" 200 612\n"}
{"type": "stderr", "data": ""}
{"type": "exit", "code": 0, "duration_ms": 30015}
```

消息类型说明：

| 类型 | 说明 |
|------|------|
| `stdout` | 标准输出数据块 |
| `stderr` | 标准错误数据块 |
| `exit` | 命令执行结束，包含退出码和耗时 |
| `error` | 错误信息（风险拒绝、未知主机等） |

WebSocket exec 使用与普通 `/exec` 相同的内置 SSH 传输，key、password、ssh-agent、jump host 和连接指纹行为保持一致；风险、scope、gate、limits、approval 和 audit 也走同一授权链路。SSH 主机指纹首次连接时会自动信任并保存到 `~/.agent2ssh/known_hosts.json`，后续算法或指纹变化会被拒绝。

---

## WebSocket Terminal

`/terminal` 提供交互式终端，浏览器连接时通过 query string 传递 token，因为标准 WebSocket 握手不能设置 `Authorization` header。

```bash
wscat -c "ws://127.0.0.1:7722/terminal?host=web1&token=$TOKEN"
```

服务端首先发送一个 JSON 文本帧，包含连接元数据：

```json
{
  "type": "connected",
  "host": "web1",
  "address": "10.0.0.12:22",
  "username": "ubuntu",
  "fingerprint_sha256": "SHA256:...",
  "host_key_algorithm": "ssh-ed25519",
  "server_banner": "SSH-2.0-OpenSSH_9.6"
}
```

随后终端输入输出使用 binary frame 传输原始字节。调整窗口大小时发送文本控制帧：

```json
{"type": "resize", "cols": 120, "rows": 36}
```

交互式终端和 `/sessions` 持久会话都使用内置 SSH 传输和远程 PTY，不依赖系统 `ssh` 或 `sshpass`。终端打开会占用 session 并发限额；终端输入按完成行复用 session write 的速率限额、风险和审批检查；被拒绝的输入不会转发给远端。jump-host 使用内置 `direct-tcpip` bastion channel，端口转发和连接保留也走内置 SSH。

---

## Audit

查询执行审计日志：

```bash
curl -H "$AUTH" "http://127.0.0.1:7722/audit?limit=50"
```

支持的查询参数：

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `limit` | integer | 20 | 返回条数上限 |
| `host` | string | - | 按主机过滤 |
| `risk_level` | string | - | 按风险等级过滤（low/medium/high/blocked） |
| `exit_code` | integer | - | 按退出码过滤 |
| `since` | string | - | ISO-8601 起始时间 |
| `until` | string | - | ISO-8601 结束时间 |

组合查询：

```bash
curl -H "$AUTH" \
  "http://127.0.0.1:7722/audit?limit=100&host=web1&risk_level=high&since=2025-01-01T00:00:00Z"
```

---

## SFTP

SFTP 端点会先执行 scope、gate、limits、风险和审批检查，再进行文件操作。用于策略匹配的操作字符串形如 `sftp upload <local> -> <remote>`、`sftp download <remote> -> <local>`、`sftp ls <path>`。所有 SFTP 成功、失败和拒绝都会写入 audit，并记录请求来源。

### 上传文件

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "local_path": "/local/app.tar.gz",
    "remote_path": "/opt/releases/app.tar.gz"
  }' \
  http://127.0.0.1:7722/sftp/upload
```

### 下载文件

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "remote_path": "/var/log/nginx/error.log",
    "local_path": "/local/error.log"
  }' \
  http://127.0.0.1:7722/sftp/download
```

### 列出远程目录

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"host": "web1", "path": "/etc/nginx"}' \
  http://127.0.0.1:7722/sftp/ls
```

### 查看远程文件信息

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"host": "web1", "path": "/etc/nginx/nginx.conf"}' \
  http://127.0.0.1:7722/sftp/stat
```

### 创建远程目录

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"host": "web1", "path": "/opt/app/releases/2025"}' \
  http://127.0.0.1:7722/sftp/mkdir
```

---

## Sessions

`/sessions` 是带缓冲读取的持久 PTY API，适合 MCP/CLI/HTTP 自动化。会话后端使用同一套内置 SSH terminal worker；`read` 只返回终端输出，不返回 `/terminal` 的连接元数据控制帧。连接认证和主机指纹会写入本地诊断日志。

### 打开会话

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"host": "web1"}' \
  http://127.0.0.1:7722/sessions
```

响应：

```json
{"id": "550e8400-e29b-41d4-a716-446655440000"}
```

### 列出所有会话

```bash
curl -H "$AUTH" http://127.0.0.1:7722/sessions
```

### 向会话写入输入

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"input": "ls -la\n"}' \
  http://127.0.0.1:7722/sessions/<session-id>/write
```

### 读取会话输出

```bash
curl -H "$AUTH" \
  "http://127.0.0.1:7722/sessions/<session-id>/read?timeout_ms=2000"
```

### 关闭会话

```bash
curl -X DELETE -H "$AUTH" \
  http://127.0.0.1:7722/sessions/<session-id>
```

---

## Forwards

### 添加端口转发

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "direction": "local",
    "bind_port": 8080,
    "target_host": "192.168.1.100",
    "target_port": 80
  }' \
  http://127.0.0.1:7722/forwards
```

### 列出所有转发

```bash
curl -H "$AUTH" http://127.0.0.1:7722/forwards
```

### 删除转发

```bash
curl -X DELETE -H "$AUTH" \
  http://127.0.0.1:7722/forwards/<forward-id>
```

---

## Approvals

### 列出待审批请求

```bash
curl -H "$AUTH" http://127.0.0.1:7722/approvals
```

响应示例：

```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "host": "web1",
    "command": "sudo rm -rf /tmp/cache",
    "risk_level": "high",
    "requested_at": "2025-06-12T10:30:15Z",
    "ttl_secs": 300,
    "status": "pending"
  }
]
```

### 批准请求

```bash
curl -X POST -H "$AUTH" \
  http://127.0.0.1:7722/approvals/<approval-id>/approve
```

### 拒绝请求

```bash
curl -X POST -H "$AUTH" \
  http://127.0.0.1:7722/approvals/<approval-id>/reject
```

审批状态值：`pending`、`approved`、`rejected`、`timed_out`。默认超时 300 秒。

---

## Risk Check

检查命令的风险等级（不执行命令）。传入 `host` 时会同时考虑该主机的 `risk_override`：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"host": "web1", "command": "rm -rf /tmp/cache"}' \
  http://127.0.0.1:7722/risk/check
```

响应：

```json
{"risk_level": "high", "matched_rule": null}
```

匹配用户自定义规则时，`matched_rule` 为 `user_rule`。用户规则只能升级内置风险；如果传入了 `host`，非 `blocked` 命令还会应用该主机的 trusted `risk_override`：

```json
{"risk_level": "blocked", "matched_rule": "user_rule"}
```

---

## Connections

管理内置 SSH 连接保留。

### 查看所有连接状态

```bash
curl -H "$AUTH" http://127.0.0.1:7722/connections
```

### 建立连接

```bash
curl -X POST -H "$AUTH" \
  http://127.0.0.1:7722/connections/web1/connect
```

### 关闭连接

```bash
curl -X POST -H "$AUTH" \
  http://127.0.0.1:7722/connections/web1/disconnect
```

---

## Playbooks

### 列出所有 Playbook

```bash
curl -H "$AUTH" http://127.0.0.1:7722/playbooks
```

### 执行 Playbook

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "playbook": "deploy-web",
    "host": "web1",
    "force": false
  }' \
  http://127.0.0.1:7722/playbooks/run
```

响应示例：

```json
{
  "playbook": "deploy-web",
  "host": "web1",
  "steps_completed": [
    {
      "step": 0,
      "command": "cd /opt/app && git pull",
      "result": {"exit_code": 0, "stdout": "Already up to date.\n", "duration_ms": 234},
      "error": null
    }
  ],
  "success": true,
  "total_duration_ms": 390
}
```

Playbook 在 `~/.agent2ssh/playbooks.toml` 中定义，详见 [配置指南](./configuration-guide.md)。

---

## Daemons

### 列出所有守护进程

```bash
curl -H "$AUTH" http://127.0.0.1:7722/daemons
```

响应示例：

```json
[
  {"alias": "localhost", "url": "http://127.0.0.1:7722", "connected": true},
  {"alias": "ci-server", "url": "http://192.168.1.100:7722", "connected": true}
]
```

### 通过守护进程代理执行

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "web1",
    "command": "uptime",
    "force": false
  }' \
  http://127.0.0.1:7722/daemons/ci-server/exec
```

如果 `alias` 为 `localhost`，在本地执行；否则转发到对应的远程守护进程。代理执行会同时检查请求 token 的 daemon scope 和 `remotes.toml` 中该 alias 的客户端 scope，然后再转发到远程 daemon；远程 daemon 还会按自身 token scope 和策略再次校验。

---

## Webhook

### 获取当前 Webhook 配置

```bash
curl -H "$AUTH" http://127.0.0.1:7722/webhook/config
```

### 更新 Webhook 配置

```bash
curl -X PUT -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/agent2ssh-webhook",
    "events": ["approval_required", "exec_blocked", "exec_completed"],
    "secret": "my-hmac-secret"
  }' \
  http://127.0.0.1:7722/webhook/config
```

支持的事件类型：

| 事件 | 触发时机 |
|------|----------|
| `approval_required` | 高风险命令需要审批时 |
| `exec_blocked` | 命令被用户规则阻止时 |
| `exec_completed` | 命令执行完成时 |

当 URL 包含 `hooks.slack.com` 时，自动使用 Slack Block Kit 格式发送消息。配置了 `secret` 时，请求头包含 `X-Agent2SSH-Signature: sha256=<hex>` 签名。

---

## Web Console

在浏览器中打开 Web 控制台：

```
http://127.0.0.1:7722/console
```

或在终端中执行：

```bash
open http://127.0.0.1:7722/console
```

Web 控制台提供可视化操作界面，详见 [Web 控制台指南](./web-console-guide.md)。

---

## 实时事件流（SSE）

Daemon 提供认证后的 Server-Sent Events 流，用于本机安全可视化和 agent activity 观察：

```bash
curl -N -H "$AUTH" http://127.0.0.1:7722/events/stream
```

事件格式为 SSE `event: agent2ssh`，`data` 字段是 JSON：

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "event_type": "session_output",
  "timestamp": "2026-06-15T13:30:00Z",
  "data": {
    "source": "daemon",
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "output_preview": "uptime\n",
    "output_bytes": 7
  }
}
```

当前主要事件类型：

| 类型 | 说明 |
|------|------|
| `exec_started` | WebSocket streaming exec 开始 |
| `exec_output` | WebSocket streaming exec 输出片段，包含 `stream` 和 bounded preview |
| `exec_completed` | 命令执行完成；普通 CLI/MCP exec 也会写入 audit 并发布完成事件 |
| `session_opened` | daemon-managed PTY session 打开 |
| `session_input` | 向 PTY session 写入输入，包含输入预览和字节数 |
| `session_output` | 读取到 PTY session 输出，包含输出预览和字节数 |
| `session_closed` | PTY session 关闭 |
| `approval_requested` / `approval_responded` | 审批请求和响应 |
| `audit_rotated` | 审计日志轮转 |
| `gate_changed` | execution gate 状态切换 |
| `gate_rejected` | execution gate 拒绝了一次非桌面来源执行 |
| `limit_rejected` | execution limits 拒绝了一次执行或 session open |
| `anomaly_detected` | audit 滑动窗口检测到异常行为，包含 `kind`、`severity` 和 `reason` |

桌面端的 Live Agent Activity 面板会订阅该事件流，并同时轮询 recent audit，用来观察 Codex、Claude Code、opencode 等 agent 通过 CLI/MCP/daemon 发起的 SSH 操作。

---

## 错误响应

所有错误返回统一的 JSON 格式：

```json
{"error": "unauthorized"}
```

常见 HTTP 状态码：

| 状态码 | 说明 |
|--------|------|
| 200 | 成功 |
| 400 | 请求参数错误 / 命令被用户规则阻止 |
| 401 | 认证失败 |
| 403 | 审批被拒绝 |
| 404 | 资源未找到 |
| 408 | 审批超时 |
| 423 | execution gate 已暂停 |
| 429 | execution limits 超限 |
| 502 | 远程守护进程错误 |

## API 参考

完整的 OpenAPI 规范请参考 [docs/api.yaml](../api.yaml)。

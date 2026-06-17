# 配置指南

Agent2SSH 的所有配置和数据文件存储在 `~/.agent2ssh/` 目录下。本指南详细说明每个配置文件的用途、格式和使用方法。

## 目录结构

```text
~/.agent2ssh/
  hosts.json       # 主机配置文件（自动管理）
  daemon.token     # 守护进程认证令牌（自动生成）
  daemon_tokens.toml # 可选的 scoped Bearer Token
  daemon.pid       # 守护进程 PID（自动管理）
  audit.jsonl      # 执行审计日志（自动追加）
  .hosts.lock      # hosts.json 跨进程写锁（自动管理）
  .audit.lock      # audit.jsonl 跨进程追加锁（自动管理）
  policy.toml      # 统一策略文件（推荐）
  risk_rules.toml  # 旧版用户自定义风险规则（兼容）
  approval_policies.toml # 旧版审批策略（兼容）
  execution_gate.toml    # 全局执行急停状态
  execution_limits.toml  # 执行速率和 session 并发限额
  anomaly.toml     # audit 异常检测阈值
  playbooks.toml   # Playbook 命令模板定义
  remotes.toml     # 远程守护进程注册表
  webhook.toml     # Webhook 通知配置
  keys/            # SSH 密钥存储目录
```

---

## hosts.json

### 用途

存储所有 SSH 主机配置文件。由 CLI、MCP 和 Daemon API 自动管理，通常不需要手动编辑。

### 文件格式

JSON 格式，包含一个 `hosts` 数组：

```json
{
  "hosts": [
    {
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
    },
    {
      "name": "bastion",
      "host": "10.0.0.1",
      "user": "admin",
      "port": 22,
      "key_path": "~/.ssh/id_ed25519",
      "jump_host": null,
      "risk_override": null,
      "tags": []
    },
    {
      "name": "internal",
      "host": "192.168.1.100",
      "user": "deploy",
      "port": 22,
      "key_path": "~/.ssh/id_ed25519",
      "jump_host": "bastion",
      "risk_override": null,
      "tags": ["production"]
    },
    {
      "name": "sandbox",
      "host": "10.0.0.50",
      "user": "test",
      "port": 22,
      "key_path": null,
      "jump_host": null,
      "risk_override": "low",
      "tags": ["dev"]
    }
  ]
}
```

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 主机别名，用于 CLI/MCP/API 中引用 |
| `host` | string | 是 | 主机地址（IP 或域名） |
| `user` | string | 否 | SSH 用户名 |
| `port` | integer | 否 | SSH 端口，默认 22 |
| `key_path` | string | 否 | SSH 私钥路径 |
| `jump_host` | string | 否 | ProxyJump 跳板机别名 |
| `risk_override` | string | 否 | 覆盖该主机非 blocked 命令的风险等级（low/medium/high） |
| `tags` | array | 否 | 标签列表，用于分组和批量执行 |
| `env` | string | 否 | 环境标签，用于按生产、预发、开发等环境过滤 |
| `role` | string | 否 | 角色标签，用于按 web、db、worker 等职责过滤 |
| `owner` | string | 否 | 负责人或团队标签，用于按归属过滤 |

### 注意事项

- 文件由 Agent2SSH 自动管理，手动编辑后需确保 JSON 格式正确
- `name` 字段必须唯一
- `jump_host` 必须引用已存在的主机别名
- `risk_override` 设置为 `"low"` 可以降低该主机上非 `blocked` 命令的风险等级
- `risk_override` 不能降级 `blocked` 命令；内置或用户规则判定为 `blocked` 的命令仍会被拒绝；显式审批策略、scope、gate 和限额仍会生效
- `env`、`role`、`owner` 和 `tags` 可用于桌面端主机视图过滤，也可用于 CLI `host list` 过滤

---

## audit.jsonl

### 用途

记录所有通过 Agent2SSH 执行的命令，用于审计和追踪。

### 文件格式

JSON Lines 格式，每行一个 JSON 对象：

```jsonl
{"id":"550e8400-e29b-41d4-a716-446655440000","ts":"2025-06-12T10:30:15.123456789Z","host":"web1","command":"uptime","exit_code":0,"duration_ms":156,"risk_level":"low"}
{"id":"550e8401-e29b-41d4-a716-446655440001","ts":"2025-06-12T10:31:22.987654321Z","host":"web1","command":"sudo systemctl restart nginx","exit_code":0,"duration_ms":234,"risk_level":"high"}
{"id":"550e8402-e29b-41d4-a716-446655440002","ts":"2025-06-12T10:32:45.111222333Z","host":"web2","command":"df -h","exit_code":0,"duration_ms":89,"risk_level":"low"}
```

### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | UUID | 审计记录唯一标识 |
| `ts` | ISO-8601 | 执行时间戳（UTC） |
| `host` | string | 主机别名 |
| `command` | string | 执行的命令 |
| `exit_code` | integer | 命令退出码（null 表示超时或错误） |
| `duration_ms` | integer | 执行耗时（毫秒） |
| `risk_level` | string | 风险等级（low/medium/high/blocked） |

### 注意事项

- 文件自动追加，不会覆盖
- 可以手动查看或分析：
  ```bash
  # 查看最近 10 条记录
  tail -n 10 ~/.agent2ssh/audit.jsonl | jq .
  
  # 统计高风险命令
  grep '"risk_level":"high"' ~/.agent2ssh/audit.jsonl | wc -l
  
  # 按主机统计执行次数
  cat ~/.agent2ssh/audit.jsonl | jq -r .host | sort | uniq -c
  ```
- 建议定期轮转或清理，避免文件过大

---

## daemon.token

### 用途

守护进程 HTTP API 的默认 Bearer Token 认证令牌。该 token 是本机 unrestricted admin token，拥有守护进程可执行的完整能力。

### 文件格式

纯文本，包含一个 UUID v4 字符串：

```text
550e8400-e29b-41d4-a716-446655440000
```

### 注意事项

- 首次启动守护进程时自动生成
- 文件权限自动设置为 `0600`（仅所有者可读写）
- **不要将此文件提交到版本控制或分享给他人**
- 如需重新生成，删除文件后重启守护进程即可
- 如果守护进程检测到权限过于宽松，会自动修复并输出警告

---

## daemon_tokens.toml

### 用途

为 daemon API 配置额外的 scoped Bearer Token。适合给远程 agent、CI 或只读巡检客户端发放最小权限 token。`daemon.token` 仍然是 unrestricted admin token；`daemon_tokens.toml` 中的每个 token 必须配置 `scope`。

### 文件格式

```toml
[[tokens]]
name = "prod-readonly"
token_env = "AGENT2SSH_PROD_READONLY_TOKEN"

[tokens.scope]
allowed_hosts = ["prod-web-1"]
allowed_tags = ["production"]
allowed_commands = ["uptime", "df *", "journalctl -n *"]
denied_commands = ["rm *", "sudo *"]

[[tokens]]
name = "ci-deploy"
token = "replace-with-random-token"

[tokens.scope]
allowed_tags = ["staging"]
allowed_commands = ["git *", "systemctl restart app"]
denied_commands = ["rm -rf *", "mkfs *"]
```

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 否 | token 名称；配置时必须唯一 |
| `token_env` | string | 否 | 从环境变量读取 token（推荐） |
| `token` | string | 否 | 明文 token |
| `scope.allowed_hosts` | array | 否 | 允许访问的主机名；空数组表示不限主机 |
| `scope.allowed_tags` | array | 否 | 允许访问的主机标签；空数组表示不限标签 |
| `scope.allowed_commands` | array | 否 | 允许执行的命令或操作模式；空数组表示不限命令 |
| `scope.denied_commands` | array | 否 | 拒绝执行的命令或操作模式；优先于 allowed 规则 |

### 注意事项

- 每个 scoped token 必须配置 `token_env` 或 `token`，并且必须配置 `scope`
- `token_env` 优先于 `token`
- scope 会在风险审批之前检查；未命中 scope 的请求不会进入审批队列
- `allowed_commands` 和 `denied_commands` 使用 glob 风格匹配，`denied_commands` 优先
- scope 覆盖 exec、playbook、SFTP、session、forward 和 connection 的 mutation 操作；非命令操作会使用类似 `sftp upload a -> b`、`session_open`、`session_close`、`connect` 的操作字符串做匹配
- PTY session 写入按已完成的输入行做授权和审计；daemon session 和 desktop 本地 session 都会缓存未完成输入，直到换行后再按完整命令授权；`session_read`、`session_list`、`forward_list` 这类观察操作主要通过 token/scope 和事件流控制，不默认写入 `audit.jsonl`

---

## daemon.pid

### 用途

记录当前运行的守护进程 PID，用于 `daemon stop` 和 `daemon status` 命令。

### 文件格式

纯文本，包含进程 ID：

```text
12345
```

### 注意事项

- 守护进程启动时自动写入
- 守护进程正常退出时自动删除
- 如果守护进程异常退出，此文件可能残留；`daemon status` 会检查进程是否仍然存活

---

## policy.toml / policy.json

### 用途

`policy.toml` 是首选的策略即代码文件，用一个可版本化文件统一管理风险规则和审批策略。Agent2SSH 会优先读取：

1. `~/.agent2ssh/policy.toml`
2. `~/.agent2ssh/policy.json`
3. 兼容旧文件：`risk_rules.toml` 和 `approval_policies.toml`

### TOML 示例

```toml
[risk.blocked]
patterns = [
    "terraform destroy*",
    "kubectl delete namespace*",
]

[risk.high]
patterns = [
    "git push *force*",
    "sudo*",
]

[risk.medium]
patterns = [
    "apt install*",
]

[[approval.policies]]
name = "prod high risk"
tags = ["prod"]
min_risk = "high"
requires_approval = true
ttl_secs = 300

[[approval.policies]]
name = "sandbox auto approve"
hosts = ["sandbox"]
requires_approval = false
```

### JSON 示例

```json
{
  "risk": {
    "blocked": { "patterns": ["terraform destroy*"] },
    "high": { "patterns": ["sudo*"] },
    "medium": { "patterns": ["apt install*"] }
  },
  "approval": {
    "policies": [
      {
        "name": "prod high risk",
        "tags": ["prod"],
        "min_risk": "high",
        "requires_approval": true,
        "ttl_secs": 300
      }
    ]
  }
}
```

### 校验和 dry-run

```bash
# 校验默认 policy.toml / policy.json
agent2ssh policy validate

# 校验指定文件
agent2ssh policy validate --path ./policy.toml --json

# 测试命令最终判定：allow / approve / block
agent2ssh policy test "terraform destroy -auto-approve" --host prod-db --json
```

`policy test` 会同时考虑内置风险规则、统一 policy 中的 risk rules、主机标签、主机 `risk_override` 和 approval policies。`blocked` 命令输出 `block`；`high` 风险或命中审批策略输出 `approve`；其他输出 `allow`。用户风险规则只能把内置风险升级，不能把内置 `high` / `blocked` 降低。

## risk_rules.toml

> 兼容旧配置。新配置建议迁移到 `policy.toml` 的 `[risk.*]` 区块。

### 用途

定义用户自定义的风险规则，用于扩展内置风险分类。用户规则只能升级风险等级，不能降低内置分类。

### 文件格式

TOML 格式，包含三个风险级别分组：

```toml
# 无条件阻止的命令（最高优先级）
[blocked]
patterns = [
    "kubectl delete namespace",
    "terraform destroy",
    "rm -rf /*",
    "mkfs*",
]

# 高风险命令（需要 daemon 审批或 force 确认）
[high]
patterns = [
    "docker system prune",
    "git push --force*",
    "sudo*",
    "chmod 777*",
]

# 中等风险命令
[medium]
patterns = [
    "apt install*",
    "yum install*",
]
```

### 规则匹配

- **精确匹配**：`"docker system prune"` 匹配包含此字符串的命令
- **Glob 模式**：`"git push *force*"` 使用 `*` 通配符匹配任意字符序列
- **大小写不敏感**：匹配时自动转换为小写
- **最终风险**：取内置风险和用户规则命中风险中的较高等级（blocked > high > medium > low）

### 示例

阻止所有 `kubectl delete` 命令：

```toml
[blocked]
patterns = ["kubectl delete*"]
```

将 `docker system prune` 标记为高风险：

```toml
[high]
patterns = ["docker system prune*"]
```

### 注意事项

- 文件修改后立即生效，无需重启守护进程（支持热加载，基于文件修改时间缓存）
- 用户规则不会覆盖并降低内置规则；例如内置 `high` 命令不会因为只命中用户 `medium` 规则而降级
- 如需在可信主机或 playbook 中降低非 `blocked` 命令风险，请使用 `risk_override`
- 文件不存在时，仅使用内置规则

---

## playbooks.toml

### 用途

定义可复用的命令序列（Playbook），适合部署、健康检查等重复性任务。

### 文件格式

TOML 格式，包含多个 Playbook 定义：

```toml
[[playbooks]]
name = "health-check"
description = "基础服务器健康检查"
steps = [
    "uptime",
    "df -h",
    "free -m",
    "systemctl status nginx",
]
tags = ["monitoring"]

[[playbooks]]
name = "deploy-web"
description = "部署 Web 应用"
steps = [
    "cd /opt/app && git pull",
    "npm install --production",
    "npm run build",
    "systemctl restart nginx",
]
tags = ["production", "web"]
risk_override = "medium"

[[playbooks]]
name = "backup-database"
description = "备份数据库"
steps = [
    "mkdir -p /backup/$(date +%Y%m%d)",
    "pg_dump mydb > /backup/$(date +%Y%m%d)/mydb.sql",
    "gzip /backup/$(date +%Y%m%d)/mydb.sql",
]
tags = ["database"]
```

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | Playbook 唯一名称 |
| `description` | string | 是 | Playbook 描述 |
| `steps` | array | 是 | 命令序列，按顺序执行 |
| `tags` | array | 否 | 标签列表 |
| `risk_override` | string | 否 | 覆盖所有步骤的非 blocked 风险等级 |

### 执行行为

- 步骤按顺序执行
- 任何步骤失败（非零退出码或错误）时停止执行，返回已完成步骤的部分结果
- 返回 `success` 状态和 `total_duration_ms` 总耗时
- 使用 MCP 工具 `ssh_playbook_run` 或 Daemon API `POST /playbooks/run` 执行
- 高风险步骤需要 daemon 审批或 `force: true` 才能执行；本地 CLI/MCP 没有可用审批处理器时会失败关闭，并提示改走 daemon 审批流或在策略允许时使用 `--force`
- daemon 审批只对被批准的具体步骤生效；不会因为前一个高风险步骤获批而自动放行后续高风险步骤。显式 `force: true` 仍表示调用方请求放行整个 playbook。

### 注意事项

- 步骤中的 shell 变量（如 `$(date)`）会在远程主机上展开
- `risk_override` 可以统一设置所有步骤的非 `blocked` 风险等级
- `risk_override` 不能降级 `blocked` 命令；内置或用户规则判定为 `blocked` 的步骤仍会被拒绝
- 文件不存在时返回空 Playbook 列表（不会报错）

---

## remotes.toml

### 用途

注册远程 Agent2SSH 守护进程实例，允许跨机器路由 SSH 操作。

### 文件格式

TOML 格式，包含多个远程守护进程配置：

```toml
[[remotes]]
alias = "ci-server"
url = "http://192.168.1.100:7722"
token_env = "AGENT2SSH_CI_TOKEN"

[[remotes]]
alias = "prod-cluster"
url = "https://daemon.example.com:7722"
token_env = "AGENT2SSH_PROD_TOKEN"

[remotes.scope]
allowed_tags = ["production"]
allowed_commands = ["uptime", "df *", "journalctl -n *"]
denied_commands = ["rm *"]

[[remotes]]
alias = "staging"
url = "http://staging.example.com:7722"
token = "550e8400-e29b-41d4-a716-446655440000"
```

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `alias` | string | 是 | 远程守护进程别名（不可为 `localhost`） |
| `url` | string | 是 | 守护进程 HTTP/HTTPS 地址 |
| `token` | string | 否 | 认证令牌（明文） |
| `token_env` | string | 否 | 认证令牌的环境变量名（推荐） |
| `scope` | table | 否 | 客户端侧权限范围，字段同 `daemon_tokens.toml` 的 `scope` |

### Token 解析优先级

1. `token_env`：从环境变量读取（推荐，更安全）
2. `token`：直接使用明文令牌

### Scope 行为

`remotes.toml` 的 `scope` 是本地客户端路由到远程 daemon 前的额外限制，字段包括：

- `allowed_hosts`：允许访问的主机名，空数组表示不限
- `allowed_tags`：允许访问的主机标签，空数组表示不限
- `allowed_commands`：允许执行的命令或操作模式，空数组表示不限
- `denied_commands`：拒绝执行的命令或操作模式，优先于 allowed 规则

远程 daemon 自身也可以通过 `daemon_tokens.toml` 为传入 token 配置服务端 scope。建议客户端 scope 和服务端 scoped token 同时使用，避免远程配置误用时扩大权限。

当 `remotes.toml` 的 scope 配置了 `allowed_tags` 时，CLI/MCP/daemon proxy 会在转发前读取远程 daemon 的 `/hosts`，用远端 host metadata 中的 tags 判断客户端侧 scope；未配置 `allowed_tags` 时不额外查询远端 tags。

### 使用方式

**CLI** -- 通过 `--daemon` 全局参数路由：

```bash
agent2ssh --daemon ci-server exec web1 "uptime"
agent2ssh --daemon ci-server host list
```

**MCP** -- 通过 `daemon_alias` 参数路由：

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

**Daemon API** -- 通过代理端点路由：

```bash
curl -X POST -H "$AUTH" \
  -H "Content-Type: application/json" \
  -d '{"host": "web1", "command": "uptime"}' \
  http://127.0.0.1:7722/daemons/ci-server/exec
```

### 注意事项

- `alias` 必须唯一，不能与 `localhost` 冲突
- `url` 必须以 `http://` 或 `https://` 开头
- 每个远程守护进程必须配置 `token_env` 或 `token`
- 推荐使用 `token_env` 而非 `token`，避免在配置文件中明文存储敏感令牌
- 配置 `scope` 可以限制该远程 daemon alias 允许执行的主机、标签和命令
- 远程守护进程的健康状态通过 `/health` 端点检测（2 秒超时）
- 生产环境应使用 HTTPS，并在远程守护进程前放置 TLS 终止反向代理（如 Caddy、nginx）

---

## webhook.toml

### 用途

配置 Webhook 通知，在特定事件发生时向外部服务发送通知。

### 文件格式

TOML 格式：

```toml
# Webhook URL（Slack、Discord、自定义服务等）
url = "https://example.com/agent2ssh-webhook"

# 订阅的事件类型
events = [
    "approval_required",
    "exec_blocked",
    "exec_completed",
]

# HMAC-SHA256 签名密钥（可选，用于 X-Agent2SSH-Signature 请求头）
secret = "my-secret-key"
```

### 支持的事件

| 事件 | 触发时机 |
|------|----------|
| `approval_required` | 高风险命令需要审批时 |
| `exec_blocked` | 命令被用户自定义规则阻止时 |
| `exec_completed` | 命令执行完成时（无论成功或失败） |
| `anomaly_detected` | audit 滑动窗口检测到异常行为时 |

默认仅订阅 `approval_required` 事件。

### Slack 集成

当 URL 包含 `hooks.slack.com` 时，自动使用 Slack Block Kit 格式：

- 标题显示事件类型（Approval Required / Command Blocked / Command Completed / Anomaly Detected）
- 字段显示主机名、命令、风险等级、退出码
- `approval_required` 事件包含打开本地 Approvals 控制台的按钮；实际批准或拒绝仍通过已认证的控制台/API 完成

### 自定义 Webhook

非 Slack URL 发送原始 JSON payload：

```json
{
  "event": "approval_required",
  "host": "web1",
  "command": "sudo rm -rf /tmp/cache",
  "approval_id": "550e8400-e29b-41d4-a716-446655440000",
  "risk_level": "high"
}
```

如果配置了 `secret`，请求头包含 HMAC-SHA256 签名：

```
X-Agent2SSH-Signature: sha256=<hex-encoded-signature>
```

### 注意事项

- Webhook 是非阻塞的，发送失败不会影响主流程
- HTTP 客户端超时为 10 秒
- 错误信息输出到 stderr
- 可以通过 MCP 工具 `ssh_webhook_config` 或 Daemon API `GET/PUT /webhook/config` 动态管理
- 文件不存在时 Webhook 功能不启用

---

## execution_limits.toml

### 用途

配置 daemon 层的执行速率和 session 并发限额。限额在 daemon 进程内强制执行，覆盖 `/exec`、`/exec-multi`、`/exec/compare`、SFTP 操作、`/playbooks/run`、session write、session open、forward add、WebSocket exec 和 `/daemons/localhost/exec`。

### 文件位置

```text
~/.agent2ssh/execution_limits.toml
```

### 默认行为

文件不存在时使用内置默认值：

| 配置 | 默认值 | 含义 |
|------|--------|------|
| `enabled` | `true` | 是否启用限额 |
| `window_secs` | `60` | 滑动窗口长度 |
| `default_source_per_minute` | `30` | 每个 source 每窗口最大执行数 |
| `default_host_per_minute` | `20` | 每个 host 每窗口最大执行数 |
| `default_tag_per_minute` | `60` | 每个 tag 每窗口最大执行数 |
| `default_source_max_sessions` | `4` | 每个 source 最大并发 session |
| `default_host_max_sessions` | `4` | 每个 host 最大并发 session |
| `default_tag_max_sessions` | `8` | 每个 tag 最大并发 session |

### 文件格式

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

`per_minute = 0` 或 `max_sessions = 0` 表示该维度不限额。超限请求返回 HTTP 429，写入 `blocked` audit，并发布 `limit_rejected` 事件。

---

## anomaly.toml

### 用途

配置 audit 滑动窗口异常检测。每次执行写入 audit 后，Agent2SSH 会检测 source 频率突增、敏感命令模式和非常规时段高危操作；命中后发布 `anomaly_detected` SSE 事件，并可通过 webhook 订阅同名事件。

### 默认行为

| 配置 | 默认值 | 说明 |
|------|--------|------|
| `enabled` | `true` | 是否启用异常检测 |
| `window_secs` | `300` | 滑动窗口长度 |
| `source_burst_threshold` | `10` | 同一 source 在窗口内达到该执行数触发 |
| `sensitive_threshold` | `1` | 敏感模式命中次数阈值 |
| `after_hours_start` | `22` | 非常规时段起始小时，UTC |
| `after_hours_end` | `6` | 非常规时段结束小时，UTC |

### 文件格式

```toml
enabled = true
window_secs = 300
source_burst_threshold = 8
sensitive_threshold = 1
sensitive_patterns = [
  "sudo*",
  "rm -rf*",
  "terraform destroy*",
  "kubectl delete*",
]
after_hours_start = 22
after_hours_end = 6
after_hours_risks = ["high", "blocked"]
```

Webhook 需要显式订阅：

```toml
events = ["approval_required", "exec_blocked", "exec_completed", "anomaly_detected"]
```

---

## keys/

### 用途

存储 Agent2SSH 管理的 SSH 密钥对。

### 目录结构

```text
~/.agent2ssh/keys/
  id_ed25519_work        # 私钥
  id_ed25519_work.pub    # 公钥
  deploy_key             # 私钥
  deploy_key.pub         # 公钥
```

### 管理方式

**生成新密钥**

通过 Desktop App 的 Keys 标签页生成 Ed25519 密钥对，或手动执行：

```bash
ssh-keygen -t ed25519 -C "agent2ssh" -f ~/.agent2ssh/keys/my_key -N ""
chmod 600 ~/.agent2ssh/keys/my_key
```

**导入现有密钥**

```bash
cp ~/.ssh/id_ed25519 ~/.agent2ssh/keys/
cp ~/.ssh/id_ed25519.pub ~/.agent2ssh/keys/
chmod 600 ~/.agent2ssh/keys/id_ed25519
```

**在主机配置中引用**

```bash
agent2ssh host add web1 --host 192.168.1.10 --key ~/.agent2ssh/keys/my_key
```

### 注意事项

- 私钥文件权限必须为 `0600`（仅所有者可读写），Agent2SSH 自动设置正确权限
- 密钥删除后，引用该密钥的主机配置不会自动更新
- 建议为不同用途使用不同的密钥（工作、个人、部署等）
- 支持的密钥类型：ed25519、rsa、ecdsa

---

## 配置文件权限

Agent2SSH 自动设置敏感文件的权限：

| 文件 | 权限 | 说明 |
|------|------|------|
| `daemon.token` | 0600 | 仅所有者可读写 |
| `daemon_tokens.toml` | 0600 | 包含明文 token 时仅所有者可读写；使用 `token_env` 时仍建议限制权限 |
| `keys/*` (私钥) | 0600 | 仅所有者可读写 |

在 Unix 系统上权限自动设置。手动创建文件时，请确保设置正确权限：

```bash
chmod 600 ~/.agent2ssh/daemon.token
chmod 600 ~/.agent2ssh/daemon_tokens.toml 2>/dev/null || true
chmod 600 ~/.agent2ssh/keys/my_key
```

---

## 备份和迁移

### 备份

```bash
# 备份整个配置目录
tar -czf agent2ssh-backup.tar.gz ~/.agent2ssh/

# 仅备份配置文件（排除运行时文件和审计日志）
tar -czf agent2ssh-config.tar.gz \
  ~/.agent2ssh/hosts.json \
  ~/.agent2ssh/policy.toml \
  ~/.agent2ssh/risk_rules.toml \
  ~/.agent2ssh/approval_policies.toml \
  ~/.agent2ssh/playbooks.toml \
  ~/.agent2ssh/remotes.toml \
  ~/.agent2ssh/daemon_tokens.toml \
  ~/.agent2ssh/webhook.toml \
  ~/.agent2ssh/keys/
```

### 迁移到新机器

```bash
# 在新机器上安装 Agent2SSH
brew tap lengyuqu/agent2ssh && brew install agent2ssh

# 解压配置
tar -xzf agent2ssh-config.tar.gz -C ~/

# 确保权限正确
chmod 600 ~/.agent2ssh/keys/* 2>/dev/null || true

# 启动守护进程（自动生成新的 daemon.token）
agent2ssh daemon start
```

### 注意事项

- **不要备份 `daemon.pid`**（运行时文件）
- **谨慎处理 `daemon.token`**（包含敏感认证信息）
- **谨慎处理 `daemon_tokens.toml`**（使用明文 `token` 时同样包含敏感认证信息；优先迁移环境变量）
- `audit.jsonl` 可能很大，可以选择性备份
- 迁移后需要更新 `remotes.toml` 中引用的环境变量

---

## 校验和验证

从 GitHub Releases 下载 Agent2SSH 二进制文件后，建议验证文件完整性以确保未被篡改。

### 下载校验和文件

每个 release 版本均附带 `CHECKSUMS-SHA256.txt` 文件，包含所有发布资产的 SHA256 校验和。从 release 页面同时下载二进制文件和对应的校验和文件。

### 验证步骤

**macOS：**

```bash
# 将校验和文件和二进制放在同一目录
cd ~/Downloads

# 验证
shasum -a 256 -c CHECKSUMS-SHA256.txt --ignore-missing
```

**Linux：**

```bash
cd ~/Downloads

# 验证（两种命令均可）
sha256sum -c CHECKSUMS-SHA256.txt --ignore-missing
# 或
shasum -a 256 -c CHECKSUMS-SHA256.txt --ignore-missing
```

**Windows (PowerShell)：**

```powershell
# 计算文件哈希
Get-FileHash .\agent2ssh.exe -Algorithm SHA256

# 手动对比 CHECKSUMS-SHA256.txt 中的值
type CHECKSUMS-SHA256.txt
```

### 预期输出

验证通过时，每行输出 `OK`：

```text
agent2ssh-x86_64-apple-darwin: OK
agent2ssh-mcp-x86_64-apple-darwin: OK
agent2ssh-daemon-x86_64-apple-darwin: OK
```

如果校验和不匹配，输出将包含 `FAILED`。此时**请勿使用**该文件，并重新下载或从其他源获取。

### 团队配置分享验证

使用 `agent2ssh config-export` 导出团队配置时，可通过 SHA256 验证文件完整性：

`config-export` 会移除 host 的 `key_path` 和 `password`。`config-import` 遇到同名 host 时会更新地址、用户、端口、jump host、标签和 env/role/owner/risk_override 等非凭据字段，并保留本机已有的 key/password。

```bash
# 导出配置
agent2ssh config-export --json > team-config.json

# 生成校验和
shasum -a 256 team-config.json

# 分享给团队成员后，他们可验证
shasum -a 256 -c <checksum> team-config.json
```

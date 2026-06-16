# 配置指南

Agent2SSH 的所有配置和数据文件存储在 `~/.agent2ssh/` 目录下。本指南详细说明每个配置文件的用途、格式和使用方法。

## 目录结构

```text
~/.agent2ssh/
  hosts.json       # 主机配置文件（自动管理）
  daemon.token     # 守护进程认证令牌（自动生成）
  daemon.pid       # 守护进程 PID（自动管理）
  audit.jsonl      # 执行审计日志（自动追加）
  risk_rules.toml  # 用户自定义风险规则
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
| `risk_override` | string | 否 | 覆盖该主机的风险等级（low/medium/high） |
| `tags` | array | 否 | 标签列表，用于分组和批量执行 |
| `env` | string | 否 | 环境标签，用于按生产、预发、开发等环境过滤 |
| `role` | string | 否 | 角色标签，用于按 web、db、worker 等职责过滤 |
| `owner` | string | 否 | 负责人或团队标签，用于按归属过滤 |

### 注意事项

- 文件由 Agent2SSH 自动管理，手动编辑后需确保 JSON 格式正确
- `name` 字段必须唯一
- `jump_host` 必须引用已存在的主机别名
- `risk_override` 设置为 `"low"` 可以跳过该主机上所有命令的风险确认
- `risk_override` 不能降级 `blocked` 命令；内置或用户规则判定为 `blocked` 的命令仍会被拒绝
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

守护进程 HTTP API 的 Bearer Token 认证令牌。

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

## risk_rules.toml

### 用途

定义用户自定义的风险规则，扩展或覆盖内置风险分类。

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

# 高风险命令（需要 force 确认）
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
- **优先级**：blocked > high > medium > 内置规则

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
- 用户规则优先于内置规则
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
| `risk_override` | string | 否 | 覆盖所有步骤的风险等级 |

### 执行行为

- 步骤按顺序执行
- 任何步骤失败（非零退出码或错误）时停止执行，返回已完成步骤的部分结果
- 返回 `success` 状态和 `total_duration_ms` 总耗时
- 使用 MCP 工具 `ssh_playbook_run` 或 Daemon API `POST /playbooks/run` 执行
- 高风险步骤需要 `force: true` 才能执行

### 注意事项

- 步骤中的 shell 变量（如 `$(date)`）会在远程主机上展开
- `risk_override` 可以统一设置所有步骤的风险等级
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

### Token 解析优先级

1. `token_env`：从环境变量读取（推荐，更安全）
2. `token`：直接使用明文令牌

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
url = "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"

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

默认仅订阅 `approval_required` 事件。

### Slack 集成

当 URL 包含 `hooks.slack.com` 时，自动使用 Slack Block Kit 格式：

- 标题显示事件类型（Approval Required / Command Blocked / Command Completed）
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

配置 daemon 层的执行速率和 session 并发限额。限额在 daemon 进程内强制执行，覆盖 `/exec`、`/exec-multi`、`/playbooks/run`、session write、session open、WebSocket exec 和 `/daemons/localhost/exec`。

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
| `keys/*` (私钥) | 0600 | 仅所有者可读写 |

在 Unix 系统上权限自动设置。手动创建文件时，请确保设置正确权限：

```bash
chmod 600 ~/.agent2ssh/daemon.token
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
  ~/.agent2ssh/risk_rules.toml \
  ~/.agent2ssh/playbooks.toml \
  ~/.agent2ssh/remotes.toml \
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

```bash
# 导出配置
agent2ssh config-export --json > team-config.json

# 生成校验和
shasum -a 256 team-config.json

# 分享给团队成员后，他们可验证
shasum -a 256 -c <checksum> team-config.json
```

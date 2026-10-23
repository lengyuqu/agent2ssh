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
  app.log          # 结构化诊断日志（JSONL，自动轮转）
  secrets.enc      # App 自建加密凭据库（主密码派生密钥，自动管理）
  known_hosts.json # SSH 主机指纹信任记录（自动管理）
  .hosts.lock      # hosts.json 跨进程写锁（自动管理）
  .audit.lock      # audit.jsonl 跨进程追加锁（自动管理）
  .app_log.lock    # app.log 跨进程写锁（自动管理）
  .known_hosts.lock # known_hosts.json 跨进程写锁（自动管理）
  .sync.lock       # WebDAV 同步跨进程锁（自动管理）
  policy.toml      # 统一策略文件（推荐）
  risk_rules.toml  # 旧版用户自定义风险规则（兼容）
  approval_policies.toml # 旧版审批策略（兼容）
  execution_gate.toml    # 全局执行急停状态
  execution_limits.toml  # 执行速率和 session 并发限额
  anomaly.toml     # audit 异常检测阈值
  playbooks.toml   # Playbook 命令模板定义
  remotes.toml     # 远程守护进程注册表
  webhook.toml     # Webhook 通知配置
  webdav.toml      # 可选 WebDAV 同步配置
  redact_rules.json      # A24 脱敏规则（seed-once，权威；设置面板可增删改）
  highlight_rules.json   # B24 终端高亮规则（设置面板可增删改）
  copy_redact_rules.json # G4 复制块脱敏规则（seed-once，无编辑界面）
  recording.json   # 终端录制开关（默认关闭）
  recordings/      # 本地 asciicast v2 录制（敏感，不同步）
  sync_version.json # 最近一次 WebDAV 同步应用的全局版本标记
  backups/         # 同步/写入前的本地版本备份
  keys/            # SSH 密钥存储目录
```

---

## hosts.json

### 用途

存储所有 SSH 主机配置文件。由 CLI、MCP 和 Daemon API 自动管理，通常不需要手动编辑。

### 文件格式

JSON 格式，包含 `hosts` 数组，也可以包含由桌面端代理管理页维护的 `proxies` 数组：

```json
{
  "proxies": [
    {
      "id": "office-socks",
      "name": "Office SOCKS",
      "protocol": "socks5",
      "host": "127.0.0.1",
      "port": 1080,
      "username": null,
      "password": null
    },
    {
      "id": "corp-http",
      "name": "Corporate HTTP",
      "protocol": "http",
      "host": "proxy.example.com",
      "port": 8080,
      "username": "deploy",
      "password": "local-secret"
    }
  ],
  "hosts": [
    {
      "name": "web1",
      "host": "192.168.1.10",
      "user": "deploy",
      "port": 22,
      "key_path": "~/.ssh/id_ed25519",
      "passphrase": "$agent2ssh-secret$",
      "jump_host": null,
      "proxy_id": "office-socks",
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

私钥口令由桌面端录入后保存在 `secrets.enc`，`hosts.json` 中仅保留
`$agent2ssh-secret$` 引用；凭据库未解锁时，程序会拒绝新增明文私钥口令。
编辑主机时留空会保留原口令，切换为密码认证会清除已保存的私钥口令。
也可以在编辑表单中勾选“清除已保存的私钥口令”进行显式删除。主机列表、
新增/更新响应、CLI JSON、守护进程 HTTP 和 MCP 响应都不会回传密码或私钥口令。

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 主机别名，用于 CLI/MCP/API 中引用 |
| `host` | string | 是 | 主机地址（IP 或域名） |
| `passphrase` | string/null | 否 | 私钥口令引用；由凭据库管理，不应手工写入明文 |
| `user` | string | 否 | SSH 用户名 |
| `port` | integer | 否 | SSH 端口，默认 22 |
| `key_path` | string | 否 | SSH 私钥路径；Windows 上私钥的文件格式有额外限制，见注意事项。私钥旁存在 `<key_path>-cert.pub`（`ssh-keygen -s` 的默认输出）时，自动改用该 OpenSSH 用户证书认证，无需额外字段 |
| `jump_host` | string | 否 | ProxyJump 跳板机别名 |
| `proxy_id` | string | 否 | 代理配置 ID，用于通过 HTTP CONNECT 或 SOCKS5 建立 SSH TCP 连接 |
| `risk_override` | string | 否 | 覆盖该主机非 blocked 命令的风险等级（low/medium/high） |
| `tags` | array | 否 | 标签列表，用于分组和批量执行 |
| `env` | string | 否 | 环境标签，用于按生产、预发、开发等环境过滤 |
| `role` | string | 否 | 角色标签，用于按 web、db、worker 等职责过滤 |
| `owner` | string | 否 | 负责人或团队标签，用于按归属过滤 |

### 注意事项

- 文件由 Agent2SSH 自动管理，手动编辑后需确保 JSON 格式正确
- `name` 字段必须唯一
- `jump_host` 必须引用已存在的主机别名
- `proxy_id` 必须引用 `proxies` 中已存在的代理 ID；删除代理时，引用它的主机会自动切换回直连
- HTTP 代理使用 `CONNECT host:port`；SOCKS5 支持无认证和用户名/密码认证
- 设置主密码后，代理密码与主机密码会加密存入 `secrets.enc`，`hosts.json` 中只保留 `$agent2ssh-secret$` 引用句柄；未设置主密码前，历史明文会在解锁/设置主密码后迁移
- CLI `host add` 当前不提供代理参数；请通过桌面端 **Proxies** 页面和主机编辑表单维护代理，或在 `hosts.json` 中手动写入 `proxies` 与主机 `proxy_id`
- `risk_override` 设置为 `"low"` 可以降低该主机上非 `blocked` 命令的风险等级
- `risk_override` 不能降级 `blocked` 命令；内置或用户规则判定为 `blocked` 的命令仍会被拒绝；显式审批策略、scope、gate 和限额仍会生效
- SSH 握手、密码/密钥/agent 认证、agent 身份枚举与签名请求、通道建立均受会话传输超时约束。对端或 agent 不应答时返回对应操作的超时错误；agent 连接和身份枚举超时只丢弃 agent 连接，保留 SSH 连接供默认密钥回退认证，已发起 SSH 认证的签名超时则关闭 SSH 连接以避免迟到回复；服务器明确拒绝 exec、shell、PTY 或 subsystem 请求时立即返回拒绝原因，不会把请求已发送当作建立成功。
- `env`、`role`、`owner` 和 `tags` 可用于桌面端主机视图过滤，也可用于 CLI `host list` 过滤
- **Windows 链接的密码学后端是随包静态构建的 OpenSSL，不再是 WinCNG**：`src-tauri/Cargo.toml` 为 `ssh2` 同时开启 `openssl-on-win32` 与 `vendored-openssl`，因此 Windows 构建与 Linux/macOS 能力一致——支持 `curve25519-sha256`、`ssh-ed25519`/ECDSA 主机密钥、`aes256-gcm` 与 `chacha20-poly1305`，也能读取 OpenSSH 私钥容器格式。这不仅解决了密钥格式问题，也解决了握手问题：WinCNG 只提供 `diffie-hellman-group*` 系列，而 OpenSSH 10.0 起已移除这些组，交集为空，会在**认证之前**就失败（`key exchange failure`）。OpenSSL 为静态链入，二进制不依赖任何 `libssl*.dll`/`libcrypto*.dll`。用 `agent2ssh algo get` 查看本构建实际可协商的算法，用 `agent2ssh keys algorithm` 查看推荐密钥类型（该构建下为 `ed25519`）
- **认证失败时的可操作诊断仍然保留**：若某个构建的 libssh2 链接的是 WinCNG（已不是 Windows 的默认情形），其私钥读取器只认传统 PEM（`-----BEGIN RSA PRIVATE KEY-----` 与 `-----BEGIN DSA PRIVATE KEY-----`）；而 `ssh-keygen` 自 OpenSSH 7.8 起**默认**写出 OpenSSH 容器格式（`-----BEGIN OPENSSH PRIVATE KEY-----`，`-t rsa` 也一样），该格式在那种构建上无法用于认证，Ed25519 私钥则任何格式都不可用。此时认证失败会直接给出这一结论而不是笼统的 `unknown error`，并建议用 `ssh-keygen -p -m PEM -f <私钥>` 就地转换（仅 RSA/DSA）。**应用内生成的密钥总是采用本构建可用的类型**，无需手工转换：`agent2ssh keys algorithm` 查询推荐类型，`keys generate <名称>` 直接生成

---

## known_hosts.json

### 用途

记录 Agent2SSH 已信任的 SSH 主机指纹。首次连接某个主机时，后端会自动写入该文件；再次连接时会校验保存的 SHA256 指纹。如果同一主机的指纹发生变化，连接会被拒绝。

### 文件格式

JSON 对象，键为 `host:port` 形式的连接身份，值包含主机别名、解析后的地址、host key 算法、SHA256 指纹和 Unix 秒级时间戳：

```json
{
  "192.168.1.10:22": {
    "host": "web1",
    "address": "192.168.1.10",
    "host_key_algorithm": "ssh-ed25519",
    "fingerprint_sha256": "SHA256:abc123...",
    "first_seen_unix": 1781836215,
    "last_seen_unix": 1781836215
  }
}
```

### 注意事项

- 该文件自动管理，不需要人工确认主机指纹
- `.known_hosts.lock` 用于跨进程写锁，避免 CLI、daemon 和桌面端并发写入冲突
- 如果你确认远端主机重装或 SSH host key 合法轮换，可以删除对应条目后重新连接
- 不要把该文件提交到版本控制；它反映了本机信任状态

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
- scope 覆盖 ping、exec、playbook、SFTP、session、forward 和 connection 的按主机操作；非命令操作会使用类似 `ping`、`sftp upload a -> b`、`session_open`、`session_close`、`connect` 的操作字符串做匹配
- scoped token 只用于按主机绑定的操作；gate、主机/片段、策略、脱敏/高亮、算法、审批、webhook、团队配置和本地配置变更等全局控制面操作必须使用 unrestricted admin token
- 主机列表、审计、健康快照、连接、会话、转发和审批列表会按 scoped token 的 allowed hosts/tags 过滤；审计导出、趋势指标、事件流、待处理 OTP prompt 和 SSH sync diff 属于敏感全局读取，仅 unrestricted admin 可用
- resident session 列表同样按会话自身的 `local:<id>` 主机和标签过滤。读取缓冲输出使用操作字符串 `resident-session read` 检查主机、标签和命令 scope；关闭使用 `resident-session close`，并经过执行 gate、限流、风险和审批。列表和读取在 gate 暂停时仍可使用，但关闭会被拒绝。
- resident 命令执行期间仍可读取会话元数据与滚动输出。通过授权的关闭请求会取消当前命令及同一会话中排队的命令，结束 shell 并回收其进程树；关闭不必等到当前命令的执行超时。
- 桌面 SFTP 删除目录先请求 daemon 的 `POST /sftp/rm-rf/authorize`，完成 scope、gate、限额、风险和审批检查。daemon 只授权，不连接 SSH；桌面校验响应中的主机和路径一致后，在自身进程执行删除，使用桌面已解锁的凭据，不跨进程传递密码。普通路径默认属于高风险，确认删除后仍需在审批队列批准；`/`、`/*`、`/.` 被直接阻断。没有 `force` 或绕过审批的回退，daemon 未运行时应先启动服务。原 `POST /sftp/rm-rf` 仍由 daemon 授权并执行，供需要在 daemon 中执行的客户端使用；其 `timeout_secs`（默认 60 秒）约束 SSH/SFTP 传输操作，审批等待使用独立的审批 TTL。
- MCP 的 `local_exec` 和 `resident_session_*` 请求由 daemon 管理执行、预热及审批期限，不再受客户端固定 30 秒总超时限制；建立 HTTP 连接仍限时 10 秒。命令的 `timeout_secs` 继续生效，resident 命令超时仍会结束整个会话。
- PTY session 写入按已完成的输入行做授权和审计；daemon session 和 desktop 本地 session 都会缓存未完成输入，直到换行后再按完整命令授权；`session_read`、`session_list`、`forward_list` 这类观察操作通过 token/scope 控制，不默认写入 `audit.jsonl`

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
| `diagnostic_error` | 任意来源写入 `error` 级诊断日志（`app.log`）时；`host` 字段携带组件名，`command` 字段携带错误消息（已脱敏） |

默认仅订阅 `approval_required` 事件。`diagnostic_error` 为可选订阅，启用后每条 `error` 级诊断都会推送一次 webhook，适合接入告警通道。

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

配置 daemon 层的执行速率和 session 并发限额。限额在 daemon 进程内强制执行，覆盖 `/exec`、`/exec-multi`、`/exec/compare`、SFTP 操作、`/playbooks/run`、session write、session open、forward add、WebSocket exec、WebSocket terminal 和 `/daemons/localhost/exec`。

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

`per_minute = 0` 或 `max_sessions = 0` 表示该维度不限额。速率限额覆盖 WebSocket terminal 的 completed-line input；session 并发限额覆盖 WebSocket terminal open。超限请求返回 HTTP 429，写入 `blocked` audit，并发布 `limit_rejected` 事件。

---

## anomaly.toml

### 用途

配置 audit 滑动窗口异常检测。每次执行写入 audit 后，Agent2SSH 会检测 source 频率突增、敏感命令模式和非常规时段高危操作；命中后发布 `anomaly_detected` SSE 事件，并可通过 webhook 订阅同名事件。

此外，daemon 会对 `error` 级诊断日志做滑动窗口聚合：当窗口内错误数达到 `diagnostic_error_threshold` 时发布一次 `anomaly_detected`（kind=`diagnostic_error_burst`），并由 `diagnostic_cooldown_secs` 抑制重复告警 —— 即错误风暴只产生一条聚合告警，而非每条错误一次 webhook。

### 默认行为

| 配置 | 默认值 | 说明 |
|------|--------|------|
| `enabled` | `true` | 是否启用异常检测 |
| `window_secs` | `300` | 滑动窗口长度 |
| `source_burst_threshold` | `10` | 同一 source 在窗口内达到该执行数触发 |
| `sensitive_threshold` | `1` | 敏感模式命中次数阈值 |
| `after_hours_start` | `22` | 非常规时段起始小时，UTC |
| `after_hours_end` | `6` | 非常规时段结束小时，UTC |
| `diagnostic_error_threshold` | `5` | 窗口内 `error` 级诊断数达到该值触发聚合告警 |
| `diagnostic_cooldown_secs` | `120` | 两次诊断错误聚合告警之间的最小间隔 |

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

通过 Desktop App 的 Keys 标签页生成，或用 CLI：

```bash
agent2ssh keys algorithm          # 本构建推荐的类型（ed25519 或 rsa）
agent2ssh keys generate id-work   # 按推荐类型生成；只输出私钥路径
agent2ssh keys generate id-work --type rsa --json
```

生成过程由 Agent2SSH 后端实现，不调用系统 `ssh-keygen`。**默认类型由本构建的 SSH 库能力决定，而不是由操作系统决定**：能读取 OpenSSH 私钥容器的构建用 Ed25519，不能的用 RSA 并直接写成传统 PEM（3072 位）。Windows 现在属于前者——它链接的是 OpenSSL——因此推荐 `ed25519`；只有 WinCNG 构建（在 Windows 上需要显式关闭 vendored OpenSSL 才会出现）仍会退回 RSA。因此本应用生成的密钥一定可用于本应用的认证——这正是 `keys algorithm` 存在的理由，脚本应当询问而不是假定。

> 若桌面端选择的是本构建无法读取的类型（例如在 WinCNG 构建上选 Ed25519），表单会在生成前给出提示；未手动选择时，请求不带类型，由后端决定。

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
- 可生成/导入的密钥类型：ed25519、rsa(PEM)、ecdsa；在 Windows 上实际可认证的只有 `rsa` 的传统 PEM 形式

---

## 配置文件权限

Agent2SSH 自动设置敏感文件的权限：

| 文件 | 权限 | 说明 |
|------|------|------|
| `daemon.token` | 0600 | 仅所有者可读写 |
| `daemon_tokens.toml` | 0600 | 包含明文 token 时仅所有者可读写；使用 `token_env` 时仍建议限制权限 |
| `keys/*` (私钥) | 0600 | 仅所有者可读写 |
| `redact_rules.json` / `highlight_rules.json` / `copy_redact_rules.json` | 0600 | 三个规则文件；统一由 `store::write_private_json` 加固，新增规则文件时不会漏掉这一步 |

在 Unix 系统上权限自动设置。手动创建文件时，请确保设置正确权限：

```bash
chmod 600 ~/.agent2ssh/daemon.token
chmod 600 ~/.agent2ssh/daemon_tokens.toml 2>/dev/null || true
chmod 600 ~/.agent2ssh/keys/my_key
```

---

## WebDAV 同步

Agent2SSH 的桌面上传与 CLI/daemon 同步共用 `webdav_sync::SYNCABLE_FILES` 白名单：`hosts.json`、`playbooks.toml`、`risk_rules.toml`、`policy.toml`、`policy.json`、`execution_limits.toml`、`anomaly.toml`、`webhook.toml`、`app_preferences.json`。

不包含 `keys/` 私钥目录、`secrets.enc` 凭据库、`known_hosts.json` 信任库、`snippets.json`、`approval_policies.toml`、`daemon.token`、`daemon_tokens.toml`、`remotes.toml`、审计/日志、录制、活动转发等本机状态。白名单里的 `hosts.json` 或 `webhook.toml` 仍可能含有旧格式凭据或 webhook secret，因此上传要求加密。

### 桌面端加密上传

桌面端 **WebDAV Sync** 页面需要单独设置“同步加密密码”；它与 WebDAV 登录密码、应用主密码用途不同。先设置或解锁应用主密码，再保存同步加密密码；该密码存入加密凭据库，配置读取只返回“已配置”状态，不回传密码。保存成功后输入框清空，之后留空保存会保留已有密码。保管好同步加密密码，解密已上传备份仍需要它。

没有同步密码的旧启用配置不会继续上传明文。凭据库未解锁或同步密码不可读取时，也会在收集文件和发起网络请求前拒绝上传。连接测试只验证 WebDAV 可访问性，不代表已具备上传所需的加密密码。

桌面把白名单文件封装为一个 JSON 快照，再用 Argon2id 派生密钥和 AES-256-GCM 加密，作为 `application/octet-stream` 上传到配置的远端文件路径。默认路径仍为 `agent2ssh/agent2ssh-sync.json`，文件后缀不代表内容是明文 JSON。这个单文件备份格式不能作为下述 CLI/daemon 版本化同步的 `sync_version.json` 使用。自动上传同样经过上述密码和加密检查。

桌面同步面板提供“下载备份”和“从云端恢复”：先保存远端地址及同步加密密码，无需开启自动上传。下载会验证备份后将原始密文保存到选定的本地位置；取消保存对话框不会下载。恢复确认后会重新获取云端备份，完整解密并校验版本、时间戳、文件白名单、重复路径、字节数和 JSON/TOML 格式，上传和下载统一限制为 20 MiB（包括加密封装），超限上传在联系 WebDAV 前拒绝，避免产生无法下载恢复的备份。错误密码、篡改、非法文件或下载失败均不会修改现有配置。

恢复前会自动创建 `pre-cloud-restore` 快照，恢复后界面显示撤销快照 ID，可在“配置快照”面板恢复之前的状态。恢复精确替换可移植配置集合，移除备份中不存在的白名单文件及失效的 `sync_version.json`；私钥、凭据库、信任记录、daemon token 和同步连接设置保留。写入中途失败会尝试回滚，若回滚也失败，错误中会保留恢复快照 ID。恢复后立即刷新主机、分组和代理列表，偏好设置需重启应用生效。桌面上传、下载和恢复串行执行，避免与自动上传重叠。

需要恢复备份内容时，先自行从 WebDAV 下载上述密文文件，再用上传时保存的独立同步加密密码在本地解密：

```text
agent2ssh webdav decrypt <input> <output> [--sync-password-env VAR]
```

`<input>` 是已下载的密文文件，`<output>` 是解密后的原始 JSON 快照。命令默认从 `AGENT2SSH_SYNC_PASSWORD` 环境变量读取同步加密密码，也可以用 `--sync-password-env VAR` 指定其他变量名；请将保存的同步密码提供给该变量。输出文件必须尚不存在，命令会拒绝覆盖已有文件。整个解密过程离线运行，不访问 WebDAV，也不会把快照应用到当前配置。

### CLI/daemon 版本化同步配置

可以使用环境变量：

```bash
export AGENT2SSH_WEBDAV_URL="https://dav.example.com/agent2ssh"
export AGENT2SSH_WEBDAV_USERNAME="alice"
export AGENT2SSH_WEBDAV_PASSWORD="app-password"
export AGENT2SSH_SYNC_PASSWORD="separate-backup-password"
```

也可以写入 `~/.agent2ssh/webdav.toml`：

```toml
url = "https://dav.example.com/agent2ssh"
username = "alice"
password_env = "AGENT2SSH_WEBDAV_PASSWORD"
sync_password_env = "AGENT2SSH_SYNC_PASSWORD"
```

CLI/daemon 从自己的选项、环境变量或 `webdav.toml` 读取同步密码，不自动读取桌面保存的同步密码。上传必须配置同步密码，每个文件分别加密；拉取加密版本时必须提供对应密码。

### CLI 命令

```bash
# 上传本地可同步配置。成功后远端 sync_version.json 的 global_version +1。
agent2ssh webdav push

# 拉取远端版本。拉取前会先创建本地备份。
agent2ssh webdav pull

# 查看本地和远端的全局版本标记。
agent2ssh webdav status

# 检查过 status 后，显式接受覆盖已分叉的一侧。
agent2ssh webdav push --force
agent2ssh webdav pull --force
```

三个命令都支持 `--url`、`--username`、`--password`、`--password-env`、`--sync-password`、`--sync-password-env`、`--config` 和 `--json`；`push`/`pull` 还支持显式冲突覆盖 `--force`。

### CLI/daemon 备份与版本标记

- 每次 `push` 或 `pull` 前都会在 `~/.agent2ssh/backups/sync-<timestamp>-<id>/` 创建本地版本备份。
- 同步状态使用稳定的 portable-config SHA-256 摘要区分 `in_sync`、`local_ahead`、`remote_ahead`、`diverged` 和 `unknown`；摘要只包含排序后的路径与文件哈希，不受时间、方向和 sync id 影响。
- schema v2 `push` 会把文件上传到按 `sync_id` 命名的不可变 `versions/` 对象，最后用 WebDAV ETag 条件写入 `sync_version.json`。远端并发变化会中止 marker 提交，不会让当前版本指向混合文件。
- `pull` 会先把完整远端版本下载到内存并校验每个文件的 SHA-256，再创建本地备份并原子替换文件，避免校验失败留下半应用状态。
- 默认拒绝覆盖 `diverged` 或较新的一侧；只有用户检查 `status` 后显式传入 `--force` 才允许覆盖。
- `pull` 兼容旧版本远端 manifest 中的 `known_hosts.json`、`secrets.enc`、`snippets.json`、`approval_policies.toml`，但会跳过它们，不覆盖本机数据。
- schema v2 中缺少白名单里的可同步文件表示删除；不在白名单的本机文件不受影响。
- daemon 的 `GET /webdav/status` 返回相同的版本、摘要、冲突状态和 metadata 警告字段。

### 本地配置快照与模板恢复

桌面“配置快照”独立于 WebDAV，无需配置远端。快照保存白名单文件及本机 `sync_version.json` 的内容和存在状态。恢复会删除快照中原本不存在、后来新增的可迁移文件，因此从空配置应用模板后，恢复自动生成的 `pre-template` 快照可以移除新增的策略与执行限额。

恢复前先验证完整 manifest、文件名白名单及所有捕获文件，并读取完全部文件；非法、重复或缺失条目会报错，不会部分应用。通过检查后先保存当前状态的 `pre-restore` 安全备份，再恢复文件并删除快照范围内的新增项。私钥、凭据库、主机信任等非同步文件不参与恢复，也不会因快照中缺失而被删除。既有 `backup_manifest.json` 格式仍可使用；执行限额修改后需要重启 daemon 才生效。

---

## 终端录制

终端录制默认关闭。可以从桌面端“会话录制”页面启用，也可以向 daemon 发送
`PUT /recordings/config`，正文为 `{"enabled":true}`。开关只影响之后新建的
WebSocket 终端会话。

录制使用 asciicast v2 JSONL 格式，保存在 `~/.agent2ssh/recordings/<UUID>.cast`。
Unix 上目录和文件分别限制为 owner-only 权限；Windows 上沿用 Agent2SSH 的
owner-only ACL 限制。录制内容是原始终端输出，可能包含密码、令牌、个人数据和
命令结果，应按敏感数据处理。`recording.json` 和整个 `recordings/` 目录均不参与
WebDAV 同步。

---

## redact_rules.json

A24：应用在脱敏时使用的正则规则集。它是**权威的**而不是叠加的——`store::redact_sensitive_text`
每次调用都读这个文件，而**审计记录、Webhook 通知、Playbook 结果、诊断导出**四条链路都经过它。

生命周期：

- 首次运行时用内置规则播种：私有 IPv4（10/172.16-31/192.168）、IPv6 环回与链路本地、
  Bearer、`sk-` 与 `AKIA`、JWT、32 位以上十六进制块。
- 之后文件归使用者所有：**删掉的规则不会被重新播种**，只有显式「恢复默认」才会回来。
- 文件缺失、不可读或 JSON 损坏时**回落到内置集合**——配置坏掉绝不能让脱敏变弱。
- 通过 `store::write_private_json` 写入，Unix 上为 `0600`。

### 桌面端编辑

设置面板的「脱敏规则」可以增删改，护栏如下：

- **匹配模式（pattern）就是规则的唯一身份**，没有 id、没有名字。「新增一条已存在的 pattern」
  「改名撞上另一条规则」「改一条已不存在的 pattern」都会被拒绝并报错，而不是静默覆盖或静默新增。
- 模式与替换文本都在**写入前**校验：正则非法，或者模式是**零宽**的
  （如 `a*`、`\b`——它会把替换文本插进每个字符之间而非替换秘密）都不会被写入。
- 替换文本留空的含义是**删除匹配内容**，面板里显示为 `(removed)`。
- 内置规则带 `Built-in` 徽章；删除内置规则时确认框会额外说明
  「应用写入的所有位置都不再对这一类秘密脱敏」。
- **删空是允许的，但后果很重**：空规则集意味着审计记录、通知与诊断导出**原样写出**。
  面板把这个状态渲染成警示而不是空列表，「恢复默认」是唯一的回退路径。

CLI、MCP 与 daemon 目前**不**暴露这些操作，只有桌面端可以编辑。
规则文件也**不参与 WebDAV 同步**（不在 `webdav_sync::SYNCABLE_FILES` 里，与另两个规则文件一致），
因此在 A 机器上改的规则不会带到 B 机器。

### 与 copy_redact_rules.json 的区别

终端「复制代码块」走的是**另一套独立规则**（`copy_redact_rules.json`，G4），它没有编辑界面。
两者互不影响：在设置面板里新增一条规则，**不会**改变复制到剪贴板时的脱敏结果。

### 直接编辑文件

面板和文件是同一份数据，也可以直接改（外部编辑后立即生效，这里没有缓存）：

```json
[
  { "pattern": "\\b10\\.\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}\\b", "replacement": "<REDACTED:ip>" },
  { "pattern": "secret-project", "replacement": "<REDACTED:project>" }
]
```

替换文本按**字面量**使用（内部走 `regex::NoExpand`），因此 `$1` 不会被展开成捕获组——
写 `$1` 只会把匹配到的东西替换成字面的 `$1` 两个字符，而不会把秘密原样放回去。

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
agent2ssh-aarch64-apple-darwin: OK
agent2ssh-mcp-aarch64-apple-darwin: OK
agent2ssh-daemon-aarch64-apple-darwin: OK
```

如果校验和不匹配，输出将包含 `FAILED`。此时**请勿使用**该文件，并重新下载或从其他源获取。

### 团队配置分享验证

使用 `agent2ssh config-export` 导出团队配置时，可通过 SHA256 验证文件完整性：

`config-export` 会移除 host 的 `key_path` 和 `password`。`config-import` 遇到同名 host 时会更新地址、用户、端口、jump host、proxy_id、标签和 env/role/owner/risk_override 等非凭据字段，并保留本机已有的 key/password。

```bash
# 导出配置
agent2ssh config-export --json > team-config.json

# 生成校验和
shasum -a 256 team-config.json

# 分享给团队成员后，他们可验证
shasum -a 256 -c <checksum> team-config.json
```

---

## 环境变量

除上述配置文件外，以下环境变量可调整 Agent2SSH 各 surface（桌面 App / CLI / MCP / daemon）的运行时行为。环境变量优先级高于默认值；除 `AGENT2SSH_CONFIG_DIR` 外都有安全的内置缺省，通常无需设置。

| 变量 | 作用域 | 默认值 | 说明 |
|------|--------|--------|------|
| `AGENT2SSH_CONFIG_DIR` | 全部 | `~/.agent2ssh` | 覆盖配置与数据目录（`hosts.json`、`audit.jsonl`、`keys/`、`app.log` 等）。测试与多实例隔离常用。 |
| `AGENT2SSH_SOURCE` | CLI / MCP / Tauri | 各 surface 自带标签（如 `cli`、`mcp`、`desktop`） | 覆盖审计与 Live Activity 中的来源标签，用于区分是哪个 agent/客户端发起的操作。 |
| `AGENT2SSH_DAEMON_ADDR` | daemon（绑定）+ 全部 client（拨号） | `127.0.0.1:7722` | 本地 daemon 的监听地址。daemon 按此绑定；CLI/MCP/Tauri/Webhook 链接经同一解析器拨号，绑定到通配地址（`0.0.0.0`/`::`）时 client 自动回退到回环。绑定非回环地址会写一条 `warn` 诊断提示对外暴露。 |
| `AGENT2SSH_TRACE_ID` | CLI / MCP | 自动生成 | 由上游调用方（如 agent）注入的关联 ID，写入诊断 `trace_id` 字段并随转发请求透传，便于跨 surface 串联一次操作。 |
| `AGENT2SSH_LOG` | daemon | `info` | daemon `tracing` 的 `EnvFilter` 级别（如 `debug`、`agent2ssh=debug,info`）。 |
| `AGENT2SSH_LOG_FORMAT` | daemon | `text` | daemon 控制台日志格式：`text` 或 `json`。 |
| `AGENT2SSH_BRIDGE_DEPS` | daemon | 关闭（仅转 `agent2ssh*`） | 是否把依赖层（hyper/reqwest/ssh2/…）的 `WARN`/`ERROR` 也桥接进 `app.log`：`1`/`true`/`all` 用内置传输层前缀集，逗号分隔则自定义前缀，未设/`0`/`false` 关闭。依赖层事件不触发 error 告警（防回环），仅落盘排查传输问题用。 |
| `AGENT2SSH_MASTER_PASSWORD` | CLI / MCP / daemon | 无 | 解锁 App 自建加密凭据库（`secrets.enc`，Argon2id + AES-256-GCM）的主密码（K1）。设置后这些无界面 surface 才能解密密码型主机的 SSH 密码；未设置时密码型主机不可用（密钥型/agent 认证不受影响）。桌面端改用启动解锁对话框，不读此变量。首次写入凭据时若已设此变量会用它初始化库。 |
| `AGENT2SSH_SECRETS_BACKEND` | 全部 | 生产 `encrypted`，测试 `memory` | 凭据库后端：`encrypted` 用真实加密库，`memory` 用进程内内存库（仅测试/无界面隔离用，重启即丢）。一般无需设置。 |
| `AGENT2SSH_WEBDAV_URL` | CLI | 无 | WebDAV 同步 collection URL。也可写入 `webdav.toml` 或用 `agent2ssh webdav * --url` 传入。 |
| `AGENT2SSH_WEBDAV_USERNAME` | CLI | 无 | WebDAV Basic Auth 用户名。 |
| `AGENT2SSH_WEBDAV_PASSWORD` | CLI | 无 | WebDAV Basic Auth 密码。也可在 `webdav.toml` 中用 `password_env` 指向自定义环境变量。 |
| `AGENT2SSH_SYNC_PASSWORD` | CLI / daemon 版本化同步 | 无 | 同步文件的 AES-256-GCM 加密密码，密钥由 Argon2id 派生；上传必须提供，拉取加密备份需使用相同密码。可用 `sync_password_env` 指向自定义变量；桌面上传改用凭据库里的独立同步密码。 |
| `AGENT2SSH_CONTAINER_RUNTIME` | CLI / 桌面 | 自动探测 | 固定容器发现使用的本地运行时：`docker`、`wslc`（Windows 的 WSL 容器 CLI）或 `apple`（Apple container）。不设时按 `docker → wslc → apple` 取第一个已安装的运行时，未安装的自动跳过；桌面端的容器面板与 e2e 脚本共用这一条选择逻辑。 |

> 说明：`remotes.toml` 与 `daemon_tokens.toml` 中的 `token_env` 字段引用的是**用户自定义**的环境变量名（如示例中的 `AGENT2SSH_PROD_TOKEN`），由你自行设置，不属于上表的内置变量。

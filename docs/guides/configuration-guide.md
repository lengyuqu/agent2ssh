# 配置指南

Agent2SSH 的所有配置和数据文件存储在 `~/.agent2ssh/` 目录下。本指南详细说明每个配置文件的用途、格式和使用方法。

## 目录结构

```text
~/.agent2ssh/
  hosts.json       # 主机配置文件（自动管理）
  daemon.token     # 守护进程认证令牌（自动生成）
  daemon_tokens.toml # 可选的 scoped Bearer Token
  daemon.pid       # 守护进程 PID（自动管理）
  daemon.owner     # 记录哪个 daemon 是桌面应用启动的（自动管理）
  audit.jsonl      # 执行审计日志（自动追加）
  app.log          # 结构化诊断日志（JSONL，自动轮转，写前脱敏）
  daemon.log       # daemon 的 stdout/stderr（原样落盘，不脱敏，仅在 spawn 新进程时按大小轮转）
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
  execution_gate.json    # 全局执行急停状态
  execution_limits.toml  # 执行速率和 session 并发限额
  anomaly.toml     # audit 异常检测阈值
  playbooks.toml   # Playbook 命令模板定义
  remotes.toml     # 远程守护进程注册表
  webhook.toml     # Webhook 通知配置
  environments.toml # 可选的自定义本地终端环境（local:<id>）
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

### 两个日志文件不是同一份东西

`app.log` 是应用写的结构化诊断（JSONL，每条都过 `redact_sensitive_text`，5 MB 内联轮转、保留 3 代），四个 surface 都往里写。`daemon.log` 是 daemon 进程自己的 stdout/stderr 重定向，内容是 `tracing` 的 `fmt` 输出，**没有任何脱敏**：`AGENT2SSH_LOG` 为默认 `info` 时，`POST /exec`、`/local-exec`、`/exec-multi`、`/exec/compare` 四个 handler 各会写一行 `command=…`，命令原文就落在这个文件里。只有 `WARN`/`ERROR` 经 `DiagnosticBridgeLayer` 桥进 `app.log` 时才会被脱敏，`info`/`debug` 不会过桥。所以排查完要分享或上传 `daemon.log` 之前，先按命令行内容决定是否需要手工裁剪。

五点容易想当然，逐条写清：

**一、`redact_sensitive_text` 是模式匹配，不是"没有秘密"的保证。** 它做三件事：跑十条内置正则（私网 IPv4、IPv6 loopback/link-local、`Bearer <20+>`、`sk-…`、`AKIA…`、JWT、32 位以上十六进制）；按空白切词后看 `key=value` 的 key 是否命中一份**精确匹配**的 11 个拼写（`password` `passwd` `token` `secret` `api-key` `apikey` `access-token` `authorization` `bearer` `cookie` `set-cookie`，比对前去掉前导 `-` 和尾部 `:`），命中则只把值替换掉；最后按高熵启发式兜底（≥40 字符且同时含字母、数字、`+/_-=`，或 ≥32 位纯十六进制）。因为 key 表是精确匹配而不是后缀或包含匹配，**表外拼写一律原样留下**——实测（`--no-default-features` 下直接调 `store::redact_sensitive_text`）：`mysql --password=hunter2` 抹成 `--password=[REDACTED]`，而 `mysql -p hunter2`、`mysql -phunter2`、`openssl enc -pass pass:hunter2`、`export API_TOKEN=abc123`、`export API_KEY=abc123`、`export PGPASSWORD=abc123` 六种全部原样输出。注意 `api-key` 与 `apikey` 在表里、`api_key` 不在——漏的是**一整类下划线拼写**，不是个别单词。这条边界不只影响日志：同一个函数还负责 `audit.jsonl` 的 `command` 字段、审批通知外发时的 `command`（Slack 或任意 webhook 的 JSON 载荷）、以及下面第五点的诊断包尾部。所以一个漏网的口令不只留在本机日志里，它会随审计记录长期留存、并随 webhook 发到外部。

**二、写进 `daemon.log` 的命令未必是用户发的那条。** 四个 `command=…` 里只有一个会变：`/exec` 走 `preflight_rewritable_request`，审批人可以批准一段改写过的文本，handler 在 `:1725-1727` 覆盖 `req.command` 之后才打日志，所以那一行是**权威化后、真要执行**的文本。`/local-exec` 走 `preflight_guarded_request`（`rewritable: false`），`/exec-multi` 与 `/exec/compare` 不经过这两个 helper 而是 `authorize_targets`，三者都不改写，日志里就是请求原文。用日志反推"用户当时发了什么"之前先确认是哪一个端点。

**三、两份日志不能按 `trace_id` 对上。** `trace_id` 是 `DiagnosticBridgeLayer` 在桥接时从 `REQUEST_TRACE_ID` 取出后塞进 `app.log` 那条 JSON 的字段，`daemon.log` 走的是 `tracing_subscriber::fmt::layer()`，不读 task-local，所以**一行 `trace_id` 都没有**。同一请求在两份文件里的关联只能靠时间戳与主机名。

**四、轮转只在 spawn 新进程时按大小触发，而且保留三代。** 入口只有一个：`start_daemon_background` 在真正 spawn 前调 `append_log_file()`（`daemon_control.rs:458`），后者先跑 `rotate_daemon_log_if_needed(…, 5 MB)` 再以追加方式打开。因此：已有健康 daemon 时 `daemon start` 提前返回（CLI 与桌面都有这道判断），**不轮转**；`daemon restart` 一定 spawn，一定检查；文件超过 5 MB 才真的转，转的时候 `.log.2 → .log.3`、`.log.1 → .log.2`、当前 → `.log.1`，即**最多三代**。运行中的 daemon 不会自己截断，日志涨到多大都不转。

**五、导出诊断包会把 `daemon.log` 的尾部带走。** `export_diagnostic_bundle` 在产物里追加 `## daemon.log tail` 段，取最后 300 行，每行过一遍 `redact_sensitive_text`（`diagnostics.rs:519-520`）——也就是说上面第一、二两点的漏网内容会随包一起离开本机。"分享前先裁剪"这条提醒真正会撞上的场景就在这里，不只是手工上传日志时。

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
主机密码、私钥口令和代理密码均遵循同一策略：凭据库锁定或加密失败时，
配置保存会失败，不会将新凭据作为明文写入。旧版 `--password` 脚本也需要先
解锁凭据库或设置 `AGENT2SSH_MASTER_PASSWORD`；推荐使用 `--password-stdin`，
避免密码出现在进程参数中。旧配置若已有明文凭据，锁库时保留原文件但拒绝
需要保存该配置的操作；解锁后会尝试迁移，旧备份中的明文不会自动清除。
`host import-config` 跳过已有的同名主机，保留其所有字段及凭据；新别名才会导入。
首次交互式设置主密码时，CLI 和桌面设置界面要求再次输入并核对；CLI 通过
`--password`、环境变量或管道提供主密码时保持单次输入，方便既有脚本使用。
旧版本在主机密码已是凭据引用时，可能将同一主机的新私钥口令写入
`hosts.json` 明文；如曾遇到该场景，应在本机私下检查配置及其备份，避免
向日志或他人展示口令。修复不会自动清除已有备份中的明文。
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
| `key_path` | string | 否 | SSH 私钥路径；各平台可读的私钥格式一致，没有 Windows 专属限制（见本节「注意事项」）。私钥旁存在 `<key_path>-cert.pub`（`ssh-keygen -s` 的默认输出）时，自动改用该 OpenSSH 用户证书认证，无需额外字段 |
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
- **各平台的密码学能力一致，且没有任何平台需要 OpenSSL**：SSH 传输层是 `russh`，密码学后端是 `ring`——两者都是纯 Rust，默认构建里没有 `ssh2`、`libssh2-sys` 或 `openssl-sys`，因此也没有 `libssl*.dll`/`libcrypto*.dll` 要随包分发。支持 `curve25519-sha256`、`ssh-ed25519`/ECDSA 主机密钥、`aes256-gcm` 与 `chacha20-poly1305`，并能读取 OpenSSH 私钥容器格式；Windows 与 Linux/macOS 不再有差异（旧版 Windows 构建曾用 WinCNG，只提供 `diffie-hellman-group*`，与 OpenSSH 10.0+ 交集为空，会在**认证之前**失败——那段历史已随旧后端一起消失）。用 `agent2ssh algo get` 查看本构建实际可协商的算法，用 `agent2ssh keys algorithm` 查看推荐密钥类型（该构建下为 `ed25519`）
- **密钥格式不再是认证失败的原因**：`russh` 的密钥解析器同时读取 OpenSSH 容器（`-----BEGIN OPENSSH PRIVATE KEY-----`，`ssh-keygen` 自 OpenSSH 7.8 起默认写出的格式）、PKCS#1 与 PKCS#8，所以 `ssh-keygen` 产出的密钥无需转换即可使用；过去那条 `ssh-keygen -p -m PEM` 的转换建议描述的是旧 libssh2 后端的限制，已不再适用。加密的传统 PEM（含 EC/SEC1）由 `legacy_pem` 按 OpenSSL 的 `DEK-Info` 方案解密，口令错误会明确报「padding 不合法」而不是 ASN.1 错误。**应用内生成的密钥总是采用本构建可用的类型**：`agent2ssh keys algorithm` 查询推荐类型，`keys generate <名称>` 直接生成

---

## known_hosts.json

### 用途

记录 Agent2SSH 已信任的 SSH 主机指纹。首次连接某个主机时，后端会自动写入该文件；再次连接时会校验保存的 SHA256 指纹。如果同一主机的指纹发生变化，连接会被拒绝。

### 文件格式

JSON 对象，键为 `host:port|<algorithm>`：`host:port` 部分是连接身份，`|` 之后是这条指纹对应的 host key 类型（`known_host_key`，`embedded_ssh.rs`）。一台服务器可以同时提供多种 host key，所以同一台主机常有多条并列记录——与 OpenSSH 的 `known_hosts` 每行一条「主机 + 密钥类型」同构。值包含主机别名、解析后的地址、host key 算法、SHA256 指纹和 Unix 秒级时间戳：

```json
{
  "192.168.1.10:22|ssh-ed25519": {
    "host": "web1",
    "address": "192.168.1.10",
    "host_key_algorithm": "ssh-ed25519",
    "fingerprint_sha256": "SHA256:abc123...",
    "first_seen_unix": 1781836215,
    "last_seen_unix": 1781836215
  },
  "192.168.1.10:22|ssh-rsa": {
    "host": "web1",
    "address": "192.168.1.10",
    "host_key_algorithm": "ssh-rsa",
    "fingerprint_sha256": "SHA256:def456...",
    "first_seen_unix": 1781836215,
    "last_seen_unix": 1781836215
  }
}
```

校验只比对**本次协商出的那一种**：同一类型指纹变了仍然拒绝连接，另一种类型的存在不算冲突（切换 `algo set --hostkey` 偏好不再误报「fingerprint changed」）。落盘形态是带 `|算法` 的键；旧版本写入的无 `|` 条目（算法在值里的 `host_key_algorithm`）仍然可用——加载时按值里的算法重键到内存，下一次写盘时迁移成新形态。手工编辑时建议直接写带 `|` 的形态，与文件实际长相一致。

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
- 这份文件在**每次认证**时都会读（走 `config_cache` 的 `(mtime, len)` 缓存）。它一旦解析失败，中间件就没法把一个 scoped token 和「未知 token」区分开，于是**除 admin token 之外的任何请求都得到 500**——不是 401，也不是 403。也就是说这个文件写坏会把所有最小权限客户端一起挡在门外，改完之后，先用一次 scoped 请求确认它还能通
- `token_env` 优先于 `token`
- scope 会在风险审批之前检查；未命中 scope 的请求不会进入审批队列
- `allowed_commands` 和 `denied_commands` 使用 glob 风格匹配，`denied_commands` 优先
- **`allowed_commands` 非空时是一张绝对白名单，不是「额外许可」**：`check_daemon_scope` 在 `allowed_commands` 非空且没有任何模式命中时直接返回错误（`remote.rs` 里 `if !scope.allowed_commands.is_empty() { … }` 那一段），没有回退、没有「其他操作默认放行」。所以只要写了这一项，就必须把该 token 需要做的**每一件事**都写成模式，包括下面这些不是 shell 命令、而是**固定**操作字符串的十项：`ping`、`session_open`、`session_read`、`session_close`、`terminal`、`resident-session open`、`resident-session read`、`resident-session close`、`connect`、`disconnect`，以及形如 `sftp upload <a> -> <b>`、`sftp rm <path>`、`forward <spec>`、`playbook:<名字>` 的参数化字符串。这十项由 `src-tauri/tests/daemon_integration.rs` 的 `configuration_guide_lists_every_fixed_action_string` 与本文件对表：代码里新增一个固定操作字符串而这里没写，那条测试就会红
- **写白名单时最容易造出一个「能开会话、关不掉」的 token**：`session_open` 命中、`session_close` 没写 → 该 token 能占会话配额却释放不了，配额满后自己的 `POST /sessions` 永远 429（scope 的 403 不会出现，因为 open 是被允许的）。`terminal` 与 `resident-session close` 同理：`GET /terminal` 占的是同一个会话配额，而它的释放只发生在 socket 关闭时；`admin` token 或另一个 token 可以替它关掉，否则只能等 daemon 重启。**`connect` / `disconnect` 是同形状的第二对**，但后果不同：`connect_host` 对同一 host 幂等（已连上就直接返回，不再占新条目），连接也没有并发配额，所以不会出现会话那种 429——代价是那条被保留的连接（含 keepalive 探测与 supervisor 自动重连）只能等 daemon 重启、或由另一个写了 `disconnect` 的 token / admin 放掉。要么把 open/read/close 三个字符串与 `connect`/`disconnect` 一起写，要么干脆不写 `allowed_commands`、改用 `allowed_hosts` / `allowed_tags` 收窄范围
- scope 覆盖 ping、exec、playbook、SFTP、session、forward 和 connection 的按主机操作；非命令操作会使用类似 `ping`、`sftp upload a -> b`、`session_open`、`session_close`、`connect`、`disconnect` 的操作字符串做匹配。这一行只是举例，**固定操作字符串的完整清单一律以上一条为准**（那里列了十项，并有一条测试盯着它）
- scoped token 只用于按主机绑定的操作；gate、主机/片段、策略、脱敏/高亮、算法、审批、webhook、团队配置和本地配置变更等全局控制面操作必须使用 unrestricted admin token
- 主机列表、审计、健康快照、连接、会话、转发和审批列表会按 scoped token 的 allowed hosts/tags 过滤；审计导出、趋势指标、事件流、待处理 OTP prompt 和 SSH sync diff 属于敏感全局读取，仅 unrestricted admin 可用
- resident session 列表同样按会话自身的 `local:<id>` 主机和标签过滤。读取缓冲输出使用操作字符串 `resident-session read` 检查主机、标签和命令 scope；关闭使用 `resident-session close`，并经过执行 gate、限流、风险和审批。列表和读取在 gate 暂停时仍可使用，但关闭会被拒绝。
- resident 命令执行期间仍可读取会话元数据与滚动输出。通过授权的关闭请求会取消当前命令及同一会话中排队的命令，结束 shell 并回收其进程树；关闭不必等到当前命令的执行超时。
- 桌面 SFTP 删除目录先请求 daemon 的 `POST /sftp/rm-rf/authorize`，完成 scope、gate、限额、风险和审批检查。daemon 只授权，不连接 SSH；桌面校验响应中的主机和路径一致后，在自身进程执行删除，使用桌面已解锁的凭据，不跨进程传递密码。普通路径默认属于高风险，确认删除后仍需在审批队列批准；`/`、`/*`、`/.` 被直接阻断。没有 `force` 或绕过审批的回退，daemon 未运行时应先启动服务。原 `POST /sftp/rm-rf` 仍由 daemon 授权并执行，供需要在 daemon 中执行的客户端使用；其 `timeout_secs`（默认 60 秒）是**连接预算**：它作为第二个实参交给 `connect_embedded_ssh`（TCP 连接、套接字读写与 session 超时），不是整棵目录树的删除总时限——`sftp_remove_dir_all_core_with_source` 的 BFS 循环逐条 `readdir`/`remove`，目录越大，总耗时越可能超过这个数。审批等待使用独立的审批 TTL。
- MCP 的 `local_exec` 和 `resident_session_*` 请求由 daemon 管理执行、预热及审批期限，不再受客户端固定 30 秒总超时限制；建立 HTTP 连接仍限时 10 秒。命令的 `timeout_secs` 继续生效，resident 命令超时仍会结束整个会话。
- PTY session 写入按已完成的输入行做授权和审计；daemon session 和 desktop 本地 session 都会缓存未完成输入，直到换行后再按完整命令授权；`session_read`、`session_list`、`forward_list` 这类观察操作通过 token/scope 控制，不默认写入 `audit.jsonl`

---

## CLI 主机凭据输入

`agent2ssh host add` 支持 `--password-stdin` 或 `--passphrase-stdin`：从重定向的标准输入读到 EOF，去掉末尾一个 LF 或 CRLF，保留其余空格和换行。两者不能同时使用，且不能与对应的 `--password` 或 `--passphrase` 同时使用。空输入、非 UTF-8 和超过 65536 字节的输入会被拒绝；标准输入直接连接终端时也会拒绝，避免输入被回显。

例如在 Bash 中交互输入密码（不要将实际密码写进命令或脚本）：

```bash
read -rs -p 'SSH password: ' ssh_password
printf '\n' >&2
printf '%s' "$ssh_password" | agent2ssh host add example --host example.com --password-stdin
unset ssh_password
```

新输入方式要求凭据能够写入加密凭据库：例如在无头环境设置 `AGENT2SSH_MASTER_PASSWORD` 以初始化或解锁库；库不可用时操作失败，不会把新密码写成明文。主密码环境变量本身也可能被同用户进程或子进程读取，应按运行环境保护。旧 `--password` 和 `--passphrase` 保持兼容，它们的值可能出现在进程参数中；旧密码入口的锁库明文回退行为不变。`--password-stdin` 与 `--passphrase-stdin` 使用同一个标准输入，需在一次调用中选择一个。

保存前会先以独占创建方式打开主机配置的临时文件：若同名文件已存在或无法打开，操作会失败，既不会覆盖或删除已有文件，也不会更新凭据库。凭据库与主机配置仍是两个独立文件；临时文件打开后若发生写入、重命名或进程崩溃，凭据库可能已经更新而配置未完成，目前不能保证自动回滚，需检查原主机和凭据后再重试。

---

## daemon.pid

### 用途

`daemon.log` 在 info 级别记录关闭触发来源：`sigterm`、`ctrl_c` 或 `http`；这不是信号发送者身份，日志级别较高时可能不输出。

记录当前运行的守护进程 PID，用于 `daemon stop` 和 `daemon status` 命令。

### 文件格式

纯文本，包含进程 ID：

```text
12345
```

### 注意事项

**macOS 保守策略（#25）**：当前没有经证明的进程对象绑定，普通 `daemon stop`、`daemon restart` 和桌面停止均拒绝发送关闭请求或信号；退出桌面不会主动向未绑定 daemon 发送停止请求或信号；但系统或进程托管环境仍可能使 daemon 退出，正在进行的会话等任务可能中断。macOS 退出前若有 PID 记录、损坏记录或健康端点迹象，会提示活动状态未知；此路径不自动向未绑定端点发送管理员 token。本机隔离桌面实验观察到 `loginwindow` 向测试 daemon 发送 SIGTERM；其他环境尚未验证。`/health` 和 PID 一致只是线索，不代表身份已绑定。新 daemon 的 HTTP `/shutdown` 默认返回 403；只有 daemon 启动环境显式设置 `AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN=1` 才开放旧路径；若 daemon 监听非回环地址，该开关也会允许有管理员令牌的远程 HTTP 请求。要承担风险并请求一次优雅关闭，可在数字 loopback 地址上使用 `agent2ssh daemon stop --allow-unbound-graceful` 或桌面确认操作：请求可能停错进程并向错误端点暴露管理员 token；客户端不会重试、强杀、清理 PID 或声称退出已确认；CLI 即使收到接受回执也以非零状态退出，提示结果未验证。旧 daemon 不认识此服务端开关，仍可能接收请求。这个开关仅影响 daemon 启动时的进程环境，设置在客户端环境里不能启用已运行 daemon 的路由。不要通过代理使用此例外；客户端无法识别所有本地代理。

恢复时，先独立核实本机进程与端点，由操作者决定是否带外停止；确认目标 PID 已不存在后，可执行 `agent2ssh daemon clear-stale --expect-pid <PID>`。该命令仅在锁内重读的文件与读取前原始内容一致且 OS 明确返回 PID 不存在时删除 PID 记录，不发送信号，也不删除 `daemon.owner`；文件损坏、非普通文件、PID 不同、权限不足或进程存活时拒绝。旧版不遵守锁，外部替换仍有竞态，PID 空闲不是原实例退出的身份证明。默认启动、直接启动 daemon 和令牌轮换均不会自动删除或替换已有 PID 文件；直接启动时以独占创建 PID 文件，已有路径（包括悬空符号链接）不会被截断或跟随创建目标。下面的旧停止/重试细节适用于非 macOS 和显式兼容路线，不应视为 macOS 默认保证。

- 守护进程启动时自动写入
- 守护进程正常退出时自动删除
- 新版 daemon 与客户端通过配置目录内的 `.daemon-pid.lock` 协调 PID 写入和清理；清理只删除仍记录原 PID 的文件，保留已被其他实例替换的记录。该锁只协调文件操作，不证明进程身份；旧版程序不遵守此锁，混用版本或使用不支持可靠文件锁的存储时仍可能竞争
- 非 macOS 上直接启动新版 daemon 时，若已有 PID 文件损坏、指向存活进程或无法确认进程已退出，启动会报错并保留文件；确认 PID 已不存在后才允许替换。不要仅凭进程名判断文件可以删除
- PID 文件为空、格式错误或 PID 为 0 时会原样保留并报错；启动和停止都不会把它当作不存在。写入可能尚未完成，可稍后重试；持续报错时先检查 daemon 进程与监听地址，确认没有相关进程后再手动处理文件
- 如果守护进程异常退出，此文件可能残留；状态检查会核对健康响应里的 PID 与文件中的 PID，并确认当前配置的 token 能访问 daemon。文件位置来自 `AGENT2SSH_CONFIG_DIR`，相对值时按**读取它的进程**的工作目录解析（同一工作目录下 `./cfg` 就是 `cfg/`，不会多出一级；跨工作目录则是两个目录，谁都不会去猜）
- 进程仍存在但身份不明，或守护进程存活但健康检查失败时，启动和停止会拒绝猜测并保留 PID 文件；非 macOS 的旧停止路径要求健康响应 PID 匹配、健康响应里的 `config_dir` 与本进程的配置目录一致，并使用当前配置的管理员 token 成功请求 `/shutdown`，先检查进程和监听地址后再处理。重命名的 daemon 可以通过匹配 PID 且接受当前 token 的健康响应显示为运行中，但不会仅凭该响应向其发送进程信号
- `/shutdown` 接受一个可选的 `expect_pid`：调用方在认证通过后声明"我要停的是这个进程"，daemon 核对后不符就回 409 并继续运行。停止路径总是带上它，所以在健康检查和发请求之间监听者被换成另一个用同一 token 的 daemon（重启、重生成）时，停止会失败而不会落到新进程上。旧版本 daemon 不认识该字段、会照旧关闭自己；它的 200 与新版 daemon 核对通过后的 200 在客户端看来完全一样（都是 `{"ok":true}`），**带 `expect_pid` 的那一类 200 都会落进同一条 warn**（不带 body 的 `request_graceful_shutdown` 走另一支，不记 warn），所以这条 warn 只说明"这次停止没有拿到检查回执"，不能反推 daemon 版本——而 daemon 侧也帮不上忙：它只在 409 时写日志，核对通过不留痕，所以"日志里没有核对记录"什么也证明不了；显式传 `expect_pid: null` 会得到 400，想不检查就别带这个字段。健康响应里没有 `config_dir` 的旧 daemon 会被判为无法识别并拒绝停止，`daemon restart` 走的是同一条停止路径、同样会被拒，所以要带外停掉它：Unix 用 `kill <pid>`（SIGTERM 会走正常清理并删掉 PID 文件），Windows 没有 SIGTERM 处理路径、只能 `taskkill /PID <pid> /F`（跳过整个 drain，PID 文件会留在磁盘上），之后 `agent2ssh daemon start` 会把指向已死进程的 PID 文件当作 stale 清掉；若它反过来报「daemon PID N is live or cannot be verified」，说明那个 PID 已被别的进程复用，按上一段的办法核对后再处理。同一个带外出口也适用于另一种「无法识别」：**可执行名不在白名单**里的 daemon（重命名过，或直接跑 `src-tauri/binaries/` 下带 target triple 的那一份）。这类 daemon 的 `daemon status` 会照常说 running、`daemon start` 说 already running、`agent2ssh doctor` 的 `daemon health` 一项也报 `pass`，只有 `daemon stop` 与 `daemon restart` 以 `cannot be safely identified` 拒绝——因为状态判据（`daemon_matches_config`）不看可执行名，而停止路径的第一步（`daemon_process_start_time`）看。把二进制改回 `agent2ssh-daemon` 再停，或按上一句带外 kill。**桌面面板上还要多一层注意**：设置里的「停止」按钮能不能点，看的只是 `http://127.0.0.1:7722/health` 有没有回 200（前端 `daemonHealthy`），既不看进程名也不看 PID 文件——所以重命名 daemon 时它会**亮绿灯并且可点，点下去却报 `cannot be safely identified`**；同理，把 `AGENT2SSH_DAEMON_ADDR` 指到别处时面板完全不受影响（前端地址是硬编码的，见 L2-159），面板说健康、CLI 说连不上，两者都不是在说同一个 daemon。非 macOS 的关窗口退出路径更安静：它先用 `process_is_alive`（看名字）判断，名字不在白名单就直接返回，**连一条「为什么没停」的日志都不写**，所以「退出应用后 daemon 还在」在重命名过的 daemon 上是预期行为而不是故障，要停它就按上面的带外出口处理。`agent2ssh daemon rotate-token` 拒绝的情况不同：它比的是 `daemon_pid_state(pid) != Stale`，只要那只 daemon 还活着且身份可核实就拒绝，理由是对的（换掉令牌等于切断正在用它的人），与可执行名无关。**另一种同形的「无法识别」是地址指向别处**：`AGENT2SSH_DAEMON_ADDR` 指向另一台主机时，`stop_daemon_process` 先用**本机**进程表核对 PID（`daemon_process_start_time`），再向**该变量解析出的地址**要 `/health`，两个观察必然一处命中一处不命中，于是落到 `health PID, config directory, or admin token did not match` 那一支——**这句文案里没有任何一个词提到「你的地址在别处」**，与「本机 PID 被复用」完全同形；`daemon status` 同理会报 `cannot be verified`，`daemon token rotate` 也会被拒（它有两道闸：`daemon_pid_state(pid) != Stale` 和 `daemon_health_pid().is_some()`，远端活着时后者恒为真，所以共用 token 在远端存活期间本地换不掉——这未必是缺陷，换掉正在用的 token 会切断那只 daemon，但用户在本地看到的理由只提 PID，看不出真正原因是地址）。要停远端 daemon，就在那台机器上用它自己的 CLI；或者把变量恢复成本机地址再操作。该检查只能收窄竞态，不能消除它：见 `docs/KNOWN-ISSUES.md` 的 K14
- daemon 成功绑定地址后才写 PID；已有健康端点或其他进程占用端口时，启动不会报告新进程已就绪。新创建的子进程未在启动期限内通过已认证健康检查时，启动器会终止并回收该子进程；若无法终止则保留或记录其 PID 以便人工检查
- 非 macOS 上，已接受受保护关闭请求的 daemon 如果在 8 秒宽限内没有退出，自动停止和重启会重新核对它的身份，核对通过才尝试强制终止，然后只等 2 秒；仍未退出就报错并保留 PID 文件。这 2 秒只是**最后一段**：一次停止按顺序还包含身份探测（`/health` 500 毫秒）、关闭请求（2 秒）、8 秒宽限、2 秒强制终止等待，最坏约 **12.5 秒**。进程快照是单 PID 刷新、读 `daemon.token` 是一次文件读，两者都在毫秒级；但**地址解析不受任何超时约束**（`to_socket_addrs` 在 `connect_timeout` 之前跑了，而 `AGENT2SSH_DAEMON_ADDR` 允许写主机名），所以配了主机名时「一次 stop 最多多久」没有上界，只按 12.5 秒算也不行。**另有一段的等待与地址无关，同样没有上界**：`.daemon-pid.lock` 走的是阻塞式 `lock_exclusive()`，没有超时（`store.rs` 的 `lock_config_file`），而 daemon 自己启动时要拿它写 PID 文件、收尾时要拿它删 PID 文件（`bin/agent2ssh-daemon.rs` 的 `write_daemon_pid` / `remove_daemon_pid_file_if`），启动器在失败清理时、CLI 与桌面在停止返回后也各要拿一次。所以一个卡住的持有者（被 `SIGSTOP` 的进程、不保证 `flock` 语义的网络文件系统）能让 `daemon start` 在「子进程已决定放弃」之后仍卡在删除那一步，也能让 `daemon stop` 在宽限超时把 daemon 强杀之后，卡在收尾的两步上——先由 `stop_daemon_process` 尾部清 `daemon.owner`（`clear_daemon_owner_if`，同一把锁），返回后调用方再删 PID 文件，两步各拿一次锁——两种都没有输出、没有上限，前面 12.5 秒的分解对这一段不适用。失败也不止两种：除了「进程忽略了终止请求」（强制终止后仍是 `StillRunning`）与「它已无法被确认成这个配置的 daemon」（`UnverifiedIdentity`），还有第三种 `UncertainExit`——读不到进程状态，而这一种可以在**进入宽限分支之前**就返回，此时根本没有任何信号发出去。区分线索在 `app.log`（不是 `daemon.log`：后者是 daemon 自己的 stdout/stderr 重定向）：出现 `daemon did not exit after a guarded shutdown; leaving it running` 说明宽限内它确实还在跑；报 `UncertainExit` 而没有这一条，说明问题在可观测性——从头就判定不出它在不在。检查 `app.log`、监听地址和系统进程信息，确定进程确实属于该配置后，由操作者自行决定如何停止；确认进程已退出后再重试启动。不能仅凭进程名、匿名健康响应或共享 token 确认身份

---

## daemon.owner

### 用途

记录"哪个 daemon 是桌面应用自己启动的"。桌面应用退出时只会停止它启动的 daemon；由 CLI（`agent2ssh daemon start`）、MCP 客户端或手动启动的 daemon 不属于桌面应用，退出应用时会保持运行，以免中断正在使用它的 agent。

### 文件格式

```json
{"pid":12345,"start_time":1761800000}
```

`start_time` 用来防止 PID 被系统回收后误认：只有 PID 和进程启动时间都对得上才算桌面应用启动的。

### 注意事项

- 由桌面应用在启动 daemon 时写入；停止时仅清理由相同 PID 和启动时间标识的记录，无法确认启动时间时保留；无需手动维护。新版写入和清理使用 `.daemon-pid.lock` 协调，旧版程序不参与此协议
- 文件缺失、损坏或与当前 daemon 对不上，都视为"不是桌面应用启动的"，退出应用时不会停止它
- 设置里的"停止 Daemon"按钮和 `agent2ssh daemon stop` 是明确的停止指令，不受此文件限制。但两者的门槛不同：CLI 的 `daemon stop` 会读 `daemon.pid`，读到损坏、权限不足或身份不可核实时会带着具体理由拒绝；桌面面板只轮询 `/health`（`api.ts` 的 `getDaemonHealth`），"停止 Daemon"按钮在 `/health` 不通时直接置灰，此时同一屏上的"启动"却仍可点，点下去才会把 PID 文件的真实错误显示在按钮下方。桌面端**不读** `daemon.pid` / `daemon.owner` 来展示状态（`daemon_status` 命令至今没有调用方），所以「面板显示 Daemon unavailable」既不等于「有残留记录」，也不等于「没有」——要判断记录状态请用 `agent2ssh daemon status`
- 桌面应用退出前，如果它启动的 daemon 里还有终端会话、常驻 shell、端口转发或待审批请求，会先弹窗确认，因为这些都只存在于 daemon 内存里，停止即丢失

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

需要模板参数时用 `advanced_steps`。它存在时**优先于** `steps`，两者可以同时出现——此时 `steps` 通常是空的，这不是坏文件：

```toml
[[playbooks]]
name = "greet-all"
description = "带参数的问候"
steps = []

[[playbooks.advanced_steps]]
command = "echo {{message}}"
description = "打印一句问候"
[[playbooks.advanced_steps.params]]
name = "message"
default = "hello"
required = false
```

`params` 里的 `name` 会替换命令里的 `{{name}}`；`default` 在调用方没传时使用，`required = true` 则强制调用方必须给值。

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | Playbook 唯一名称 |
| `description` | string | 是 | Playbook 描述 |
| `steps` | array | 是 | 命令序列，按顺序执行；用 `advanced_steps` 时可以为空数组 |
| `tags` | array | 否 | 标签列表 |
| `risk_override` | string | 否 | 覆盖所有步骤的非 blocked 风险等级 |
| `advanced_steps` | array | 否 | 带参数的单步定义，存在时优先于 `steps`；每项含 `command`/`description`/`params` |

### 执行行为

- 步骤按顺序执行
- 任何步骤失败（非零退出码或错误）时停止执行，返回已完成步骤的部分结果
- 返回 `success` 状态和 `total_duration_ms` 总耗时
- 使用 MCP 工具 `ssh_playbook_run` 或 Daemon API `POST /playbooks/run` 执行
- 高风险步骤需要 daemon 审批或 `force: true` 才能执行；本地 CLI/MCP 没有可用审批处理器时会失败关闭。CLI 的 `playbook run` 没有 daemon 转发，命中策略时只能改策略，或到桌面端/HTTP 接口 `POST /playbooks/run` 上审批
- daemon 审批只对被批准的具体步骤生效；不会因为前一个高风险步骤获批而自动放行后续高风险步骤。显式 `force: true` 仍表示调用方请求放行整个 playbook。

### 注意事项

- 步骤中的 shell 变量（如 `$(date)`）会在远程主机上展开
- `risk_override` 可以统一设置所有步骤的非 `blocked` 风险等级
- `risk_override` 不能降级 `blocked` 命令；内置或用户规则判定为 `blocked` 的步骤仍会被拒绝
- 文件不存在时返回空 Playbook 列表（不会报错）

---

## environments.toml

### 用途

定义自己的本地终端环境，用 `local:<id>` 寻址，和内置的 `local:pwsh`、`local:gitbash` 等走同一条执行路径（一次性 `/local-exec` 和常驻会话 `/resident-sessions` 都支持）。一个环境 = 一个内置基础环境 + 可选的工作目录、进程环境变量和一行 prelude（每条命令前在同一个 shell 里先执行）。

因为 host id 就是环境名，token scope 的 `allowed_hosts`、审批策略的 host 模式和审计日志都能直接按 `local:<id>` 区分，不需要新的概念。

### 文件格式

```toml
[[environment]]
id = "py-ml"                          # 通过 local:py-ml 使用
description = "ML 工具链"              # 可选
base = "gitbash"                      # default | pwsh | cmd | gitbash | vs | wsl，默认 default
cwd = "D:/work/ml"                    # 可选；必须是绝对路径，开头的 ~/ 会展开
prelude = "source .venv/Scripts/activate"   # 可选；单行，用 ; 串联多条（cmd 用 &&）

[environment.env]                     # 可选；设置到 shell 进程上的环境变量
CUDA_VISIBLE_DEVICES = "0"

[[environment]]
id = "ubuntu-dev"
base = "wsl"
distro = "Ubuntu-22.04"               # 仅 base = "wsl" 可用
prelude = "export JAVA_HOME=/opt/jdk-21"   # WSL 的变量写在这里，不能用 env 表
```

### 字段说明

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | 是 | 1–32 个字符，`[a-z0-9_-]`，以字母或数字开头。区分大小写、精确匹配。不能与内置环境（含别名）、容器 runtime（`docker`/`wslc`/`apple`）、`local`、`default` 冲突 |
| `base` | 否 | 基础环境，默认 `default`（应用默认 shell） |
| `distro` | 否 | WSL 发行版名，仅 `base = "wsl"` |
| `cwd` | 否 | shell 的启动目录，必须是绝对路径；每次执行时检查目录存在 |
| `prelude` | 否 | 单行，最长 4 KiB，在基础环境自己的初始化（如 VS 开发者 shell）之后、命令之前执行 |
| `env` | 否 | 最多 64 个变量；名称 `[A-Za-z_][A-Za-z0-9_]*`；不能设置 `AGENT2SSH_LOCAL_EXEC_DEPTH` |

### 注意事项

- **只在守护进程启动时读取一次，不会热重载。** 修改文件后需要重启 daemon 才生效。这是有意的：prelude 和环境变量在命令之前执行，风险分类器看不到它们，所以不允许运行中的进程被一条本地命令改写的文件影响。
- **prelude 在加载时做风险分类**，评级为 `high`/`blocked` 的定义会被拒绝。但分类器解析的是 bash，因此只对 `gitbash`、`wsl` 以及非 Windows 上的 `default` 生效；`pwsh`、`cmd`、`vs` 不做这个检查（对 PowerShell 它会把 `$env:FOO = 'bar'` 判成 high、把 `Remove-Item -Recurse -Force` 判成 low），只依赖"不热重载"这一条。请只写你自己审过的 prelude。
- **同一个语言限制也适用于命令本身，而且现在可见了**：`agent2ssh exec`、`local_exec` 与常驻会话在 `pwsh`/`cmd`/`vs` 环境里跑的命令，其风险等级仍是 bash 分类器的结论。这个结论**不会被用来放行或拦截**以外的任何判断（策略、执行门、审计照旧用它），但结果和审计条目会带上 `risk_unclassified: true`，`agent2ssh risk --host local:cmd` 会在输出里给出说明，审批卡片上也会显示一条提示。`gitbash`、`wsl` 和 SSH 主机不带这个标记。把它当成"这条命令没有被真正评估过"，而不是"它被评估为低风险"。
- `base = "wsl"` 时不能写 `env`：`wsl.exe` 不会把自己的进程环境传进发行版。把变量写成 prelude 里的 `export`。
- 单个定义校验失败只会拒绝它自己，其他定义照常加载；使用被拒绝的环境会返回具体原因，不会退化成普通 shell。整个文件解析失败只会让这次启动内的自定义环境不可用（文件在启动时读一次，见上一条），内置环境不受影响。daemon 启动时会把这些问题写入日志——**只有启动时那一条**：注册表之后不再读盘，所以日志里没有新报错不等于文件已经改好了，别把「最近没看到报错」当成验证。
- **运行中改这个文件不会有任何效果，把它改坏、再改好也一样。** 注册表是进程内快照（`local_environments::current()` 缓存，`reload()` 只在启动时调一次，没有请求路径能重读）：启动时文件是好的 → 运行中写坏它，已注册的定义照常工作，`local:<id>` 既不报 `could not be loaded` 也不变；启动时文件是坏的 → 运行中修好它，到重启前仍然不可用，未知 id 的 400 会一直挂着 `environments.toml could not be loaded` 的注解。要让改动生效只能重启 daemon。
- 未知字段会被当作错误（防止把 `env` 拼成 `envs` 而静默丢失变量）。
- 文件不存在表示没有自定义环境，不会报错。
- 值是明文存储；不要把密钥直接写进去。

### `local:wsl` 的命令是怎么执行的

`local:wsl`（默认发行版）和 `local:wsl:<发行版>` 都是**一次性 exec**：`wsl.exe` 先按 `--shell-type login` 建立该发行版的登录环境（`/etc/profile.d`、它自己的 `PATH` 和工具链），再由发行版的 `/bin/sh` 解释命令字符串。

两条由此而来的规则：

- **命令体由 `sh` 执行，不是 bash。** `sh` 是 Linux 发行版必须提供的解释器（Alpine 只有 `ash`，没有 `bash`），所以 `local:wsl` 不假设 bash 存在。需要 bash 语法（`[[ ]]`、数组）时显式调用：`bash -c '[[ -f x ]] && echo yes'`。
- **一次 exec 只跑一条命令字符串。** 命令作为单个参数传给 `sh -c`，其中的引号、`;`、管道都由那个 shell 解释，和 `local`、`local:gitbash` 一致。

常驻会话（`agent2ssh resident open -H local:wsl`）走另一条路径：它不带命令，直接打开发行版的交互式登录 shell，之后每条命令写进同一个 shell 的 stdin，所以 `cd`、`export` 的状态跨命令保留，bash 语法在那里也可用。

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

当 URL 包含 `hooks.slack.com` 时，自动使用 Slack Block Kit 格式。**这里有两个渲染器，不是一套模板**：
`notify_approval_pending` 走 `format_slack_approval_notification`（`notify.rs:162`），`fire_webhook` 走
`format_slack_message`（`notify.rs:299`）。对 `approval_required` 事件**两者都会发**——`bin/agent2ssh-daemon.rs`
里同一个 `tokio::spawn` 先 `notify_approval_pending` 再 `fire_webhook`——所以一次待审批在 Slack 里是
**两条消息**。这条重复发送是已知缺口，记在 `OPT-LOOP-L2-PENDING.md` 的 L2-75。读下文时先分辨是哪一条：

| | 审批那条（`format_slack_approval_notification`） | 通用那条（`format_slack_message`） |
|---|---|---|
| 标题 | `Agent2SSH: Approval Required` | 按事件与 `remote_outcome_unknown` 选（Approval Required / Command Blocked / Command Completed / Anomaly Detected） |
| 字段 | `Host`、`Command`、`Risk Level`、`Approval ID` 四个固定字段 | `Host` 与 `Command` 固定；`Risk Level`、`Exit Code` 是**条件字段**，只有事件带了非空值才出现 |
| 按钮 | Approve / Reject，**点不动**（见下） | `approval_required` 时给 `Open Approvals`，URL 是 `{daemon}/console#approvals-<id>`，可用 |

通用那条的两个条件字段还有两处已知读法：`diagnostic_error` 的卡片只有 `Host` 与 `Command` 两个 field
（它的 `risk_level` 与 `exit_code` 都是 `None`），而且 `Host` 里装的是**组件名**、`Command` 里装的是错误文本
（见上一节的字段说明），在 Slack 里读起来就是「一个组件名 + 一段错误消息」，不是「一台主机 + 一条命令」；
命令没执行时（例如被 gate 拦下）`exec_completed` 也没有 `Exit Code`。

审批那条的 Approve / Reject 按钮**当前点不动**，不要把它们当操作入口。URL 由 `approval_action_url`
（`approval.rs:901`）拼出：基址是 daemon 的解析地址（默认 `127.0.0.1:7722`），中间是
`/approval/{id}/respond`，按钮再各接一段 `/approve` 与 `/reject`——**后两段不存在**，点开是 404。
即使去掉后缀，`/approval/{id}/respond` 也只是 `POST` + `{"approved": bool}` 的 admin 路由，浏览器的
GET 一样打不动；换个机器点开时，`127.0.0.1` 还指向**点的人自己的机器**。
实际批准或拒绝请用 `Open Approvals` 按钮或 `/console#approvals`。这是已知缺口，已在
`docs/reports/OPT-LOOP-L2-PENDING.md` 的 L2-44 登记

### 自定义 Webhook

非 Slack URL 发送原始 JSON，**但审批事件有两个形状、而且会投递两次**（`notify_approval_pending` 先发，
`fire_webhook` 再发；见上文 Slack 对照表与 L2-75）。审批那条的形状：

```json
{
  "event": "approval_required",
  "host": "web1",
  "command": "sudo rm -rf /tmp/cache",
  "approval_id": "550e8400-e29b-41d4-a716-446655440000",
  "risk_level": "high",
  "approval_url": "http://127.0.0.1:7722/approval/550e8400-e29b-41d4-a716-446655440000/respond"
}
```

六个键，**没有** `exit_code` 与 `remote_outcome_unknown`。通用那条是 `WebhookEvent` 直出：固定
`event` / `host` / `command`，按需出现 `approval_id`、`risk_level`、`exit_code`、
`remote_outcome_unknown`。按「一个形状」写解析器，会在审批那条上拿到永远不出现的 `exit_code`，
并丢掉实际存在的 `approval_url`。

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

配置 daemon 层的执行速率和 session 并发限额。限额在 daemon 进程内强制执行。速率限额覆盖：`/exec`、`/local-exec`、`/exec-multi`、`/exec/compare`、`/exec/stream`、全部 `/sftp/*`（含 `/sftp/rm-rf` 与 `/sftp/rm-rf/authorize`）、`/sessions/:id/write`、`/resident-sessions` 与 `/resident-sessions/:id/exec`、`DELETE /resident-sessions/:id`、`/forwards`、`/forwards/multi`、`DELETE /forwards/:id`、`/forwards/:id/start`、`/forwards/:id/stop`、`/connections/:host/connect`、`/connections/:host/disconnect`、`/playbooks/run`、`/terminal`（WebSocket 交互终端）、`/terminal/broadcast`（多终端广播，普通 POST）、`/daemons/:alias/exec`。

清单之外的路由**一律不受限流**，而且范围比「只读/预览」大得多。除 `/exec/preview`、`/playbooks/:name/dry-run`、`/ping` 与各类列表查询外，配置与规则写入（`POST /hosts`、`/hosts/import`、`DELETE /hosts/:name`、`/snippets/*`、`/redact/rules/*`、`/highlight/rules/*`、`/algo/prefs`、`/recordings/*`）、审批与门控（`/approvals/:id/approve`、`/approvals/:id/reject`、`/gate/pause`、`/gate/resume`）以及 `POST /shutdown` 都不在其中。`POST /shutdown` 尤其值得记住：它的 handler 只累加请求计数并校验 admin token，既不进限流也不读执行门，所以 `execution_limits.toml` 里写什么都约束不到停止路径。

判据只有一个：handler 是否真的调用 `reject_if_rate_limited`（或它的三个包装 `preflight_guarded_request`、`preflight_rewritable_request`、`register_limited_session_for_command`）。13 个 handler 直接调用，另有一批经共享前置（`:570`）统一进入。想让一条新路由受限流，只能在那里加调用点——写进文档不会让它受限。

### 文件位置

```text
~/.agent2ssh/execution_limits.toml
```

### 默认行为

文件不存在时使用内置默认值。**文件是被监视的**：daemon 在每次限额检查前比较 `(mtime, len)`，文件变了就重读，所以改完立即对下一次执行生效，不需要重启。重载只替换阈值，已记录的事件和已开会话保留——否则每改一次文件就等于把窗口清零、放一批请求过去。读到损坏的文件时保留上一份限额并写一条警告，不会让每次执行都失败。

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

`per_minute = 0` 或 `max_sessions = 0` 表示该维度不限额。速率限额的完整覆盖面见本节开头的清单，不是只有 WebSocket：在 WebSocket terminal 上它按 **completed-line input** 计（每写完一行算一次），而不是按连接或按帧。session 并发限额覆盖 session open 与 WebSocket terminal open。超限请求返回 HTTP 429，写入 `blocked` audit，并发布 `limit_rejected` 事件。

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
agent2ssh keys algorithm          # 推荐类型，恒为 ed25519（脚本的稳定接口）
agent2ssh keys generate id-work   # 按推荐类型生成；只输出私钥路径
agent2ssh keys generate id-work --type rsa --json
```

生成过程由 Agent2SSH 后端实现，不调用系统 `ssh-keygen`。**默认类型现在是常量，不由构建能力也不由操作系统决定**：`default_for_build()` 直接返回 `ed25519`，理由是 `russh` 能读取本应用写出的所有私钥格式，没有任何构建会给出不同答案；`--type rsa` 仍然可用，写出的是 3072 位传统 PKCS#1 PEM（`-----BEGIN RSA PRIVATE KEY-----`），公钥仍是标准的单行 `ssh-rsa …`。因此本应用生成的密钥一定可用于本应用的认证——`keys algorithm` 保留下来，是给脚本一个不会漂移的接口，脚本仍应询问而不是假定。

> 未手动选择类型时，请求不带类型，由后端按 `keys algorithm` 的结论决定。

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
- **能生成的类型只有 `ed25519` 与 `rsa`**：`keys generate --type` 只接受这两个值（不指定时用 `keys algorithm` 的结论，恒为 `ed25519`），CLI 的 `keys` 也只有 `algorithm` 与 `generate` 两个子命令。**能导入的类型更宽**：桌面端 Keys 页的导入只做文件复制、按 `.pub` 前缀标类型，所以 `russh` 能读的格式（含 ECDSA P-256/384/521）都可以导入并用于认证——但本应用**不能生成**它们
- **各平台一致，Windows 没有额外限制**。旧版曾写「Windows 上实际可认证的只有 `rsa` 的传统 PEM 形式」，那是 libssh2 + WinCNG 后端的限制，已随该后端一起移除（见本节上方「各平台的密码学能力一致」，以及 `AGENTS.md` 与 `CHANGELOG` 的 Removed 段）。`russh` 的解析器直接读 OpenSSH 容器、PKCS#1 与 PKCS#8，`ssh-keygen` 产出的密钥无需任何转换

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
### 换了机器之后：为什么密码都不见了

`secrets.enc` **有意不进同步**，这会让一台新机器在拉取配置后出现一个看起来像故障的状态：`hosts.json` 里的密码字段还是 `$agent2ssh-secret$` 引用，但本机没有能解开它的库，于是每台密码型主机都提示需要重新输入密码，`ssh_exec` 直接失败。

这不是数据丢失，也不是同步坏了：

- **同步过去的只有引用**，`hosts.json` 里没有明文可丢；
- **库本身没有走网络**，所以远端备份里根本没有它，无从"漏同步"；
- **密钥型、agent 型主机不受影响**，它们的认证材料在 `keys/` 或 agent 里，本来就不经过 `secrets.enc`。

要恢复密码型主机，从**原机器**把 `~/.agent2ssh/secrets.enc` 手动拷到新机器同一路径即可（同权限 `0600`）。主密码相同就仍然解得开。传输这个文件要按密钥对待：它由主密码派生密钥加密，但拿到文件后可以离线暴力尝试主密码，所以别走聊天工具或公开网盘。

另一条路是逐个重新输入：在新机器上设置主密码，然后在主机编辑表单里重新填一次密码，代价是多台主机要重复一遍。『凭据库该不该跨机器同步』是明确的产品决定而不是遗漏——最省事的做法（把库传上去）等于把全部密码的离线猜解材料上传到一台第三方 WebDAV 服务器上。

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

恢复前先验证完整 manifest、文件名白名单及所有捕获文件，并读取完全部文件；非法、重复或缺失条目会报错，不会部分应用。通过检查后先保存当前状态的 `pre-restore` 安全备份，再恢复文件并删除快照范围内的新增项。私钥、凭据库、主机信任等非同步文件不参与恢复，也不会因快照中缺失而被删除。既有 `backup_manifest.json` 格式仍可使用。执行限额与 `policy.toml` 一样是热加载的：daemon 在每次限额检查前按 `(mtime, len)` 判断文件是否变化，变了就重读，改完立即对下一次执行生效（记录的事件与已开会话保留，不会因为重载而重置窗口）。读到损坏的文件时保留上一份限额并记一条警告，而不是让每次执行都失败。

**`hosts.json.bak` 不是一致性快照（阶段 2 设计进行中，尚未实现）**：常规保存只把上一次的 `hosts.json` 用 `fs::copy` 复制成 `hosts.json.bak`（`store.rs:841-851`，复制失败仅告警、不阻断保存），它防的是「上一次内容写坏了可手工回卷」，不保证跨文件一致。凭据库 `secrets.enc` 既没有配对副本，也不在 WebDAV 同步白名单与本地配置快照的恢复范围内；上述恢复路径都直接覆盖 `hosts.json` 而不碰 `secrets.enc`。因此手工把 `hosts.json.bak` 拷回、恢复旧配置快照或从远端 pull 回旧版本后，文件里的 `$agent2ssh-secret$` 引用仍会按 account 名解析到**当前** `secrets.enc` 中的密文——若这期间改过口令，可能用错凭据且不会有任何提示。阶段 2 的内容 tag 与成对快照设计见 `docs/plans/cross-file-transaction-phase2.md`，目前只是评审稿。请勿把 `.bak` 当作可安全回卷的成对备份；确需回卷时先另行备份当前 `secrets.enc`，并逐项核对主机凭据。

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
- 文件缺失时按内置规则重新播种。**不可读或 JSON 损坏**时两条路径分岔：列表/编辑面回落到内置集合
  （`load_user_rules`），而**实时脱敏路径不走这个回落**——`redact_with_user_rules` 直接返回字面量
  `[REDACTION UNAVAILABLE]`，宁可整条不产出可读内容也不退到更弱的规则集（fail-closed）。所以
  「配置坏掉不会漏秘密」成立，但代价是那段时间的审计记录、通知与诊断导出一律只有这个标记。
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

四个界面**都能编辑**这套规则，行为一致：CLI 是 `agent2ssh redact list|add|update|remove|reset`，MCP 是
`ssh_redact_rule_list|add|update|remove|reset` 五个工具，daemon 是 `GET|POST /redact/rules` 与
`/redact/rules/{update,delete,reset}`（逐行见 `daemon-api-quickstart.md` 的端点表），桌面端是 Redaction 面板。
daemon 侧有个权限差：`GET /redact/rules` 只要有效 token，而四个**写**端点（add / update / delete / reset）
额外要求**无 scope 的 admin token**——host-scoped token 会拿到 403（`require_admin`），这与
`POST /containers/start-service` 同一条理由：改的是本机行为，不是某台主机上的事。
**删除需要显式确认**：`redact remove` / `ssh_redact_rule_remove` / `POST /redact/rules/delete` 少了
`--force` / `force: true` 会被拒绝且什么都不改（CLI 退出码 2，daemon 与 MCP 返回 400），
`reset` 是唯一的回退路径——这是为了让「删空规则集 = 审计与通知原样写出」不能由一次误触达成。
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

# 仅备份配置文件（排除运行时文件、审计日志、信任库与急停状态）
tar -czf agent2ssh-config.tar.gz \
  ~/.agent2ssh/hosts.json \
  ~/.agent2ssh/policy.toml \
  ~/.agent2ssh/risk_rules.toml \
  ~/.agent2ssh/approval_policies.toml \
  ~/.agent2ssh/playbooks.toml \
  ~/.agent2ssh/remotes.toml \
  ~/.agent2ssh/execution_limits.toml \
  ~/.agent2ssh/anomaly.toml \
  ~/.agent2ssh/environments.toml \
  ~/.agent2ssh/app_preferences.json \
  ~/.agent2ssh/daemon_tokens.toml \
  ~/.agent2ssh/webhook.toml \
  ~/.agent2ssh/keys/
```

这份清单**不等于** WebDAV 同步的 `webdav_sync::SYNCABLE_FILES`：后者刻意不含凭据
（`daemon_tokens.toml`、`secrets.enc`、`keys/`），也不含 `environments.toml` 与
`approval_policies.toml`——前者是执行前的信任边界，后者是「谁能批准什么」。反过来，本清单刻意
**不含** `known_hosts.json`（换机器应当重新信任，而不是继承）与 `execution_gate.json`
（急停状态属于机器当前处境，不该被备份带回新机器）。

`policy.toml` 与 `policy.json` 是同一个策略文件的两种形态（`policy::existing_policy_path` 先找
`.toml`，不存在才用 `.json`），所以上面只列了前者：**只用 `policy.json` 的机器要把它补进 tar 命令**，
否则策略文件根本不会被带走，而使用者会以为备过——恢复后策略为空，等于静默放宽。同一目录下的
`redact_rules.json`、`highlight_rules.json`、`copy_redact_rules.json`（你编辑过的脱敏/高亮/复制规则）、
`ssh_algos.json`、`telemetry.toml`、`webdav.toml`、`snippets.json` 也都是配置，只是**只在对应功能
用过之后才存在**，所以没有放进上面那条命令；改过就先确认文件在，再加进去。

### 迁移到新机器

```bash
# 在新机器上安装 Agent2SSH
brew tap lengyuqu/agent2ssh && brew install agent2ssh

# 解压配置
tar -xzf agent2ssh-config.tar.gz -C ~/

# 确保权限正确
chmod 600 ~/.agent2ssh/keys/* 2>/dev/null || true

# 启动守护进程（daemon.token 非空则原样复用，只在文件缺失或为空时才生成新的）
agent2ssh daemon start
```

`daemon start` **不会**无条件换 token：`load_or_create_daemon_token` 读到非空的 `daemon.token` 就只
`chmod 600` 再原样返回；文件存在但为空时才重新生成并打一条 warn，文件根本不存在时直接新建（不告警）。
所以用上面「备份整个配置目录」的
`tar` 连同 `daemon.token` 一起搬到新机器时，**旧 token 继续有效**——已经配好 token 的客户端不必改配置，
反过来说，token 泄露后靠「重启一次」是换不掉的，要显式删掉该文件再启动。

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
| `AGENT2SSH_CONFIG_DIR` | 全部 | `~/.agent2ssh` | 覆盖配置与数据目录（`hosts.json`、`audit.jsonl`、`keys/`、`app.log` 等）。测试与多实例隔离常用；相对路径按各进程当前工作目录解析，跨目录启动 CLI 和 daemon 时应使用同一绝对路径。 |
| `AGENT2SSH_SOURCE` | CLI / MCP / Tauri | 各 surface 自带标签（如 `cli`、`mcp`、`desktop`） | 覆盖审计与 Live Activity 中的来源标签，用于区分是哪个 agent/客户端发起的操作。 |
| `AGENT2SSH_DAEMON_ADDR` | daemon（绑定）+ client（拨号，桌面窗口除外） | `127.0.0.1:7722` | 本地 daemon 的监听地址。daemon 按此绑定；CLI/MCP 客户端、Tauri 后端命令与 Webhook 链接经同一解析器（`local_daemon_url`）拨号，绑定到通配地址（`0.0.0.0`/`::`）时 client 自动回退到回环。**桌面窗口的直连请求不跟随此变量**：`src/api.ts` 的 `daemonUrl` 固定写 `http://127.0.0.1:7722`，健康指示、闸门状态、会话、终端 WebSocket 与 SSE 活动流（`/events/stream`，只有在订阅时才有请求）都打默认地址。所以把本变量改成非默认值后，CLI 与 MCP 正常，而桌面不只是显示 `Daemon unavailable`：窗口挂载时 `src/App.tsx` 的 `ensureLocalDaemon` 会调后端命令 `daemon_start`（它跟随本变量）真的在那个地址起一个 daemon，再用前端固定地址复查、永远复查不到，于是每次挂载都留下一次「启动成功」或「已有健康 daemon，跳过启动」的诊断加一块空面板。反复开关窗口不会堆进程（`daemon_start` 见 PID 文件里已有健康 daemon 就跳过），只会堆诊断行。绑定非回环地址会写一条 `warn` 诊断提示对外暴露。**把本变量指向另一台主机时，本机的 `daemon stop` 与 `daemon status` 必然失败**：停止路径先用本机进程表核对 PID、再向这个地址要 `/health`，两个观察必然一处命中一处不命中，于是报 `health PID, config directory, or admin token did not match`——**这句文案里没有一个词提到地址**。要停远端 daemon 就在那台机器上用它自己的 CLI（详见 `daemon.pid` 一节的停止说明）。 |
| `AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN` | macOS daemon | 未设置 | 仅值为 `1` 时启用旧版未绑定 HTTP `/shutdown`；必须设置在 daemon 启动环境。若监听非回环地址，远程管理员请求也受影响。 |
| `AGENT2SSH_TRACE_ID` | CLI / MCP | 自动生成 | 由上游调用方（如 agent）注入的关联 ID，写入诊断 `trace_id` 字段并随转发请求透传，便于跨 surface 串联一次操作。 |
| `AGENT2SSH_LOG` | daemon | `info` | daemon `tracing` 的 `EnvFilter` 级别（如 `debug`、`agent2ssh=debug,info`）。 |
| `AGENT2SSH_LOG_FORMAT` | daemon | `text` | daemon 控制台日志格式：`text` 或 `json`。 |
| `AGENT2SSH_BRIDGE_DEPS` | daemon | 关闭（仅转 `agent2ssh*`） | 是否把依赖层（hyper/reqwest/russh/…）的 `WARN`/`ERROR` 也桥接进 `app.log`：`1`/`true`/`all` 用内置传输层前缀集，逗号分隔则自定义前缀，未设/`0`/`false` 关闭。依赖层事件不触发 error 告警（防回环），仅落盘排查传输问题用。 |
| `AGENT2SSH_MASTER_PASSWORD` | CLI / MCP / daemon | 无 | 解锁 App 自建加密凭据库（`secrets.enc`，Argon2id + AES-256-GCM）的主密码（K1）。设置后这些无界面 surface 才能解密密码型主机的 SSH 密码；未设置时密码型主机不可用（密钥型/agent 认证不受影响）。桌面端改用启动解锁对话框，不读此变量。首次写入凭据时若已设此变量会用它初始化库。 |
| `AGENT2SSH_SECRETS_BACKEND` | 全部 | 生产 `encrypted`，测试 `memory` | 凭据库后端：`encrypted` 用真实加密库，`memory` 用进程内内存库（仅测试/无界面隔离用，重启即丢）。一般无需设置。 |
| `AGENT2SSH_WEBDAV_URL` | CLI | 无 | WebDAV 同步 collection URL。也可写入 `webdav.toml` 或用 `agent2ssh webdav * --url` 传入。 |
| `AGENT2SSH_WEBDAV_USERNAME` | CLI | 无 | WebDAV Basic Auth 用户名。 |
| `AGENT2SSH_WEBDAV_PASSWORD` | CLI | 无 | WebDAV Basic Auth 密码。也可在 `webdav.toml` 中用 `password_env` 指向自定义环境变量。 |
| `AGENT2SSH_SYNC_PASSWORD` | CLI / daemon 版本化同步 | 无 | 同步文件的 AES-256-GCM 加密密码，密钥由 Argon2id 派生；上传必须提供，拉取加密备份需使用相同密码。可用 `sync_password_env` 指向自定义变量；桌面上传改用凭据库里的独立同步密码。 |
| `AGENT2SSH_CONTAINER_RUNTIME` | CLI / 桌面 | 自动探测 | 固定容器发现使用的本地运行时：`docker`、`wslc`（Windows 的 WSL 容器 CLI）或 `apple`（Apple container）。不设时按 `docker → wslc → apple` 取第一个已安装的运行时，未安装的自动跳过；桌面端的容器面板与 e2e 脚本共用这一条选择逻辑。 |

> 说明：`remotes.toml` 与 `daemon_tokens.toml` 中的 `token_env` 字段引用的是**用户自定义**的环境变量名（如示例中的 `AGENT2SSH_PROD_TOKEN`），由你自行设置，不属于上表的内置变量。

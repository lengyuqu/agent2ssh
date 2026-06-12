# CLI 快速入门

Agent2SSH 提供命令行工具 `agent2ssh`，用于管理 SSH 主机、执行远程命令、传输文件、管理会话和端口转发等操作。

## 安装验证

安装完成后，验证 CLI 是否可用：

```bash
agent2ssh --version
# agent2ssh 0.1.0
```

### 安装方式

**Homebrew (macOS)**

```bash
brew tap lengyuqu/agent2ssh
brew install agent2ssh
```

**从源码构建**

```bash
git clone https://github.com/lengyuqu/agent2ssh.git
cd agent2ssh
npm install && npm run build
cd src-tauri
cargo build --release --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo build --release --no-default-features --features daemon --bin agent2ssh-daemon
```

也可以从 [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases) 下载预编译二进制文件。

---

## 主机管理 (host)

### 列出主机

查看所有已配置的 SSH 主机：

```bash
agent2ssh host list
```

以 JSON 格式输出：

```bash
agent2ssh host list --json
```

### 添加主机

添加一个新的 SSH 主机配置：

```bash
agent2ssh host add <name> --host <addr> [--user <u>] [--port <p>] [--key <path>] [--jump <alias>] [--risk-override <level>] [--tags <t1,t2>]
```

参数说明：

| 参数 | 必填 | 说明 |
|------|------|------|
| `name` | 是 | 主机别名（用于后续命令引用） |
| `--host` | 是 | 主机地址（IP 或域名） |
| `--user` | 否 | SSH 用户名 |
| `--port` | 否 | SSH 端口（默认 22） |
| `--key` | 否 | SSH 私钥路径 |
| `--jump` | 否 | ProxyJump 跳板机别名 |
| `--risk-override` | 否 | 覆盖该主机的风险等级（low/medium/high） |
| `--tags` | 否 | 逗号分隔的标签列表，用于分组 |
| `--json` | 否 | 以 JSON 格式输出结果 |

示例 -- 添加带跳板机的主机：

```bash
agent2ssh host add bastion --host 10.0.0.1 --user admin --key ~/.ssh/id_ed25519
agent2ssh host add internal --host 192.168.1.100 --user deploy --jump bastion
```

示例 -- 设置风险覆盖（沙箱主机跳过确认）：

```bash
agent2ssh host add sandbox --host 10.0.0.50 --user test --risk-override low
```

### 删除主机

```bash
agent2ssh host rm <name>
```

以 JSON 格式输出：

```bash
agent2ssh host rm myserver --json
```

### 导入 SSH 配置

从 `~/.ssh/config` 文件批量导入主机（已存在的别名会被跳过）：

```bash
agent2ssh host import-config [--path ~/.ssh/config]
```

指定自定义配置文件路径：

```bash
agent2ssh host import-config --path /path/to/custom/ssh_config
```

---

## 命令执行

### 单主机执行

在指定主机上运行命令：

```bash
agent2ssh exec <host> "<command>" [--json] [--force] [--timeout-secs N]
```

参数说明：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `host` | - | 主机别名 |
| `command` | - | 远程命令 |
| `--json` | 关 | 以 JSON 格式输出（含 stdout、stderr、exit_code、duration_ms、risk_level） |
| `--force` | 关 | 执行高风险命令时必须携带 |
| `--timeout-secs` | 60 | 超时秒数 |
| `--stdin` | 无 | 将字符串传递到远程命令的 stdin |

示例：

```bash
# 普通命令
agent2ssh exec myserver "uptime"

# JSON 输出
agent2ssh exec myserver "uptime" --json

# 指定超时
agent2ssh exec myserver "long-running-task" --timeout-secs 300

# 传递 stdin
agent2ssh exec myserver "cat > /tmp/data.txt" --stdin "hello world"

# 高风险命令需要 --force
agent2ssh exec myserver "sudo systemctl restart nginx" --force
```

### 多主机并发执行

```bash
agent2ssh exec-multi <h1> <h2> --command "<cmd>" [--force] [--tags <tag>]
```

参数说明：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `hosts` | - | 一个或多个主机别名 |
| `--command` | - | 远程命令 |
| `--force` | 关 | 执行高风险命令时必须携带 |
| `--tags` | 无 | 按标签筛选主机（逗号分隔） |
| `--timeout-secs` | 60 | 超时秒数 |
| `--json` | 关 | JSON 输出 |

示例：

```bash
# 指定主机列表
agent2ssh exec-multi web1 web2 web3 --command "df -h"

# 按标签批量执行
agent2ssh exec-multi --command "systemctl restart nginx" --tags production --force

# JSON 输出
agent2ssh exec-multi web1 web2 --command "uptime" --json
```

---

## 文件传输 (sftp)

### 上传文件

```bash
agent2ssh sftp put <host> <local> <remote> [--json]
```

### 下载文件

```bash
agent2ssh sftp get <host> <remote> <local> [--json]
```

### 列出远程目录

```bash
agent2ssh sftp ls <host> <path> [--json]
```

### 查看远程文件信息

```bash
agent2ssh sftp stat <host> <path> [--json]
```

### 创建远程目录

递归创建目录（类似 `mkdir -p`）：

```bash
agent2ssh sftp mkdir <host> <path> [--json]
```

---

## 交互会话 (session)

持久化 PTY 会话允许保持交互式 SSH 连接，适合需要多次输入输出的场景。

### 打开会话

```bash
agent2ssh session open <host> [--json]
```

返回会话 ID（UUID），后续操作使用此 ID。

### 向会话写入输入

```bash
agent2ssh session write <session-id> "<input>"
```

### 读取会话输出

```bash
agent2ssh session read <session-id> [--timeout-ms N] [--json]
```

`--timeout-ms` 默认 2000 毫秒。

### 列出所有会话

```bash
agent2ssh session list [--json]
```

### 关闭会话

```bash
agent2ssh session close <session-id> [--json]
```

---

## 端口转发 (forward)

管理 SSH 端口转发隧道（本地转发 `-L` 或远程转发 `-R`）。

### 添加端口转发

```bash
agent2ssh forward add <host> --direction <local|remote> --bind-port <N> --target-host <addr> --target-port <N> [--json]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--direction` | local | 转发方向：`local` 或 `remote` |
| `--bind-port` | - | 绑定端口号 |
| `--target-host` | - | 目标主机地址 |
| `--target-port` | - | 目标端口号 |

### 列出所有转发

```bash
agent2ssh forward list [--json]
```

### 删除转发

```bash
agent2ssh forward rm <forward-id> [--json]
```

---

## 连通性检测 (ping)

检测一个或多个主机的 SSH 连通性和延迟：

```bash
agent2ssh ping <hosts...> [--timeout-secs N] [--json]
```

`--timeout-secs` 默认 5 秒。

示例：

```bash
agent2ssh ping web1 web2 web3
agent2ssh ping web1 --timeout-secs 3 --json
```

---

## 审计日志 (audit)

查询命令执行历史记录：

```bash
agent2ssh audit [--limit N] [--host H] [--risk LEVEL] [--exit-code N] [--since ISO] [--until ISO] [--json]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--limit` | 20 | 返回条数上限 |
| `--host` | 无 | 按主机别名过滤 |
| `--risk` | 无 | 按风险等级过滤（low/medium/high/blocked） |
| `--exit-code` | 无 | 按退出码过滤 |
| `--since` | 无 | ISO-8601 时间下界 |
| `--until` | 无 | ISO-8601 时间上界 |

示例：

```bash
agent2ssh audit --limit 50 --host myserver --risk high --json
agent2ssh audit --since 2025-01-01T00:00:00Z --until 2025-06-01T00:00:00Z
```

---

## 风险检查 (risk)

检查命令的风险等级（不执行命令）：

```bash
agent2ssh risk "<command>" [--host H] [--json]
```

风险等级说明：

| 等级 | 说明 |
|------|------|
| `low` | 安全的只读命令（ls、cat、whoami 等） |
| `medium` | 修改状态的命令（apt install、git push 等） |
| `high` | 潜在破坏性命令（sudo、rm -rf、chmod 777 等） |
| `blocked` | 无条件拒绝的危险命令（mkfs、rm -rf /、fork bomb 等） |

---

## 守护进程管理 (daemon)

### 启动守护进程

```bash
agent2ssh daemon start
```

守护进程启动后会在 `127.0.0.1:7722` 监听 HTTP API，并提供 Web 控制台。

### 停止守护进程

```bash
agent2ssh daemon stop
```

### 查看状态

```bash
agent2ssh daemon status
```

### 重启守护进程

```bash
agent2ssh daemon restart
```

### 列出所有守护进程

列出本地和远程守护进程及其连接状态：

```bash
agent2ssh daemon list [--json]
```

---

## 远程守护进程路由

通过 `--daemon` 全局参数将操作路由到远程守护进程：

```bash
agent2ssh --daemon <alias> exec <host> "<command>"
agent2ssh --daemon <alias> host list
```

远程守护进程需要在 `~/.agent2ssh/remotes.toml` 中配置，详见 [配置指南](./configuration-guide.md)。

---

## 实用示例

### 示例 1：初始化工作环境并批量检查服务器状态

```bash
# 1. 导入现有的 SSH 配置
agent2ssh host import-config

# 2. 添加一组带标签的生产服务器
agent2ssh host add web1 --host 10.0.1.10 --user deploy --key ~/.ssh/id_ed25519 --tags production,web
agent2ssh host add web2 --host 10.0.1.11 --user deploy --key ~/.ssh/id_ed25519 --tags production,web
agent2ssh host add db1  --host 10.0.1.20 --user deploy --key ~/.ssh/id_ed25519 --tags production,db

# 3. 检查所有主机的连通性
agent2ssh ping web1 web2 db1

# 4. 在所有 production 服务器上并发检查磁盘使用情况
agent2ssh exec-multi --command "df -h" --tags production --json
```

### 示例 2：通过跳板机安全访问内网服务器

```bash
# 1. 添加跳板机
agent2ssh host add bastion --host bastion.example.com --user admin --key ~/.ssh/id_ed25519

# 2. 添加内网服务器，通过跳板机连接
agent2ssh host add internal-db --host 192.168.1.50 --user deploy --jump bastion --port 22

# 3. 验证连通性
agent2ssh ping internal-db

# 4. 执行命令（自动通过跳板机）
agent2ssh exec internal-db "pg_isready" --json

# 5. 创建本地端口转发，方便本地 GUI 工具连接
agent2ssh forward add internal-db --direction local --bind-port 5432 --target-host localhost --target-port 5432
```

### 示例 3：自动化部署工作流

```bash
# 1. 上传新版本包
agent2ssh sftp put web1 ./dist/app-v2.0.tar.gz /opt/releases/app-v2.0.tar.gz

# 2. 解压并部署（设置较长超时）
agent2ssh exec web1 "cd /opt/releases && tar xzf app-v2.0.tar.gz && ./deploy.sh" --timeout-secs 300 --json

# 3. 验证部署结果
agent2ssh exec web1 "curl -s http://localhost:8080/health" --json

# 4. 查看审计日志确认操作记录
agent2ssh audit --host web1 --limit 5 --json
```

---

## 本地数据

所有配置和数据存储在 `~/.agent2ssh/` 目录下：

```text
~/.agent2ssh/
  hosts.json       # 主机配置
  daemon.token     # 守护进程认证令牌
  daemon.pid       # 守护进程 PID
  audit.jsonl      # 执行审计日志
  risk_rules.toml  # 用户自定义风险规则
  playbooks.toml   # Playbook 定义
  remotes.toml     # 远程守护进程配置
  webhook.toml     # Webhook 通知配置
  keys/            # SSH 密钥存储
```

详细配置说明请参考 [配置指南](./configuration-guide.md)。

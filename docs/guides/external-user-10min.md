# 外部用户 10 分钟接入剧本

目标：让第一次接触 Agent2SSH 的用户，在不阅读完整文档的情况下，把一个已有 SSH 主机接入 CLI，并让 Claude Code、Codex 或其他 MCP 客户端能看到 `agent2ssh` 工具。

## 适用范围

- 已经有一台能用普通 `ssh` 登录的机器。
- 本机可以安装或下载 Agent2SSH `v0.1.1`。
- 先验证低风险命令，例如 `hostname`、`uptime`、`whoami`。
- 不要求配置远程 daemon、审批流、playbook 或桌面端。

## 第 0 分钟：准备 SSH 基线

先确认原生 SSH 可用：

```bash
ssh <user>@<host> "hostname && uptime"
```

如果这一步失败，先修复普通 SSH。Agent2SSH 复用本机 SSH 凭据，不应该成为排查网络、密钥或跳板机问题的第一层。

## 第 1-2 分钟：安装 Agent2SSH

macOS 推荐 Homebrew：

```bash
brew tap lengyuqu/agent2ssh
brew install agent2ssh
```

其他平台从 GitHub Releases 下载对应压缩包，解压后把三个二进制放进 PATH：

- `agent2ssh`
- `agent2ssh-mcp`
- `agent2ssh-daemon`

验证：

```bash
agent2ssh --version
agent2ssh-mcp --version
```

## 第 3-4 分钟：导入或添加主机

如果主机已经在 `~/.ssh/config`：

```bash
agent2ssh host import-config
agent2ssh host list
```

否则手动添加：

```bash
agent2ssh host add mybox --host <host> --user <user> --key ~/.ssh/id_ed25519
agent2ssh host list
```

如果使用非标准端口：

```bash
agent2ssh host add mybox --host <host> --user <user> --port 2222 --key ~/.ssh/id_ed25519
```

如果使用跳板机，先添加跳板机，再添加目标机：

```bash
agent2ssh host add bastion --host <bastion-host> --user <user> --key ~/.ssh/id_ed25519
agent2ssh host add internal --host <internal-host> --user <user> --jump bastion
```

## 第 5 分钟：验证 CLI 执行

先跑只读命令：

```bash
agent2ssh ping mybox
agent2ssh exec mybox "hostname && uptime"
agent2ssh exec mybox "whoami" --json
```

预期结果：

- `ping` 显示主机可达。
- `exec` 返回远端输出。
- `--json` 输出包含 `stdout`、`stderr`、`exit_code`、`risk_level`。

## 第 6-7 分钟：接入 MCP 客户端

Agent2SSH 的 MCP server 是 stdio 进程，配置核心都是：

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

如果 `agent2ssh-mcp` 不在 PATH，改成绝对路径，例如：

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

### Codex

在 Codex 配置里添加 MCP server：

```toml
[mcp_servers.agent2ssh]
command = "agent2ssh-mcp"
args = []
```

如果需要隔离测试配置：

```toml
[mcp_servers.agent2ssh.env]
AGENT2SSH_CONFIG_DIR = "/tmp/agent2ssh-test"
```

重启 Codex 后，让它列出 MCP 工具或调用 `ssh_list_hosts`。

### Claude Code / Claude Desktop

在客户端 MCP 配置中加入 `agent2ssh` server。配置文件位置随客户端和平台不同而变化；核心 JSON 与上方一致。

重启客户端后，让 agent 执行：

```text
List my Agent2SSH hosts using ssh_list_hosts, then run hostname on mybox.
```

### 其他 MCP 客户端

只要客户端支持 stdio MCP server，就使用同一条命令：

```bash
agent2ssh-mcp
```

更多客户端模板见 [mcp-client-templates.md](mcp-client-templates.md)。

## 第 8 分钟：确认安全控制面

查看执行计划，不真正运行命令：

```bash
agent2ssh exec mybox "sudo systemctl restart nginx" --plan
```

高风险命令需要显式确认：

```bash
agent2ssh exec mybox "sudo systemctl restart nginx" --force
```

如果启动了 daemon，可以查看或切换全局 gate：

```bash
agent2ssh daemon start
agent2ssh status
agent2ssh pause
agent2ssh resume
```

## 第 9 分钟：收集反馈信息

最小反馈包：

```bash
agent2ssh --version
agent2ssh host list --json
agent2ssh exec mybox "hostname" --json
agent2ssh audit --limit 5 --json
```

提交问题时请脱敏：

- 删除真实 IP、用户名、路径、token、密钥名。
- 保留平台、安装方式、命令、错误信息、预期行为。
- 如果是 MCP 问题，说明使用的客户端和配置片段。

## 第 10 分钟：提交反馈

优先使用 GitHub issue：

- Bug：选择 `Bug report` 模板。
- 接入反馈：选择 `External adoption report` 模板。

如果不方便公开机器信息，可以只提交匿名接入报告：

- 不填真实主机名、IP、公司名或用户名。
- 只保留平台、安装方式、MCP 客户端、成功/失败步骤和错误类型。
- 不上传 audit 原文，改为粘贴脱敏后的关键字段。

## 隐私与匿名反馈

`v0.1.1` 默认不包含自动遥测，也不会自动上传 audit、命令、主机列表或 MCP 调用记录。

当前的匿名反馈是手动 opt-in：用户主动通过 issue、私信或其他渠道提交脱敏信息。后续如果引入运行时遥测，必须满足：

- 默认关闭。
- 配置项清晰，例如 `telemetry.enabled = false`。
- 只收集版本、平台、安装方式、功能入口成功/失败等采用指标。
- 不收集命令正文、stdout/stderr、主机地址、用户名、路径、token 或密钥信息。

## 快速失败排查

`agent2ssh-mcp` 找不到：

```bash
which agent2ssh-mcp
agent2ssh-mcp --version
```

主机列表为空：

```bash
agent2ssh host import-config
agent2ssh host list
```

CLI 可用但 MCP 不可用：

- 确认 MCP 客户端使用的 `command` 是绝对路径或在客户端 PATH 中。
- 重启 MCP 客户端。
- 用 CLI 先跑通 `agent2ssh host list` 和 `agent2ssh exec`。

高风险命令被拒：

- 先用 `--plan` 看风险原因。
- 确认不是 `blocked` 风险。`blocked` 命令不能用 `--force` 绕过。
- 高风险但允许的命令需要 `--force` 或审批路径。

# Skill 分发指南

本文档描述 Agent2SSH 作为 MCP (Model Context Protocol) skill 的使用方式、安装前提、版本管理以及安全建议。

---

## Skill 概述

Agent2SSH 以 `agent2ssh-mcp` 二进制形式暴露 MCP stdio 服务器，将 SSH 操作能力（主机管理、命令执行、SFTP、会话、端口转发、Playbook、审计、审批、健康检查、指标、execution gate 和 remote daemon 等）封装为 51 个 MCP 工具，供任何支持 MCP 协议的 AI 客户端（Claude Desktop、Cursor、Codex 等）直接调用。

**工作原理**：

1. AI 客户端通过 stdio 启动 `agent2ssh-mcp` 进程
2. 客户端发送 JSON-RPC 请求调用工具
3. `agent2ssh-mcp` 执行本地 SSH 操作并返回结果

无需额外服务器——MCP server 与 AI 客户端运行在同一台机器上。

除 MCP 工具外，仓库还内置一份标准 **Agent Skill**（`skills/agent2ssh/SKILL.md`，带 `name`/`description`/`version` frontmatter），为 agent 提供 CLI/MCP 的使用惯例（风险分级、审批流、`--force` 语义、常见工作流）。它随二进制一起编译内嵌，可通过以下任一方式安装：

```bash
# CLI：安装 / 查看状态 / 卸载（默认目录 ~/.claude/skills/agent2ssh，可用 --dir 覆盖）
agent2ssh integrate skill install
agent2ssh integrate skill status
agent2ssh integrate skill uninstall

# MCP 客户端注册也有对应的一条命令：
agent2ssh integrate list
agent2ssh integrate add claude_code   # 或 claude_desktop / cursor / codex / gemini_cli / windsurf 等
```

桌面端 **MCP Agents 面板**提供同样的图形化操作：客户端探测与 MCP 注册/更新/卸载，以及 Agent Skill 的安装/更新（内置版本高于已装版本时提示）/卸载。

---

## 安装前提

### 方式一：预编译二进制（推荐）

从 [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases) 下载对应平台的二进制文件：

| 平台 | 文件名 |
|------|--------|
| macOS (Apple Silicon) | `agent2ssh-mcp` (aarch64-apple-darwin) |
| macOS (Intel) | `agent2ssh-mcp` (x86_64-apple-darwin) |
| Linux (x86_64) | `agent2ssh-mcp` (x86_64-unknown-linux-gnu) |
| Windows (x86_64) | `agent2ssh-mcp.exe` (x86_64-pc-windows-msvc) |

下载后将二进制放入系统 PATH 中，例如：

```bash
# macOS / Linux
sudo mv agent2ssh-mcp /usr/local/bin/
chmod +x /usr/local/bin/agent2ssh-mcp

# Homebrew
brew install agent2ssh
```

### 方式二：从源码编译

需要 Rust 1.70+ 工具链：

```bash
cargo install --path src-tauri --bin agent2ssh-mcp --no-default-features
```

### 验证安装

```bash
agent2ssh-mcp --version
# 或手动测试 stdio 通信
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | agent2ssh-mcp
```

---

## 版本匹配

Agent2SSH 的所有组件（CLI、MCP server、daemon、Tauri 桌面应用）共享同一版本号，遵循 [语义化版本](versioning.md)。

| Skill 版本 | Agent2SSH 版本 | 兼容性 |
|-----------|---------------|--------|
| 0.1.x | 0.1.x | 完全兼容 |

**建议**：始终保持 MCP server 与 CLI/daemon 版本一致。工具集在次版本（minor）之间向后兼容，补丁版本（patch）之间完全兼容。

---

## 最小权限建议

在授予 AI 客户端 Agent2SSH 访问权限时，需了解不同工具的副作用范围。完整 51 个工具定义见 [MCP Tools Reference](skills.md)，以下按风险级别分类：

### 只读类（Read-only）

安全级别高，不会触发任何变更，可放心授予 AI 客户端。

```
ssh_list_hosts          ssh_list_daemons
ssh_ping                ssh_audit
ssh_sftp_ls             ssh_sftp_stat
ssh_session_list        ssh_session_read
ssh_forward_list        ssh_risk_check
ssh_approval_list       ssh_connection_status
ssh_playbook_list       ssh_config_export
ssh_webhook_config (get)
```

### 写入类（Write/Mutate）

会产生副作用，建议配合统一 `policy.toml` / `policy.json`、execution gate、执行限额和审批流程使用。

```
ssh_exec                ssh_exec_multi
ssh_session_open        ssh_session_write
ssh_session_close       ssh_sftp_upload
ssh_sftp_download       ssh_sftp_mkdir
ssh_forward_add         ssh_forward_remove
ssh_add_host            ssh_remove_host
ssh_import_config       ssh_connect
ssh_disconnect          ssh_approval_respond
ssh_playbook_run        ssh_webhook_config (set)
ssh_config_import
```

---

## 更新策略

### 检测新版本

```bash
# 查看当前版本
agent2ssh-mcp --version

# 通过 CLI 检查更新（如果已安装）
agent2ssh --version
```

### Homebrew 更新

```bash
brew update
brew upgrade agent2ssh
```

### 手动更新

1. 从 [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases) 下载最新版本
2. 替换旧的二进制文件
3. 重启 AI 客户端（MCP server 会在下次调用时重新启动）

### 自动更新建议

对于团队部署，建议使用版本锁定策略：

```toml
# 在 CI/CD 中固定版本
[agent2ssh]
version = "=0.1.1"
```

### 变更日志

每次版本更新的详细变更记录见 [CHANGELOG.md](../CHANGELOG.md)。

---

## 安全建议

1. **最小权限原则**：如果 AI 客户端仅需查询信息，可在 `policy.toml` 中将写入类命令或操作模式设为 `blocked`
2. **审批流程**：daemon 路由可以为高风险命令创建审批请求；本地 MCP 路径没有审批处理器时会失败关闭
3. **风险规则**：通过 `policy.toml` / `policy.json` 自定义哪些命令需要额外确认；旧版 `risk_rules.toml` 仅作为兼容入口，且用户规则只能升级内置风险
4. **Per-Host 覆盖**：为生产环境主机设置更严格的风险等级，或在可信沙箱中降低非 `blocked` 命令风险
5. **审计日志**：所有命令执行均记录在 `~/.agent2ssh/audit.jsonl` 中
6. **密钥安全**：`TeamConfigExport` 自动剥离 SSH 密钥路径，可安全分享

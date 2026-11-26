# MCP 客户端配置模板

Agent2SSH 作为 MCP (Model Context Protocol) stdio 服务器运行，可与任何支持 MCP 协议的 AI 客户端集成。以下是常见客户端的配置模板。

> **推荐：一条命令自动注册。** CLI 提供 `agent2ssh integrate` 子命令，可自动探测已安装的客户端并写入（带备份的）MCP 配置，无需手动编辑下面的模板。每个客户端都会在 Windows、macOS、Linux 各自可能出现的位置里找已经存在的配置文件：找到就改那一份，都没有才在当前系统的默认路径新建。Skill 目录用同一条规则。
>
> ```bash
> agent2ssh integrate list                # 查看各客户端的探测/注册状态
> agent2ssh integrate add claude_code     # 注册（支持 claude_code / claude_desktop / cursor / codex / trae / trae_cn / qoder / qoder_cn / windsurf / agy 等）
> agent2ssh integrate rm claude_code      # 移除注册（保留备份）
> agent2ssh integrate skill install       # 将 Agent Skill 安装到 ~/.claude/skills/agent2ssh
> ```
>
> 桌面端的 MCP Agents 面板提供同样的能力（含 Agent Skill 的安装/更新/卸载）。以下手动模板适用于未被 `integrate` 覆盖的客户端或自定义场景。

---

## 1. Claude Desktop

配置文件位置：

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "agent2ssh-mcp",
      "args": [],
      "env": {
        "AGENT2SSH_SOURCE": "claude_desktop"
      }
    }
  }
}
```

如果 `agent2ssh-mcp` 不在系统 PATH 中，请使用绝对路径：

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "/usr/local/bin/agent2ssh-mcp",
      "args": [],
      "env": {
        "AGENT2SSH_SOURCE": "claude_desktop"
      }
    }
  }
}
```

配置完成后重启 Claude Desktop，即可在对话中使用 SSH 相关工具。

---

## 2. Cursor

配置文件位置：项目根目录下 `.cursor/mcp.json`

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "agent2ssh-mcp",
      "env": {
        "AGENT2SSH_SOURCE": "cursor"
      }
    }
  }
}
```

如果需要指定额外参数或环境变量：

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "agent2ssh-mcp",
      "env": {
        "AGENT2SSH_SOURCE": "cursor",
        "AGENT2SSH_CONFIG_DIR": "/custom/path/.agent2ssh"
      }
    }
  }
}
```

在 Cursor 设置 > MCP 页面可确认服务器已连接。

---

## 3. Codex

Codex uses TOML-style MCP server entries:

```toml
[mcp_servers.agent2ssh]
command = "agent2ssh-mcp"
args = []

[mcp_servers.agent2ssh.env]
AGENT2SSH_SOURCE = "codex"
```

If `agent2ssh-mcp` is not in PATH, use an absolute path:

```toml
[mcp_servers.agent2ssh]
command = "/usr/local/bin/agent2ssh-mcp"
args = []

[mcp_servers.agent2ssh.env]
AGENT2SSH_SOURCE = "codex"
```

For isolated testing, set a separate config directory:

```toml
[mcp_servers.agent2ssh.env]
AGENT2SSH_SOURCE = "codex"
AGENT2SSH_CONFIG_DIR = "/tmp/agent2ssh-test"
```

Restart Codex after editing the config, then ask it to call `ssh_list_hosts`.

---

## 4. OpenAI Agents (Python)

使用 Python MCP SDK 连接 Agent2SSH：

```python
import asyncio
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

async def main():
    server_params = StdioServerParameters(
        command="agent2ssh-mcp",
        args=[],
    )

    async with stdio_client(server_params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()

            # 列出所有可用工具
            tools = await session.list_tools()
            for tool in tools.tools:
                print(f"- {tool.name}: {tool.description}")

            # 调用工具：列出主机
            result = await session.call_tool("ssh_list_hosts", {})
            print(result.content[0].text)

            # 调用工具：执行远程命令
            result = await session.call_tool("ssh_exec", {
                "host": "web1",
                "command": "uname -a",
            })
            print(result.content[0].text)

if __name__ == "__main__":
    asyncio.run(main())
```

安装依赖：

```bash
pip install mcp
```

---

## 5. 通用 MCP 客户端 (TypeScript)

使用 `@modelcontextprotocol/sdk` 包：

```typescript
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

async function main() {
  const transport = new StdioClientTransport({
    command: "agent2ssh-mcp",
    args: [],
  });

  const client = new Client(
    { name: "my-agent", version: "1.0.0" },
    { capabilities: {} }
  );

  await client.connect(transport);

  // 列出工具
  const { tools } = await client.listTools();
  console.log(`Available tools: ${tools.length}`);

  // 调用工具
  const result = await client.callTool({
    name: "ssh_list_hosts",
    arguments: {},
  });
  console.log(result);

  // 执行命令
  const execResult = await client.callTool({
    name: "ssh_exec",
    arguments: {
      host: "web1",
      command: "hostname",
    },
  });
  console.log(execResult);

  await client.close();
}

main().catch(console.error);
```

安装依赖：

```bash
npm install @modelcontextprotocol/sdk
```

---

## 6. Antigravity（agy）

`agent2ssh integrate add agy` 和其它客户端走同一套探测。它在当前用户主目录下找实际存在的文件（Windows 上主目录是 `%USERPROFILE%`），按这个顺序选用：

1. `~/.gemini/config/mcp_config.json` — 当前 CLI、App、IDE 都读的全局配置
2. `~/.gemini/antigravity/mcp_config.json` — 旧布局；只有这份存在时才写这里

两份都不存在时，注册会创建第 1 条。Skill 同样先找已经装过的目录（`~/.gemini/config/skills/agent2ssh`，其次 CLI 专用的 `~/.gemini/antigravity-cli/skills/agent2ssh` 和 `~/.gemini/skills/agent2ssh`），没有安装时写入 App 和 CLI 共用的第一处。工作区里的 `.agents/mcp_config.json` 只对当前仓库生效，`integrate` 不改它。

探测把 `agy` 命令、Antigravity 应用，以及 `~/.gemini/config`、`~/.gemini/antigravity`、`~/.gemini/antigravity-cli` 当作已安装。单独一个 `~/.gemini`（旧 Gemini CLI）不算。

---

## 7. Windsurf（现名 Devin Desktop）

Windsurf 在 2026 年 6 月改名为 Devin Desktop，MCP 配置也随之从 `.codeium` 迁到 `.devin`。`agent2ssh integrate add windsurf` 按本机实际安装的版本选择写入位置：

| 版本 | 配置文件 |
| --- | --- |
| Devin Desktop（当前） | macOS / Linux：`~/.config/devin/mcp_config.json`（或 `$XDG_CONFIG_HOME`）；Windows：`%APPDATA%\devin\mcp_config.json` |
| 旧版 Windsurf | `~/.codeium/windsurf/mcp_config.json` |

两处都有文件时写已经存在的那一份；只有一处有安装痕迹时优先那一处；都没有就创建当前版本的位置。

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

---

## 前提条件

在使用上述任何客户端之前，请确保：

1. **已安装 agent2ssh-mcp**：通过 Homebrew、cargo install 或从 [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases) 下载预编译二进制
2. **已配置主机**：运行 `agent2ssh host import-config` 从 `~/.ssh/config` 导入，或通过 `agent2ssh host add` 手动添加
3. **SSH 密钥已就位**：确保对应的 SSH 密钥文件存在且权限正确（`chmod 600`）

## 验证连接

配置完成后可通过以下方式验证 MCP 连接是否正常：

1. 在客户端中查找 MCP 服务器列表，确认 `agent2ssh` 显示为已连接状态
2. 尝试调用 `ssh_list_hosts` 工具，应返回已配置的主机列表
3. 尝试调用 `ssh_ping` 工具检查主机可达性

## 可用工具

Agent2SSH 共暴露 76 个 MCP 工具，涵盖主机管理、命令片段、命令执行、SFTP、会话、端口转发、审计、审批、健康检查、指标、execution gate、远程 daemon、本地执行（`local_exec`）与常驻会话（`resident_session_*`）以及脱敏/高亮/算法三类本地配置。完整列表参见 [MCP Tools Reference](../skills.md)。

第一次给外部用户接入时，建议优先使用 [外部用户 10 分钟接入剧本](external-user-10min.md)，再回到本页查具体客户端模板。

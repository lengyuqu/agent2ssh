# MCP 客户端配置模板

Agent2SSH 作为 MCP (Model Context Protocol) stdio 服务器运行，可与任何支持 MCP 协议的 AI 客户端集成。以下是常见客户端的配置模板。

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
      "args": []
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
      "args": []
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
      "command": "agent2ssh-mcp"
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
        "AGENT2SSH_CONFIG_DIR": "/custom/path/.agent2ssh"
      }
    }
  }
}
```

在 Cursor 设置 > MCP 页面可确认服务器已连接。

---

## 3. Codex / OpenAI Agents (Python)

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

## 4. 通用 MCP 客户端 (TypeScript)

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

## 5. Windsurf / Codeium

配置文件位置：`~/.codeium/windsurf/mcp_config.json`

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

Agent2SSH 共暴露 50 个 MCP 工具，涵盖主机管理、命令执行、SFTP、会话、端口转发、审计、审批、健康检查、指标和远程 daemon 等。完整列表参见 [MCP Tools Reference](../skills.md)。

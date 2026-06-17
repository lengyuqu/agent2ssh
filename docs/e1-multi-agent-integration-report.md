# E1 多 agent 接入验证报告

## 目标

验证 Agent2SSH MCP stdio server 对不同 agent 客户端的基础接入路径保持一致：初始化、工具枚举和安全工具调用都能工作。

## 本次验证范围

本次完成的是协议级 smoke：

- `codex`
- `opencode`
- `cursor`
- `claude-code`

验证方式是用同一个 `agent2ssh-mcp` 二进制，分别设置 `AGENT2SSH_SOURCE`，通过 JSON-RPC stdio 执行：

1. `initialize`
2. `tools/list`
3. `tools/call` -> `ssh_risk_check`，命令为 `rm -rf /`

这能覆盖 MCP server 对不同来源标识的基础兼容性，但不自动打开各客户端 UI。

## 可复现命令

```bash
python3 scripts/e1-mcp-client-smoke.py
```

可用环境变量覆盖 MCP 二进制：

```bash
AGENT2SSH_MCP_BIN=/path/to/agent2ssh-mcp python3 scripts/e1-mcp-client-smoke.py
```

## 验收结果

- 4 个 source label 均能完成 MCP initialize。
- `tools/list` 均返回 51 个工具。
- `ssh_risk_check` 均将 `rm -rf /` 判定为 `blocked`。

## 边界

真实客户端 UI 的菜单、配置文件路径、重启行为和权限提示仍属于外部 dogfood 范围。E1 当前关闭的是 MCP 协议和 source 标识兼容性，不替代 R4 的真人接入反馈。

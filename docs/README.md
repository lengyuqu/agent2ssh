# Agent2SSH documentation

English is the language of the product README, this index, the architecture note, the MCP tool reference, and the daemon API contract. The operator guides under `guides/` are written in Chinese.

This tree keeps documents that describe the current product. Completed plans, design proposals, regression logs, and investigation evidence are not kept here; git history still has them. Open limits live in [KNOWN-ISSUES.md](KNOWN-ISSUES.md).

## Start here

- [Help overview](guides/help.md) — desktop areas, common commands, safety notes
- [CLI quickstart](guides/cli-quickstart.md)
- [MCP quickstart](guides/mcp-quickstart.md)
- [10-minute external setup](guides/external-user-10min.md)
- [Web console](guides/web-console-guide.md)
- [Daemon API quickstart](guides/daemon-api-quickstart.md)

## Reference

- [Configuration guide](guides/configuration-guide.md) — `~/.agent2ssh/` layout and fields
- [Daemon identity and safe stop](guides/daemon-identity.md) — instance records, the stop path each platform actually takes, refusal and recovery rules
- [MCP client templates](guides/mcp-client-templates.md)
- [Architecture](architecture.md) — components, safety model, transport matrix, control plane
- [MCP tools](skills.md) — the 76 tools; `tools/list` offers the 26 `core` tools unless `AGENT2SSH_MCP_TOOLS=all`
- [Skill distribution](skill-distribution.md) — install, update, and uninstall the bundled Agent Skill
- [API contract](api.yaml) — daemon HTTP and WebSocket. A new daemon route and its `paths:` entry land together
- [Known issues](KNOWN-ISSUES.md) — current limits and unverified ranges
- [Release](RELEASE.md) — version policy and the release checklist
- [Branch protection](guides/branch-protection.md) — direct-to-`main` rules on GitHub

## 简体中文

索引以英文为准。日常操作说明在 `guides/`，是中文。当前限制见 [KNOWN-ISSUES.md](KNOWN-ISSUES.md)。已完成的计划和调查记录不在仓库里，需要时从 git 历史取出。

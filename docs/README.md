# Agent2SSH 文档

这里是 Agent2SSH 所有技术文档的入口，按使用场景分为三类。

## 快速入门

面向新用户的一站式指南：

- [帮助总览](guides/help.md) — 桌面端功能区、常用命令与安全说明
- [CLI 快速入门](guides/cli-quickstart.md) — 命令行主入口
- [MCP 快速入门](guides/mcp-quickstart.md) — 让 AI 客户端调用 SSH 能力
- [10 分钟接入剧本](guides/external-user-10min.md) — 外部用户快速上手
- [Web 控制台指南](guides/web-console-guide.md) — 浏览器端操作面
- [Daemon API 快速入门](guides/daemon-api-quickstart.md) — HTTP/WebSocket 接口

## 配置与参考

运行时配置和 API 文档：

- [配置指南](guides/configuration-guide.md) — `~/.agent2ssh/` 全部文件布局与字段说明
- [MCP 客户端模板](guides/mcp-client-templates.md) — Claude/Cursor/Codex/Windsurf 等配置
- [架构说明](architecture.md) — 组件、安全模型、控制面与持久化
- [MCP Tools Reference](skills.md) — 51 个 MCP 工具完整定义
- [Skill 分发指南](skill-distribution.md) — 安装方式、权限分级与更新策略
- [API 契约](api.yaml) — OpenAPI 规范（daemon HTTP/WS 端点）

## 项目与维护

面向贡献者与维护者的规划、发布与质量文档：

- [计划（合并版）](PLAN.md) — 活跃 Plan 2 + 历史归档（P0–K）+ Q1/Q2 执行报告
- [发布与版本](RELEASE.md) — 版本策略与发布清单
- [缺陷检查报告](DEFECTS.md) — 测试状态与已知缺陷跟踪
- [回归与研究报告](reports/REGRESSION-LOG.md) — 11 份历史回归/研究证据合集

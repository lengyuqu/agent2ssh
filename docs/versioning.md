# 版本策略（Versioning Policy）

本文件说明 agent2ssh 项目的版本号管理规则。

---

## 版本策略

- 所有组件（CLI、MCP server、daemon、桌面 App）共享同一版本号，来源于 `src-tauri/Cargo.toml` 的 `version` 字段
- 遵循 [Semantic Versioning 2.0](https://semver.org/)：`MAJOR.MINOR.PATCH`
- **MAJOR**：破坏性 API/协议变更（MCP tool 删除/重命名、daemon API 字段删除）
- **MINOR**：新功能，向后兼容（新增 MCP tool、新增 daemon 端点）
- **PATCH**：仅修复 bug

---

## MCP 协议版本

- MCP 协议版本独立于应用版本（当前 `2024-11-05`）
- 协议版本变更不影响应用版本号

---

## Daemon API 版本

- daemon HTTP API 跟随应用版本
- 破坏性变更需在 CHANGELOG 中标注

---

## 版本号同步位置

| 文件                          | 字段                          |
|-------------------------------|-------------------------------|
| `src-tauri/Cargo.toml`        | `version`                     |
| `src-tauri/tauri.conf.json`   | `version`                     |
| `package.json`                | `version`                     |
| `scripts/agent2ssh.rb`        | Homebrew formula version      |

---

## 预发布版本

在正式发布前可使用预发布标识：

- Alpha：`0.1.0-alpha.1`
- Beta：`0.1.0-beta.1`
- Release Candidate：`0.1.0-rc.1`

预发布版本仅发布到 GitHub Releases（标记为 Pre-release），不推送至 Homebrew 或正式渠道。

---

## 版本号更新流程

详细操作步骤见 [docs/release-checklist.md](release-checklist.md)。

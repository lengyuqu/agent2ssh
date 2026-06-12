# 发布清单（Release Checklist）

本文件描述 agent2ssh 每次发布新版本时需要执行的完整流程。

---

## 1. 发布前（Pre-release）

- [ ] 确认 `main` 分支 CI 全绿（`build` job + `tauri-bundle` job）
- [ ] 更新 `CHANGELOG.md`
- [ ] 版本号同步修改：
  - `src-tauri/Cargo.toml` — `[package] version`
  - `package.json` — `"version"`
  - `src-tauri/tauri.conf.json` — `"version"`
- [ ] 本地运行前端构建，确保无报错：
  ```bash
  npm run build
  ```
- [ ] 本地运行 Rust 测试，确保全部通过：
  ```bash
  cargo test --no-default-features --lib
  ```

---

## 2. 打标签并推送（Tag and Push）

```bash
git tag -a v0.X.0 -m "Release v0.X.0"
git push github main --tags
git push git233 main --tags
```

> **说明**：`github` 和 `git233` 是两个远程仓库，均需推送标签以触发 CI 和备份。

---

## 3. CI 构建（CI Builds）

- [ ] 确认 `build` job 已通过（4 平台编译：macOS x86_64 / aarch64、Linux x86_64、Windows x86_64）
- [ ] 确认 `tauri-bundle` job 已产出安装包（`.dmg` / `.AppImage` / `.msi`）
- [ ] 检查 GitHub Releases 页面，确认所有构建资产完整

---

## 4. 发布后（Post-release）

- [ ] 更新 Homebrew formula（`scripts/agent2ssh.rb`）的 `version` 和 `sha256`
- [ ] 在 macOS / Linux / Windows 各执行 `scripts/verify-install.sh`
- [ ] 确认 CLI、daemon、MCP server 均可正常启动：
  - CLI：`agent2ssh --version`
  - Daemon：`agent2ssh daemon status`
  - MCP Server：`agent2ssh-mcp --help`

---

## 快速参考

| 文件                        | 字段                          |
|-----------------------------|-------------------------------|
| `src-tauri/Cargo.toml`      | `[package] version = "0.X.0"` |
| `package.json`              | `"version": "0.X.0"`          |
| `src-tauri/tauri.conf.json` | `"version": "0.X.0"`          |

版本号策略详见 [docs/versioning.md](versioning.md)。

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
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
  ```
- [ ] 本地运行 CLI / MCP / daemon 编译检查：
  ```bash
  cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
  cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
  ```
- [ ] 本地运行集成与 smoke 测试：
  ```bash
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
  ```
- [ ] 如需完整本机验收，可运行：
  ```bash
  ./scripts/e2e-local.sh
  ```
- [ ] 如需本地 Tauri 打包，先准备 sidecar 二进制：
  ```bash
  TARGET="$(rustc -vV | sed -n 's/^host: //p')"
  cargo build --manifest-path src-tauri/Cargo.toml --release --target "$TARGET" --no-default-features --bin agent2ssh --bin agent2ssh-mcp
  cargo build --manifest-path src-tauri/Cargo.toml --release --target "$TARGET" --no-default-features --features daemon --bin agent2ssh-daemon
  ./scripts/prepare-sidecars.sh "$TARGET"
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
- [ ] 确认每个平台的 `CHECKSUMS-SHA256.txt` 已上传为 release asset

---

## 3.5 校验和验证（Checksum Verification）

- [ ] 下载 release 资产和对应的 `CHECKSUMS-SHA256.txt`
- [ ] 验证下载文件的 SHA256 校验和：

```bash
# macOS / Linux
shasum -a 256 -c CHECKSUMS-SHA256.txt --ignore-missing
# 或
sha256sum -c CHECKSUMS-SHA256.txt --ignore-missing
```

- [ ] 确认所有文件校验通过（输出 `OK`）

> **用户提示**：在 README 和安装文档中建议用户在安装前验证校验和。详见 [配置指南 - 校验和验证](guides/configuration-guide.md#校验和验证)。

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

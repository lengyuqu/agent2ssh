# 发布与版本（Release & Versioning）

本文件合并了原 `versioning.md`（版本策略）与 `release-checklist.md`（发布清单），是当前发布的唯一参考。

当前发布收口版本：`v0.2.1`。

---

## 1. 版本策略

- 所有组件（CLI、MCP server、daemon、桌面 App）共享同一版本号，来源于 `src-tauri/Cargo.toml` 的 `version` 字段
- 遵循 [Semantic Versioning 2.0](https://semver.org/)：`MAJOR.MINOR.PATCH`
- **MAJOR**：破坏性 API/协议变更（MCP tool 删除/重命名、daemon API 字段删除）
- **MINOR**：新功能，向后兼容（新增 MCP tool、新增 daemon 端点）
- **PATCH**：仅修复 bug

### MCP 协议版本

- MCP 协议版本独立于应用版本（当前 `2024-11-05`）
- 协议版本变更不影响应用版本号

### Daemon API 版本

- daemon HTTP API 跟随应用版本
- 破坏性变更需在 CHANGELOG 中标注

### 版本号同步位置

| 文件                          | 字段                          |
|-------------------------------|-------------------------------|
| `src-tauri/Cargo.toml`        | `version`                     |
| `src-tauri/tauri.conf.json`   | `version`                     |
| `package.json`                | `version`                     |
| `package-lock.json`           | root package `"version"`      |
| `docs/api.yaml`               | `info.version = "X.Y.Z"`      |
| `scripts/agent2ssh.rb`        | Homebrew formula version      |

### 预发布版本

在正式发布前可使用预发布标识：

- Alpha：`0.1.0-alpha.1`
- Beta：`0.1.0-beta.1`
- Release Candidate：`0.1.0-rc.1`

预发布版本仅发布到 GitHub Releases（标记为 Pre-release），不推送至 Homebrew 或正式渠道。

### 版本号更新流程

详细操作步骤见下方「发布清单」第 1 节（发布前版本号同步修改）。

---

## 2. 发布清单（Release Checklist）

每次发布新版本时需要执行的完整流程。

### 2.1 发布前（Pre-release）

- [ ] 确认 `main` 分支 CI 全绿（`build` job + `tauri-bundle` job）
- [ ] 更新 `CHANGELOG.md`
- [ ] 版本号同步修改：
  - `src-tauri/Cargo.toml` — `[package] version`
  - `package.json` — `"version"`
  - `package-lock.json` — root package `"version"`
  - `src-tauri/tauri.conf.json` — `"version"`
  - `docs/api.yaml` — `info.version`
  - `scripts/agent2ssh.rb` — Homebrew formula `version`
- [ ] 本地运行前端构建，确保无报错：
  ```bash
  npm run build
  ```
- [ ] 本地运行格式、Clippy 和 diff 卫生检查，确保无 warning / whitespace error：
  ```bash
  cargo fmt --manifest-path src-tauri/Cargo.toml --check
  cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings
  git diff --check
  ```
- [ ] 本地运行 Rust 测试，确保全部通过（两套配置均需零 warning）：
  ```bash
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon
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
- [ ] 文档一致性自动检查（已内置于测试套件）：
  - MCP 工具名与 `docs/skills.md` 交叉比对（`mcp_tools_match_skills_md_documentation`）
  - `/exec`、`/exec-multi`、`/playbooks/run`、`/audit/export` 请求响应 schema fixture 检查
  - CLI `exec`、`exec-multi`、`playbook run` 的 `--help` 与文档参数对齐检查
- [ ] 桌面国际化静态检查：
  - 抽取前端 `t("...")` 字面量和动态模块 label，确认中文表无缺译
  - 检查翻译前后 `{placeholder}` 集合一致
- [ ] 确认 CI 的 `Contract consistency` job 通过；该 job 显式运行上述 S3 契约检查，且 `build` matrix 依赖它。
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

### 2.2 打标签并推送（Tag and Push）

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push github main --tags
git push git233 main --tags
```

> **说明**：`github` 和 `git233` 是两个远程仓库，均需推送标签以触发 CI 和备份。

### 2.3 CI 构建（CI Builds）

- [ ] 确认 `build` job 已通过（4 平台编译：macOS x86_64 / aarch64、Linux x86_64、Windows x86_64）
- [ ] 确认 `tauri-bundle` job 已产出安装包（`.dmg` / `.AppImage` / `.msi`）
- [ ] 检查 GitHub Releases 页面，确认所有构建资产完整
- [ ] 确认每个平台的 `CHECKSUMS-SHA256.txt` 已上传为 release asset

### 2.4 校验和验证（Checksum Verification）

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

### 2.5 发布后（Post-release）

- [ ] 更新 Homebrew formula（`scripts/agent2ssh.rb`）的 `version` 和 `sha256`
- [ ] 在 macOS / Linux / Windows 各执行 `scripts/verify-install.sh`
- [ ] 确认 CLI、daemon、MCP server 均可正常启动：
  - CLI：`agent2ssh --version`
  - Daemon：`agent2ssh daemon status`
  - MCP Server：通过 stdio 发送 `tools/list` 确认可用工具数

---

## 3. 快速参考

| 文件                        | 字段                          |
|-----------------------------|-------------------------------|
| `src-tauri/Cargo.toml`      | `[package] version = "X.Y.Z"` |
| `package.json`              | `"version": "X.Y.Z"`          |
| `package-lock.json`         | root package `"version"`      |
| `src-tauri/tauri.conf.json` | `"version": "X.Y.Z"`          |
| `docs/api.yaml`             | `info.version = "X.Y.Z"`      |
| `scripts/agent2ssh.rb`      | `version "X.Y.Z"`             |

版本号策略详见本文档第 1 节。

---

## 4. 已知限制

- PTY session 首次读取可能返回 SSH 登录 banner/prompt，命令输出可能需要后续 read
- `agent2ssh-daemon` 和 `agent2ssh-mcp` 运行即启动服务；安装验证脚本只检查二进制存在和可执行权限，避免阻塞在服务进程上
- Windows 运行时已由 2026-06-22 真机测试确认；后续平台差异按明确 bug 处理
- Webhook 出站使用非阻塞 fire，远端慢时通知可能超时且无自动重试
- macOS 本地打包未配置 Apple notarization 环境变量时会跳过公证；正式发布需配置 Apple ID/API key 与 Team ID 后再发布

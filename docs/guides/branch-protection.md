# 分支策略

状态：**直推 `main`**（2026-10-22 起为当前生效策略）· 取代此前的「禁止直推」草案

## 当前策略

所有变更直接 `git push origin main`，不走 Pull Request。

```bash
git add -A
git commit -F -      # 或 -m
git push origin main
```

## 为什么

| 条件 | 现状 |
|---|---|
| 常规提交者 | 1 人（维护者），另一台机器是同一个人的 macOS 开发机 |
| 审查者 | 没有第三方可审查——开 PR 等于自己批自己 |
| 协作方式 | 两机轮流推进，靠 `git pull --ff-only` 保持线性；PR 会引入 merge commit，反而破坏线性历史 |
| 分支保护的收益 | 拦住的只有维护者自己，成本是每次改动多两次往返 |

一句话：**单人 + 双机的仓库，PR 是仪式，不是控制。**

## 代价与补偿

直推放弃了「CI 通过才能合并」这道门。补偿方式是**把门禁搬到本地**，提交前跑完：

```bash
npm test && npm run lint

cd src-tauri
cargo fmt --check
cargo clippy --no-default-features --bin agent2ssh --bin agent2ssh-mcp -- -D warnings
cargo clippy --no-default-features --features daemon --bin agent2ssh-daemon -- -D warnings
cargo clippy --lib -- -D warnings
cargo test --no-default-features          # 全 target，不只是 --lib
cargo test --no-default-features --features daemon
```

`cargo test --no-default-features` **不带 `--lib`** 是重点：`connect_deadline`、`cli_contract`、
`daemon_integration` 这些集成测试只在全 target 时才跑。2026-10-22 有一次提交只跑了 `--lib` 和
`cli_smoke`，漏掉 `connect_deadline`，三个平台的 CI `build` 腿同时红——那个测试正是发现握手
超时回归的地方。

## 两个远端，别搞混

| 远端 | 地址 | 作用 |
|---|---|---|
| `origin`（Gitea） | `git233.phynews.com:18443/workbuddy/agent2ssh` | 日常推拉，本仓库唯一的 remote |
| GitHub（`lengyuqu/agent2ssh`） | `github.com/lengyuqu/agent2ssh` | CI（`.github/workflows/ci.yml`）实际运行的地方 |

**推 Gitea 不会触发 GitHub Actions**，两者是独立仓库。CI 是否跟上了 `main`，用这个查：

```bash
gh api repos/lengyuqu/agent2ssh/commits/main --jq .sha    # GitHub 上的头
git rev-parse main                                        # 本地的头
gh run list -R lengyuqu/agent2ssh -L 5                    # 最近的运行
```

两个 sha 不一致时，GitHub 上的 CI 结果描述的是旧代码，不能用来判断当前提交。

## 什么情况下切回 PR

满足任一条就该启用（下方的配置清单保留备用）：

- 出现第二位常规提交者，或需要外部贡献者的代码审查；
- 单一改动开始跨越多个子系统、review 收益大于往返成本；
- 发布流程需要「禁止直推」作为合规证据。

## 备用：PR + 分支保护的配置清单

启用时的 GitHub 侧设置：

| 规则 | 值 |
|------|-----|
| Require pull request before merging | 启用 |
| Required approvals | 1 |
| Require status checks to pass | 启用，勾选 `contract-consistency`、`build` |
| Require branches to be up to date | 启用 |
| Require linear history | 启用 |
| Include administrators | 启用 |
| Allow force pushes / deletions | 均禁用 |

步骤：**Settings → Branches → Add branch protection rule** → pattern 填 `main` → 逐项勾选上表 →
添加 `contract-consistency`、`build` 为必需检查 → Save。

启用后验证：`echo >> README.md && git commit -am test && git push origin main` 应被拒绝，
报错形如 `GH006: Protected branch update failed for refs/heads/main`。

注意：以上只在 **GitHub** 侧生效。Gitea（`origin`）有自己的一套保护设置，两边需要分别配置，
否则「GitHub 拦住了、Gitea 没拦住」会让人误判。

## 相关文档

- [CI/Release 工作流](../../.github/workflows/ci.yml)
- [CODEOWNERS](../../.github/CODEOWNERS)
- [发布与版本](../RELEASE.md)
- [Windows 验证交接记录](../reports/WINDOWS-HANDOVER.md)

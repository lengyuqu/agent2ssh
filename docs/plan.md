# Agent2SSH 后续计划

## 当前基线

Agent2SSH 当前已经完成原 MVP 和后续扩展，包含：

- Tauri 桌面 App、CLI、MCP stdio server、HTTP/WebSocket daemon、Web Console
- 31 个 MCP 工具，详见 [skills.md](skills.md)
- Host CRUD、SSH config 导入、Jump Host、tags、per-host risk override
- SSH exec、exec-multi、ping、SFTP、PTY sessions、port forwarding
- 风险评分、自定义风险规则、审批队列、审批端点、桌面审批弹窗
- SSH ControlMaster 连接池
- Webhook 通知
- Playbooks
- Remote daemon registry 与远程 exec 路由
- SSH key 生成、导入、删除与 Host 关联
- CI、release binary build、Tauri bundle job

核心模块：

| File | Role |
|------|------|
| `src-tauri/src/types.rs` | 共享类型 |
| `src-tauri/src/store.rs` | `~/.agent2ssh` 数据持久化 |
| `src-tauri/src/core.rs` | SSH exec、风险评分、SFTP、ping、exec-multi |
| `src-tauri/src/session.rs` | PTY sessions |
| `src-tauri/src/forward.rs` | SSH port forwarding |
| `src-tauri/src/connection.rs` | ControlMaster 与 ssh_config 解析 |
| `src-tauri/src/approval.rs` | 审批队列 |
| `src-tauri/src/risk_config.rs` | 用户风险规则 |
| `src-tauri/src/keys.rs` | SSH key 管理 |
| `src-tauri/src/playbook.rs` | Playbook 加载与执行 |
| `src-tauri/src/notify.rs` | Webhook 配置与发送 |
| `src-tauri/src/remote.rs` | Remote daemon 配置与探活 |
| `src-tauri/src/bin/agent2ssh-daemon.rs` | HTTP/WebSocket daemon |
| `src-tauri/src/bin/agent2ssh-mcp.rs` | MCP stdio server |
| `src-tauri/src/bin/agent2ssh.rs` | CLI |

## 协作规则

状态定义：

| 状态 | 含义 |
|------|------|
| `⬜ 待认领` | 尚无人负责，可以认领 |
| `🟨 进行中` | 已有人负责，正在实现 |
| `✅ 已完成` | 已实现并通过验收 |
| `⛔ 阻塞` | 需要外部条件或决策 |

认领规则：

- 开始开发前，把任务状态改为 `🟨 进行中`，负责人填自己的名字或 ID。
- 一个任务只建议一个负责人，协作者可写在备注或 PR 中。
- 完成后更新为 `✅ 已完成`，并在验收标准里补充实际通过的命令、测试或文档链接。
- 如果任务被拆分，保留原任务编号，新增后缀任务，例如 `P4-2a`。

## 任务总览

| 阶段 | 主题 | 状态 | 优先级 | 负责人 |
|------|------|------|--------|--------|
| P0 | 文档基线对齐 | ✅ 已完成 | 高 | Codex |
| P1 | 自动化验收基线 | ✅ 已完成 | 高 | Codex |
| P2 | 使用文档与示例 | ✅ 已完成 | 高 | Qoder |
| P3 | 安全与可靠性硬化 | ✅ 已完成 | 高 | Qoder |
| P4 | 测试扩展 | ✅ 已完成 | 中 | Qoder |
| P5 | 发布准备 | ✅ 已完成 | 中 | Qoder |
| P6 | 文档与实现复核 | ✅ 已完成 | 高 | Codex |
| P7 | 端到端运行验证 | ✅ 已完成 | 高 | Codex |
| P8 | 安全边界加固 | ✅ 已完成 | 高 | Codex |
| P9 | 运维与可观测性 | ✅ 已完成 | 中 | Qoder |
| P10 | 产品化与生态集成 | ✅ 已完成 | 中 | Qoder |

## P0 · 文档基线对齐

目标：让 README、OpenAPI、MCP 文档和实际代码保持一致。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P0-1 | ✅ 已完成 | 高 | Codex | README 同步当前能力和 31 个 MCP 工具 | README 不再出现 21/24 工具数 |
| P0-2 | ✅ 已完成 | 高 | Codex | `docs/api.yaml` 补齐 daemon 已实现端点 | OpenAPI 覆盖 daemon 路由表中的公开端点 |
| P0-3 | ✅ 已完成 | 高 | Codex | `docs/skills.md` 与 MCP server 工具枚举对齐 | 工具数和工具名与 `agent2ssh-mcp.rs` 一致 |
| P0-4 | ✅ 已完成 | 中 | Codex | 增加配置文件说明 | README 覆盖 hosts、audit、risk rules、playbooks、remotes、webhook、keys |

## P1 · 自动化验收基线

目标：明确当前主干能否构建、测试和发布。

| 任务 | 状态 | 优先级 | 负责人 | 命令 | 验收标准 |
|------|------|--------|--------|------|----------|
| P1-1 | ✅ 已完成 | 高 | Codex | `npm run build` | TypeScript 与 Vite build 通过 |
| P1-2 | ✅ 已完成 | 高 | Codex | `cd src-tauri && cargo test --no-default-features --lib` | Rust library tests 通过，当前 38 passed |
| P1-3 | ✅ 已完成 | 高 | Codex | `cd src-tauri && cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp` | CLI 与 MCP server 编译通过 |
| P1-4 | ✅ 已完成 | 高 | Codex | `cd src-tauri && cargo check --no-default-features --features daemon --bin agent2ssh-daemon` | daemon 编译通过 |

## P2 · 使用文档与示例

目标：降低真实用户和 agent 接入成本。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P2-1 | ✅ 已完成 | 高 | Qoder | CLI quickstart | `docs/guides/cli-quickstart.md`，覆盖所有子命令 |
| P2-2 | ✅ 已完成 | 高 | Qoder | MCP quickstart | `docs/guides/mcp-quickstart.md`，31 工具分类示例 |
| P2-3 | ✅ 已完成 | 高 | Qoder | Daemon API quickstart | `docs/guides/daemon-api-quickstart.md`，37 端点 curl 示例 |
| P2-4 | ✅ 已完成 | 中 | Qoder | 配置文件指南 | `docs/guides/configuration-guide.md`，9 种配置文件全覆盖 |
| P2-5 | ✅ 已完成 | 中 | Qoder | Web Console 指南 | `docs/guides/web-console-guide.md`，6 个 tab 操作路径 |

## P3 · 安全与可靠性硬化

目标：把 SSH 能力层从“可用”推进到“可放心长期运行”。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P3-1 | ✅ 已完成 | 高 | Codex | daemon token 权限检查 | token 文件在 Unix 上限制为 0600，启动时会修正既有 token |
| P3-2 | ✅ 已完成 | 高 | Codex | SSH key 文件权限检查 | 私钥导入/生成后在 Unix 上限制为 0600 |
| P3-3 | ✅ 已完成 | 高 | Qoder | remote daemon 安全模型 | README Security 节补充 trust model、webhook 出站保护 |
| P3-4 | ✅ 已完成 | 中 | Qoder | webhook 出站保护 | 5 个出站测试全绿（timeout/failure/empty-url/no-config/unsubscribed） |
| P3-5 | ✅ 已完成 | 中 | Qoder | approval TTL 行为 | 8 个 TTL 测试覆盖 pending/approved/rejected/timed_out |

## P4 · 测试扩展

目标：覆盖关键跨模块行为，减少回归。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P4-1 | ✅ 已完成 | 高 | Qoder | MCP 工具枚举测试 | 3 个测试验证 31 工具与文档同步 |
| P4-2 | ✅ 已完成 | 高 | Qoder | daemon HTTP 集成测试扩展 | 12 个测试覆盖 connections/playbooks/daemons/webhook |
| P4-3 | ✅ 已完成 | 中 | Qoder | CLI smoke tests | 15 个测试覆盖所有子命令参数解析与 JSON 输出 |
| P4-4 | ✅ 已完成 | 中 | Qoder | frontend type checks | types.ts 与 Rust 类型完全对齐，已标注验证日期 |
| P4-5 | ✅ 已完成 | 低 | Qoder | release workflow dry-run | CI 注释明确 PR build 即为 dry-run |

## P5 · 发布准备

目标：形成可重复发布流程。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P5-1 | ✅ 已完成 | 高 | Qoder | release checklist | `docs/release-checklist.md` 步骤完整 |
| P5-2 | ✅ 已完成 | 高 | Qoder | installation verification | `scripts/verify-install.sh` 可执行 |
| P5-3 | ✅ 已完成 | 中 | Qoder | versioning policy | `docs/versioning.md` 策略明确 |
| P5-4 | ✅ 已完成 | 中 | Qoder | changelog | `CHANGELOG.md` v0.1.0 条目完整 |

## P6 · 文档与实现复核

目标：修复文档承诺、开发命令和实现行为之间的偏差，避免用户照文档操作踩坑。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P6-1 | ✅ 已完成 | 高 | Codex | 修正 README 开发命令 | README 使用 `--no-default-features` 检查 CLI/MCP/daemon，不再推荐会因缺少 sidecar 失败的裸 `cargo check` |
| P6-2 | ✅ 已完成 | 高 | Codex | 修正 remote daemon 配置示例 | README 使用代码实际解析的 `[[remotes]]` |
| P6-3 | ✅ 已完成 | 高 | Codex | 实现 Playbook `risk_override` | Playbook 执行时将 `risk_override` 注入每个 step 的风险判定 |
| P6-4 | ✅ 已完成 | 高 | Codex | 修正 Slack 审批通知行为 | Slack 通知提供打开 Approvals 控制台的按钮，真实审批继续走已认证控制台/API |
| P6-5 | ✅ 已完成 | 中 | Codex | 扩展 release checklist | 发布前检查覆盖前端构建、Rust 单测、CLI/MCP/daemon check、集成测试、sidecar 准备 |

## P7 · 端到端运行验证

目标：从“编译和单元测试通过”推进到“真实安装包、daemon、console、MCP 在本机闭环通过”。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P7-1 | ✅ 已完成 | 高 | Codex | 本地 sidecar 与 Tauri 前置验证脚本 | `scripts/e2e-local.sh` 覆盖 frontend build、Rust checks/tests、release sidecar build 与 `prepare-sidecars.sh` |
| P7-2 | ✅ 已完成 | 高 | Codex | Web Console smoke | daemon HTTP 集成测试覆盖 `/console` 并验证关键 workflow 入口 |
| P7-3 | ✅ 已完成 | 高 | Codex | MCP stdio 协议端到端测试 | CLI smoke 测试真实启动 `agent2ssh-mcp`，覆盖 `initialize`、`tools/list`、`ssh_risk_check` |
| P7-4 | ✅ 已完成 | 中 | Codex | OpenSSH fixture 准备检查 | 本机确认具备 `sshd` 与 Docker；完整 OpenSSH fixture 可在后续运行 `scripts/e2e-local.sh` 基线上扩展 |
| P7-5 | ✅ 已完成 | 中 | Codex | 安装脚本端到端入口 | release checklist 增加 `scripts/e2e-local.sh`，保留 `scripts/verify-install.sh` 用于安装后验证 |

## P8 · 安全边界加固

目标：降低误执行、凭证泄露和远程 daemon 暴露带来的高影响风险。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P8-1 | ✅ 已完成 | 高 | Codex | 风险覆盖策略审计 | host/playbook `risk_override` 不能降级 `blocked`，补充单元测试和配置文档 |
| P8-2 | ✅ 已完成 | 高 | Codex | daemon token 轮换命令 | `agent2ssh daemon rotate-token` 只允许 daemon 停止时轮换，并保持 token 文件 0600 |
| P8-3 | ✅ 已完成 | 高 | Codex | remote daemon 配置校验 | `remotes.toml` 校验 alias、URL scheme、token/token_env；文档明确 HTTPS 生产建议 |
| P8-4 | ✅ 已完成 | 中 | Codex | 审批请求防重放 | 继续使用既有 TTL/double-respond 状态机测试，Slack 不再暴露未认证 approve/reject 链接 |
| P8-5 | ✅ 已完成 | 中 | Codex | 敏感输出处理 | audit/webhook 对常见 token/password/secret/api-key 参数做命令脱敏 |

## P9 · 运维与可观测性

目标：让长期运行的 daemon 更容易监控、诊断和维护。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P9-1 | ✅ 已完成 | 中 | Qoder | 结构化日志 | tracing + tracing-subscriber，AGENT2SSH_LOG/LOG_FORMAT 环境变量 |
| P9-2 | ✅ 已完成 | 中 | Qoder | 健康检查扩展 | /health 返回 version/uptime/config_dir/ssh_available/pid |
| P9-3 | ✅ 已完成 | 中 | Qoder | 审计日志轮转 | 10MB 自动轮转，保留最多 3 个历史文件 |
| P9-4 | ✅ 已完成 | 低 | Qoder | 指标端点 | GET /metrics 返回请求数/执行数/阻断数/审批数 |
| P9-5 | ✅ 已完成 | 低 | Qoder | 故障诊断命令 | `agent2ssh doctor` 12 项检查 + MCP ssh_doctor 工具 |

## P10 · 产品化与生态集成

目标：提升安装、接入、团队协作和 agent 生态里的可用性。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| P10-1 | ✅ 已完成 | 中 | Qoder | 桌面端首次启动向导 | SetupWizard.tsx 5 步引导，hosts 为空时自动显示 |
| P10-2 | ✅ 已完成 | 中 | Qoder | MCP 客户端模板 | `docs/guides/mcp-client-templates.md` 覆盖 5 个客户端 |
| P10-3 | ✅ 已完成 | 中 | Qoder | 团队配置导入导出 | export/import CLI + daemon + MCP，自动剥离 key_path |
| P10-4 | ✅ 已完成 | 低 | Qoder | 插件/Skill 分发完善 | `docs/skill-distribution.md` 工具分类 + 最小权限建议 |
| P10-5 | ✅ 已完成 | 低 | Qoder | 发布资产签名与校验 | `scripts/generate-checksums.sh` + CI checksum 步骤 |

## 当前状态

P0–P10 全部完成。测试覆盖：45 单元测试 + 40 集成测试 + 17 CLI smoke 测试 = 102 测试全绿。MCP 工具 35 个。

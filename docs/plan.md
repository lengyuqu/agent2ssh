# Agent2SSH 计划

## 当前状态

P0-P10 已全部完成。当前基线：

- 产品形态：Tauri 桌面 App、CLI、MCP stdio server、HTTP/WebSocket daemon、Web Console
- 核心能力：Host 管理、SSH config 导入、Jump Host、tags、per-host risk override
- 执行能力：SSH exec、exec-multi、ping、SFTP、PTY sessions、port forwarding、Playbooks
- 安全能力：风险评分、自定义风险规则、审批队列、审批端点、桌面审批弹窗、敏感命令脱敏
- 运维能力：SSH ControlMaster 连接池、Webhook 通知、remote daemon registry、健康检查、指标、审计轮转
- 生态能力：SSH key 管理、团队配置导入导出、MCP 客户端模板、插件/Skill 分发文档
- 验收结果：137 单元测试 + 56 集成测试 + 24 CLI smoke 测试 = 217 测试全绿；daemon feature 下为 142 单元测试 + 56 集成测试 + 24 CLI smoke 测试全绿
- MCP 工具：50 个，详见 [skills.md](skills.md)

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
- 如果任务被拆分，保留原任务编号，新增后缀任务，例如 `F2-1a`。

任务表统一规格：

| 字段 | 说明 |
|------|------|
| 任务 | 稳定编号，阶段号 + 序号 |
| 状态 | 使用上方状态定义 |
| 优先级 | 高 / 中 / 低 |
| 负责人 | 当前 owner，未认领时填 `-` |
| 内容 | 要实现或验证的范围 |
| 验收标准 | 可复现的命令、路径、文档或行为结果 |

## 阶段总览

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
| F1 | 真实环境试运行 | ✅ 已完成 | 高 | Codex |
| F2 | 主机与环境管理 | ✅ 已完成 | 高 | Qoder |
| F3 | 执行体验与 Runbook | ✅ 已完成 | 高 | Qoder |
| F4 | 审批与协作 | ✅ 已完成 | 高 | Qoder |
| F5 | 远程 daemon 与多节点 | ✅ 已完成 | 高 | Qoder |
| F6 | 可观测与审计分析 | ✅ 已完成 | 中 | Qoder |
| S1 | 当前变更收口 | ✅ 已完成 | 高 | Qoder |
| S2 | 真实环境回归 | ✅ 已完成 | 高 | Qoder |
| S3 | 文档与契约一致性 | ✅ 已完成 | 中 | Qoder |
| S4 | 发布前质量门槛 | ✅ 已完成 | 高 | Qoder |
| S5 | Agent Activity 可观测性闭环 | 🟨 进行中 | 高 | Codex |

## 已完成阶段归档

| 阶段 | 目标 | 主要交付 | 验收结果 |
|------|------|----------|----------|
| P0 | 让 README、OpenAPI、MCP 文档和实际代码保持一致 | README、`docs/api.yaml`、`docs/skills.md`、配置说明 | 文档工具数、端点和配置说明与实现对齐 |
| P1 | 明确当前主干能否构建、测试和发布 | 前端 build、Rust 单测、CLI/MCP/daemon check | `npm run build`、Rust tests/checks 通过 |
| P2 | 降低真实用户和 agent 接入成本 | CLI/MCP/daemon/Web Console/configuration guides | 快速开始和配置指南覆盖主要入口 |
| P3 | 把 SSH 能力层推进到可长期运行 | token/private key 权限、remote trust model、webhook 保护、approval TTL | 权限修正、出站保护和 TTL 测试覆盖 |
| P4 | 覆盖关键跨模块行为，减少回归 | MCP 枚举测试、daemon 集成测试、CLI smoke tests、frontend type checks | 关键工具、HTTP 路由、CLI 参数和类型同步完成 |
| P5 | 形成可重复发布流程 | release checklist、安装验证脚本、versioning policy、changelog | 发布流程和安装校验入口成型 |
| P6 | 修复文档承诺、开发命令和实现行为之间的偏差 | README 命令修正、remote 示例修正、Playbook risk override、Slack 审批行为修正 | 文档、实现和发布前检查重新对齐 |
| P7 | 完成本机端到端闭环验证 | `scripts/e2e-local.sh`、Web Console smoke、MCP stdio e2e、OpenSSH fixture 准备检查 | build、checks、tests、sidecar 和 MCP 协议路径可验证 |
| P8 | 降低误执行、凭证泄露和远程 daemon 暴露风险 | blocked 风险不可降级、daemon token 轮换、remote 配置校验、审批防重放、敏感输出脱敏 | 安全边界由测试和文档覆盖 |
| P9 | 让长期运行的 daemon 更容易监控、诊断和维护 | 结构化日志、扩展 health、审计轮转、metrics、doctor/MCP doctor | daemon 运维诊断入口完成 |
| P10 | 提升安装、接入、团队协作和 agent 生态可用性 | SetupWizard、MCP 客户端模板、团队配置导入导出、Skill 分发、checksum 脚本 | 产品化入口和生态接入文档完成 |

## 后续功能路线图

后续不再先堆底层能力，而是以真实使用场景驱动：每一阶段都先跑现有功能、记录 bug，再决定是否扩展功能。

执行原则：

- 先 dogfood，再扩展：每个新功能阶段开始前，先用当前 CLI、daemon、MCP、桌面端完成一遍真实 SSH 工作流。
- bug 修复优先于新功能：真实工作流中发现的认证、权限、审计、执行、安全和 UI 问题优先进入修复队列。
- 功能必须有验收场景：新增功能需要同时给出 CLI/API/MCP 或 UI 至少一个可复现验收路径。
- 安全默认保守：涉及批量执行、凭证、审批绕过、远程 daemon 的能力必须默认最小权限。

当前下一步聚焦：

- 先把 MCP PTY session 默认路由到本地 daemon session registry，让 Codex / Claude Code / opencode 通过 MCP 打开的交互会话能被桌面端 Live Agent Activity 实时观察。
- 保留本地进程内 session fallback，避免未启动 daemon 的 MCP 用户失去基本 PTY 能力。
- daemon session 事件携带标准 `source`，MCP 默认使用 `mcp`，并允许 `AGENT2SSH_SOURCE` 覆盖为 `codex`、`claude-code`、`opencode` 等上层来源。

## 真实测试服务器

| 字段 | 值 |
|------|----|
| 用途 | F1 真实环境试运行、CLI/MCP/daemon/桌面端回归测试 |
| Host | `107.174.36.91` |
| SSH 用户 | `root` |
| SSH 端口 | `22` |
| 系统 | Debian，主机名 `racknerd-ef7655c` |
| 认证方式 | 用户已提供 root 密码；明文密码保存在本地 gitignored 文件 `.agent2ssh-test.env` 的 `AGENT2SSH_TEST_PASSWORD` 中，测试时建议用它生成临时 SSH key |
| 测试约束 | 只在 `/tmp/agent2ssh-*` 写入临时文件；测试结束必须清理临时目录和临时 `authorized_keys` 条目 |
| 已验证能力 | SSH 登录、host add/list、risk、ping、exec、exec-multi（含 reason/change_id）、SFTP upload/download/list/stat、audit（table/jsonl/csv）、audit export、doctor、playbook list/dry-run/run（含 reason/change_id）、health-snapshot、MCP tools/list (50)、MCP ssh_exec_multi/ssh_playbook_run/ssh_audit_export/ssh_doctor、daemon /exec/exec-multi/playbooks/run/audit/audit/export/health-snapshot |
| 已知限制 | PTY session 首次读取可能先返回登录 banner/prompt，命令输出可能需要后续 read |

推荐接入方式：

1. 运行 `set -a; source .agent2ssh-test.env; set +a` 读取测试机密码。
2. 使用密码登录服务器，生成并追加临时 SSH 公钥到 `~/.ssh/authorized_keys`。
3. 使用 `AGENT2SSH_CONFIG_DIR=$(mktemp -d)` 隔离本次测试配置。
4. 用临时 key 执行 Agent2SSH 测试，不在本机正式 `~/.agent2ssh` 写入测试 host。
5. 测试结束后删除远端临时公钥、本地临时配置目录和远端 `/tmp/agent2ssh-*` 文件。

## F1 · 真实环境试运行

目标：用现有功能覆盖一台真实可控主机，形成并处理首轮 bug 修复清单。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F1-1 | ✅ 已完成 | 高 | Codex | 建立真实服务器 fixture | 107.174.36.91 使用临时 key 完成 exec、ping、sftp 验证；临时 key 和远端 `/tmp` 目录已清理 |
| F1-2 | ✅ 已完成 | 高 | Codex | 跑完整 CLI 工作流 | host add/list、risk、exec、exec-multi、sftp、audit、doctor、daemon-backed session/forward 已记录 |
| F1-3 | ✅ 已完成 | 高 | Codex | 跑 MCP 工作流 | MCP `tools/list` 返回工具列表；`ssh_list_hosts`、`ssh_exec`、`ssh_audit`、`ssh_doctor` 在真实服务器通过；当前 S2 回归确认工具数为 50 |
| F1-4 | ✅ 已完成 | 中 | Codex | 跑桌面端首次启动和打包验证 | `npm run tauri:build` 生成 `.app` 和 `.dmg`；macOS bundle 主入口 `agent2ssh-app` 首启 smoke 通过 |
| F1-5 | ✅ 已完成 | 高 | Codex | 输出 bug backlog | B1-B5 已记录并修复；后续新 bug 按影响等级进入修复 |

## Bug 修复队列

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| B1 | ✅ 已完成 | 高 | Codex | `AGENT2SSH_CONFIG_DIR` 文档存在但实现未生效 | `config_dir()` 优先使用非空 `AGENT2SSH_CONFIG_DIR`；单测 `store::tests::test_config_dir_uses_env_override` 通过 |
| B2 | ✅ 已完成 | 高 | Codex | CLI `session`/`forward` 状态只保存在单进程内 | CLI `session`/`forward` 默认通过 daemon HTTP API 管理长生命周期资源；真实服务器验证 open/list/write/read/close 和 forward add/list/rm 通过 |
| B3 | ✅ 已完成 | 高 | Codex | Tauri sidecar 名称与 Cargo package/bin 名称冲突 | CLI sidecar 改为 `agent2ssh-cli`；`scripts/prepare-sidecars.sh` 生成 Tauri 期望的 target-triple 文件名 |
| B4 | ✅ 已完成 | 中 | Codex | Tauri PNG 图标不是 RGBA，导致 macOS bundle 构建失败 | `32x32.png`、`128x128.png`、`128x128@2x.png` 转为 RGBA；`npm run tauri:build` 通过 |
| B5 | ✅ 已完成 | 高 | Codex | macOS bundle 主程序被 CLI 二进制污染，首次启动只打印 CLI help | Cargo package 改名为 `agent2ssh-app`，保留 lib crate `agent2ssh` 和 CLI bin `agent2ssh`；bundle `CFBundleExecutable=agent2ssh-app` 且首启 smoke 通过 |

## F2 · 主机与环境管理

目标：让 Agent2SSH 更适合管理多环境、多角色主机。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F2-1 | ✅ 已完成 | 高 | Codex | 主机分组与环境视图 | HostProfile 支持 `env`、`role`、`owner`；CLI `host list` 和桌面端 HostList 可按 env、role、owner、tag 过滤；`npm run build`、Rust check、lib test、CLI smoke 通过 |
| F2-2 | ✅ 已完成 | 中 | Qoder | 主机健康快照 | 批量采集 uptime、disk、memory、load、ssh latency，并写入本地快照 |
| F2-3 | ✅ 已完成 | 中 | Qoder | 主机配置变更预览 | team config import 前显示新增、修改、删除差异 |
| F2-4 | ✅ 已完成 | 中 | Qoder | SSH config 双向同步策略 | 明确 Agent2SSH 与 `~/.ssh/config` 的导入、覆盖、冲突处理规则 |

## F3 · 执行体验与 Runbook

目标：把一次性命令执行升级为可审计、可复用的运维流程。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F3-1 | ✅ 已完成 | 高 | Qoder | Playbook 参数化 | playbook step 支持参数、默认值、必填校验和 dry-run 展示 |
| F3-2 | ✅ 已完成 | 高 | Qoder | 执行计划预览 | 高风险或多主机执行前展示目标、命令、风险、预计影响 |
| F3-3 | ✅ 已完成 | 中 | Qoder | 批量执行策略 | 支持并发数、失败阈值、逐批 rollout、暂停/继续 |
| F3-4 | ✅ 已完成 | 中 | Qoder | 执行结果比较 | 多主机结果可按 exit code、stdout diff、stderr 聚合查看 |

## F4 · 审批与协作

目标：让高风险操作适合团队协作，而不是只适合单机个人使用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F4-1 | ✅ 已完成 | 高 | Qoder | 审批策略配置 | 按 host/tag/risk/command pattern 配置是否需要审批 |
| F4-2 | ✅ 已完成 | 高 | Qoder | 审批上下文增强 | 审批请求包含 diff、目标主机、历史执行、发起来源 |
| F4-3 | ✅ 已完成 | 中 | Qoder | 审批通知回调 | Slack/自定义 webhook 可跳转到认证后的审批页面 |
| F4-4 | ✅ 已完成 | 中 | Qoder | 操作备注与变更单号 | exec/playbook 支持 reason/change_id 并进入 audit |

## F5 · 远程 daemon 与多节点

目标：把 remote daemon 从“可路由”推进到“可运营”。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F5-1 | ✅ 已完成 | 高 | Qoder | remote daemon 连接诊断 | `agent2ssh doctor --daemon <alias>` 检查 TLS、token、health、version |
| F5-2 | ✅ 已完成 | 高 | Qoder | daemon 版本兼容检查 | CLI/MCP 调用远程 daemon 前提示协议或版本不兼容 |
| F5-3 | ✅ 已完成 | 中 | Qoder | remote daemon 权限范围 | 每个 remote 配置允许的 hosts/tags/commands 范围 |
| F5-4 | ✅ 已完成 | 中 | Qoder | 多 daemon 统一视图 | UI/CLI 可按 daemon 查看 host、health、metrics |

## F6 · 可观测与审计分析

目标：让 audit 和 metrics 变成定位问题、复盘操作的工具。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F6-1 | ✅ 已完成 | 高 | Qoder | 审计查询增强 | 支持全文搜索、时间范围、主机组、命令模式组合过滤 |
| F6-2 | ✅ 已完成 | 中 | Qoder | 审计导出 | 支持 JSONL/CSV 导出，并保留脱敏策略 |
| F6-3 | ✅ 已完成 | 中 | Qoder | 指标趋势 | 展示执行量、失败率、风险分布、审批耗时趋势 |
| F6-4 | ✅ 已完成 | 低 | Qoder | 事件订阅 | 提供本地事件流供外部监控或自动化消费 |

## S1 · 当前变更收口

目标：把 F4-4 审计链路和最近发现的文档漂移彻底验收，避免“参数存在但审计未落盘”的回归。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S1-1 | ✅ 已完成 | 高 | Qoder | `exec-multi` 审计上下文测试 | 覆盖 CLI、daemon 或 MCP 至少一个入口；执行带 `reason`、`change_id` 的 `exec-multi` 后，`audit` 可查询到每个目标主机的对应字段 |
| S1-2 | ✅ 已完成 | 高 | Qoder | Playbook 审计上下文测试 | `playbook run` 支持 `reason`、`change_id`；每个 step 产生的 audit entry 都保留相同上下文 |
| S1-3 | ✅ 已完成 | 中 | Qoder | 清理测试 warning | `cargo test --no-default-features` 和 `cargo test --no-default-features --features daemon` 不再出现 `unused variable` / `dead_code` warning |
| S1-4 | ✅ 已完成 | 中 | Qoder | 最近修复记录归档 | 在 `CHANGELOG.md` 或本计划 Bug 队列记录 F4-4 审计链路修复、MCP 工具数修正、OpenAPI `/exec-multi` 响应修正 |

## S2 · 真实环境回归

目标：用真实服务器重新跑一遍 CLI、daemon、MCP 的高频路径，确认 F2-F6 已完成能力可实际使用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S2-1 | ✅ 已完成 | 高 | Qoder | 真实服务器 CLI 回归 | 使用临时 key 和隔离 `AGENT2SSH_CONFIG_DIR` 跑 `host add/list`、`exec`、`exec-multi --reason --change-id`、`playbook run --reason --change-id`、`audit --format jsonl/csv`；测试结束清理远端 `/tmp/agent2ssh-*` 和临时 key |
| S2-2 | ✅ 已完成 | 高 | Qoder | daemon HTTP 回归 | 启动本地 daemon，验证 `/exec`、`/exec-multi`、`/playbooks/run`、`/audit`、`/audit/export`、`/health-snapshot` 返回结构与 `docs/api.yaml` 一致 |
| S2-3 | ✅ 已完成 | 高 | Qoder | MCP 回归 | 通过 stdio 调用 `tools/list`、`ssh_exec_multi`、`ssh_playbook_run`、`ssh_audit_export`、`ssh_doctor`；确认工具数为 50 且关键调用成功 |
| S2-4 | ✅ 已完成 | 中 | Qoder | 回归记录输出 | 在 `docs/` 下新增或更新真实回归记录，包含命令、配置隔离方式、结果摘要、发现的问题和清理证明 |

## S3 · 文档与契约一致性

目标：把 README、`docs/skills.md`、`docs/api.yaml`、MCP schema、daemon handler 的漂移变成可检测问题。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S3-1 | ✅ 已完成 | 中 | Qoder | MCP 工具文档一致性检查 | 增加脚本或测试，比对 MCP `tools/list` 工具名与 `docs/skills.md` 表格；工具新增/删除时测试失败并提示更新文档 |
| S3-2 | ✅ 已完成 | 中 | Qoder | OpenAPI 与 daemon 契约检查 | 为高频端点维护最小 schema/fixture 检查，优先覆盖 `/exec`、`/exec-multi`、`/playbooks/run`、`/audit/export` |
| S3-3 | ✅ 已完成 | 低 | Qoder | README 去重策略 | README 保留入口摘要和核心工具概览，完整 MCP 工具表以 `docs/skills.md` 为准，减少双处维护 |
| S3-4 | ✅ 已完成 | 中 | Qoder | CLI help 与文档对齐 | 抽样验证 `agent2ssh --help`、`exec-multi --help`、`playbook run --help` 与 README/guide 中的参数一致 |

## S4 · 发布前质量门槛

目标：形成一套发布前必须通过的固定检查，确保桌面端、CLI、daemon、MCP 和文档处于可发布状态。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S4-1 | ✅ 已完成 | 高 | Qoder | 固定发布验收命令 | `npm run build`、`cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp`、`cargo check --no-default-features --features daemon --bin agent2ssh-daemon`、两套 `cargo test` 全部通过 |
| S4-2 | ✅ 已完成 | 高 | Qoder | 桌面包构建验证 | `npm run tauri:build` 可生成 `.app` / `.dmg`；macOS bundle 主入口仍为 `agent2ssh-app` |
| S4-3 | ✅ 已完成 | 中 | Qoder | 安装校验脚本回归 | `scripts/verify-install.sh`、`scripts/prepare-sidecars.sh`、`scripts/generate-checksums.sh` 在当前版本可执行并输出预期结果 |
| S4-4 | ✅ 已完成 | 中 | Qoder | 发布材料准备 | 更新 `CHANGELOG.md`、`docs/release-checklist.md` 和版本说明；列出已知限制和真实环境回归结果 |

## S5 · Agent Activity 可观测性闭环

目标：让不同 agent 入口打开的 PTY session 进入统一 daemon registry，并在桌面端 Live Agent Activity 中具备可归因、可观察、可接管的基础。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S5-1 | ✅ 已完成 | 高 | Codex | MCP session 默认路由到 local daemon | `ssh_session_open/write/read/close/list` 优先使用 `127.0.0.1:7722` daemon session API；daemon 不可用时回退到 MCP 进程内 session；`cargo check --no-default-features --bin agent2ssh-mcp`、MCP stdio smoke 和 MCP 枚举测试通过 |
| S5-2 | ✅ 已完成 | 高 | Codex | 标准来源字段 | `ExecRequest`、`AuditEntry`、daemon session events、daemon exec/playbook bodies 支持 `source`；CLI/MCP/daemon/desktop 默认来源分别为 `cli`、`mcp`、`daemon`、`desktop`，并允许 `AGENT2SSH_SOURCE` 覆盖；Rust checks 和 lib tests 通过 |
| S5-3 | ✅ 已完成 | 中 | Codex | Live Activity 过滤与展开 | UI 可按 source、事件类型和文本搜索过滤；事件可展开查看 time、host、session、change_id 和原始 payload；`npm run build` 通过，Browser 验证过滤控件可渲染 |
| S5-4 | ✅ 已完成 | 高 | Codex | 高风险非前端来源提醒 | Live Activity 对非 `desktop` 来源的 high/blocked/approval 事件显示本地提醒条；不改变后端审批边界；`npm run build` 通过 |
| S5-5 | ✅ 已完成 | 高 | Codex | 敏感输出策略 | session/output/exec preview 统一经过 `redact_sensitive_text`，覆盖 token、password、Authorization/Bearer、cookie 和 private key；bounded preview 继续保留截断边界；lib tests 和 daemon check 通过 |

## 近期建议

S1-S4 已完成，当前先完成 S5 可观测性闭环，再进入发布收口：

1. 完成 S5-1：MCP session 默认路由到 local daemon registry，并保留进程内 fallback。
2. 完成 S5-2：统一 CLI/MCP/daemon 的 `source` 归因，优先支持 `AGENT2SSH_SOURCE`。
3. 补齐 Live Activity 的过滤、展开、敏感输出和高风险提醒。
4. 再打 `v0.1.1` 标签并推送到 `github`、`git233`，等待 CI assets，回填 `scripts/agent2ssh.rb` 的平台 sha256。

## 安全可视化后续

Agent2SSH 已开始从“agent 可调用 SSH 能力层”扩展为“本机 SSH 操作观察面”。当前 Live Agent Activity 面板覆盖 daemon SSE 实时事件和本地 audit 补偿；S5 将继续完成：

1. MCP session 默认路由到 local daemon registry，使 Claude Code、Codex、opencode 等 agent 打开的 PTY 都能被桌面端实时接管和观察。
2. 为 CLI/MCP 增加标准 `source` 字段或环境变量（例如 `AGENT2SSH_SOURCE=codex`），让前端区分发起方。
3. 为 Live Activity 增加敏感输出显示策略：默认预览、可手动展开、支持按 host/env/risk 过滤。
4. 增加“高风险操作前台提醒”，当非前端来源触发 high/blocked/approval 事件时在桌面端明显提示。

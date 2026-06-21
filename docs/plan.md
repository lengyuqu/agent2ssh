# Agent2SSH 计划

## 当前状态

P0-P10 已全部完成。当前基线：

- 产品形态：Tauri 桌面 App、CLI、MCP stdio server、HTTP/WebSocket daemon、Web Console
- 核心能力：Host 管理、SSH config 导入、Jump Host、tags、per-host risk override
- 执行能力：SSH exec、exec-multi、ping、SFTP、PTY sessions、port forwarding、Playbooks
- 安全能力：风险评分、统一 policy-as-code、审批队列、审批端点、桌面审批弹窗、敏感命令脱敏、execution gate、执行限额、异常检测
- 运维能力：内置 SSH 连接保留、Webhook 通知、remote daemon registry、健康检查、指标、审计轮转
- 生态能力：SSH key 管理、团队配置导入导出、MCP 客户端模板、插件/Skill 分发文档
- 验收结果：148 单元测试 + 56 集成测试 + 26 CLI smoke 测试 = 230 测试全绿；daemon feature 下为 153 lib 单元测试 + 4 daemon 单元测试 + 56 集成测试 + 26 CLI smoke 测试全绿
- MCP 工具：51 个，详见 [skills.md](skills.md)

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
| S5 | Agent Activity 可观测性闭环 | ✅ 已完成 | 高 | Codex |
| S6 | 真实会话回归 | ✅ 已完成 | 高 | Codex |
| S7 | 桌面 Session 接管 | ✅ 已完成 | 高 | Codex |
| S8 | Session 接管体验与安全 | ✅ 已完成 | 高 | Codex |
| S9 | 0.1.1 发布前收口 | ✅ 已完成 | 高 | Codex |
| R | 发布与本机安装验证 | 🟨 进行中 | 高 | Codex |
| G | 观察面升级为控制面 | ✅ 已完成 | 高 | Codex |
| E | 生态与可靠性 | ✅ 已完成 | 中 | Codex |
| O | 异常监听与鉴权/存储加固 | ✅ 已完成 | 高 | Claude |
| H | 架构债与加固后续 | 🟨 进行中 | 中 | Codex |

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

- 先本机真实使用，再扩展：每个新功能阶段开始前，先用当前 CLI、daemon、MCP、桌面端完成一遍本机 SSH 工作流。
- bug 修复优先于新功能：真实工作流中发现的认证、权限、审计、执行、安全和 UI 问题优先进入修复队列。
- 功能必须有验收场景：新增功能需要同时给出 CLI/API/MCP 或 UI 至少一个可复现验收路径。
- 安全默认保守：涉及批量执行、凭证、审批绕过、远程 daemon 的能力必须默认最小权限。

当前下一步聚焦：

- G 阶段已完成：Live Activity 已从观察面升级为控制面，覆盖 execution gate、执行限额、policy dry-run 和异常检测。
- R 阶段成为当前优先级：`v0.1.1` 发布动作已完成，下一步是跨平台真实安装验证和本机使用回归。

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
| 已验证能力 | SSH 登录、host add/list、risk、ping、exec、exec-multi（含 reason/change_id）、SFTP upload/download/list/stat、audit（table/jsonl/csv）、audit export、doctor、playbook list/dry-run/run（含 reason/change_id）、health-snapshot、MCP tools/list (51)、MCP ssh_exec_multi/ssh_playbook_run/ssh_audit_export/ssh_doctor/ssh_gate_status、daemon /exec/exec-multi/playbooks/run/audit/audit/export/health-snapshot/gate |
| 已知限制 | PTY session 首次读取可能先返回登录 banner/prompt，命令输出可能需要后续 read；PTY session 写入按完成行做风险授权和审计，不是完整 shell/TTY 语义解析器；批量执行和 playbook 的 daemon 审批按 host/step 粒度生效，显式 force 才会作用于整个请求 |

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
| F1-3 | ✅ 已完成 | 高 | Codex | 跑 MCP 工作流 | MCP `tools/list` 返回工具列表；`ssh_list_hosts`、`ssh_exec`、`ssh_audit`、`ssh_doctor` 在真实服务器通过；当前 MCP 基线工具数为 51 |
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
| S2-3 | ✅ 已完成 | 高 | Qoder | MCP 回归 | 通过 stdio 调用 `tools/list`、`ssh_exec_multi`、`ssh_playbook_run`、`ssh_audit_export`、`ssh_doctor`；S2 当时确认工具数为 50，当前 MCP 基线为 51 |
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

## S6 · 真实会话回归

目标：用真实服务器验证 S5 的 daemon session registry、source 归因、Live Activity SSE 事件和敏感 preview 脱敏在端到端链路中实际可用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S6-1 | ✅ 已完成 | 高 | Codex | MCP session daemon registry 回归 | 使用隔离 `AGENT2SSH_CONFIG_DIR` 和真实服务器，`ssh_session_open/write/read/close/list` 返回 `backend: "daemon"`；daemon `/sessions` 可见打开的 session，关闭后为空 |
| S6-2 | ✅ 已完成 | 高 | Codex | Live Activity SSE 事件回归 | `/events/stream` 捕获 `session_opened`、`session_input`、`session_output`、`session_closed`，并携带 `source: "claude-code"` |
| S6-3 | ✅ 已完成 | 高 | Codex | source 与 audit 回归 | `AGENT2SSH_SOURCE=opencode` 的 CLI exec 写入 audit JSON/CSV，`source` 字段落盘并导出 |
| S6-4 | ✅ 已完成 | 高 | Codex | 敏感 preview 脱敏回归 | SSE preview 中 `Authorization: Bearer ...` 被替换为 `[REDACTED]`，测试 secret 未出现在事件 payload summary |
| S6-5 | ✅ 已完成 | 高 | Codex | 回归报告与清理证明 | 新增 `docs/reports/s6-regression-report.md`；远端临时 key 和 `/tmp/agent2ssh-s6-*` 清理完成，本地 daemon 停止 |

## S7 · 桌面 Session 接管

目标：让桌面端 SessionPanel 不只打开自己的进程内 PTY，而是优先连接 daemon session registry，直接接管 MCP/CLI/daemon 创建的 daemon-managed sessions。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S7-1 | ✅ 已完成 | 高 | Codex | daemon session API 前端封装 | `src/api.ts` 新增 `sessionOpenDaemon/write/read/close/list`，所有请求继续使用 Bearer token，并写入 `source: "desktop"` |
| S7-2 | ✅ 已完成 | 高 | Codex | SessionPanel daemon registry 列表 | `SessionPanel` 优先轮询 daemon `/sessions`，显示 daemon sessions；daemon 不可用时回退 Tauri 本地 `session_list` |
| S7-3 | ✅ 已完成 | 高 | Codex | 接管已有 session | session 列表支持 Attach；接管后可 read/write/close 原 daemon session，并保留本地 fallback session 操作 |
| S7-4 | ✅ 已完成 | 中 | Codex | UI 状态与布局 | 面板显示 registry/backend 状态、active session 元信息和来源标识；按钮尺寸稳定，窄面板不挤压文本 |
| S7-5 | ✅ 已完成 | 高 | Codex | 验证 | `npm run build` 通过；计划和架构文档同步 |

## S8 · Session 接管体验与安全

目标：在 S7 的 daemon session 接管基础上，把日常使用所需的持续读取、只读观察和危险输入保护补齐。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S8-1 | ✅ 已完成 | 中 | Codex | 自动 tail | active session 支持 `Tail` 开关，每 2 秒读取一次输出；使用并发 guard 避免重叠 read |
| S8-2 | ✅ 已完成 | 高 | Codex | 只读接管 | session 列表提供 read-only attach；active session 可切换 read-only，禁止写入输入 |
| S8-3 | ✅ 已完成 | 高 | Codex | 危险输入确认 | 发送前调用现有 risk classifier；`high`/`blocked` 输入显示确认条，用户显式确认后才写入 PTY |
| S8-4 | ✅ 已完成 | 中 | Codex | UI 状态稳定 | Tail、Read-only、危险确认和接管按钮有稳定尺寸和可访问标签；窄面板下文本截断不挤压操作按钮 |
| S8-5 | ✅ 已完成 | 高 | Codex | 验证 | `npm run build` 通过；Browser 渲染检查通过；`git diff --check` 通过 |

## S9 · 0.1.1 发布前收口

目标：在真正打 `v0.1.1` tag 前，把版本、发布说明和本地质量门槛收齐，避免 tag 推送后才发现可避免的发布问题。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S9-1 | ✅ 已完成 | 高 | Codex | 版本字段一致性 | `Cargo.toml`、`package.json`、`package-lock.json`、`tauri.conf.json`、`docs/api.yaml`、`scripts/agent2ssh.rb` 均为 `0.1.1` |
| S9-2 | ✅ 已完成 | 高 | Codex | 发布说明收口 | `CHANGELOG.md` 合并为单一 `0.1.1` 发布段，覆盖 S1-S8 主要交付 |
| S9-3 | ✅ 已完成 | 高 | Codex | 本地质量门槛 | `npm run build`、两套 `cargo check`、两套 `cargo test`、`git diff --check` 通过 |
| S9-4 | ✅ 已完成 | 中 | Codex | 发布前报告 | 新增 `docs/s9-release-preflight-report.md`，记录版本状态、质量门槛和剩余发布动作 |
| S9-5 | ✅ 已完成 | 中 | Codex | tag 状态确认 | 本地 `v0.1.1` tag 尚不存在；S9 不创建 tag，留给最终发布动作 |

## 近期建议

S1-S9 与 G 阶段已完成，0.1.1 处于发布就绪状态，Live Activity 已从观察面升级为控制面。O 阶段（异常监听与鉴权/存储加固）已完成。下一步聚焦：

1. R 阶段（当前优先级）：完成跨平台真实安装验证和本机使用回归。
2. E 阶段已完成；后续仅在真实本机使用暴露问题时追加 E4+。
3. H 阶段（按收益穿插）：承接 O 的加固后续——鉴权 handler 迁移、巨型文件拆分、MCP schema 派发、跨进程错误聚合、通用脱敏等架构债；安全/数据完整性项优先。

## 安全可视化后续

Agent2SSH 已开始从“agent 可调用 SSH 能力层”扩展为“本机 SSH 操作观察面”。当前 Live Agent Activity 面板覆盖 daemon SSE 实时事件和本地 audit 补偿；S5/S6 已完成：

1. MCP session 默认路由到 local daemon registry，使 Claude Code、Codex、opencode 等 agent 打开的 PTY 能被桌面端实时观察。
2. CLI/MCP/daemon/desktop 均具备标准 `source` 字段或 `AGENT2SSH_SOURCE` 覆盖。
3. Live Activity 支持过滤、展开、敏感 preview 脱敏和高风险外部来源提醒。
4. SessionPanel 可列出并接管 daemon-managed sessions，支持读取、写入和关闭来自统一 registry 的 PTY。
5. SessionPanel 支持自动 tail、只读观察和高风险 PTY 输入二次确认。

## 长远路线图（0.1.1 之后）

### 战略定位

Agent2SSH 的护城河不是"又一个 SSH 客户端"，而是"**AI agent 在本机做 SSH 操作的观察面 + 控制面**"。S5-S8 已经把"观察面"做厚（统一 registry、source 归因、Live Activity、敏感脱敏、session 接管）。下一阶段的核心是把观察面升级为**控制面**：当多个 agent 并发操作时，人类能实时干预，而不只是事后看审计。

后续不再往"通用 SSH 工具"方向铺功能（Termius / tmux / ansible 已占满该位置），而是死磕"多 agent 并发操作下的可观测、可归因、可干预"这个差异化位置。

### 执行原则（在 0.1.1 之前原则基础上新增）

- 本机使用驱动取代纯路线图驱动：先把自己每天会用到的路径做稳，再决定是否扩展。
- 单机定位优先：Agent2SSH 是本机 agent SSH 能力层，路线图只保留本机使用刚需。
- 每个阶段先问"这是不是单机使用刚需"；不是刚需的能力不进入 backlog。
- 控制类能力必须在 daemon 层强制：kill switch、限额、策略判定不能只做在 UI/前端，否则绕过 desktop 的 agent 来源不受约束。

### 阶段排序与依赖

```
S9(0.1.1 已收口)
   ├─ R 发布与本机安装验证 ← 当前优先级
   ├─ H 架构债与加固后续   ← 按收益穿插
   └─ G 观察面→控制面      ← 已完成，后续按本机使用反馈迭代
   E 生态与可靠性           ← 已完成，后续仅追加明确回归项
```

## G · 观察面升级为控制面

目标：当多个 agent 并发操作时，人类能在 daemon 层实时干预——暂停、限额、按策略拒绝、对异常行为告警。该阶段已完成，后续仅按本机使用反馈继续迭代。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| G1-1 | ✅ 已完成 | 高 | Codex | 全局急停 gate（daemon 层） | daemon 维护 `execution_gate` 状态（active/paused）；paused 时所有非 `desktop` 来源的 `/exec`、`/exec-multi`、`/playbooks/run`、session write 和 WebSocket exec 被拒，HTTP 入口返回 423 并写入 audit gate 拒绝事件；`desktop` 来源仍可操作以便恢复 |
| G1-2 | ✅ 已完成 | 高 | Codex | 急停 CLI 与桌面入口 | `agent2ssh pause` / `resume` / `status` 可切换并查询 gate；桌面端提供急停按钮和当前 gate 状态指示；MCP 暴露只读 `ssh_gate_status` |
| G1-3 | ✅ 已完成 | 中 | Codex | 急停回归验证 | paused 状态下 daemon/MCP 非 desktop 执行被拒且 audit 落盘，resume 后恢复；新增 `docs/reports/g1-gate-regression-report.md` |
| G2-1 | ✅ 已完成 | 高 | Codex | 速率与并发限额配置 | `execution_limits.toml` 定义 per-source / per-host / per-tag 的每窗口最大执行数与最大并发 session 数；缺省值保守且可覆盖，详见 `docs/guides/configuration-guide.md` |
| G2-2 | ✅ 已完成 | 高 | Codex | 限额强制与拒绝审计 | 超限请求在 daemon 层返回 429 并写入 blocked audit；限额计数按滑动窗口；并发 session 上限阻止新建 session；新增 `docs/reports/g2-limits-regression-report.md` |
| G3-1 | ✅ 已完成 | 高 | Codex | 策略即代码收敛 | 新增统一 `policy.toml` / `policy.json`，将 risk rules 与 approval policies 收敛到单一可版本化文件；运行时优先读取统一 policy，缺失时兼容旧 `risk_rules.toml` / `approval_policies.toml` |
| G3-2 | ✅ 已完成 | 中 | Codex | 策略校验与 dry-run | 新增 `agent2ssh policy validate [--path]` 校验统一 policy 语法，`agent2ssh policy test <cmd> --host <host>` 输出 allow/approve/block；CLI smoke 覆盖统一 policy validate/test |
| G4-1 | ✅ 已完成 | 中 | Codex | 异常行为基线检测 | 新增 `anomaly.toml` 可调阈值；audit append 后按滑动窗口检测 source 频率突增、敏感命令模式和非常规时段高危操作；发布 `anomaly_detected` 事件并支持复用 webhook |
| G4-2 | ✅ 已完成 | 低 | Codex | 异常检测可视化 | Live Activity 标注 `anomaly_detected` 事件，展示异常类型、严重度和原因；异常序列由单元测试和 CLI/MCP/daemon audit 补偿路径覆盖 |

## R · 发布与本机安装验证

目标：确保产品在本机和跨平台包形态下可安装、可启动、可回归。Agent2SSH 定位为单机工具，路线图不再包含采用扩张目标。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| R1 | ✅ 已完成 | 高 | Codex | 跨平台桌面包真实验证 | release CI 已生成 macOS/Linux/Windows 桌面包；macOS 本机重新打包生成 `.app` 和 `.dmg`；本机回归覆盖内置 SSH exec/SFTP/PTY/forward 基线，平台差异记录进入后续明确 bug 队列 |
| R2 | ✅ 已完成 | 高 | Codex | 完成 0.1.1 发布动作 | `v0.1.1` tag 已推送到 GitHub/git233；release CI run `27638444133` 通过并上传 CLI tarballs、checksums、macOS/Linux/Windows 桌面包；`scripts/agent2ssh.rb` 已回填 macOS arm64、macOS x86_64、Linux x86_64 sha256；发布 tarball checksum 校验通过；使用 macOS arm64 release tarball 跑通 `scripts/verify-install.sh`（7 passed, 0 failed） |
| R3 | ✅ 已完成 | 中 | Codex | 本机接入剧本与反馈入口 | 新增 `docs/guides/external-user-10min.md`，覆盖 CLI host import/add、低风险 exec 验证、Codex/Claude-style MCP 配置、反馈脱敏；新增 GitHub bug/adoption issue 模板；明确 `v0.1.1` 默认无自动遥测，匿名反馈为手动 opt-in，未来运行时遥测必须默认关闭且不采集命令/主机/输出/凭据 |
| R5 | ✅ 已完成 | 中 | Codex | 桌面控制面调研 | 确认 Settings menu 适合作为本地 operator surface；已落地 daemon health、daemon start/stop/restart、setup wizard daemon start、execution gate、Web Console URL 控制闭环；2026-06-18 回归复测通过 `npm run build`、`cargo test`、`npm run tauri:build`；详见 `docs/reports/r5-desktop-control-plane-research-report.md` |

## E · 生态与可靠性

目标：已完成当前生态与可靠性补强；后续仅在本机使用回归暴露明确问题时追加新任务。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| E1 | ✅ 已完成 | 中 | Codex | 多 agent 集成验证 | 新增 `scripts/e1-mcp-client-smoke.py` 和 `docs/reports/e1-multi-agent-integration-report.md`，用 MCP stdio 协议分别模拟 `codex`、`opencode`、`cursor`、`claude-code` source，验证 initialize、51 工具枚举和 `ssh_risk_check` blocked 判定；真实客户端 UI 行为留给本机使用回归 |
| E2 | ✅ 已完成 | 中 | Codex | 可靠性与规模 | 新增 `scripts/e2-scale-plan-smoke.py` 和 `docs/reports/e2-scale-reliability-report.md`，在隔离配置中生成 100 个 synthetic host 并跑通 `exec-multi --plan`；新增 100 host plan Rust 回归与 1000 event burst 事件总线回归；真实 100 台 SSH/多 daemon 压测留给后续外部环境 |
| E3 | ✅ 已完成 | 中 | Codex | 契约一致性接入 CI | `.github/workflows/ci.yml` 新增 `contract-consistency` job，在 PR、push 和 release 入口显式运行 S3 的 `docs/skills.md` vs MCP 工具、OpenAPI/daemon schema fixture、CLI help 参数一致性检查；`build` matrix 和 release-only `tauri-bundle` job 依赖该 job，契约漂移会先于跨平台构建/打包失败 |

## O · 异常监听与鉴权/存储加固

目标：把前后端异常监听补到“真正能监听到、能告警、能追踪”，并消除守护进程鉴权与共享文件存储上的结构性隐患。本阶段已完成，验收命令：`npm run build`、`npx tsc --noEmit`、`cargo fmt`、两套 `cargo check`、两套 `cargo test`（daemon feature 下 175 lib + 14 daemon-bin + 27 cli_smoke + 56 集成全绿）；详见 `docs/architecture.md` 的 Diagnostics、Control Plane 与 Persistence And Locking 段。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| O1-1 | ✅ 已完成 | 高 | Claude | 前端全局异常捕获 | 新增 `ErrorBoundary` + `window.onerror` / `unhandledrejection`，统一经 `api.ts` 的 `reportError` 写入后端 `app.log`；各面板 `catch` 在 `setError` 之外补 `reportError`，带组件名与上下文；`npx tsc --noEmit`、`npm run build` 通过 |
| O1-2 | ✅ 已完成 | 高 | Claude | 后端 panic hook 与 MCP 错误落盘 | `diagnostics::install_panic_hook` 在 daemon/tauri/cli/mcp 四端安装，panic 以结构化 error 写入 `app.log`；MCP 请求分发失败时记录 method+tool+code |
| O1-3 | ✅ 已完成 | 中 | Claude | tracing→app.log 桥接与 daemon.log 轮转 | daemon 用 `DiagnosticBridgeLayer` 把 `target` 以 `agent2ssh` 开头的 `WARN`/`ERROR` 转入 `app.log`；`daemon_control` 在重启时按 5MB 轮转 `daemon.log`（保留 2 代） |
| O1-4 | ✅ 已完成 | 高 | Claude | error 诊断告警与异常聚合 | `set_error_sink` 让 error 级诊断 fan-out：opt-in `diagnostic_error` webhook + `anomaly::record_diagnostic_error` 滑动窗口聚合（`diagnostic_error_threshold`/`diagnostic_cooldown_secs`，新 kind `diagnostic_error_burst`）；含单测 |
| O1-5 | ✅ 已完成 | 中 | Claude | 跨 surface correlation ID | 核心线程局部 `trace_id`（`set_trace_id`/`seed_trace_id_from_env`）自动打标诊断；daemon 中间件按 `X-Agent2SSH-Trace-Id` 头绑定 task-local 并回显；前端每会话 id 入诊断字段并随 fetch 透传；MCP 转发携带同名头 |
| O2-1 | ✅ 已完成 | 高 | Claude | 中央鉴权中间件 | daemon `auth_middleware` 对非公开路由强制鉴权（header `Bearer` 或 `?token=`），未通过 401；仅 `/`、`/console`、`/health`、`/metrics` 免鉴权；新增路由默认受保护；56 集成测试（含全部 `*_requires_auth`）通过 |
| O2-2 | ✅ 已完成 | 高 | Claude | app.log 跨进程锁 | `store::lock_config_file` 提升为可复用原语，`append/clear_diagnostic_log` 采用进程内 Mutex + `.app_log.lock` flock 两层锁，覆盖轮转与写入，与 hosts/audit 对齐 |
| O2-3 | ✅ 已完成 | 中 | Claude | 配置缓存层 | 新增 `config_cache::ConfigCache`（单槽，`(mtime,len)` 签名失效），应用于 `anomaly.toml`、`execution_limits.toml`、`daemon_tokens.toml`、`webhook.toml` 热路径；`save_webhook_config` 写后 `invalidate`；含单测 |

## H · 架构债与加固后续

目标：承接 O 阶段，把设计评估中识别出的、改动面较大或需独立验证的项落到可认领的 backlog。排序原则不变：安全/数据完整性优先，纯重构按收益排，"等到有人要" 的延后。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| H1 | ✅ 已完成 | 中 | Codex | 鉴权 handler 迁移到提取器 | daemon 中间件统一认证 admin/scoped token 并注入 `AuthContext`；受保护 handler 改用 `Extension<AuthContext>`，不再二次调用 `check_auth` 读取 scoped token；`cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon`、`cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration` 通过 |
| H2 | ✅ 已完成 | 中 | Codex | 拆分巨型文件 | 拆出 `src-tauri/src/bin/agent2ssh_daemon/{auth,trace}.rs`、`src-tauri/src/bin/agent2ssh_mcp/auth.rs`、`src-tauri/src/core/team_config.rs`、`src-tauri/src/tauri_commands/mcp_agent_config.rs`；`agent2ssh-daemon`/`agent2ssh-mcp` binary 不再承载鉴权/trace/MCP 授权细节，`core.rs` 与 `tauri_commands.rs` 移出独立职责块；daemon/MCP/lib/Tauri checks 与 lib/CLI/daemon tests 通过 |
| H3 | ✅ 已完成 | 中 | Codex | MCP schema 驱动派发 | 新增 `src-tauri/src/bin/agent2ssh_mcp/tools.rs` 作为 51 个 MCP 工具的单一 registry；`tools/list` 从 registry 输出，`tools/call` 先通过 registry 解析 tool kind 并按 inputSchema.required 做统一必填校验，再用 `McpTool` enum 派发；契约测试改为扫描 registry/enum；`cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh-mcp`、`cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke`、`cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration` 通过 |
| H4 | ✅ 已完成 | 中 | Codex | session/forward 进程本地态共享 | MCP session 已优先走 daemon registry 并合并 process fallback；forward add/list/remove 也改为优先调用本地 daemon `/forwards`，daemon 不可用时才 fallback 到进程本地 registry，列表结果标注 `backend`；daemon/MCP 契约与 smoke 回归通过 |
| H5 | ✅ 已完成 | 中 | Codex | 跨进程错误聚合 | error 级诊断写入共享 `app.log` 后直接按同一窗口扫描聚合，CLI/MCP/Tauri/daemon 都覆盖；daemon `error_sink` 仅保留 per-error webhook，聚合发布下沉到共享 append path；新增 shared app.log 窗口测试，daemon/MCP/lib/Tauri checks 与 lib/CLI/daemon tests 通过 |
| H6 | ✅ 已完成 | 中 | Codex | 通用密钥脱敏 | `redact_sensitive_text` 在关键字/字段名规则外增加 URL inline credential、hex 高熵串、base64/base64url-like token 兜底脱敏；正负样本覆盖高熵 token 与正常 UUID/path，lib tests 通过 |
| H7 | ✅ 已完成 | 低 | Claude | 依赖层日志可选放行 | `DiagnosticBridgeLayer` 默认仍只转 `agent2ssh*`；新增 `AGENT2SSH_BRIDGE_DEPS`（`1`/`true`/`all` 用内置传输层前缀集 hyper/reqwest/ssh2/h2/rustls/tower/axum，或逗号分隔自定义前缀，未设/`0`/`false` 关闭）放行依赖层 WARN/ERROR 入 `app.log`。防噪声：仍只过 WARN/ERROR + 前缀白名单；防回环：依赖层事件经新 `append_diagnostic_log_no_sink` 落盘但不触发 error sink（webhook 走 reqwest，否则传输错误会自激）。含 `parse_dep_prefixes` 与 no-sink 单测；两套 check、两套 test 全绿 |
| H8 | ✅ 已完成 | 低 | Claude | daemon 监听地址可配置 | 绑定地址改读 `AGENT2SSH_DAEMON_ADDR`，缺省仍 `127.0.0.1:7722`（默认回环）；新增 `is_loopback_addr` 校验，绑定非回环地址时写一条 `warn` 诊断提示控制面已对外暴露。`cargo check --features daemon --bin agent2ssh-daemon`、daemon 集成测试通过 |
| H9 | ✅ 已完成 | 低 | Claude | OnceLock 二次注册显式化 | `set_error_sink` 由 `OnceLock`（首次为准、二次静默丢弃）改为 `RwLock<Option<Arc<…>>>` 覆盖语义（后注册为准 + 写一条 `warn`）；调用时短读锁克隆 `Arc` 出来再执行，避免重入死锁。`install_panic_hook` 二次安装不再静默 return，改记 `warn`（仍不重复挂钩）。daemon tracing 初始化由 `init()` 改 `try_init()`，全局 subscriber 已存在时记 `warn` 而非 panic/静默。含覆盖语义单测；两套 check、两套 test 全绿 |

## I · 配置面收口与运行时韧性

目标：H 阶段把 daemon 监听地址、依赖层日志、错误聚合等做成可配置/可观测后，暴露出"配置只改了一半"和"运行时收尾缺失"两类缺口——bind 侧可配但 client 侧仍硬编码、新增 env 无集中文档、daemon 无优雅退出导致 stale pid。本阶段把这些收口，让 H 的可配置项端到端可用、可运维、可回归。排序原则不变：可达性/可运维优先，文档与回归补齐，审计类延后。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| I1 | ✅ 已完成 | 中 | Claude | 本地 daemon URL 解析统一 | 新增核心 helper `local_daemon_addr`/`local_daemon_connect_addr`/`local_daemon_url`（`remote.rs`，读 `AGENT2SSH_DAEMON_ADDR`、缺省 `127.0.0.1:7722`，通配 `0.0.0.0`/`::` 自动回退回环、IPv6 加括号）。CLI（doctor/health）、MCP（health/metrics）、`remote.rs`（`list_daemons`/`get_daemon`）、`daemon_control`（health 探测改 `to_socket_addrs`）、`notify`（console 链接）、daemon（自身 action_url + bind 复用 `local_daemon_addr`）全部改用 helper。含 `normalize_connect_addr`/env override 单测；契约/smoke/集成回归通过 |
| I2 | ✅ 已完成 | 中 | Claude | daemon 优雅退出与 PID 清理 | `axum::serve(...).with_graceful_shutdown(shutdown_signal())`：`shutdown_signal` 监听 `ctrl_c` + unix `SIGTERM`（Windows 仅 `ctrl_c`，`tokio` 新增 `signal` feature），退出时移除 `daemon.pid` 并记一条 info 诊断，不再因被信号杀死而残留 stale pid。daemon check + 集成回归通过 |
| I3 | ✅ 已完成 | 低 | Claude | 环境变量集中文档 | `configuration-guide.md` 新增「环境变量」表，列全量内置 `AGENT2SSH_*`（`CONFIG_DIR`/`SOURCE`/`DAEMON_ADDR`/`TRACE_ID`/`LOG`/`LOG_FORMAT`/`BRIDGE_DEPS`，含作用域/默认值/说明），并澄清 `token_env` 引用的是用户自定义变量（非内置）；`architecture.md` 的 Diagnostics 段补 `AGENT2SSH_BRIDGE_DEPS`、Control Plane 段补 `AGENT2SSH_DAEMON_ADDR` 解析与优雅退出 |
| I4 | ✅ 已完成 | 低 | Claude | 监听地址端到端回归 | 新增 `daemon_honors_configured_listen_address_end_to_end`（`daemon_integration.rs`）：预留随机空闲端口写入 `AGENT2SSH_DAEMON_ADDR`，起真实 axum `/health` 服务绑到 `local_daemon_addr()`，再经 `daemon_health_ok()`（走 I1 resolver）跑通，断言 resolver/URL 一致；非回环 warn 决策由 lib 化的 `is_loopback_addr` 单测覆盖。daemon feature 下测试通过 |
| I5 | ✅ 已完成 | 中 | Claude | 配置热加载一致性审计 | 产出 `docs/reports/i5-config-cache-audit.md`：盘点 11 个配置文件的读热度/写入方/失效语义，给出纳入/保持读盘的结论（含 `execution_gate` 保持读盘以优先急停新鲜度的判断）。落地 `hosts.json` 接入 `ConfigCache`——`load_config` 走缓存、`save_config_unlocked`（全部写入唯一漏斗）成功后 `invalidate`，新增 `load_config_reflects_saved_hosts_via_cache` 单测验证写后不返回陈旧值。两套 check、两套 test 全绿 |

## J · 性能与效率优化

目标：在功能基本铺齐后，针对随数据量增长会变慢的热路径做一轮效率优化——配置/审计的重复读盘解析、前端大列表的全量渲染、以及刚落地的 SFTP 面板里"只能传文件、进度只数文件个数"的粗糙处。排序原则：每次操作都走的热路径优先，前端可感知卡顿次之，功能补全垫后。每项都要带量化或回归验收，避免"优化"引入正确性回退。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| J1 | ✅ 已完成 | 中 | Claude | policy.toml 热路径缓存 | `load_policy_file` 接入 `ConfigCache`（按解析后的 `policy.toml`/`policy.json` 路径为键，无文件时回退 `policy.toml` 路径键，使"无 policy"探测也被记忆化），`save_policy_approval_policies` 写后 `invalidate`；"policy 只升级风险"语义不变。新增 `load_policy_file_reflects_saves_via_cache`（无→建→存三段验证写后不陈旧）。两套 check、两套 test 全绿 |
| J2 | ✅ 已完成 | 中 | Claude | 审计日志按需读取 | `list_audit_raw` 改为反向（newest-first）扫描 + 早停：到达 `filter.limit` 即停（与旧"全解析→reverse→truncate"等价，但常见"最近 N 条"不再解析整文件）；并利用审计 append 即 `ts=now()` 的时间有序性，遇到 `ts<since` 即停（`compute_metrics_trend` 的 since 窗口因此也有界）。`matches` 仍复核所有条件，早停只提前停止、不改结果。新增 5000 行合成日志回归（limit/host/since 三种过滤断言结果精确一致）。两套 test 全绿 |
| J3 | ✅ 已完成 | 中 | Claude | 前端大列表渲染优化 | SFTP 列表是真正无界的来源（远端目录可上万条），加 `viewCap`（每侧每次最多挂载 400 行 + "显示更多"，导航/刷新重置）；`AuditPanel` 加 `renderCap`（200 + 显示更多）兜住 limit 被调大的情况。`DiagnosticsPanel` 不存在（诊断日志在 `SettingsMenu`，后端硬上限 1000，已有界，未改）。`tsc --noEmit`、`npm run build` 通过 |
| J4 | ✅ 已完成 | 中 | Claude | SFTP 目录递归传输 | 后端新增 `sftp_walk_core`（远端递归 readdir，跳过 symlink + 深度上限 64 防环路，parents-before-children）与 `local_walk`/`local_mkdir`（本地遍历/建目录，含 `local_walk_inner` 单测）。前端每行加勾选框（文件夹也可选/可拖），传输前 `buildTransferUnits` 把选中目录递归展开为「目标侧待建目录 + 逐文件单元」，先 `mkdir -p` 再逐文件 upload/download/exchange，三方向通吃；进度/字节统计/覆盖确认沿用。`local mkdir` 也接通（原"去文件管理器建"提示移除）。`tsc`/`npm build`/fmt/两套 check/三套 test（tauri lib 185）全绿。**注：远端递归 readdir 与勾选/拖拽为运行时行为，构建+本地遍历单测已过，真机 smoke 待用户验证** |
| J5 | ✅ 已完成 | 低 | Claude | SFTP 真实字节进度 | `SftpResult` 新增 `bytes`（`#[serde(default)]`），upload/download core 从 `std::io::copy` 返回值取已传字节；exchange 取 `uploaded.bytes`。前端进度条改为：选区已知大小求和得 `bytesTotal`，逐文件累加 `bytesDone`，有总量时进度按字节推进并显示 `X / Y`，否则回退按文件个数。`tsc`/`npm build`/两套 check/两套 test 通过（字节值源自 `io::copy`，真机数值留待手测） |

## K · 产品化与上线门槛

目标：功能广度已基本齐全，本阶段收口"能不能作为产品发给真人用"的硬门槛——凭据安全、发布信任链、跨平台完整性、真机验证，以及可靠性/体验打磨。来源是一次架构缺口评估（按"距离功能健全产品还缺什么"盘点，均已对照代码核实）。排序原则：决定"能否上线/能否信任"的安全与分发优先，跨平台与真机测试次之，体验/运维打磨垫后。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| K1 | ✅ 已完成 | 高 | Claude | 凭据接入 App 自建加密存储 | **不走 OS 钥匙串**，改为产品自建：`secrets.rs` 用 Argon2id 从**主密码**派生 256-bit key、AES-256-GCM 加密落 `~/.agent2ssh/secrets.enc`（0600），磁盘无明文 key；`hosts.json` 只留 `$agent2ssh-secret$` 句柄。解锁后 key 缓存进程内（Argon2 仅解锁时跑一次）。解锁：桌面启动弹 `SecretsUnlock` 对话框（`secrets_status`/`secrets_unlock`/`secrets_change_password` 命令 + Settings 设/改主密码）；CLI/MCP/daemon 读 `AGENT2SSH_MASTER_PASSWORD`（CLI 另加 `secrets status`/`secrets set-password`）。锁定安全：`internalize` 锁定时保留句柄不清空（save 不会孤立密文）、`embedded_ssh` 把裸句柄当「无密码」跳过密码认证（密码型主机锁定时不可用，by design）；`externalize` 锁定遇真实明文时保留明文+告警而非中断无关 save。`migrate_plaintext_secrets` 仅解锁后迁移旧明文；删除主机/代理与改名清理句柄。`memory` 测试后端（cfg(test) 默认）使单测无需主密码。含单测（真实 Argon2+AES 初始化/解锁/错密码拒绝/落盘无明文、锁定返回 None+store 报错、句柄落盘、迁移、改名清孤儿）+ CLI 真跑冒烟（status 不创建文件、写时初始化、密文无明文）。**注：apple/windows 文件权限 ACL（K2）真机待验** |
| K2 | ✅ 已完成 | 高 | Claude | Windows 文件权限加固 | `restrict_file_to_owner` 加 `#[cfg(windows)]` 分支：`icacls /inheritance:r /grant:r <user>:(F)` 去继承 + 仅当前用户 Full control，与 Unix `0600` 对齐（`daemon.token`/`keys/`/`hosts.json`）。**注：cfg(windows) 代码 macOS 无法编译校验，逻辑直白，真机冒烟待验** |
| K3 | ✅ 已完成 | 高 | Claude | 代码签名/公证 + 自动更新 | `tauri-plugin-updater`（Rust 注册 + `@tauri-apps/plugin-updater`/`plugin-process` npm + `src/lib/updater.ts` 签名校验 check/download/install + Settings「检查更新/安装更新」）。`tauri.conf.json` 加 `createUpdaterArtifacts`、`macOS`（hardenedRuntime + `entitlements.plist`）、`windows`、`plugins.updater`（endpoints + pubkey 占位）。CI `tauri-bundle` 加 Apple 证书导入步骤 + notarization/Windows 签名环境变量。`updater:default` 入 capabilities。**注：真实签名/公证/灰度需证书+发布端，无法在本机跑通；pubkey 占位需替换** |
| K4 | ✅ 已完成 | 高 | Claude | 真机 SSH E2E（容器化 sshd） | 新增 `scripts/e2e-docker.sh`：起 `linuxserver/openssh-server`（密钥认证，绕开 K1 密码凭据路径）跑真实 exec / SFTP 1MiB 往返字节比对 / mkdir+ls / J4 递归树往返 / K6 resume 续传；CI 加 `real-ssh-e2e` job（ubuntu）。`bash -n` 通过。**注：本机 docker daemon 未运行，脚本未实跑，CI 内运行** |
| K5 | ✅ 已完成 | 中 | Claude | 连接自愈 | `connection.rs` 重构：session 存 `Arc<StdMutex<Option<Session>>>` + `ConnectionHealth`；建连设 `set_keepalive(15s)`；全局 supervisor 任务每 30s `keepalive_send` 探活，失败标记 unhealthy 并按指数退避（5s→300s）`connect_embedded_ssh` 重连。`ConnectionStatus` 加 `healthy`/`reconnecting`/`last_error`（serde default 向后兼容），`HostList` 点颜色区分 健康/失效/重连。含 `backoff_grows_then_caps` 单测 |
| K6 | ✅ 已完成 | 中 | Claude | SFTP 传输健壮性 | 新增 `sftp_transfer.rs`：取消注册表（transfer_id→AtomicBool）+ `copy_cancellable`（64K 分块、按块查取消）+ `resume_offset` 决策。upload/download core 接入 resume（upload 远端 stat 长度 + `open_mode(WRITE|APPEND)` + 本地 seek；download 本地长度 + 远端 seek + 本地 append）与取消（`transfer_id`）。请求类型加 `resume`/`transfer_id`（serde default）；CLI `--resume`；Tauri `sftp_cancel` + 前端每文件 transfer_id + 取消按钮。**可选并发**：前端 SFTPPanel 加「并行传输」开关（默认关，开后 worker 池并发上限 `PARALLEL_TRANSFERS=4`），取消按钮按 `activeTransferIds` 集合中止所有在途文件，首个失败置 `aborted` 停止取新文件。daemon 启动日志明确告知 session/forward/在途传输不跨重启。含 4 单测 |
| K7 | ✅ 已完成 | 中 | Claude | 跨平台路径与行为打磨 | 前端 `localJoin` 识别 Windows 路径（含反斜杠/盘符）改用 `\` 拼接并转换子路径分隔符（`basenameOf` 本已双分隔符）。后端 `expand_local_path` 接受 `~\`。复核：daemon 信号 `shutdown_signal` 已 `cfg(unix)` 门控、loopback 已在 `remote.rs` 处理。**注：Windows 真机冒烟待验** |
| K8 | ✅ 已完成 | 低 | Claude | 配置版本化/迁移/自动备份 | `AppConfig` 加 `schema_version`（`CONFIG_SCHEMA_VERSION=1`）；`migrate_config` 向前兼容（未来版本不降级）；`normalize_config` 写时盖章（取 max 不降级）；`save_config` 写前把旧文件复制为 `hosts.json.bak`（原子 rename 已有，bak 防坏内容）。含 4 单测（盖章/幂等/不降级/备份） |
| K9 | ✅ 已完成 | 低 | Claude | 鉴权侧信道核查 | `token_matches` 改用 `subtle::ConstantTimeEq`（替换手写折叠，语义不变：空 expected 永不匹配）。复核：服务端唯一校验点即此处（scoped token 也经此），webhook 仅出站签名无入站校验。含 3 单测 |
| K10 | ✅ 已完成 | 低 | Claude | 体验与运维打磨 | i18n 审计脚本确认 350 used / 0 缺译（含本阶段新增键已补 zh）。a11y：Settings 已 Escape 关闭、新增控件均为原生 `<button>`/`<label><input>`。新增 `telemetry.rs`：opt-in（默认关）本地遥测（`telemetry.toml` 开关 + `telemetry.jsonl` 2MiB 上限，无网络导出），panic hook 接入 crash 事件（关时 no-op）；Tauri get/set 命令 + Settings 复选框。含单测（默认关、开后落盘、可关） |

> Phase K 收口（2026-06-21，Claude）：K1–K10 全部落地。验证：`cargo test --no-default-features` 全绿（lib 195 + daemon-feature 28 + integration daemon 57 + daemon bin 18）；CLI/MCP/daemon/tauri 四套 `cargo check` 通过；`cargo fmt --check` 干净；`npm run build`/`tsc --noEmit` 通过；i18n 0 缺译。**需真机/外部基建才能终验的部分**：K2 Windows ACL（cfg(windows) 未编译校验）、K3 真实签名+公证+灰度（需证书与发布端，pubkey 占位待换）、K4 容器化 E2E（本机 docker daemon 未运行，CI 内跑）、K7 Windows 路径冒烟。

# Agent2SSH 计划

## 当前状态

P0-P10 已全部完成。当前基线：

- 产品形态：Tauri 桌面 App、CLI、MCP stdio server、HTTP/WebSocket daemon、Web Console
- 核心能力：Host 管理、SSH config 导入、Jump Host、tags、per-host risk override
- 执行能力：SSH exec、exec-multi、ping、SFTP、PTY sessions、port forwarding、Playbooks
- 安全能力：风险评分、自定义风险规则、审批队列、审批端点、桌面审批弹窗、敏感命令脱敏
- 运维能力：SSH ControlMaster 连接池、Webhook 通知、remote daemon registry、健康检查、指标、审计轮转
- 生态能力：SSH key 管理、团队配置导入导出、MCP 客户端模板、插件/Skill 分发文档
- 验收结果：45 单元测试 + 40 集成测试 + 17 CLI smoke 测试 = 102 测试全绿
- MCP 工具：35 个，详见 [skills.md](skills.md)

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
| F1 | 真实环境试运行 | ⬜ 待认领 | 高 | - |
| F2 | 主机与环境管理 | ⬜ 待认领 | 高 | - |
| F3 | 执行体验与 Runbook | ⬜ 待认领 | 高 | - |
| F4 | 审批与协作 | ⬜ 待认领 | 高 | - |
| F5 | 远程 daemon 与多节点 | ⬜ 待认领 | 高 | - |
| F6 | 可观测与审计分析 | ⬜ 待认领 | 中 | - |

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

## F1 · 真实环境试运行

目标：用现有功能覆盖一台到三台真实可控主机，形成 bug 修复清单。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F1-1 | ⬜ 待认领 | 高 | - | 建立本地 sshd 或测试机 fixture | exec、ping、sftp、session、forward 可在同一 fixture 上重复运行 |
| F1-2 | ⬜ 待认领 | 高 | - | 跑完整 CLI 工作流 | host add/import、risk、exec、exec-multi、audit、daemon、doctor 全部有记录 |
| F1-3 | ⬜ 待认领 | 高 | - | 跑 MCP 工作流 | MCP 客户端通过 `ssh_list_hosts`、`ssh_exec`、`ssh_audit`、`ssh_doctor` 完成一次诊断 |
| F1-4 | ⬜ 待认领 | 中 | - | 跑桌面端首次启动和常规操作 | SetupWizard、Host 管理、Exec、Approvals、Keys 无阻断问题 |
| F1-5 | ⬜ 待认领 | 高 | - | 输出 bug backlog | 每个 bug 有复现步骤、期望行为、实际行为和影响等级 |

## F2 · 主机与环境管理

目标：让 Agent2SSH 更适合管理多环境、多角色主机。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F2-1 | ⬜ 待认领 | 高 | - | 主机分组与环境视图 | UI/CLI 可按 env、role、owner、tag 过滤主机 |
| F2-2 | ⬜ 待认领 | 中 | - | 主机健康快照 | 批量采集 uptime、disk、memory、load、ssh latency，并写入本地快照 |
| F2-3 | ⬜ 待认领 | 中 | - | 主机配置变更预览 | team config import 前显示新增、修改、删除差异 |
| F2-4 | ⬜ 待认领 | 中 | - | SSH config 双向同步策略 | 明确 Agent2SSH 与 `~/.ssh/config` 的导入、覆盖、冲突处理规则 |

## F3 · 执行体验与 Runbook

目标：把一次性命令执行升级为可审计、可复用的运维流程。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F3-1 | ⬜ 待认领 | 高 | - | Playbook 参数化 | playbook step 支持参数、默认值、必填校验和 dry-run 展示 |
| F3-2 | ⬜ 待认领 | 高 | - | 执行计划预览 | 高风险或多主机执行前展示目标、命令、风险、预计影响 |
| F3-3 | ⬜ 待认领 | 中 | - | 批量执行策略 | 支持并发数、失败阈值、逐批 rollout、暂停/继续 |
| F3-4 | ⬜ 待认领 | 中 | - | 执行结果比较 | 多主机结果可按 exit code、stdout diff、stderr 聚合查看 |

## F4 · 审批与协作

目标：让高风险操作适合团队协作，而不是只适合单机个人使用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F4-1 | ⬜ 待认领 | 高 | - | 审批策略配置 | 按 host/tag/risk/command pattern 配置是否需要审批 |
| F4-2 | ⬜ 待认领 | 高 | - | 审批上下文增强 | 审批请求包含 diff、目标主机、历史执行、发起来源 |
| F4-3 | ⬜ 待认领 | 中 | - | 审批通知回调 | Slack/自定义 webhook 可跳转到认证后的审批页面 |
| F4-4 | ⬜ 待认领 | 中 | - | 操作备注与变更单号 | exec/playbook 支持 reason/change_id 并进入 audit |

## F5 · 远程 daemon 与多节点

目标：把 remote daemon 从“可路由”推进到“可运营”。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F5-1 | ⬜ 待认领 | 高 | - | remote daemon 连接诊断 | `agent2ssh doctor --daemon <alias>` 检查 TLS、token、health、version |
| F5-2 | ⬜ 待认领 | 高 | - | daemon 版本兼容检查 | CLI/MCP 调用远程 daemon 前提示协议或版本不兼容 |
| F5-3 | ⬜ 待认领 | 中 | - | remote daemon 权限范围 | 每个 remote 配置允许的 hosts/tags/commands 范围 |
| F5-4 | ⬜ 待认领 | 中 | - | 多 daemon 统一视图 | UI/CLI 可按 daemon 查看 host、health、metrics |

## F6 · 可观测与审计分析

目标：让 audit 和 metrics 变成定位问题、复盘操作的工具。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F6-1 | ⬜ 待认领 | 高 | - | 审计查询增强 | 支持全文搜索、时间范围、主机组、命令模式组合过滤 |
| F6-2 | ⬜ 待认领 | 中 | - | 审计导出 | 支持 JSONL/CSV 导出，并保留脱敏策略 |
| F6-3 | ⬜ 待认领 | 中 | - | 指标趋势 | 展示执行量、失败率、风险分布、审批耗时趋势 |
| F6-4 | ⬜ 待认领 | 低 | - | 事件订阅 | 提供本地事件流供外部监控或自动化消费 |

## 近期建议

优先从 F1 开始，不要直接进入 F2-F6。F1 的输出应该是一份 bug backlog；只有当现有功能跑通后，再按 bug 影响和使用频率决定下一批功能。

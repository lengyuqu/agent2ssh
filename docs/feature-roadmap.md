# Agent2SSH 功能路线图

当前 P0-P10 已完成，后续不再先堆底层能力，而是以真实使用场景驱动：每一阶段都先跑现有功能、记录 bug，再决定是否扩展功能。

## 原则

- 先 dogfood，再扩展：每个新功能阶段开始前，先用当前 CLI、daemon、MCP、桌面端完成一遍真实 SSH 工作流。
- bug 修复优先于新功能：真实工作流中发现的认证、权限、审计、执行、安全和 UI 问题优先进入修复队列。
- 功能必须有验收场景：新增功能需要同时给出 CLI/API/MCP 或 UI 至少一个可复现验收路径。
- 安全默认保守：涉及批量执行、凭证、审批绕过、远程 daemon 的能力必须默认最小权限。

## F1 · 真实环境试运行

目标：用现有功能覆盖一台到三台真实可控主机，形成 bug 修复清单。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| F1-1 | 高 | 建立本地 sshd 或测试机 fixture | exec、ping、sftp、session、forward 可在同一 fixture 上重复运行 |
| F1-2 | 高 | 跑完整 CLI 工作流 | host add/import、risk、exec、exec-multi、audit、daemon、doctor 全部有记录 |
| F1-3 | 高 | 跑 MCP 工作流 | MCP 客户端通过 `ssh_list_hosts`、`ssh_exec`、`ssh_audit`、`ssh_doctor` 完成一次诊断 |
| F1-4 | 中 | 跑桌面端首次启动和常规操作 | SetupWizard、Host 管理、Exec、Approvals、Keys 无阻断问题 |
| F1-5 | 高 | 输出 bug backlog | 每个 bug 有复现步骤、期望行为、实际行为和影响等级 |

## F2 · 主机与环境管理

目标：让 Agent2SSH 更适合管理多环境、多角色主机。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| F2-1 | 高 | 主机分组与环境视图 | UI/CLI 可按 env、role、owner、tag 过滤主机 |
| F2-2 | 中 | 主机健康快照 | 批量采集 uptime、disk、memory、load、ssh latency，并写入本地快照 |
| F2-3 | 中 | 主机配置变更预览 | team config import 前显示新增、修改、删除差异 |
| F2-4 | 中 | SSH config 双向同步策略 | 明确 Agent2SSH 与 `~/.ssh/config` 的导入、覆盖、冲突处理规则 |

## F3 · 执行体验与 Runbook

目标：把一次性命令执行升级为可审计、可复用的运维流程。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| F3-1 | 高 | Playbook 参数化 | playbook step 支持参数、默认值、必填校验和 dry-run 展示 |
| F3-2 | 高 | 执行计划预览 | 高风险或多主机执行前展示目标、命令、风险、预计影响 |
| F3-3 | 中 | 批量执行策略 | 支持并发数、失败阈值、逐批 rollout、暂停/继续 |
| F3-4 | 中 | 执行结果比较 | 多主机结果可按 exit code、stdout diff、stderr 聚合查看 |

## F4 · 审批与协作

目标：让高风险操作适合团队协作，而不是只适合单机个人使用。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| F4-1 | 高 | 审批策略配置 | 按 host/tag/risk/command pattern 配置是否需要审批 |
| F4-2 | 高 | 审批上下文增强 | 审批请求包含 diff、目标主机、历史执行、发起来源 |
| F4-3 | 中 | 审批通知回调 | Slack/自定义 webhook 可跳转到认证后的审批页面 |
| F4-4 | 中 | 操作备注与变更单号 | exec/playbook 支持 reason/change_id 并进入 audit |

## F5 · 远程 daemon 与多节点

目标：把 remote daemon 从“可路由”推进到“可运营”。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| F5-1 | 高 | remote daemon 连接诊断 | `agent2ssh doctor --daemon <alias>` 检查 TLS、token、health、version |
| F5-2 | 高 | daemon 版本兼容检查 | CLI/MCP 调用远程 daemon 前提示协议或版本不兼容 |
| F5-3 | 中 | remote daemon 权限范围 | 每个 remote 配置允许的 hosts/tags/commands 范围 |
| F5-4 | 中 | 多 daemon 统一视图 | UI/CLI 可按 daemon 查看 host、health、metrics |

## F6 · 可观测与审计分析

目标：让 audit 和 metrics 变成定位问题、复盘操作的工具。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| F6-1 | 高 | 审计查询增强 | 支持全文搜索、时间范围、主机组、命令模式组合过滤 |
| F6-2 | 中 | 审计导出 | 支持 JSONL/CSV 导出，并保留脱敏策略 |
| F6-3 | 中 | 指标趋势 | 展示执行量、失败率、风险分布、审批耗时趋势 |
| F6-4 | 低 | 事件订阅 | 提供本地事件流供外部监控或自动化消费 |

## 近期建议

优先从 F1 开始，不要直接进入 F2-F6。F1 的输出应该是一份 bug backlog；只有当现有功能跑通后，再按 bug 影响和使用频率决定下一批功能。

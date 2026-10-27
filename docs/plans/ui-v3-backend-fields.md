# UI v3 改版后端配合任务单（Rust 侧）

> 日期：2026-08-28 · 来源：UI v3「午夜指挥舱」改版收口时识别的待后端项
> 前置：前端 T01–T05 已合入 main（94c09e4）。本单为 Rust/daemon 侧增量任务清单，供排期。
> 约束：改 `ApprovalRequest` / 审计条目等结构体时必须同步 `src/types.ts`（serde rename_all 对齐）与 `docs/skills.md`（如涉及 MCP 工具表，跑 CI 门禁测试）。

## B1 · 审批请求补充 Agent 来源与命中规则

- 现状（2026-10-27 更正）：后端 `ApprovalRequest.context`（`ApprovalContext`）已携带 `source`（请求方标签）、`risk_details.matched_policy`、`reason`、`change_id`，`/approvals` 与审批持久化均已返回；缺口原本在前端类型与界面。前端已接线（`src/types.ts` 补 `context`，队列卡显示「来自」，详情栏显示请求方 / 命中策略 / 原因 / 变更单号，`docs/api.yaml` 补 schema）。
- 仍未做：独立的 `session_id`、`matched_snippet`（命令内高危片段偏移）。风险分类走 AST 与多层解包（`core.rs::classify_risk`、`risk_extra`），没有现成的命中位置可取，需要先让分类器返回命中证据，工作量较大，未随本次改动处理。
- 原任务清单（以下 `agent_name` 项已由 `context.source` 覆盖）：
  - [ ] `ApprovalRequest` 增加 `agent_name: String`（或 `source: AgentSource` 枚举，与 MCP agent 注册对齐）与 `session_id`
  - [ ] 增加可选 `matched_rule: Option<String>`（命中策略规则名）与可选 `matched_snippet: Option<String>`（命令中的高危片段及起止偏移，用于片段级着色）
  - [ ] REST `/approvals` 与事件总线 `approval_requested` 事件同步携带新字段
- 前端收益：工作台队列卡与详情栏展示「谁在请求、命中哪条规则、命令哪一段高危」。

## B2 · 审计条目补充裁决结果与执行上下文

- 状态（2026-10-27）：后端与审计页已落地。`AuditEntry` 新增 `decision`（`approved / rejected / timed_out / auto_allowed`，另加 `blocked` 表示被风险规则直接拒绝）与 `decided_by`（`human / rule / timeout`）；`AuditFilter`、`GET /audit`、`GET /audit/export` 支持 `decision` 与 `agent`（按 `source` 精确匹配，忽略大小写）；审计表新增「裁决」列。
- 取舍：执行成功条目的 `approved`/`auto_allowed` 由进程内「已批准账本」（`store::note_human_approval`，10 分钟过期）与审计写入配对得出，避免改动 70 余处 `append_audit` 调用点；没有执行（`exit_code` 为空）且无明确裁决的条目保持 `None`，不猜测。旧条目同样为 `None`。
- 未做：CLI/MCP 的审计过滤参数、CSV 导出新列、工作台「今日已审」改用 `decision` 统计。

- 现状：审计结果三态（approved/rejected/自动放行）前端靠 exit_code 等近似推断。
- 任务：
  - [ ] 审计条目增加 `decision: Decision`（approved / rejected / timed_out / auto_allowed）与 `decided_by: String`（人工/规则）
  - [ ] 审计 REST 过滤参数支持 `decision` 与 `agent`
- 前端收益：审计页结果三态与筛选 chips 用真实数据；「今日已审」统计去近似化。

## B3 · 主机配置补充「仅人工」标记与策略摘要数据源

- 状态（2026-10-27）：已落地。`HostProfile.ai_allowed`（默认 true）；授权层 `authorize_command_with_approval` 对 `ai_allowed=false` 的主机拒绝非人工来源（`execution_control::HUMAN_SOURCES` = `desktop`、`cli`；`mcp`、`daemon` 及自定义 `AGENT2SSH_SOURCE` 一律拒绝），拒绝记入审计（`decision=blocked`）。新增 `GET /hosts/{name}/policy`（`HostPolicySummary`：每个风险级别的审批要求 `auto/required/conditional`、适用的审批策略名、用户自定义 blocked 规则），主机页策略摘要行在选中单台主机时显示该主机的实时策略，主机表显示「仅人工」徽标，编辑表单有对应开关。
- 已知边界：agent 若在 shell 里调用 CLI，来源是 `cli`，不会被拦——这是与「人在终端用 CLI」无法区分的取舍（已与需求方确认）。含 `command_pattern` 的策略无法按主机静态判断，摘要标为 `conditional`。
- 未做：CLI/MCP 暴露 `ai_allowed`（`host add` 参数、工具入参）、主机策略的 MCP 工具。

- 现状：主机卡「AI 可用 / 仅人工」chip 与策略摘要行（low 自动放行 / med-high 需审批 / 删除类禁止）目前是前端常量文案。
- 任务：
  - [ ] Host 配置增加 `ai_allowed: bool`（默认 true）
  - [ ] 提供 host 级策略读取端点（或在现有 policy 端点上返回 per-host 摘要：auto-allow 级别、需审批级别、禁止规则集），供策略摘要行渲染真实数据
- 前端收益：策略摘要行从静态文案变为实时配置映射，点击可进策略编辑。

## B4 · 「编辑后执行」审批改写

- 状态（2026-10-27）：已落地，范围限于 daemon `POST /exec`。`POST /approvals/{id}/approve` 可带 `{"command": "..."}`，批准的是改写后的文本并由它执行；空 body 仍是原样批准，格式错误的 body 返回 400（不会当成普通批准）。
- 重新评估：改写命令必须不是 `blocked`、风险不高于原请求（同样应用主机 `risk_override`），否则拒绝并让请求方重新发起；`authorize_command_with_approval` 在执行前再核一次，不信任 handler。
- 只有 `/exec` 允许改写（审批 `context.rewritable`）：exec-multi、playbook、SFTP、会话、转发等路径批准的是它们自己拼出的文本，替换会让「批准的」与「执行的」脱节，所以这些请求拒绝改写，前端仍显示「复制命令并前往执行」。
- 审计：执行条目的 `command` 是实际执行的命令，`original_command` 记录原请求，`decision=approved`、`decided_by=human`；审批记录的 `rewritten_command` 与 `approved_command_snapshot` 为改写后的文本。
- 未做：MCP/CLI 的 `ssh_approval_respond` 暂不支持带命令批准；其他执行路径的改写支持。

- 现状：审批端点仅 approve/reject，前端「编辑后执行」降级为复制命令 + 跳转执行面板。
- 任务：
  - [ ] 审批响应增加 `approve_with_command(new_command: String)`（或 approve 请求体可选 `override_command`），改写后的命令需重走风险评估并在审计中记录原始命令与改写命令
- 前端收益：详情栏「编辑后执行」从降级方案恢复为设计稿完整交互。

## B5 · 批准后抽屉展示 exec 输出（可选，P2）

- 现状：daemon exec 走独立通道（`request_and_wait_for_approval` 阻塞后自行执行），批准后前端抽屉无输出可看。
- 任务：
  - [ ] daemon 侧批准后的 exec 若能复用/旁路输出到该主机的 `/terminal` WS 会话（或新增 `/approvals/{id}/output` SSE），则前端抽屉可实时展示执行过程
  - [ ] 评估成本：若改造成本高，可接受维持现状（抽屉仅作实时视口），本项可降级不做

## 验收

- 全部字段变更跑齐：`cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`、`--test daemon_integration`、`--test cli_smoke`
- 涉及 MCP 工具签名变更时：`docs/skills.md` 同步 + `mcp_tools_match_skills_md_documentation` 门禁通过
- 前端联调：`npm test && npm run build` + `npm run tauri:dev` 目检对应界面

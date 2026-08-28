# UI v3 改版后端配合任务单（Rust 侧）

> 日期：2026-08-28 · 来源：UI v3「午夜指挥舱」改版收口时识别的待后端项
> 前置：前端 T01–T05 已合入 main（94c09e4）。本单为 Rust/daemon 侧增量任务清单，供排期。
> 约束：改 `ApprovalRequest` / 审计条目等结构体时必须同步 `src/types.ts`（serde rename_all 对齐）与 `docs/skills.md`（如涉及 MCP 工具表，跑 CI 门禁测试）。

## B1 · 审批请求补充 Agent 来源与命中规则

- 现状：`ApprovalRequest`（`src-tauri` 侧结构体）缺少 agent 标识与策略命中详情，前端工作台只能显示主机/命令/风险级。
- 任务：
  - [ ] `ApprovalRequest` 增加 `agent_name: String`（或 `source: AgentSource` 枚举，与 MCP agent 注册对齐）与 `session_id`
  - [ ] 增加可选 `matched_rule: Option<String>`（命中策略规则名）与可选 `matched_snippet: Option<String>`（命令中的高危片段及起止偏移，用于片段级着色）
  - [ ] REST `/approvals` 与事件总线 `approval_requested` 事件同步携带新字段
- 前端收益：工作台队列卡与详情栏展示「谁在请求、命中哪条规则、命令哪一段高危」。

## B2 · 审计条目补充裁决结果与执行上下文

- 现状：审计结果三态（approved/rejected/自动放行）前端靠 exit_code 等近似推断。
- 任务：
  - [ ] 审计条目增加 `decision: Decision`（approved / rejected / timed_out / auto_allowed）与 `decided_by: String`（人工/规则）
  - [ ] 审计 REST 过滤参数支持 `decision` 与 `agent`
- 前端收益：审计页结果三态与筛选 chips 用真实数据；「今日已审」统计去近似化。

## B3 · 主机配置补充「仅人工」标记与策略摘要数据源

- 现状：主机卡「AI 可用 / 仅人工」chip 与策略摘要行（low 自动放行 / med-high 需审批 / 删除类禁止）目前是前端常量文案。
- 任务：
  - [ ] Host 配置增加 `ai_allowed: bool`（默认 true）
  - [ ] 提供 host 级策略读取端点（或在现有 policy 端点上返回 per-host 摘要：auto-allow 级别、需审批级别、禁止规则集），供策略摘要行渲染真实数据
- 前端收益：策略摘要行从静态文案变为实时配置映射，点击可进策略编辑。

## B4 · 「编辑后执行」审批改写

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

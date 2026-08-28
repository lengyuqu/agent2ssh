# v3「午夜指挥舱」增量实现方案与任务分解

> 作者：架构师 Bob（高见远） · 依据：`UI_DESIGN_PROPOSAL.md`（v3 定稿，唯一设计依据）
> 范围：仅前端（`src/`），不改 Rust 后端、不改 shadcn 原语 API。
> 原则：最小侵入——视觉主体靠一次「token 换血」全局生效，结构性改动集中在审批工作台与顶栏。

---

## 1. 增量实现方案摘要

### 1.1 核心策略：一次换血，两层映射

现有代码的 40+ 业务面板全部基于 shadcn 语义 token（`bg-card` / `text-muted-foreground` / `border-border`…），**不直接持有色值**。因此本方案不逐组件改样式，而是：

```
v3 设计令牌（:root 源头）        →   现有 shadcn 语义变量（派生）      →   组件零改动
--canvas-0 #0A0F18 侧栏/命令块底  →   --sidebar / --sidebar-*
--canvas-1 #0E1420 主画布        →   --background / --foreground
--canvas-2 #131A28 卡片/选中行    →   --card / --secondary / --muted
--canvas-3 #1A2334 浮层          →   --popover
--line      #1C2637             →   --border
--edge      #263042             →   --input
--edge-strong #33405A           →   --ring（hover 描边由组件层用 edge-strong）
--text-1    #E6EDF3             →   --foreground
--text-2    #8B98AC             →   --muted-foreground
--text-3    #5A6778             →   新增 --faint（弱化/时间戳/注脚）
--action    #2DD4BF             →   --primary
--action-ink #062B26            →   --primary-foreground
--risk-low  #34D399             →   --success
--risk-medium #FFB224           →   --warning
--risk-high #F25555             →   --destructive（blocked 实底复用同值）
```

同时在 `@theme inline` 增补设计稿原语的直通映射，供新组件使用：
`--color-canvas-0..3`、`--color-faint`、`--color-risk-low/medium/high`、`--color-edge-strong`，
即新组件可直接写 `bg-canvas-0`、`text-risk-high`、`border-edge-strong`。

### 1.2 Token 换血的具体做法（index.css + theme.tsx）

1. **`:root` 即指挥舱**：把 `:root, :root[data-theme="light"]` 拆开——
   - `:root`（无 `data-theme`）写入指挥舱全套变量 + `color-scheme: dark`，并**删除** `@media (prefers-color-scheme: dark)` 分支（system 不再跟随 OS，默认即指挥舱）；
   - `:root[data-theme="light"]` 保留现有浅色值不动（兼容存量用户）。
2. **现有 5 套主题选择器全部保留**：`dark` / `dracula` / `nord` / `solarized-light` 分支原样不动。
3. **theme.tsx**：
   - `Theme` 联合类型 `"midnight-ops" | "system" | "light" | ...`；`THEMES` 列表首位插入 `{ id: "midnight-ops", label: "Midnight Ops", swatch: "#2DD4BF" }`；
   - `applyTheme()`：`midnight-ops` 与 `system` 都不写 `data-theme`（即落在 `:root` 指挥舱）；`initialTheme()` 默认返回 `"midnight-ops"`；存量 localStorage 里的旧值继续有效（`system` 语义变为指挥舱，见 §5 风险）。
4. **mono 字体**：`@theme` 覆盖 `--font-mono: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`，全局 `font-mono` 类即生效；UI 正文字体维持 Inter。
5. **圆角**：沿用现有 `--radius: 0.625rem`（10px 卡片）体系，`rounded-xl`（14px）给 Dialog/Palette 窗口，控件用 `rounded-md`（9px 观感）——不新增圆角变量。
6. **去阴影**：删除 toast / CommandPalette / Dialog 上的 `shadow-*` 用法，层级一律 `bg-canvas-3 + border-edge`（对齐铁律 3）。
7. **终端跟随**：`terminalThemes.ts` 的 `"app"`（Match app）主题色值改为指挥舱深色底 + 风险三族侧条色，`command-block-bar` / `command-block-selection` 的 rgba 白改为 `var(--risk-*)` 驱动。

### 1.3 布局结构改造（App.tsx，最小侵入）

```
<div flex-col h-screen>                       ← 不变
  <TopBar/>                                   ← 新增：A2 标识 + Agent 在线 pills + daemon/gate/凭据/连接数（原 Footbar 内容并入）+ ⌘K 入口
  <main flex-1 flex>
    <aside 150px bg-canvas-0>                 ← 侧栏重绘：窄化为 150px，导航重排（approvals 置顶 + 青色激活 + 数字 badge），折叠逻辑保留
    <section flex-1>
      {activeModule === "approvals"
        ? <ApprovalWorkbench pending={...}/>  ← 新增：队列 250px + 详情三栏，替代 ApprovalTimeline + 自动弹 ApprovalDialog
        : <原 module-page 渲染/>}             ← 其余模块不动
  （Footbar 移除）
```

- **ApprovalDialog 不删除**：降级为「高危两步确认」弹窗（`⌘↵` 批准高危时复用），文案从「高危命令」泛化为确认语义。
- App.tsx 中 `currentApproval && <ApprovalDialog/>` 的**自动弹窗行为移除**（改为工作台常驻 + 侧栏 badge + Toast 提醒）；`⌘⇧A` 快捷键改为「跳转工作台并聚焦第一条待审」。
- 审批轮询逻辑（`pollApprovals`）保留在 App 层供全局 badge；工作台内部自带 10s 轮询 + SSE 即时刷新（复用 ApprovalTimeline 现有模式）。

### 1.4 状态顶栏（Footbar → TopBar）

- 新建 `TopBar.tsx`：`canvas-0` 底、单行 36px；左 = A2 标识，中 = Agent 在线 pills（daemon 状态色点 + gate 状态 + 凭据锁 + 活跃连接数，全部迁自 Footbar 的 props），右 = 版本号（mono）+ ⌘K 按钮。
- `Footbar.tsx` 删除；其 4 个 props 原样上移给 TopBar，无数据层改动。

### 1.5 i18n 与测试

- 所有新文案进 `src/i18n.tsx`（en + zh 双语，0 缺失门禁）；预计新增 ~35 key。
- `npm test`（现有 HostList / SnippetsDialog / api.broadcast 三组测试不触碰被删文件）与 `npm run build` 每个任务收尾必须通过；`npm run tauri:dev` 目检对比设计稿。

---

## 2. 完整文件清单

| 路径 | 操作 | 所属任务 | 说明 |
|---|---|---|---|
| `src/index.css` | 修改 | T01 | `:root` 指挥舱换血、拆 light 分支、删 media query、`@theme inline` 增补 canvas/risk/faint 映射、mono 字体、去阴影 |
| `src/theme.tsx` | 修改 | T01 | 新增 `midnight-ops` 主题并设为默认；`applyTheme`/`initialTheme` 适配 |
| `src/terminalThemes.ts` | 修改 | T01 | `"app"` 主题色值对齐指挥舱 |
| `src/components/ui/toast.tsx` | 修改 | T01/T05 | 去阴影 → 左缘 3px 语义色、canvas-3 底、右上堆叠 ≤3、3.5s 消退（T01 先做色映射，T05 做布局） |
| `src/components/ui/dialog.tsx` | 修改 | T01 | canvas-3 底 + edge 描边、去阴影、rounded-xl |
| `src/components/ui/badge.tsx` | 修改 | T01 | 色值经语义变量自动换血，仅校验/微调 |
| `src/components/ui/button.tsx` | 修改 | T01 | default 变体 = `--action` 实底 + `--action-ink` 字 |
| `src/components/ui/card.tsx`、`input.tsx`、`select.tsx`、`state.tsx`、`icon-button.tsx`、`textarea.tsx`、`data-table.tsx` | 修改 | T01 | 仅随 token 换血校验描边/圆角，不改 API |
| `src/i18n.tsx` | 修改 | T01–T05 | 各任务新文案（en+zh） |
| `src/App.tsx` | 修改 | T02/T03 | 布局改造、移除自动弹窗、`⌘⇧A` 改跳转、接入 TopBar、删 Footbar 引用 |
| `src/components/approval/ApprovalWorkbench.tsx` | 新增 | T02 | 三栏容器：开屏大数字统计 + 队列 + 详情；自带轮询/SSE/键盘 |
| `src/components/approval/ApprovalQueue.tsx` | 新增 | T02 | 250px 队列卡：Agent→主机 + 风险 chip + mono 命令；选中 canvas-2 + 左缘 2px 风险色 |
| `src/components/approval/ApprovalDetail.tsx` | 新增 | T02 | 命令块（canvas-0、mono、高危着色）+ 三按钮 + 键盘提示 |
| `src/components/RiskBadge.tsx` | 修改 | T02 | 色映射对齐 `--risk-*`（low/medium/high 文字+描边，blocked 实底 canvas-0 字） |
| `src/components/ApprovalDialog.tsx` | 修改 | T02 | 降级为高危两步确认弹窗（文案/视觉对齐，API 不变） |
| `src/components/ApprovalTimeline.tsx` | 删除 | T02 | 被 ApprovalWorkbench 取代（历史记录并入工作台「已裁决」分组或暂由工作台承载） |
| `src/components/TopBar.tsx` | 新增 | T03 | 状态顶栏（Footbar 内容 + Agent pills + ⌘K 入口） |
| `src/components/Footbar.tsx` | 删除 | T03 | 状态并入顶栏 |
| `src/components/HostList.tsx` | 修改 | T04 | 主机卡片皮肤：状态点 / 「AI 可用·仅人工」chip / mono 延迟 / 离线弱化 opacity-75 |
| `src/components/AddHostForm.tsx` | 修改 | T04 | 表单控件描边/聚焦随 token 校验 |
| `src/components/AuditPanel.tsx` | 修改 | T04 | 顶部风险计数 chips（实底/描边切换）、mono 时间戳/命令按风险族着色、结果三态 |
| `src/components/AuditCharts.tsx` | 修改 | T04 | 图表配色对齐风险三族 |
| `src/components/CommandPalette.tsx` | 修改 | T05 | canvas-3 浮层 460px、五分组（待审批置顶/跳转/快捷操作/MCP 工具 `>`/连接主机） |
| `src/components/TerminalDrawer.tsx` | 新增 | T05 | 右侧下沉终端抽屉（canvas-0），包 TerminalView；审批「批准执行」与审计「回放」唤起 |
| `src/components/SettingsMenu.tsx` | 修改 | T03 | 从主区 header 迁至 TopBar 触发的菜单（props 不变） |

> 不修改：`api.ts`、`eventsBus.tsx`、`types.ts`、其余 ~30 个业务面板（token 换血自动覆盖其视觉）。

---

## 3. 有序任务列表

### T01 · Token 换血 + 主题系统（P0，本轮）

**文件**：`src/index.css`、`src/theme.tsx`、`src/terminalThemes.ts`、`src/components/ui/{toast,dialog,badge,button,card,input,select,state,icon-button,textarea,data-table}.tsx`、`src/i18n.tsx`（主题名文案）
**内容**：按 §1.1/§1.2 执行；`:root` 写入指挥舱 14 个设计令牌并派生语义变量；拆 `light` 分支、删 `prefers-color-scheme` 分支；`@theme inline` 增补 `canvas-0..3 / faint / risk-low/medium/high / edge-strong`；`--font-mono` 换 JetBrains Mono 栈；THEMES 增 `midnight-ops` 并置顶为默认；终端 `"app"` 主题对齐。
**验收**：
1. `npm test && npm run build` 通过；
2. 清空 localStorage 启动即指挥舱深色；主题选择器可切回 light/dark/dracula/nord/solarized-light 且各自观感不变；
3. 全 `src/` 无新增 hex 裸色值（`grep -rn '#[0-9a-fA-F]\{3,8\}' src/components` 仅 terminalThemes/swatch 白名单命中）；
4. toast/dialog 无 `shadow-*` 残留。

### T02 · 审批工作台三栏主界面（P0，核心工作量）

**文件**：`src/App.tsx`、`src/components/approval/{ApprovalWorkbench,ApprovalQueue,ApprovalDetail}.tsx`（新增）、`src/components/RiskBadge.tsx`、`src/components/ApprovalDialog.tsx`、**删除** `src/components/ApprovalTimeline.tsx`、`src/i18n.tsx`
**内容**：按 §1.3；`activeModule` 默认改为 `"approvals"`；开屏 30px 大数字「N 项待您裁决」+ 弱化统计（执行中/今日已审，今日已审从 audit 列表统计，不新增后端）；队列卡与详情栏按设计稿 3.1；键盘 `↑↓` 选卡、`↵` 聚焦详情、`⌘↵` 批准（high/blocked 先弹 ApprovalDialog 两步确认）、`⌫` 拒绝；移除 App 自动弹 ApprovalDialog，`⌘⇧A` 改为跳转工作台并聚焦第一条。
**验收**：
1. `npm test && npm run build` 通过（ApprovalTimeline 无测试引用，需 grep 确认）；
2. 有待审时侧栏 approvals 项显示数字 badge（青色激活态）；批准/拒绝后队列即时减少，SSE 事件即时刷新；
3. 高危命令批准必须经两步确认，`⌫` 拒绝不弹确认；
4. 无待审时详情栏显示弱化空态（text-3，非报错）。

### T03 · 状态顶栏 + 取消 Footbar（P0）

**文件**：`src/components/TopBar.tsx`（新增）、`src/App.tsx`、`src/components/SettingsMenu.tsx`、`src/components/Footbar.tsx`（删除）、`src/i18n.tsx`
**内容**：按 §1.4；侧栏同步重绘为 150px `canvas-0`（导航 approvals 置顶 + 青色激活 + 其余模块顺序重排，折叠态保留）；原主区 header 的 gate pill/待审 pill 上移顶栏，主区只留模块标题。
**验收**：
1. `npm test && npm run build` 通过；
2. 全局搜索无 `Footbar` 引用残留；`package.json` version 展示迁至 TopBar；
3. daemon/gate/凭据/连接数/版本五项状态在顶栏完整可见，`tauri:dev` 目检窄窗口（<1024px）不换行溢出。

### T04 · 主机配置中心 + 审计日志皮肤（P0）

**文件**：`src/components/{HostList,AddHostForm,AuditPanel,AuditCharts}.tsx`、`src/i18n.tsx`
**内容**：主机卡片加「策略摘要行」：`low 自动放行 · med/high 需审批 · 删除类禁止` 三段分着 risk-low/medium/high（数据来自前端常量文案，非后端）；离线/仅人工主机整卡 `opacity-75`；审计页顶部风险计数 chips（选中实底反白/未选描边）、行内 mono 时间戳 text-3 + 命令按风险族着色 + 结果三态（approved=low / rejected=high / auto=text-3）。
**验收**：
1. `npm test && npm run build` 通过（HostList.test.tsx 必须全绿——只加类名/包裹层，不改行为与文案 key）；
2. 策略摘要行三段颜色与 RiskBadge 三族一致；审计筛选 chips 点击后实底反白；
3. 全部新文案 en/zh 双语齐全。

### T05 · ⌘K 分组 + Toast 形态 + 终端抽屉（P1，可留下一轮）

**文件**：`src/components/CommandPalette.tsx`、`src/components/ui/toast.tsx`、`src/components/TerminalDrawer.tsx`（新增）、`src/components/AuditPanel.tsx`、`src/App.tsx`、`src/i18n.tsx`
**内容**：Palette 按 3.5 五分组重排（待审批置顶，需把 pending 列表传入 props——新增 prop，默认空数组保持兼容）；Toast 改右上堆叠 ≤3 条、canvas-3 底 + 左缘 3px 语义色、3.5s 消退、错误常驻；新建右侧终端抽屉（宽 ~480px，canvas-0），审批「批准执行」与审计「回放」唤起并执行/回放对应命令，复用 TerminalView 与 `command-block-bar`。
**验收**：
1. `npm test && npm run build` 通过；
2. `⌘K` 打开 460px canvas-3 浮层，无阴影、五分组顺序正确；Toast 不超过 3 条堆叠；
3. 抽屉内 xterm 交互与命令块侧条（风险三族着色）正常，Esc/点击遮罩可收起。

**依赖关系**：T01 → T02/T03（可并行）→ T04 → T05。T04 依赖 T01 的令牌与 T03 的顶栏布局。

---

## 4. 共享约定（跨文件，工程师必读）

1. **令牌命名**：设计令牌以 CSS 变量形式只存在于 `src/index.css` 的 `:root`（`--canvas-*`、`--line`、`--edge*`、`--text-*`、`--action*`、`--risk-*`）；组件层只允许两种写法：
   - Tailwind 语义类（`bg-card`、`text-muted-foreground`、`border-border`…，含 `bg-canvas-0`、`text-risk-high` 等本次新增的 utility）；
   - 内联 `var(--risk-high)`（仅限 SVG/chart 等类名覆盖不了的场合）。
   **禁止**在 tsx 中写 hex/rgb 裸色值。
2. **风险三族 → RiskLevel 映射**（全界面同源，RiskBadge 是唯一实现点）：
   - `low → --risk-low`（绿，文字+淡底）、`medium → --risk-medium`（琥珀）、`high → --risk-high`（红，文字+淡底）、`blocked → --risk-high` **实底 + `--canvas-0` 文字**；
   - ApprovalStatus/审计结果语义复用：`approved = low 绿`、`rejected = high 红`、`timed_out/自动放行 = text-3 灰`；
   - Toast 左缘色：success/error/warning 分别 = risk-low/risk-high/risk-medium；后台任务进行中 = `--action` 青。
3. **mono 字体使用场景**：一切数据——命令、主机名/IP、时间戳、延迟/指标、风险 chip（大写）、指纹、版本号。UI 文案、按钮、菜单用 Inter。实现上直接用 `font-mono`（T01 已把栈换成 JetBrains Mono 优先）。
4. **行动色纪律**：`--action`（青）只出现在可点击动作——主按钮、当前导航、链接、⌘K 入口、焦点环（2px）。装饰、状态、计数 badge 一律不得使用青色（待审数字 badge 用前景色+描边，仅激活态用青底）。
5. **层级纪律**：只有四级画布（canvas-0..3）+ 1px 描边；禁止新增 `shadow-*` 与渐变；悬浮层一律 `bg-popover`（= canvas-3）。
6. **i18n**：新文案 key 用英文原句作 key（沿用现有惯例），en 是 key 本身、zh 必须补翻译；提交前跑 `npm run lint`（biome）+ 0 缺失门禁。
7. **shadcn 原语**：`ui/*.tsx` 的 props/variants 类型签名不变，只改 cva 内的类名映射；业务组件不得绕过原语直接写样式（chart/终端像素定位除外）。

---

## 5. 风险与待明确事项

| # | 事项 | 影响 | 处置建议 |
|---|---|---|---|
| 1 | **`system` 主题语义变化**：原「跟随 OS 明暗」，换血后 = 指挥舱深色。存量 localStorage 存有 `system`/`light` 的用户升级后观感突变 | 中（符合设计意图：默认仅一套深色皮肤） | 在主题菜单中把 `system` 项移除或改名「指挥舱（默认）」；不写迁移代码 |
| 2 | **Footbar 删除** | 低：仅 App.tsx 一处引用，无测试覆盖；props 四项原样上移 TopBar | T03 一并删除 + grep 校验零引用 |
| 3 | **ApprovalDialog 自动弹窗移除**：`⌘⇧A` 原逻辑「聚焦 #approval-dialog-cancel」失效 | 低 | T02 改为 `setActiveModule("approvals")` + 聚焦队列首卡 |
| 4 | **「编辑后执行」无后端 API**：审批接口只有 approve/reject，无法携带修改后的命令 | 高（设计稿三按钮之一） | P0 先实现「拒绝 / 批准执行」两按钮 + 「复制命令」；「编辑后执行」降级为把命令填入执行面板（跳转 execute 模块），真正改写审批需后端支持，列为待明确 |
| 5 | **「命中规则」展示缺数据**：`ApprovalRequest` 无 matched_rule 字段 | 中 | P0 详情栏只做 risk_level 全行着色；高危片段级着色与规则名展示待后端补充字段后做（P1） |
| 6 | **「策略规则」导航项无对应面板** | 中 | P0 左栏不放死链：先并入「主机配置」的策略摘要行；独立策略规则面板列 P1 待产品确认 |
| 7 | **ApprovalTimeline 历史记录归属**：工作台是否承载全部历史 | 中 | T02 先把工作台做成 pending 队列 + 折叠的「今日已裁决」分组（复用其轮询/渲染 cap 模式）；完整历史检索 P1 |
| 8 | **text-3 对比度**：#5A6778 on canvas-1 ≈ 3.9:1，低于设计稿自定的 4.5:1 底线 | 低 | 实施时若目检吃力，上调至 #64748B（≈4.6:1），只动 token 不动组件 |
| 9 | **JetBrains Mono 未打包**：依赖系统字体回退 | 低 | 不引入字体文件（Tauri 包体优先）；回退栈保证 mono 度量一致，后续按需内置 woff2 |
| 10 | **终端抽屉与现有 TerminalPanel 并存**：抽屉会话与终端模块会话是否共享 | 中 | P1 实现时复用 daemon 会话模型（SessionInfo），抽屉仅是同一会话的第二视口，不新开会话；实现前与后端确认会话 attach 语义 |

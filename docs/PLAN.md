# Agent2SSH 计划（合并版）

> 本文件是 Agent2SSH 的唯一规划源。
> - **活跃计划**：Plan 2（功能与 UI 演进），见下方「活跃计划」章节。
> - **历史归档**：原始大计划（P0–K）已全部完成，降级为「历史归档」章节供追溯。
> - **附录**：Plan 2 Q1/Q2 执行报告。

---

## 活跃计划：Plan 2（功能与 UI 演进）

## Agent2SSH Plan 2 — 功能与 UI 演进计划

> 日期：2026-07
> 定位：0.2.1 质量收口已完成，项目从"质量闭环"转入"功能+UI 演进"阶段。本计划覆盖旧 plan2.md Q1 ✅ 和 Q2 剩余项，并扩展为体验驱动的多阶段演进路线。

### 1. 当前判断

项目已完成 P0-P10、F1-F6、S1-S9、G、E、O、H、I、J、K 全部阶段，0.2.1 已发布。基线数据：203 Rust lib tests、29 CLI/MCP smoke、57 daemon integration、442 i18n keys / 0 缺译、14 low-risk defects（8 FE + 6 Rust）。

旧 plan2.md 的定位是"质量收口"，Q1（发布可信度）已完成，Q2（凭据/WebDAV 回归）部分完成。后续不再继续质量收口路线，而是进入功能+UI 演进——让桌面端从"可用的 SSH 能力层"升级为"高效、直观、可操作的 SSH 操作面"。

**与旧 plan2.md 的关系**：
- Q1 ✅ 全部纳入，不再重复
- Q2 剩余项（真 WebDAV push/pull、网络故障恢复、跨设备流程、SecretsUnlock UI、密码主密码 SSH 主机端到端）保留为 Q2' 子阶段
- Q3-Q6 原定位（真实接入、债务、诊断、性能）不再作为独立阶段，其有价值的子项已分散到 V 阶段的验收标准中

**技术栈现状（写 V 阶段任务前核对过代码，避免任务描述与实际栈脱节）**：
- 前端是 **Tailwind v4 + shadcn 风格 token/primitive 体系**（`src/index.css` + `src/components/ui/`），**没有 MUI 依赖**（`package.json` 无 `@mui/*`）。下文所有任务描述已按此纠正，不再出现"MUI Snackbar / MUI ThemeProvider / MUI DataGrid"字样——新增交互一律基于现有 `ui/` primitives 或轻量 headless 库（如 TanStack Table），避免引入第二套设计系统。
- **主题系统已经上线**，不是待办项：`src/theme.tsx` + `src/index.css` 已实现 6 套主题（system/light/dark/dracula/nord/solarized-light），通过 `<html data-theme>` + CSS 变量切换，`SettingsMenu` 里已有选择器。原 V2-4"暗色模式"任务已按此改写为覆盖度审查，而不是从零实现。
- Recharts / Monaco / TanStack Table / D3-force-graph / react-diff-viewer 目前均未引入，V2-V4 中提到它们时确实是新增依赖，评审时按"新依赖引入"对待（打包体积、许可证、维护成本）。

### 2. 优先级总览

| 阶段 | 主题 | 项数 | 预估周期 |
|------|------|------|----------|
| Q2' | 凭据/WebDAV 回归收尾 | 5 | 1-2 周（并行于 V1） |
| V1 | 基础体验骨架 | 5 | 2-3 周 |
| V2 | 核心交互升级 | 5 | 3-4 周 |
| V3 | 效率工具链 | 4 | 3-4 周 |
| V4 | 高级可视化与自动化 | 6 | 4-5 周 |

### 3. Q2' · 凭据/WebDAV 回归收尾

目标：完成旧 plan2 Q2 的 5 项真实环境验证，在 V1 启动同时并行推进。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q2'-1 | 高 | 真 WebDAV push/pull 回归 | 对真实 WebDAV 服务完成 push/pull，覆盖 hosts.json、secrets.enc、policy、limits、playbooks 同步；确认不同步 known_hosts.json、tokens、audit、logs、私钥 |
| Q2'-2 | 高 | WebDAV 网络故障恢复 | 模拟远端旧 manifest、未知文件、网络失败、认证失败；错误提示包含下一步动作 |
| Q2'-3 | 中 | 跨设备拉取后流程文档 | 配置指南补"新设备 pull → 解锁 → 验证 host-key → 避免覆盖本地信任库"短流程 |
| Q2'-4 | 中 | SecretsUnlock 桌面 UI walkthrough | 手动走一遍桌面启动 → 输入主密码 → 解锁 → 锁定 → 改密码 → 错密码拒绝 |
| Q2'-5 | 中 | 密码型 SSH 主机端到端 | 用主密码解锁后，对密码认证 SSH 主机完成 exec/SFTP 全链路 |

验收命令：`cargo test --no-default-features secrets::tests webdav_sync::tests` + `npm run build`

### 4. V1 · 基础体验骨架

目标：建立桌面端操作入口的骨架——首页总览、全局状态、快速搜索和统一反馈，让用户打开 App 后 3 秒内知道系统状态、5 秒内找到任何功能入口。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V1-1 | ✅ 已完成 | P0 | 中 | Dashboard / 健康总览页 | `src/components/Dashboard.tsx`：新增首页模块（Ctrl+K/侧栏第一项），聚合 6 张卡片——主机健康(connectionStatuses)、审批待办数(pendingApprovals)、异常告警数(SSE `anomaly_detected` 事件的会话内计数，非历史总量——anomaly.rs 本身是 fire-and-forget 事件，没有持久化的历史计数可查，已在卡片 hint 里注明"自打开仪表盘起")、24h 执行量(复用既有 `list_audit` 命令按 `since` 过滤计数)、凭据锁定状态、daemon 运行状态；Host/24h 卡片可点击跳转 Host Management / Audit；`npm run build` 通过，Playwright 截图验证 light/dark 下 6 卡片数据渲染正确 |
| V1-2 | ✅ 已完成 | P0 | 低 | 全局状态栏 Footbar | `src/components/Footbar.tsx`：底部固定栏显示 daemon 状态(green/red)、gate 状态、凭据(locked/unlocked)、活跃连接数、版本号（从 `package.json` 读取）；`npm run build` 通过，Playwright 截图验证 light/dark 主题下渲染正常，各状态字段随 mock 数据正确切换 |
| V1-3 | ✅ 已完成 | P0 | 低 | 命令面板 Ctrl+K | `src/components/CommandPalette.tsx`：全局模态搜索框，索引 Modules + Hosts（按 name/host/user/group/role/owner/tags 匹配）；Ctrl+K/Cmd+K 打开、Esc 关闭、↑↓ 选择、Enter 跳转（模块直接切换，主机跳转到 Host Management 并选中）；`npm run build` 通过，Playwright 验证搜索过滤、键盘导航、跳转后状态正确。范围收敛：未索引"命令/审批"关键词，因为目前没有独立的审批列表页可跳转（审批走既有的 ApprovalDialog 弹层），留给后续评估是否需要 |
| V1-4 | ✅ 已完成 | P1 | 低 | Toast / 通知条统一 | `src/components/ui/toast.tsx`：`ToastProvider` + `useToast()`，success/error/warning 三种，5s 自动消失+手动关闭，基于现有 `ui/` primitives（未引入 MUI）；已接入 `main.tsx`（包住整个 App）；**App.tsx 全量转换**（26 处 `setError`/banner 改为 `showToast`）。**本次补完**：AddHostForm、ExecPanel、ForwardPanel、KeysPanel、McpAgentsPanel、MultiExecPanel、PingPanel、PlaybooksPanel、ProxyPanel、SFTPPanel（顶层操作反馈，区别于 `s.error` 驱动的每侧 `ErrorState`）、SyncPanel 的一次性操作反馈（保存/删除/连接失败等）改为 `showToast`。**有意保留为局部 state（非误伤，是设计选择）**：SecretsUnlock（登录式密码错误需贴着输入框常驻，不宜 5s 消失）、SettingsMenu（诊断导出路径/更新版本号等分区内的常驻状态文案）、SetupWizard（每步骤绑定的校验反馈）、LiveActivityPanel 的连接离线原因（已改用 `ErrorState` 组件渲染，但仍是持久状态而非 toast，因为 `status` 徽标本身就是持久态）；`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V1-5 | ✅ 已完成 | P1 | 低 | 空态/加载态/错误态一致性 | `src/components/ui/state.tsx`：`EmptyState`/`LoadingState`/`ErrorState` 三个共享组件（统一图标+文案+可选操作按钮）。**已接入**：AuditPanel（空态）、HostList（两处空态）、SFTPPanel（error/loading/empty 三态）、ForwardPanel（无隧道空态）、KeysPanel（无密钥空态）、PlaybooksPanel（无 playbook 空态）、ProxyPanel（无代理空态）、LiveActivityPanel（无活动空态 + 连接失败态）。ExecPanel/MultiExecPanel 的终端输出占位符按 CLAUDE.md 约定保留固定深色终端面板样式，不套用 token 化的 `EmptyState`；ApprovalDialog 是静态确认弹层，没有空/加载/错误态可套用——审批列表/时间线视图留给 V2-2；`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --test cli_smoke`

### 5. V2 · 核心交互升级

目标：让审批、审计和通知从"可看"升级为"可操作、可分析、可实时感知"；主题系统已上线（见第 1 节），本阶段只做覆盖度审查，不重建。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V2-1 | ✅ 已完成 | P0 | 中 | 实时桌面通知系统 | 新增 `src/eventsBus.tsx`：单一共享 SSE 连接（`EventsProvider` + `useAgentEvents`/`useEventsStatus`），`LiveActivityPanel` 与 `Dashboard` 的异常计数都改用它，替换掉各自独立开的 SSE 连接（原来是 3 条并发连接，现在 1 条）。新增 `src/components/NotificationCenter.tsx`：订阅 `approval_requested` 弹可操作 toast（内嵌批准/拒绝，`durationMs: null` 常驻直到处理或被 `approval_responded` 事件关闭）、`anomaly_detected` 弹只读 toast（8s）。`ui/toast.tsx` 扩展支持 `title`/`actions`/`durationMs`（`null`=常驻）与 `dismissToast(id)`。**连接状态变更通知**未走 SSE——`host_connected`/`host_disconnected` 事件类型在 `events.rs` 里定义了但后端从未 `publish_event` 过，是死枚举；改为在 `App.tsx` 现有的 5s `pollConnections` 里做前后快照 diff，状态翻转时 `showToast`，如实反映了实际数据来源而非假装走了 SSE。**设计取舍**：可操作 toast 与已有的阻塞式 `ApprovalDialog`（`pendingApprovals[0]` 自动弹出）会短暂共存——两者调用同一后端 API，处理一个另一个会在下次轮询后自然消失，不是正确性问题，故未改动既有阻塞弹窗行为。`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V2-2 | ✅ 已完成 | P1 | 中 | 审批时间线视图+批量操作 | 新增模块 `approvals`（追加到 `MODULES` 末尾，不插入中间，避免打乱 V2-5 已发布的 Ctrl/⌘+1~9 映射）+ `src/components/ApprovalTimeline.tsx`：全量审批历史（`api.fetchApprovals()` 本身就不过滤 status，之前只有 `App.tsx` 会话内过滤成 pending）按 `requested_at` 倒序的竖向时间轴，每条含时间戳/主机/`RiskBadge`/状态 `Badge`（pending=warning、approved=success、rejected=destructive、timed_out=secondary）；仅 pending 项可勾选，批量批准/拒绝复用既有单条 `approvalApprove`/`approvalReject` REST 端点（`Promise.allSettled` 并发调用），执行结果走既有 exec 授权管线自然落入 audit，未新增后端逻辑；沿用 `AuditPanel` 的 `RENDER_CAP_STEP` 分页模式防止长期运行的审批历史（后端未做过期清理）撑爆 DOM；订阅事件总线的 `approval_requested`/`approval_responded` 做即时 refetch，外加 10s 轮询兜底。Dashboard 的"Pending approvals"卡片现在可点击跳转到这个新模块。`npm run build` 通过 |
| V2-3 | ✅ 已完成 | P1 | 中 | 审计可视化图表 | 引入 `recharts`（新依赖，`vendor-charts` chunk ~92KB gzip，已按现有 `vite.config.ts` manualChunks 规则单独分包避免和 `vendor-react` 打包环产生循环 chunk 警告）。新增 `src/components/AuditCharts.tsx`，渲染在 Audit 模块 `AuditPanel` 上方，24h/7d/30d 范围切换驱动全部图表（对齐 dataviz skill 的"筛选器统一作用于下方所有图表"原则）：① 执行量趋势——单序列面积图（`--primary`，10% 透明度描边+柱面）；② 风险分布——柱状图，四档配色复用 `RiskBadge` 已有的语义色（success/warning/destructive），不是新造的分类色板；③ 按来源统计——水平柱状图取代原计划的"来源饼图"（dataviz skill 明确把"比较量级"归类为 sequential 单色柱状图，饼图不在推荐表里，遂改用条形图，见下方说明）；④ 主机活跃时段热力图——自建 CSS grid（Recharts 无原生热力图），取执行量 Top 8 主机 × 24 小时格子，颜色用 `color-mix(in srgb, var(--primary) N%, var(--card))` 单色渐变，零执行格子用 `--muted` 而非色阶最浅端（避免"看起来仍有一点活动"的误读）。所有图表颜色直接引用 CSS 自定义属性字符串（如 `stroke="var(--primary)"`），SVG 属性原生支持 `var()`，因此 6 套主题下自动跟随，无需按主题重新计算或做单独校验。数据来源独立于 `AuditPanel` 的可调 `limit`/筛选（`AuditPanel` 默认 limit 太小，不够支撑 30 天趋势），改为自己按 30 天窗口 + 5000 条上限拉取一次，与 `Dashboard.tsx` 现有的 24h 计数请求同一约定。`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V2-4 | ✅ 已完成 | P2 | 低 | 主题系统覆盖度审查（原"暗色模式"，已上线不必重做） | 对全部 `src/components/*.tsx` + `App.tsx` 做了硬编码颜色（hex / Tailwind 命名色板 / 依赖 `prefers-color-scheme` 的 `dark:` 变体）扫描。**修复 6 处真实问题**：① `LiveActivityPanel` 状态徽标 `connecting` 从 `bg-sky-500/15 text-sky-500` 改为 `bg-primary/15 text-primary`（与 `live`/`offline` 已用 token 的写法对齐）；② 同文件异常 `severity` 徽标 `text-orange-600` 改为 `text-warning`；③ 同文件 `item.detail`/`item.raw` 预览框 `bg-[#0f172a] text-slate-200` 改为 `bg-muted text-foreground`（与紧邻的 `item.command` 预览已用 token 保持一致，且它只是文本/JSON 预览而非终端仿真，不属于终端例外）；④ `ApprovalDialog` 命令预览框 `bg-[#1e293b] text-slate-100` 改为 `bg-muted text-foreground`；⑤⑥ `PlaybooksPanel` 步骤输出预览、`TerminalPanel` 标签页关闭按钮 hover 用的是 Tailwind `dark:` 变体（默认跟随系统 `prefers-color-scheme`），与 App 自己的 `data-theme` 显式主题切换是两套独立机制——用户显式选中 dark/dracula/nord 等主题但操作系统仍是浅色模式时，这两处会撞色（浅色调叠加在深色卡片上，对比度不足）；改为不依赖 `dark:` 的 `bg-foreground/5` / `hover:bg-foreground/10`，随 `--foreground`/`--background` 自动适配当前 `data-theme`。**确认为既有合理例外，未改动**：`ExecPanel`/`MultiExecPanel`/`ErrorBoundary` 的终端输出块固定深色（`#0e1620`/`#e6edf3`，CLAUDE.md 已记录）；`PingPanel` 与侧栏固定深色状态色（CLAUDE.md 已记录）；`Dialog`/`CommandPalette` 的 `bg-black/50` 遮罩层；`SettingsMenu` 主题色块 `border-black/15`；`TerminalPanel` 终端画布本身的颜色（由独立的终端主题子系统 `terminalThemes.ts` 驱动，非 App 主题 token）。**观察但未处理**（超出本项"硬编码撞色"范围，属于全主题通用的可访问性问题，需要产品决策）：侧栏激活模块项用固定 `text-white`，在 Nord（`--sidebar-accent: #88c0d0`）、Dracula（`#bd93f9`）等浅色高亮下与白色文字对比度明显不足（估算 <3:1），但这在全部 6 套主题下都存在、并非某一主题独有的新问题，未擅自改动配色。**未发现对应 UI**：i18n 中 "Session"/"Attach to this session" 等词条当前没有任何组件在用，桌面端目前没有独立 Session 面板，故审查范围里的"Session"页跳过。`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V2-5 | ✅ 已完成 | P1 | 低 | 键盘快捷键体系 | `src/App.tsx` 全局 `keydown` 处理：Ctrl/⌘+K 打开命令面板（沿用 V1-3）、Ctrl/⌘+1~9 直接切换到对应模块（`MODULES` 前 9 项）、Ctrl/⌘+Shift+A 在有待处理审批时聚焦 `ApprovalDialog` 的取消按钮、无待处理审批时 `showToast` 提示；`src/components/ExecPanel.tsx` / `MultiExecPanel.tsx` 的命令 `Textarea` 绑定 Ctrl/⌘+Enter 直接执行；所有绑定 `preventDefault()` 避免浏览器默认行为；快捷键列表在 `SettingsMenu` 新增"Keyboard shortcuts"分区展示；新增 i18n 键已补齐中文翻译；`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿。未做浏览器交互回归（Tauri `invoke` 在纯 vite dev 环境不可用），已用类型检查 + 代码走查替代 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --features daemon --test daemon_integration`

### 6. V3 · 效率工具链

目标：让 SFTP 和终端从"基本可用"升级为"日常效率工具"——文件预览省去下载、分屏覆盖多主机场景、表格交互减少查找时间。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V3-1 | ✅ 已完成 | P1 | 低/中 | SFTP 文件预览+面包屑导航 | 面包屑其实在 SFTPPanel 里已经存在（`buildBreadcrumbs()`，点击跳转目录），本项只补预览。新增后端能力（`core.rs::sftp_read_text_core_with_source` 复用既有 `sftp_dir_operation_core` helper，和 `sftp_ls`/`sftp_stat` 一样过 `authorize_desktop_operation` 授权网关，不是绕过安全管线的旁路）+ `tauri_commands.rs::sftp_read_text`/`local_read_text`（本地侧不经授权，和 `local_ls`/`local_walk` 现有约定一致），都在 `generate_handler!` 里注册，服务端各自按 ~1MB 硬上限读取并要求合法 UTF-8，超限/非文本直接 `Err`。前端双击文件行触发 `FilePreview.tsx`（`React.lazy` 懒加载，Monaco 只在真正打开一次预览后才拉取，不进首屏包）；size 已知且 ≥1MB 的文件直接跳过网络请求展示元信息卡片，读取失败（二进制/超限/权限错误）同样落到元信息卡片而不是报错崩溃。**Monaco 版本特意锁定 `0.53.0`（非 `^`）**：`^0.55.1` 会带出有已知 XSS 通报的 `dompurify@3.2.7`（精确锁定版本，上游没法通过 `npm audit fix` 更新），0.53.0 这条依赖链根本不存在；只注册基础 `editor.worker`，JSON/CSS/TS 的语言服务诊断显式关掉（`setDiagnosticsOptions({ validate: false })` 等），避免为一个只读预览搭进 TS 编译器体量的 worker（省了 ~8MB）。CSP 是 `script-src 'self'`，`@monaco-editor/react` 默认走 CDN 加载器会被拦，改成 `loader.config({ monaco })` 指向本地打包的包。**如实说明**：`vendor-monaco` chunk 打包后仍有 ~4.3MB / gzip 1.1MB——这是桌面应用的本地资源，不走网络，主要成本是首次打开预览时的一次性 JS 解析/编译，不是数据在无网/弱网环境的下载体验；"≤1s" 验收目标覆盖的是文件内容读取（一次 IPC 调用，实际远快于 1s），冷启动那次额外的 Monaco 解析时间未纳入严格量化。`npm run build`/`cargo test --lib`/`cargo test --no-default-features --lib`（208/215 全绿）/`cargo fmt --check` 均通过；新增 3 条 `local_read_text_inner` 单元测试（正常读取/超限拒绝/二进制拒绝） |
| V3-2 | ✅ 已完成 | P1 | 高 | 终端分屏+Session 分组 | `TerminalPanel.tsx` 从"单 Tab 显示、其余隐藏挂载"重构为最多 4 窗格（单/水平二分/垂直二分/2×2 四宫格），窗格边界可拖拽调整比例（`pointermove` 实时更新百分比，非固定网格）；**所有已打开的 Tab 始终保持挂载/连接**，未分配到任何窗格的仅 `visibility:hidden`（保留原有"切走不断线"的行为，没有因为改成多窗格而让后台会话被杀掉）。左侧新增按主机分组的会话树（可折叠），点击某会话把它指派到当前"聚焦窗格"；每个窗格头部也有一个下拉可单独换绑会话。`TerminalView.tsx` 改造为 `forwardRef` 暴露 `sendText`/`focus`，并新增基于用户自己按键的逐行缓冲（Enter 落一行、退格/Ctrl+U/Ctrl+C 清缓冲——和 `docs/architecture.md` 里后端"整行边界"审计缓冲是同一思路，只是这边是给历史搜索用，不做鉴权），Ctrl+R 通过 `term.attachCustomKeyEventHandler` 拦截、打开本 App 自己的历史搜索浮层而不转发给远端 shell。**明确的行为取舍**：这意味着 Ctrl+R 不再触发 bash 自带的 reverse-i-search——只要窗格数>1（多窗格模式）就用本地搜索浮层替代，选中一条历史只是把文本重新"打"回输入框（不带回车），用户还能改了再按 Enter，不会盲目重跑高风险命令。`npm run build` 通过；未接入真实 daemon 做 4 窗格并发连接的手动烟测（同前几轮，纯 vite dev 环境下 Tauri `invoke`/WS 不可用），用类型检查 + 代码走查 + 分层 z-index 修正（窗格头部/边框显式 `z-[2]` 盖过终端画布的 `z-1`，避免画布压住窗格头）替代 |
| V3-3 | ✅ 已完成 | P1 | 中 | 表格/列表交互增强 | 引入 `@tanstack/react-table`（headless，零新样式依赖，复用现有 token/primitive，独立 `vendor-table` chunk）。新增 `ui/data-table.tsx` 共享 `SortIcon`/`ColumnVisibilityMenu`（两处表格都用得到，不是单次抽象）。`HostList.tsx` 从卡片列表改成真表格：姓名/地址/状态可排序，标签/详情列可显示隐藏，勾选行 + 批量删除（新增 `App.tsx::handleBatchRemoveHosts`，并发调用既有单条 `removeHost`，只刷新一次而不是 N 次）。`AuditPanel.tsx` 同样表格化：时间/主机/风险可排序，勾选行 + "复制所选为 JSON"批量操作（audit 是不可变审计记录，没有做批量删除这种会破坏审计完整性的操作，选了个安全的只读批量动作）。`npm run build` 通过 |
| V3-4 | ✅ 已完成 | P1 | 中 | 模块间 Breadcrumb 导航 | 新增 `Breadcrumb.tsx` + `App.tsx::RELATED_MODULES` 映射表：Host→Execute/Files/Tunnels/Terminal/Audit、Execute→Host/Audit、Audit→Host/Approvals、Approvals→Execute/Audit 等，网状而非线性 Tab，点击直接跳模块。Dashboard 的"待处理审批"卡片顺手接上了跳转到新 Approvals 模块（V2-2 遗留的一个小尾巴）。`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --lib` + `cargo fmt --check`

### 7. V4 · 高级可视化与自动化

目标：补上拓扑感知、自动化编排和视觉一致性——让多主机运维从"逐台操作"升级为"全局视角+模板驱动"。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V4-1 | ✅ 已完成 | P2 | 高 | 连接拓扑可视化 | 用 `d3-force`（纯物理引擎，~50KB，不带 `react-force-graph`/three.js）+ 手写 SVG 渲染，没有引入带自己一套主题系统、难与 6 套主题对齐的图表包。新增 `ConnectionTopology.tsx`：daemon 为中心节点，向每台主机连线；主机→跳板机（`jump_host`，虚线）、主机→代理（`proxy_id`）、主机→隧道目标（读 `forwardList()`，虚线）；节点颜色按连接健康态（success/warning/destructive/muted，复用 HostList 的健康态判断逻辑）与代理类别着色；点击主机节点跳转 Host Management 并选中该主机。物理仿真用 ref 直接写 SVG 属性而非每帧 setState，避免 React 重渲染开销。新增 `topology` 模块（追加到 `MODULES` 末尾）。`npm run build` 通过 |
| V4-2 | ✅ 已完成 | P2 | 高 | Playbook 可视化创建/编辑 | **范围裁剪，如实说明**：查了 `playbook.rs`/`PlaybookStepDefinition`，后端播放引擎目前只支持线性步骤列表，没有任何条件分支/if-else 运行时概念——为不存在的能力做可视化编辑器会是纯装饰性 UI，故未做"条件分支可视化"，只做了真正对得上后端能力的两项：① 步骤编排器：`PlaybooksPanel.tsx` 从单个 `Textarea`（换行分隔字符串）改成真正的 `steps: string[]` 数组，每步一行，原生拖放排序（`draggable`/`onDragStart`/`onDrop`，和 SFTPPanel 已有的拖放模式一致，没有引入新的拖拽库）；② YAML 双向同步：新增 `js-yaml` 依赖，"步骤/YAML"两个视图对同一份表单状态可互相转换（`yamlFromForm`/`formFromYaml`），解析失败时保留用户输入并提示错误而不是丢弃编辑。保存路径完全复用既有 `api.savePlaybook`/`api.runPlaybook`，未改动 `Playbook` 类型或后端播放引擎，保存后 `Run` 立即可执行。`npm run build` 通过 |
| V4-3 | ✅ 已完成 | P2 | 中 | 配置模板+一键快照恢复 | 复用 `webdav_sync.rs` 已有的 `create_sync_backup`/`SYNCABLE_FILES` 机制（原本只在推拉同步前做内部安全备份），新增 `list_config_snapshots`/`create_named_snapshot`/`restore_config_snapshot`/`delete_config_snapshot`/`apply_config_template`（均在 `webdav_sync.rs`，路径遍历做了校验），新增 4 个 Tauri 命令 + `ConfigSnapshotsPanel.tsx`（新模块 `config`）。3 个内置模板（基础安全/开发环境/生产运维）是手写、schema 校验过的真实 `policy.toml` + `execution_limits.toml`（对照 `policy.rs::AgentPolicyFile`/`limits.rs::ExecutionLimitConfig` 字段编写，policy 只做升级不降级，符合既有约束），应用/恢复前都会先自动打一个安全快照。**如实说明一个能力边界**：`policy.toml` 走 mtime 缓存，改了立即生效；`execution_limits.toml` 只在 daemon 启动时读一次（代码里 `load_execution_limits()` 目前没有任何调用点接到热更新路径），改完需要重启 daemon 才生效——UI 文案里写明了这点。新增 Rust 单测覆盖创建/列出/恢复/删除快照与模板文件白名单校验。`npm run build`、`cargo test --lib`（218/211）、`cargo fmt --check` 通过 |
| V4-4 | ✅ 已完成 | P2 | 低 | 跨主机比较 UX 增强 | 没有引入 `react-diff-viewer`（其主题系统较难和本项目 6 套主题精确对齐），改用小巧且维护良好的 `diff`（jsdiff）做行级 diff 算法，配色手写 `HostDiffView.tsx`——复用 ExecPanel 已有的"终端输出固定深色"例外方案的调色板家族，新增两个红/绿高亮色阶（因为 token 化的 success/destructive 在固定深色背景上跨 6 主题未必保证对比度）。选两台主机对比 stdout，统计"相同/仅 A 独有/仅 B 独有"行数。嵌入 `MultiExecPanel.tsx`，会话重新执行不同主机集合时会自动重置选择，不留陈旧对比。`npm run build` 通过 |
| V4-5 | ✅ 已完成 | P2 | 中 | 响应式布局优化 | `tauri.conf.json` 的 `minWidth` 从 900 降到 800，让"800px 最小宽度"字面上可达到（原先 900 的硬限制下窗口物理上到不了 800px）。侧栏新增可持久化的图标模式折叠开关（`PanelLeftClose`/`PanelLeftOpen`），**排查出一个真实的响应式 bug**：最初把折叠态样式全部套在 `lg:` 前缀下，结果在 800–1024px 这个恰恰最需要该功能的区间，原有 `max-lg:w-full` 全宽堆叠规则仍会无条件生效，折叠开关形同虚设；改成折叠态类名不带断点前缀（折叠后任何宽度都保持 76px 图标栏），同时让 `<main>`/内容区的 `max-lg:flex-col`/`max-lg:overflow-visible` 也在折叠时关闭，保持左右布局而不是纵向堆叠。命令面板窄屏（`max-sm`，640px 以下）改为全屏覆盖而不是带边距的居中卡片。SFTP 面板双栏其实早就在 `xl` 断点以下隐式单列堆叠（`grid` 没加 `grid-cols-*` 时的默认行为），这次复核确认无需改动。`npm run build` 通过 |
| V4-6 | ✅ 已完成 | P2 | 低/中 | 图标/配色系统统一（沿用现有 token，不新建） | 复核了 V1-V4 全部新增/改动组件（`ConnectionTopology`/`ConfigSnapshotsPanel`/`HostDiffView`/`ApprovalTimeline`/`AuditCharts`/`Breadcrumb`/`FilePreview`/`NotificationCenter`/`HostList`/`AuditPanel`/`TerminalPanel` 等）的硬编码颜色扫描，除已确认的终端类固定深色例外（ExecPanel 家族 + 本轮新增的 HostDiffView 红绿高亮）外无遗留硬编码色。全部 17 个模块（含本阶段新增的 `topology`/`config`/`approvals`）图标互不重复。发现一处**未处理的既有可访问性问题**（非本阶段引入）：侧栏激活态用固定 `text-white`，在 Nord/Dracula 等浅色高亮下对比度不足——这在 V2-4 就已记录，仍然是产品配色决策，不属于本次"硬编码撞色"修复范围，未擅自更改。`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --lib` + `cargo fmt --check`

### 8. 不建议现在做的事

以下方向不在本计划范围内，旧 plan2.md 已排除，新 plan2 继续排除：

- ❌ Cloud Console 独立产品化（当前定位是本机操作面，不是云端控制台）
- ❌ 账号体系与用户认证（单机工具，无多用户需求）
- ❌ 组织 RBAC 与团队权限（单机定位，团队协作靠 WebDAV 配置同步 + 审批流）
- ❌ SaaS 化与多租户（无真实用户需求驱动）
- ❌ 继续增加 MCP 工具数量（51 工具已覆盖核心工作流，除非有明确缺口）
- ❌ 把桌面端改成大型运维平台（更适合作为本地 operator surface）

### 9. 推荐执行顺序

```
Q2' ── 并行于 V1 ──→ V1 ──→ V2 ──→ V3 ──→ V4
(1-2周)            (2-3周)  (3-4周)  (3-4周)  (4-5周)
```

- **Q2' 并行于 V1**：凭据/WebDAV 回归可与 V1 前端骨架同步推进，无依赖冲突
- **V1 → V2**：V2 通知系统依赖 V1 的 Toast 组件；V2 审批时间线/图表依赖 V1 的空态/加载态共享组件
- **V2 → V3**：V3 的表格/面包屑是新增交互面，建议排在 V2 通知与批量操作之后，避免同一批面板的交互模式被改两次；不依赖主题系统（token 体系已在 V 阶段启动前就存在）
- **V3 → V4**：V4 拓扑/Playbook 可独立开发，但建议在 V3 表格交互稳定后进入，避免 UI 层同时大改

每个阶段完成后跑该阶段验收命令 + `scripts/e2e-local.sh`，确保新 UI 不破坏现有后端基线。

### 10. 最小验收命令

每阶段合并或发布候选至少跑：

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
git diff --check
```

发布候选再加：

```bash
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
npm run tauri:build
./scripts/e2e-local.sh
```

V4 发布候选额外验证桌面端视觉一致性：macOS `.app` 启动后逐一检查暗色/亮色模式、Footbar、命令面板、面包屑在各模块的渲染稳定性。

## 历史归档：原始大计划（P0–K）

## Agent2SSH 计划

### 当前状态

P0-P10 已全部完成。当前基线：

- 产品形态：Tauri 桌面 App、CLI、MCP stdio server、HTTP/WebSocket daemon、Web Console
- 核心能力：Host 管理、SSH config 导入、Jump Host、tags、per-host risk override
- 执行能力：SSH exec、exec-multi、ping、SFTP、PTY sessions、port forwarding、Playbooks
- 安全能力：风险评分、统一 policy-as-code、审批队列、审批端点、桌面审批弹窗、敏感命令脱敏、execution gate、执行限额、异常检测
- 运维能力：内置 SSH 连接保留、Webhook 通知、remote daemon registry、健康检查、指标、审计轮转
- 生态能力：SSH key 管理、团队配置导入导出、MCP 客户端模板、插件/Skill 分发文档
- 验收结果：当前本地回归包含 203 个 Rust lib 单测、29 个 CLI/MCP smoke 测试、57 个 daemon 集成测试，以及前端 `npm run build`、macOS `npm run tauri:build` 打包验证
- MCP 工具：51 个，详见 [skills.md](skills.md)

### 协作规则

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

### 阶段总览

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
| R | 发布与本机安装验证 | ✅ 已完成 | 高 | Codex |
| G | 观察面升级为控制面 | ✅ 已完成 | 高 | Codex |
| E | 生态与可靠性 | ✅ 已完成 | 中 | Codex |
| O | 异常监听与鉴权/存储加固 | ✅ 已完成 | 高 | Claude |
| H | 架构债与加固后续 | ✅ 已完成 | 中 | Codex |
| I | 配置面收口与运行时韧性 | ✅ 已完成 | 中 | Claude |
| J | 性能与效率优化 | ✅ 已完成 | 中 | Claude |
| K | 产品化与上线门槛 | ✅ 已完成 | 高 | Claude |

### 已完成阶段归档

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

### 路线图归档与后续处理原则

后续不再先堆底层能力，而是以真实使用场景驱动：每一阶段都先跑现有功能、记录 bug，再决定是否扩展功能。

执行原则：

- 先本机真实使用，再扩展：每个新功能阶段开始前，先用当前 CLI、daemon、MCP、桌面端完成一遍本机 SSH 工作流。
- bug 修复优先于新功能：真实工作流中发现的认证、权限、审计、执行、安全和 UI 问题优先进入修复队列。
- 功能必须有验收场景：新增功能需要同时给出 CLI/API/MCP 或 UI 至少一个可复现验收路径。
- 安全默认保守：涉及批量执行、凭证、审批绕过、远程 daemon 的能力必须默认最小权限。

当前状态：

- G 阶段已完成：Live Activity 已从观察面升级为控制面，覆盖 execution gate、执行限额、policy dry-run 和异常检测。
- R/K 阶段已完成：`v0.1.1` 发布、跨平台包验证、Windows 真机测试和上线门槛项均已收口。
- 前后端性能优化已完成到 J8；WebDAV 已排除本机 `known_hosts.json` 信任库同步，桌面国际化静态审计为 442 checked keys / 0 缺译 / 0 placeholder mismatch。当前路线图无剩余计划项。后续只按真实使用暴露的明确 bug、性能回归或发布运营事项另开任务，不再扩展新的大功能面。

### 真实测试服务器

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

### F1 · 真实环境试运行

目标：用现有功能覆盖一台真实可控主机，形成并处理首轮 bug 修复清单。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F1-1 | ✅ 已完成 | 高 | Codex | 建立真实服务器 fixture | 107.174.36.91 使用临时 key 完成 exec、ping、sftp 验证；临时 key 和远端 `/tmp` 目录已清理 |
| F1-2 | ✅ 已完成 | 高 | Codex | 跑完整 CLI 工作流 | host add/list、risk、exec、exec-multi、sftp、audit、doctor、daemon-backed session/forward 已记录 |
| F1-3 | ✅ 已完成 | 高 | Codex | 跑 MCP 工作流 | MCP `tools/list` 返回工具列表；`ssh_list_hosts`、`ssh_exec`、`ssh_audit`、`ssh_doctor` 在真实服务器通过；当前 MCP 基线工具数为 51 |
| F1-4 | ✅ 已完成 | 中 | Codex | 跑桌面端首次启动和打包验证 | `npm run tauri:build` 生成 `.app` 和 `.dmg`；macOS bundle 主入口 `agent2ssh-app` 首启 smoke 通过 |
| F1-5 | ✅ 已完成 | 高 | Codex | 输出 bug backlog | B1-B5 已记录并修复；后续新 bug 按影响等级进入修复 |

### Bug 修复队列

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| B1 | ✅ 已完成 | 高 | Codex | `AGENT2SSH_CONFIG_DIR` 文档存在但实现未生效 | `config_dir()` 优先使用非空 `AGENT2SSH_CONFIG_DIR`；单测 `store::tests::test_config_dir_uses_env_override` 通过 |
| B2 | ✅ 已完成 | 高 | Codex | CLI `session`/`forward` 状态只保存在单进程内 | CLI `session`/`forward` 默认通过 daemon HTTP API 管理长生命周期资源；真实服务器验证 open/list/write/read/close 和 forward add/list/rm 通过 |
| B3 | ✅ 已完成 | 高 | Codex | Tauri sidecar 名称与 Cargo package/bin 名称冲突 | CLI sidecar 改为 `agent2ssh-cli`；`scripts/prepare-sidecars.sh` 生成 Tauri 期望的 target-triple 文件名 |
| B4 | ✅ 已完成 | 中 | Codex | Tauri PNG 图标不是 RGBA，导致 macOS bundle 构建失败 | `32x32.png`、`128x128.png`、`128x128@2x.png` 转为 RGBA；`npm run tauri:build` 通过 |
| B5 | ✅ 已完成 | 高 | Codex | macOS bundle 主程序被 CLI 二进制污染，首次启动只打印 CLI help | Cargo package 改名为 `agent2ssh-app`，保留 lib crate `agent2ssh` 和 CLI bin `agent2ssh`；bundle `CFBundleExecutable=agent2ssh-app` 且首启 smoke 通过 |

### F2 · 主机与环境管理

目标：让 Agent2SSH 更适合管理多环境、多角色主机。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F2-1 | ✅ 已完成 | 高 | Codex | 主机分组与环境视图 | HostProfile 支持 `env`、`role`、`owner`；CLI `host list` 和桌面端 HostList 可按 env、role、owner、tag 过滤；`npm run build`、Rust check、lib test、CLI smoke 通过 |
| F2-2 | ✅ 已完成 | 中 | Qoder | 主机健康快照 | 批量采集 uptime、disk、memory、load、ssh latency，并写入本地快照 |
| F2-3 | ✅ 已完成 | 中 | Qoder | 主机配置变更预览 | team config import 前显示新增、修改、删除差异 |
| F2-4 | ✅ 已完成 | 中 | Qoder | SSH config 双向同步策略 | 明确 Agent2SSH 与 `~/.ssh/config` 的导入、覆盖、冲突处理规则 |

### F3 · 执行体验与 Runbook

目标：把一次性命令执行升级为可审计、可复用的运维流程。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F3-1 | ✅ 已完成 | 高 | Qoder | Playbook 参数化 | playbook step 支持参数、默认值、必填校验和 dry-run 展示 |
| F3-2 | ✅ 已完成 | 高 | Qoder | 执行计划预览 | 高风险或多主机执行前展示目标、命令、风险、预计影响 |
| F3-3 | ✅ 已完成 | 中 | Qoder | 批量执行策略 | 支持并发数、失败阈值、逐批 rollout、暂停/继续 |
| F3-4 | ✅ 已完成 | 中 | Qoder | 执行结果比较 | 多主机结果可按 exit code、stdout diff、stderr 聚合查看 |

### F4 · 审批与协作

目标：让高风险操作适合团队协作，而不是只适合单机个人使用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F4-1 | ✅ 已完成 | 高 | Qoder | 审批策略配置 | 按 host/tag/risk/command pattern 配置是否需要审批 |
| F4-2 | ✅ 已完成 | 高 | Qoder | 审批上下文增强 | 审批请求包含 diff、目标主机、历史执行、发起来源 |
| F4-3 | ✅ 已完成 | 中 | Qoder | 审批通知回调 | Slack/自定义 webhook 可跳转到认证后的审批页面 |
| F4-4 | ✅ 已完成 | 中 | Qoder | 操作备注与变更单号 | exec/playbook 支持 reason/change_id 并进入 audit |

### F5 · 远程 daemon 与多节点

目标：把 remote daemon 从“可路由”推进到“可运营”。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F5-1 | ✅ 已完成 | 高 | Qoder | remote daemon 连接诊断 | `agent2ssh doctor --daemon <alias>` 检查 TLS、token、health、version |
| F5-2 | ✅ 已完成 | 高 | Qoder | daemon 版本兼容检查 | CLI/MCP 调用远程 daemon 前提示协议或版本不兼容 |
| F5-3 | ✅ 已完成 | 中 | Qoder | remote daemon 权限范围 | 每个 remote 配置允许的 hosts/tags/commands 范围 |
| F5-4 | ✅ 已完成 | 中 | Qoder | 多 daemon 统一视图 | UI/CLI 可按 daemon 查看 host、health、metrics |

### F6 · 可观测与审计分析

目标：让 audit 和 metrics 变成定位问题、复盘操作的工具。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| F6-1 | ✅ 已完成 | 高 | Qoder | 审计查询增强 | 支持全文搜索、时间范围、主机组、命令模式组合过滤 |
| F6-2 | ✅ 已完成 | 中 | Qoder | 审计导出 | 支持 JSONL/CSV 导出，并保留脱敏策略 |
| F6-3 | ✅ 已完成 | 中 | Qoder | 指标趋势 | 展示执行量、失败率、风险分布、审批耗时趋势 |
| F6-4 | ✅ 已完成 | 低 | Qoder | 事件订阅 | 提供本地事件流供外部监控或自动化消费 |

### S1 · 当前变更收口

目标：把 F4-4 审计链路和最近发现的文档漂移彻底验收，避免“参数存在但审计未落盘”的回归。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S1-1 | ✅ 已完成 | 高 | Qoder | `exec-multi` 审计上下文测试 | 覆盖 CLI、daemon 或 MCP 至少一个入口；执行带 `reason`、`change_id` 的 `exec-multi` 后，`audit` 可查询到每个目标主机的对应字段 |
| S1-2 | ✅ 已完成 | 高 | Qoder | Playbook 审计上下文测试 | `playbook run` 支持 `reason`、`change_id`；每个 step 产生的 audit entry 都保留相同上下文 |
| S1-3 | ✅ 已完成 | 中 | Qoder | 清理测试 warning | `cargo test --no-default-features` 和 `cargo test --no-default-features --features daemon` 不再出现 `unused variable` / `dead_code` warning |
| S1-4 | ✅ 已完成 | 中 | Qoder | 最近修复记录归档 | 在 `CHANGELOG.md` 或本计划 Bug 队列记录 F4-4 审计链路修复、MCP 工具数修正、OpenAPI `/exec-multi` 响应修正 |

### S2 · 真实环境回归

目标：用真实服务器重新跑一遍 CLI、daemon、MCP 的高频路径，确认 F2-F6 已完成能力可实际使用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S2-1 | ✅ 已完成 | 高 | Qoder | 真实服务器 CLI 回归 | 使用临时 key 和隔离 `AGENT2SSH_CONFIG_DIR` 跑 `host add/list`、`exec`、`exec-multi --reason --change-id`、`playbook run --reason --change-id`、`audit --format jsonl/csv`；测试结束清理远端 `/tmp/agent2ssh-*` 和临时 key |
| S2-2 | ✅ 已完成 | 高 | Qoder | daemon HTTP 回归 | 启动本地 daemon，验证 `/exec`、`/exec-multi`、`/playbooks/run`、`/audit`、`/audit/export`、`/health-snapshot` 返回结构与 `docs/api.yaml` 一致 |
| S2-3 | ✅ 已完成 | 高 | Qoder | MCP 回归 | 通过 stdio 调用 `tools/list`、`ssh_exec_multi`、`ssh_playbook_run`、`ssh_audit_export`、`ssh_doctor`；S2 当时确认工具数为 50，当前 MCP 基线为 51 |
| S2-4 | ✅ 已完成 | 中 | Qoder | 回归记录输出 | 在 `docs/` 下新增或更新真实回归记录，包含命令、配置隔离方式、结果摘要、发现的问题和清理证明 |

### S3 · 文档与契约一致性

目标：把 README、`docs/skills.md`、`docs/api.yaml`、MCP schema、daemon handler 的漂移变成可检测问题。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S3-1 | ✅ 已完成 | 中 | Qoder | MCP 工具文档一致性检查 | 增加脚本或测试，比对 MCP `tools/list` 工具名与 `docs/skills.md` 表格；工具新增/删除时测试失败并提示更新文档 |
| S3-2 | ✅ 已完成 | 中 | Qoder | OpenAPI 与 daemon 契约检查 | 为高频端点维护最小 schema/fixture 检查，优先覆盖 `/exec`、`/exec-multi`、`/playbooks/run`、`/audit/export` |
| S3-3 | ✅ 已完成 | 低 | Qoder | README 去重策略 | README 保留入口摘要和核心工具概览，完整 MCP 工具表以 `docs/skills.md` 为准，减少双处维护 |
| S3-4 | ✅ 已完成 | 中 | Qoder | CLI help 与文档对齐 | 抽样验证 `agent2ssh --help`、`exec-multi --help`、`playbook run --help` 与 README/guide 中的参数一致 |

### S4 · 发布前质量门槛

目标：形成一套发布前必须通过的固定检查，确保桌面端、CLI、daemon、MCP 和文档处于可发布状态。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S4-1 | ✅ 已完成 | 高 | Qoder | 固定发布验收命令 | `npm run build`、`cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp`、`cargo check --no-default-features --features daemon --bin agent2ssh-daemon`、两套 `cargo test` 全部通过 |
| S4-2 | ✅ 已完成 | 高 | Qoder | 桌面包构建验证 | `npm run tauri:build` 可生成 `.app` / `.dmg`；macOS bundle 主入口仍为 `agent2ssh-app` |
| S4-3 | ✅ 已完成 | 中 | Qoder | 安装校验脚本回归 | `scripts/verify-install.sh`、`scripts/prepare-sidecars.sh`、`scripts/generate-checksums.sh` 在当前版本可执行并输出预期结果 |
| S4-4 | ✅ 已完成 | 中 | Qoder | 发布材料准备 | 更新 `CHANGELOG.md`、`docs/release-checklist.md` 和版本说明；列出已知限制和真实环境回归结果 |

### S5 · Agent Activity 可观测性闭环

目标：让不同 agent 入口打开的 PTY session 进入统一 daemon registry，并在桌面端 Live Agent Activity 中具备可归因、可观察、可接管的基础。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S5-1 | ✅ 已完成 | 高 | Codex | MCP session 默认路由到 local daemon | `ssh_session_open/write/read/close/list` 优先使用 `127.0.0.1:7722` daemon session API；daemon 不可用时回退到 MCP 进程内 session；`cargo check --no-default-features --bin agent2ssh-mcp`、MCP stdio smoke 和 MCP 枚举测试通过 |
| S5-2 | ✅ 已完成 | 高 | Codex | 标准来源字段 | `ExecRequest`、`AuditEntry`、daemon session events、daemon exec/playbook bodies 支持 `source`；CLI/MCP/daemon/desktop 默认来源分别为 `cli`、`mcp`、`daemon`、`desktop`，并允许 `AGENT2SSH_SOURCE` 覆盖；Rust checks 和 lib tests 通过 |
| S5-3 | ✅ 已完成 | 中 | Codex | Live Activity 过滤与展开 | UI 可按 source、事件类型和文本搜索过滤；事件可展开查看 time、host、session、change_id 和原始 payload；`npm run build` 通过，Browser 验证过滤控件可渲染 |
| S5-4 | ✅ 已完成 | 高 | Codex | 高风险非前端来源提醒 | Live Activity 对非 `desktop` 来源的 high/blocked/approval 事件显示本地提醒条；不改变后端审批边界；`npm run build` 通过 |
| S5-5 | ✅ 已完成 | 高 | Codex | 敏感输出策略 | session/output/exec preview 统一经过 `redact_sensitive_text`，覆盖 token、password、Authorization/Bearer、cookie 和 private key；bounded preview 继续保留截断边界；lib tests 和 daemon check 通过 |

### S6 · 真实会话回归

目标：用真实服务器验证 S5 的 daemon session registry、source 归因、Live Activity SSE 事件和敏感 preview 脱敏在端到端链路中实际可用。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S6-1 | ✅ 已完成 | 高 | Codex | MCP session daemon registry 回归 | 使用隔离 `AGENT2SSH_CONFIG_DIR` 和真实服务器，`ssh_session_open/write/read/close/list` 返回 `backend: "daemon"`；daemon `/sessions` 可见打开的 session，关闭后为空 |
| S6-2 | ✅ 已完成 | 高 | Codex | Live Activity SSE 事件回归 | `/events/stream` 捕获 `session_opened`、`session_input`、`session_output`、`session_closed`，并携带 `source: "claude-code"` |
| S6-3 | ✅ 已完成 | 高 | Codex | source 与 audit 回归 | `AGENT2SSH_SOURCE=opencode` 的 CLI exec 写入 audit JSON/CSV，`source` 字段落盘并导出 |
| S6-4 | ✅ 已完成 | 高 | Codex | 敏感 preview 脱敏回归 | SSE preview 中 `Authorization: Bearer ...` 被替换为 `[REDACTED]`，测试 secret 未出现在事件 payload summary |
| S6-5 | ✅ 已完成 | 高 | Codex | 回归报告与清理证明 | 新增 `docs/reports/REGRESSION-LOG.md#s6-真实会话回归`；远端临时 key 和 `/tmp/agent2ssh-s6-*` 清理完成，本地 daemon 停止 |

### S7 · 桌面 Session 接管

目标：让桌面端 SessionPanel 不只打开自己的进程内 PTY，而是优先连接 daemon session registry，直接接管 MCP/CLI/daemon 创建的 daemon-managed sessions。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S7-1 | ✅ 已完成 | 高 | Codex | daemon session API 前端封装 | `src/api.ts` 新增 `sessionOpenDaemon/write/read/close/list`，所有请求继续使用 Bearer token，并写入 `source: "desktop"` |
| S7-2 | ✅ 已完成 | 高 | Codex | SessionPanel daemon registry 列表 | `SessionPanel` 优先轮询 daemon `/sessions`，显示 daemon sessions；daemon 不可用时回退 Tauri 本地 `session_list` |
| S7-3 | ✅ 已完成 | 高 | Codex | 接管已有 session | session 列表支持 Attach；接管后可 read/write/close 原 daemon session，并保留本地 fallback session 操作 |
| S7-4 | ✅ 已完成 | 中 | Codex | UI 状态与布局 | 面板显示 registry/backend 状态、active session 元信息和来源标识；按钮尺寸稳定，窄面板不挤压文本 |
| S7-5 | ✅ 已完成 | 高 | Codex | 验证 | `npm run build` 通过；计划和架构文档同步 |

### S8 · Session 接管体验与安全

目标：在 S7 的 daemon session 接管基础上，把日常使用所需的持续读取、只读观察和危险输入保护补齐。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S8-1 | ✅ 已完成 | 中 | Codex | 自动 tail | active session 支持 `Tail` 开关，每 2 秒读取一次输出；使用并发 guard 避免重叠 read |
| S8-2 | ✅ 已完成 | 高 | Codex | 只读接管 | session 列表提供 read-only attach；active session 可切换 read-only，禁止写入输入 |
| S8-3 | ✅ 已完成 | 高 | Codex | 危险输入确认 | 发送前调用现有 risk classifier；`high`/`blocked` 输入显示确认条，用户显式确认后才写入 PTY |
| S8-4 | ✅ 已完成 | 中 | Codex | UI 状态稳定 | Tail、Read-only、危险确认和接管按钮有稳定尺寸和可访问标签；窄面板下文本截断不挤压操作按钮 |
| S8-5 | ✅ 已完成 | 高 | Codex | 验证 | `npm run build` 通过；Browser 渲染检查通过；`git diff --check` 通过 |

### S9 · 0.1.1 发布前收口

目标：在真正打 `v0.1.1` tag 前，把版本、发布说明和本地质量门槛收齐，避免 tag 推送后才发现可避免的发布问题。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| S9-1 | ✅ 已完成 | 高 | Codex | 版本字段一致性 | `Cargo.toml`、`package.json`、`package-lock.json`、`tauri.conf.json`、`docs/api.yaml`、`scripts/agent2ssh.rb` 均为 `0.1.1` |
| S9-2 | ✅ 已完成 | 高 | Codex | 发布说明收口 | `CHANGELOG.md` 合并为单一 `0.1.1` 发布段，覆盖 S1-S8 主要交付 |
| S9-3 | ✅ 已完成 | 高 | Codex | 本地质量门槛 | `npm run build`、两套 `cargo check`、两套 `cargo test`、`git diff --check` 通过 |
| S9-4 | ✅ 已完成 | 中 | Codex | 发布前报告 | 新增 `docs/s9-release-preflight-report.md`，记录版本状态、质量门槛和剩余发布动作 |
| S9-5 | ✅ 已完成 | 中 | Codex | tag 状态确认 | 本地 `v0.1.1` tag 尚不存在；S9 不创建 tag，留给最终发布动作 |

### 收口结论

S1-S9、G、O、R、H、I、J、K 阶段已完成，0.1.1 发布、Windows 真机验证和前后端性能收口均已完成。当前无剩余路线图计划：

1. 前端/后端只在真实使用中发现明确性能回归或 bug 时追加任务。
2. E 阶段已完成；生态/可靠性后续仅按真实本机使用暴露的问题追加 E4+。
3. 平台差异不再作为宽泛验证债处理，只按明确 bug 进入修复队列。

### 安全可视化后续

Agent2SSH 已开始从“agent 可调用 SSH 能力层”扩展为“本机 SSH 操作观察面”。当前 Live Agent Activity 面板覆盖 daemon SSE 实时事件和本地 audit 补偿；S5/S6 已完成：

1. MCP session 默认路由到 local daemon registry，使 Claude Code、Codex、opencode 等 agent 打开的 PTY 能被桌面端实时观察。
2. CLI/MCP/daemon/desktop 均具备标准 `source` 字段或 `AGENT2SSH_SOURCE` 覆盖。
3. Live Activity 支持过滤、展开、敏感 preview 脱敏和高风险外部来源提醒。
4. SessionPanel 可列出并接管 daemon-managed sessions，支持读取、写入和关闭来自统一 registry 的 PTY。
5. SessionPanel 支持自动 tail、只读观察和高风险 PTY 输入二次确认。

### 长远路线图（0.1.1 之后）

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
   ├─ R 发布与本机安装验证 ← 已完成
   ├─ H 架构债与加固后续   ← 已完成
   └─ G 观察面→控制面      ← 已完成，后续按本机使用反馈迭代
   E 生态与可靠性           ← 已完成，后续仅追加明确回归项
```

### G · 观察面升级为控制面

目标：当多个 agent 并发操作时，人类能在 daemon 层实时干预——暂停、限额、按策略拒绝、对异常行为告警。该阶段已完成，后续仅按本机使用反馈继续迭代。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| G1-1 | ✅ 已完成 | 高 | Codex | 全局急停 gate（daemon 层） | daemon 维护 `execution_gate` 状态（active/paused）；paused 时所有非 `desktop` 来源的 `/exec`、`/exec-multi`、`/playbooks/run`、session write 和 WebSocket exec 被拒，HTTP 入口返回 423 并写入 audit gate 拒绝事件；`desktop` 来源仍可操作以便恢复 |
| G1-2 | ✅ 已完成 | 高 | Codex | 急停 CLI 与桌面入口 | `agent2ssh pause` / `resume` / `status` 可切换并查询 gate；桌面端提供急停按钮和当前 gate 状态指示；MCP 暴露只读 `ssh_gate_status` |
| G1-3 | ✅ 已完成 | 中 | Codex | 急停回归验证 | paused 状态下 daemon/MCP 非 desktop 执行被拒且 audit 落盘，resume 后恢复；新增 `docs/reports/REGRESSION-LOG.md#g1-gate-回归` |
| G2-1 | ✅ 已完成 | 高 | Codex | 速率与并发限额配置 | `execution_limits.toml` 定义 per-source / per-host / per-tag 的每窗口最大执行数与最大并发 session 数；缺省值保守且可覆盖，详见 `docs/guides/configuration-guide.md` |
| G2-2 | ✅ 已完成 | 高 | Codex | 限额强制与拒绝审计 | 超限请求在 daemon 层返回 429 并写入 blocked audit；限额计数按滑动窗口；并发 session 上限阻止新建 session；新增 `docs/reports/REGRESSION-LOG.md#g2-limits-回归` |
| G3-1 | ✅ 已完成 | 高 | Codex | 策略即代码收敛 | 新增统一 `policy.toml` / `policy.json`，将 risk rules 与 approval policies 收敛到单一可版本化文件；运行时优先读取统一 policy，缺失时兼容旧 `risk_rules.toml` / `approval_policies.toml` |
| G3-2 | ✅ 已完成 | 中 | Codex | 策略校验与 dry-run | 新增 `agent2ssh policy validate [--path]` 校验统一 policy 语法，`agent2ssh policy test <cmd> --host <host>` 输出 allow/approve/block；CLI smoke 覆盖统一 policy validate/test |
| G4-1 | ✅ 已完成 | 中 | Codex | 异常行为基线检测 | 新增 `anomaly.toml` 可调阈值；audit append 后按滑动窗口检测 source 频率突增、敏感命令模式和非常规时段高危操作；发布 `anomaly_detected` 事件并支持复用 webhook |
| G4-2 | ✅ 已完成 | 低 | Codex | 异常检测可视化 | Live Activity 标注 `anomaly_detected` 事件，展示异常类型、严重度和原因；异常序列由单元测试和 CLI/MCP/daemon audit 补偿路径覆盖 |

### R · 发布与本机安装验证

目标：确保产品在本机和跨平台包形态下可安装、可启动、可回归。Agent2SSH 定位为单机工具，路线图不再包含采用扩张目标。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| R1 | ✅ 已完成 | 高 | Codex | 跨平台桌面包真实验证 | release CI 已生成 macOS/Linux/Windows 桌面包；macOS 本机重新打包生成 `.app` 和 `.dmg`；本机回归覆盖内置 SSH exec/SFTP/PTY/forward 基线；Windows 真机测试已于 2026-06-22 由用户确认完成，平台差异后续只按明确 bug 队列处理 |
| R2 | ✅ 已完成 | 高 | Codex | 完成 0.1.1 发布动作 | `v0.1.1` tag 已推送到 GitHub/git233；release CI run `27638444133` 通过并上传 CLI tarballs、checksums、macOS/Linux/Windows 桌面包；`scripts/agent2ssh.rb` 已回填 macOS arm64、macOS x86_64、Linux x86_64 sha256；发布 tarball checksum 校验通过；使用 macOS arm64 release tarball 跑通 `scripts/verify-install.sh`（7 passed, 0 failed） |
| R3 | ✅ 已完成 | 中 | Codex | 本机接入剧本与反馈入口 | 新增 `docs/guides/external-user-10min.md`，覆盖 CLI host import/add、低风险 exec 验证、Codex/Claude-style MCP 配置、反馈脱敏；新增 GitHub bug/adoption issue 模板；明确 `v0.1.1` 默认无自动遥测，匿名反馈为手动 opt-in，未来运行时遥测必须默认关闭且不采集命令/主机/输出/凭据 |
| R5 | ✅ 已完成 | 中 | Codex | 桌面控制面调研 | 确认 Settings menu 适合作为本地 operator surface；已落地 daemon health、daemon start/stop/restart、setup wizard daemon start、execution gate、Web Console URL 控制闭环；2026-06-18 回归复测通过 `npm run build`、`cargo test`、`npm run tauri:build`；详见 `docs/reports/REGRESSION-LOG.md#r5-desktop-control-plane-调研` |

### E · 生态与可靠性

目标：已完成当前生态与可靠性补强；后续仅在本机使用回归暴露明确问题时追加新任务。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| E1 | ✅ 已完成 | 中 | Codex | 多 agent 集成验证 | 新增 `scripts/e1-mcp-client-smoke.py` 和 `docs/reports/REGRESSION-LOG.md#e1-多-agent-接入验证报告`，用 MCP stdio 协议分别模拟 `codex`、`opencode`、`cursor`、`claude-code` source，验证 initialize、51 工具枚举和 `ssh_risk_check` blocked 判定；真实客户端 UI 行为留给本机使用回归 |
| E2 | ✅ 已完成 | 中 | Codex | 可靠性与规模 | 新增 `scripts/e2-scale-plan-smoke.py` 和 `docs/reports/REGRESSION-LOG.md#e2-可靠性与规模报告`，在隔离配置中生成 100 个 synthetic host 并跑通 `exec-multi --plan`；新增 100 host plan Rust 回归与 1000 event burst 事件总线回归；真实 100 台 SSH/多 daemon 压测不再作为当前路线图剩余项，若后续具备外部压测环境则另立专项 |
| E3 | ✅ 已完成 | 中 | Codex | 契约一致性接入 CI | `.github/workflows/ci.yml` 新增 `contract-consistency` job，在 PR、push 和 release 入口显式运行 S3 的 `docs/skills.md` vs MCP 工具、OpenAPI/daemon schema fixture、CLI help 参数一致性检查；`build` matrix 和 release-only `tauri-bundle` job 依赖该 job，契约漂移会先于跨平台构建/打包失败 |

### O · 异常监听与鉴权/存储加固

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

### H · 架构债与加固后续

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

### I · 配置面收口与运行时韧性

目标：H 阶段把 daemon 监听地址、依赖层日志、错误聚合等做成可配置/可观测后，暴露出"配置只改了一半"和"运行时收尾缺失"两类缺口——bind 侧可配但 client 侧仍硬编码、新增 env 无集中文档、daemon 无优雅退出导致 stale pid。本阶段把这些收口，让 H 的可配置项端到端可用、可运维、可回归。排序原则不变：可达性/可运维优先，文档与回归补齐，审计类延后。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| I1 | ✅ 已完成 | 中 | Claude | 本地 daemon URL 解析统一 | 新增核心 helper `local_daemon_addr`/`local_daemon_connect_addr`/`local_daemon_url`（`remote.rs`，读 `AGENT2SSH_DAEMON_ADDR`、缺省 `127.0.0.1:7722`，通配 `0.0.0.0`/`::` 自动回退回环、IPv6 加括号）。CLI（doctor/health）、MCP（health/metrics）、`remote.rs`（`list_daemons`/`get_daemon`）、`daemon_control`（health 探测改 `to_socket_addrs`）、`notify`（console 链接）、daemon（自身 action_url + bind 复用 `local_daemon_addr`）全部改用 helper。含 `normalize_connect_addr`/env override 单测；契约/smoke/集成回归通过 |
| I2 | ✅ 已完成 | 中 | Claude | daemon 优雅退出与 PID 清理 | `axum::serve(...).with_graceful_shutdown(shutdown_signal())`：`shutdown_signal` 监听 `ctrl_c` + unix `SIGTERM`（Windows 仅 `ctrl_c`，`tokio` 新增 `signal` feature），退出时移除 `daemon.pid` 并记一条 info 诊断，不再因被信号杀死而残留 stale pid。daemon check + 集成回归通过 |
| I3 | ✅ 已完成 | 低 | Claude | 环境变量集中文档 | `configuration-guide.md` 新增「环境变量」表，列全量内置 `AGENT2SSH_*`（`CONFIG_DIR`/`SOURCE`/`DAEMON_ADDR`/`TRACE_ID`/`LOG`/`LOG_FORMAT`/`BRIDGE_DEPS`，含作用域/默认值/说明），并澄清 `token_env` 引用的是用户自定义变量（非内置）；`architecture.md` 的 Diagnostics 段补 `AGENT2SSH_BRIDGE_DEPS`、Control Plane 段补 `AGENT2SSH_DAEMON_ADDR` 解析与优雅退出 |
| I4 | ✅ 已完成 | 低 | Claude | 监听地址端到端回归 | 新增 `daemon_honors_configured_listen_address_end_to_end`（`daemon_integration.rs`）：预留随机空闲端口写入 `AGENT2SSH_DAEMON_ADDR`，起真实 axum `/health` 服务绑到 `local_daemon_addr()`，再经 `daemon_health_ok()`（走 I1 resolver）跑通，断言 resolver/URL 一致；非回环 warn 决策由 lib 化的 `is_loopback_addr` 单测覆盖。daemon feature 下测试通过 |
| I5 | ✅ 已完成 | 中 | Claude | 配置热加载一致性审计 | 产出 `docs/reports/REGRESSION-LOG.md#i5-配置热加载一致性审计`：盘点 11 个配置文件的读热度/写入方/失效语义，给出纳入/保持读盘的结论（含 `execution_gate` 保持读盘以优先急停新鲜度的判断）。落地 `hosts.json` 接入 `ConfigCache`——`load_config` 走缓存、`save_config_unlocked`（全部写入唯一漏斗）成功后 `invalidate`，新增 `load_config_reflects_saved_hosts_via_cache` 单测验证写后不返回陈旧值。两套 check、两套 test 全绿 |

### J · 性能与效率优化

目标：在功能基本铺齐后，针对随数据量增长会变慢的热路径做一轮效率优化——配置/审计的重复读盘解析、前端大列表的全量渲染、以及刚落地的 SFTP 面板里"只能传文件、进度只数文件个数"的粗糙处。排序原则：每次操作都走的热路径优先，前端可感知卡顿次之，功能补全垫后。每项都要带量化或回归验收，避免"优化"引入正确性回退。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| J1 | ✅ 已完成 | 中 | Claude | policy.toml 热路径缓存 | `load_policy_file` 接入 `ConfigCache`（按解析后的 `policy.toml`/`policy.json` 路径为键，无文件时回退 `policy.toml` 路径键，使"无 policy"探测也被记忆化），`save_policy_approval_policies` 写后 `invalidate`；"policy 只升级风险"语义不变。新增 `load_policy_file_reflects_saves_via_cache`（无→建→存三段验证写后不陈旧）。两套 check、两套 test 全绿 |
| J2 | ✅ 已完成 | 中 | Claude | 审计日志按需读取 | `list_audit_raw` 改为反向（newest-first）扫描 + 早停：到达 `filter.limit` 即停（与旧"全解析→reverse→truncate"等价，但常见"最近 N 条"不再解析整文件）；并利用审计 append 即 `ts=now()` 的时间有序性，遇到 `ts<since` 即停（`compute_metrics_trend` 的 since 窗口因此也有界）。`matches` 仍复核所有条件，早停只提前停止、不改结果。新增 5000 行合成日志回归（limit/host/since 三种过滤断言结果精确一致）。两套 test 全绿 |
| J3 | ✅ 已完成 | 中 | Claude | 前端大列表渲染优化 | SFTP 列表是真正无界的来源（远端目录可上万条），加 `viewCap`（每侧每次最多挂载 400 行 + "显示更多"，导航/刷新重置）；`AuditPanel` 加 `renderCap`（200 + 显示更多）兜住 limit 被调大的情况。`DiagnosticsPanel` 不存在（诊断日志在 `SettingsMenu`，后端硬上限 1000，已有界，未改）。`tsc --noEmit`、`npm run build` 通过 |
| J4 | ✅ 已完成 | 中 | Claude | SFTP 目录递归传输 | 后端新增 `sftp_walk_core`（远端递归 readdir，跳过 symlink + 深度上限 64 防环路，parents-before-children）与 `local_walk`/`local_mkdir`（本地遍历/建目录，含 `local_walk_inner` 单测）。前端每行加勾选框（文件夹也可选/可拖），传输前 `buildTransferUnits` 把选中目录递归展开为「目标侧待建目录 + 逐文件单元」，先 `mkdir -p` 再逐文件 upload/download/exchange，三方向通吃；进度/字节统计/覆盖确认沿用。`local mkdir` 也接通（原"去文件管理器建"提示移除）。`tsc`/`npm build`/fmt/两套 check/三套 test（tauri lib 185）全绿；运行时问题后续只按明确 bug 处理 |
| J5 | ✅ 已完成 | 低 | Claude | SFTP 真实字节进度 | `SftpResult` 新增 `bytes`（`#[serde(default)]`），upload/download core 从 `std::io::copy` 返回值取已传字节；exchange 取 `uploaded.bytes`。前端进度条改为：选区已知大小求和得 `bytesTotal`，逐文件累加 `bytesDone`，有总量时进度按字节推进并显示 `X / Y`，否则回退按文件个数。`tsc`/`npm build`/两套 check/两套 test 通过；字节值以后续真实使用中的明确异常按 bug 处理 |
| J6 | ✅ 已完成 | 中 | Codex | 前端 audit 按需刷新 | `App.refresh()` 不再在 host/proxy/group 刷新时同步读取 `audit.jsonl`；新增 `refreshAudit()` 只在进入 Audit 页、执行完成和审计手动刷新时调用。Live Activity 改为自维护最近 audit 补偿，不再订阅 App 级 audit state，减少无关模块切换和 host 编辑时的日志 I/O |
| J7 | ✅ 已完成 | 中 | Codex | Live Activity 高频事件批处理 | daemon SSE 事件进入 100ms 前端队列后批量 `setEvents`，避免批量执行/事件 burst 时每条事件触发一次 React render；audit 补偿轮询从 3s 降到 10s，Activity 页面未挂载时不再产生补偿轮询 |
| J8 | ✅ 已完成 | 高 | Codex | 后端审计倒序分块读取 | `list_audit_raw` 从整文件 `read_to_string` 改为 64KiB 倒序分块扫描；常见最近 N 条、since 窗口和 `limit=0` 查询不再把完整 `audit.jsonl` 读入内存。验证：`npm run build`、`cargo fmt --check`、`cargo test --no-default-features --lib`、`cargo test --no-default-features --test cli_smoke`、`cargo test --no-default-features --features daemon --test daemon_integration` 通过 |

### K · 产品化与上线门槛

目标：功能广度已基本齐全，本阶段收口"能不能作为产品发给真人用"的硬门槛——凭据安全、发布信任链、跨平台完整性、真机验证，以及可靠性/体验打磨。来源是一次架构缺口评估（按"距离功能健全产品还缺什么"盘点，均已对照代码核实）。排序原则：决定"能否上线/能否信任"的安全与分发优先，跨平台与真机测试次之，体验/运维打磨垫后。

| 任务 | 状态 | 优先级 | 负责人 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| K1 | ✅ 已完成 | 高 | Claude | 凭据接入 App 自建加密存储 | **不走 OS 钥匙串**，改为产品自建：`secrets.rs` 用 Argon2id 从**主密码**派生 256-bit key、AES-256-GCM 加密落 `~/.agent2ssh/secrets.enc`（0600），磁盘无明文 key；`hosts.json` 只留 `$agent2ssh-secret$` 句柄。解锁后 key 缓存进程内（Argon2 仅解锁时跑一次）。解锁：桌面启动弹 `SecretsUnlock` 对话框（`secrets_status`/`secrets_unlock`/`secrets_change_password` 命令 + Settings 设/改主密码）；CLI/MCP/daemon 读 `AGENT2SSH_MASTER_PASSWORD`（CLI 另加 `secrets status`/`secrets set-password`）。锁定安全：`internalize` 锁定时保留句柄不清空（save 不会孤立密文）、`embedded_ssh` 把裸句柄当「无密码」跳过密码认证（密码型主机锁定时不可用，by design）；`externalize` 锁定遇真实明文时保留明文+告警而非中断无关 save。`migrate_plaintext_secrets` 仅解锁后迁移旧明文；删除主机/代理与改名清理句柄。`memory` 测试后端（cfg(test) 默认）使单测无需主密码。含单测（真实 Argon2+AES 初始化/解锁/错密码拒绝/落盘无明文、锁定返回 None+store 报错、句柄落盘、迁移、改名清孤儿）+ CLI 真跑冒烟（status 不创建文件、写时初始化、密文无明文）。Windows 文件权限 ACL 已随 K2 真机确认 |
| K2 | ✅ 已完成 | 高 | Claude | Windows 文件权限加固 | `restrict_file_to_owner` 加 `#[cfg(windows)]` 分支：`icacls /inheritance:r /grant:r <user>:(F)` 去继承 + 仅当前用户 Full control，与 Unix `0600` 对齐（`daemon.token`/`keys/`/`hosts.json`）。Windows 真机冒烟已于 2026-06-22 由用户确认完成 |
| K3 | ✅ 已完成 | 高 | Claude | 代码签名/公证 + 自动更新 | `tauri-plugin-updater`（Rust 注册 + `@tauri-apps/plugin-updater`/`plugin-process` npm + `src/lib/updater.ts` 签名校验 check/download/install + Settings「检查更新/安装更新」）。`tauri.conf.json` 加 `createUpdaterArtifacts`、`macOS`（hardenedRuntime + `entitlements.plist`）、`windows`、`plugins.updater`（endpoints + pubkey 占位）。CI `tauri-bundle` 加 Apple 证书导入步骤 + notarization/Windows 签名环境变量。`updater:default` 入 capabilities。证书、真实公证、灰度发布端与 pubkey 替换属于发布运营配置，不再作为当前路线图剩余项 |
| K4 | ✅ 已完成 | 高 | Claude | 真机 SSH E2E（容器化 sshd） | 新增 `scripts/e2e-docker.sh`：起 `linuxserver/openssh-server`（密钥认证，绕开 K1 密码凭据路径）跑真实 exec / SFTP 1MiB 往返字节比对 / mkdir+ls / J4 递归树往返 / K6 resume 续传；CI 加 `real-ssh-e2e` job（ubuntu）。`bash -n` 通过；本地 Docker 环境差异不再作为当前路线图剩余项 |
| K5 | ✅ 已完成 | 中 | Claude | 连接自愈 | `connection.rs` 重构：session 存 `Arc<StdMutex<Option<Session>>>` + `ConnectionHealth`；建连设 `set_keepalive(15s)`；全局 supervisor 任务每 30s `keepalive_send` 探活，失败标记 unhealthy 并按指数退避（5s→300s）`connect_embedded_ssh` 重连。`ConnectionStatus` 加 `healthy`/`reconnecting`/`last_error`（serde default 向后兼容），`HostList` 点颜色区分 健康/失效/重连。含 `backoff_grows_then_caps` 单测 |
| K6 | ✅ 已完成 | 中 | Claude | SFTP 传输健壮性 | 新增 `sftp_transfer.rs`：取消注册表（transfer_id→AtomicBool）+ `copy_cancellable`（64K 分块、按块查取消）+ `resume_offset` 决策。upload/download core 接入 resume（upload 远端 stat 长度 + `open_mode(WRITE|APPEND)` + 本地 seek；download 本地长度 + 远端 seek + 本地 append）与取消（`transfer_id`）。请求类型加 `resume`/`transfer_id`（serde default）；CLI `--resume`；Tauri `sftp_cancel` + 前端每文件 transfer_id + 取消按钮。**可选并发**：前端 SFTPPanel 加「并行传输」开关（默认关，开后 worker 池并发上限 `PARALLEL_TRANSFERS=4`），取消按钮按 `activeTransferIds` 集合中止所有在途文件，首个失败置 `aborted` 停止取新文件。daemon 启动日志明确告知 session/forward/在途传输不跨重启。含 4 单测 |
| K7 | ✅ 已完成 | 中 | Claude | 跨平台路径与行为打磨 | 前端 `localJoin` 识别 Windows 路径（含反斜杠/盘符）改用 `\` 拼接并转换子路径分隔符（`basenameOf` 本已双分隔符）。后端 `expand_local_path` 接受 `~\`。复核：daemon 信号 `shutdown_signal` 已 `cfg(unix)` 门控、loopback 已在 `remote.rs` 处理。Windows 真机冒烟已于 2026-06-22 由用户确认完成 |
| K8 | ✅ 已完成 | 低 | Claude | 配置版本化/迁移/自动备份 | `AppConfig` 加 `schema_version`（`CONFIG_SCHEMA_VERSION=1`）；`migrate_config` 向前兼容（未来版本不降级）；`normalize_config` 写时盖章（取 max 不降级）；`save_config` 写前把旧文件复制为 `hosts.json.bak`（原子 rename 已有，bak 防坏内容）。含 4 单测（盖章/幂等/不降级/备份） |
| K9 | ✅ 已完成 | 低 | Claude | 鉴权侧信道核查 | `token_matches` 改用 `subtle::ConstantTimeEq`（替换手写折叠，语义不变：空 expected 永不匹配）。复核：服务端唯一校验点即此处（scoped token 也经此），webhook 仅出站签名无入站校验。含 3 单测 |
| K10 | ✅ 已完成 | 低 | Claude | 体验与运维打磨 | i18n 审计脚本最新确认 442 checked keys / 0 缺译 / 0 placeholder mismatch（含 SSH fingerprint、WebDAV Sync、MCP 解绑、Sync 模块 label 和 ErrorBoundary 恢复页）。a11y：Settings 已 Escape 关闭、新增控件均为原生 `<button>`/`<label><input>`。新增 `telemetry.rs`：opt-in（默认关）本地遥测（`telemetry.toml` 开关 + `telemetry.jsonl` 2MiB 上限，无网络导出），panic hook 接入 crash 事件（关时 no-op）；Tauri get/set 命令 + Settings 复选框。含单测（默认关、开后落盘、可关） |

> Phase K 收口（2026-06-21，Claude）：K1–K10 全部落地。验证：`cargo test --no-default-features` 全绿（lib 195 + daemon-feature 28 + integration daemon 57 + daemon bin 18）；CLI/MCP/daemon/tauri 四套 `cargo check` 通过；`cargo fmt --check` 干净；`npm run build`/`tsc --noEmit` 通过；i18n 0 缺译。2026-06-22 用户补充确认 Windows 真机测试已完成，覆盖 Windows ACL 与路径冒烟。后续补充验证：Rust lib 203、CLI/MCP smoke 29、daemon integration 57、i18n 442 checked keys / 0 缺译 / 0 placeholder mismatch、`npm run tauri:build` 重新生成 `.app` 和 `.dmg`。K3/K4 涉及的证书、公证、灰度发布端和本地 Docker 环境差异已归类为发布运营或环境事项，不再作为路线图剩余计划。

## 附录：Plan 2 Q1/Q2 执行报告

## Plan 2 Q1/Q2 Execution Report

Date: 2026-06-26

### Scope

This report records the first execution pass against `plan2.md`, focused on:

- Q1 release confidence and local quality gates.
- Feasible local parts of Q2 credential encryption and WebDAV sync regression.

Q3 external adoption, cross-platform install smoke, real WebDAV server push/pull, and multi-device recovery remain external validation items.

### Q1 Results

### Completed

- Added Rust format, Clippy, and diff-whitespace checks to `scripts/e2e-local.sh`.
- Added the same format/Clippy/diff checks to `docs/release-checklist.md`.
- Fixed current Clippy blockers under `cargo clippy --no-default-features --all-targets -- -D warnings`.
- Verified macOS local Tauri packaging still produces `.app` and `.dmg` bundles.

### Clippy Fixes

The Clippy cleanup was intentionally mechanical:

- Introduced a `ConnectionHandleSnapshot` type alias for retained-connection supervision snapshots.
- Removed redundant branches and guards.
- Replaced unnecessary `sort_by`, `iter().any`, `vec!`, `clone`, `Ok(...?)`, and `return` patterns.
- Moved `keys.rs` tests to the end of the file to satisfy item-order linting.
- Added a local `#[allow(clippy::enum_variant_names)]` only for the MCP tool enum because the `Ssh*` prefix mirrors exported MCP tool names and avoids a large non-behavioral rename.

### Validation Commands

Passed:

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
./scripts/e2e-local.sh
npm run tauri:build
```

`npm run tauri:build` produced:

- `src-tauri/target/release/bundle/macos/Agent2SSH.app`
- `src-tauri/target/release/bundle/dmg/Agent2SSH_0.2.1_aarch64.dmg`

Notarization was skipped because Apple notarization credentials were not configured in the local environment.

### Remaining Q1 Notes

- Frontend has no dedicated ESLint setup today. The current frontend static gate remains `npm run build` (`tsc && vite build`). Adding ESLint should be a separate explicit change because it will introduce new dependencies and rule decisions.
- CI already covers contract consistency, Rust tests, Rust checks, frontend build, release binary builds, and release bundle jobs. Clippy is now enforced by local `e2e-local.sh` and the release checklist; adding it to CI should be considered after confirming cross-platform Clippy output is stable.

### Q2 Results

### Completed Locally

Credential-store CLI smoke with isolated `AGENT2SSH_CONFIG_DIR`:

- `secrets status --json` starts as `{ initialized: false, unlocked: false }`.
- `secrets set-password --password ...` initializes `secrets.enc`.
- A new process without `AGENT2SSH_MASTER_PASSWORD` reports initialized but locked.
- A new process with `AGENT2SSH_MASTER_PASSWORD` reports initialized and unlocked.
- Recursive grep of the isolated config directory did not find the master password in plaintext.

Passed focused regression tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features webdav_sync::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features secrets::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features store::tests::migrate_secrets_moves_legacy_plaintext
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features store::tests::passwords_persist_as_marker_not_plaintext
```

These cover:

- encrypted store init/unlock and locked-store behavior,
- plaintext credential migration into secret references,
- password persistence as marker rather than plaintext,
- WebDAV sync file selection excluding local trust/runtime/private-key files,
- backup content selection,
- legacy remote `known_hosts.json` tolerance without local overwrite.

### Remaining Q2 Items

Not completed in this local pass:

- Real WebDAV `push` / `pull` against an actual remote collection.
- Network failure, authentication failure, and remote conflict recovery against a real WebDAV service.
- Cross-device pull/unlock/host-key verification workflow.
- Desktop `SecretsUnlock` manual UI walkthrough.
- MCP/daemon password-host execution using a real password-auth SSH host and `AGENT2SSH_MASTER_PASSWORD`.

These require a real WebDAV endpoint, a second device/profile, a desktop manual run, or a password-auth test host.

### Recommendation

Next work should continue with Q2 real-environment validation before opening Q3 external adoption. The codebase now has a stronger local release gate, so new changes should use `./scripts/e2e-local.sh` as the default preflight.


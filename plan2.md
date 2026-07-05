# Agent2SSH Plan 2 — 功能与 UI 演进计划

> 日期：2026-07
> 定位：0.2.1 质量收口已完成，项目从"质量闭环"转入"功能+UI 演进"阶段。本计划覆盖旧 plan2.md Q1 ✅ 和 Q2 剩余项，并扩展为体验驱动的多阶段演进路线。

## 1. 当前判断

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

## 2. 优先级总览

| 阶段 | 主题 | 项数 | 预估周期 |
|------|------|------|----------|
| Q2' | 凭据/WebDAV 回归收尾 | 5 | 1-2 周（并行于 V1） |
| V1 | 基础体验骨架 | 5 | 2-3 周 |
| V2 | 核心交互升级 | 5 | 3-4 周 |
| V3 | 效率工具链 | 4 | 3-4 周 |
| V4 | 高级可视化与自动化 | 6 | 4-5 周 |

## 3. Q2' · 凭据/WebDAV 回归收尾

目标：完成旧 plan2 Q2 的 5 项真实环境验证，在 V1 启动同时并行推进。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q2'-1 | 高 | 真 WebDAV push/pull 回归 | 对真实 WebDAV 服务完成 push/pull，覆盖 hosts.json、secrets.enc、policy、limits、playbooks 同步；确认不同步 known_hosts.json、tokens、audit、logs、私钥 |
| Q2'-2 | 高 | WebDAV 网络故障恢复 | 模拟远端旧 manifest、未知文件、网络失败、认证失败；错误提示包含下一步动作 |
| Q2'-3 | 中 | 跨设备拉取后流程文档 | 配置指南补"新设备 pull → 解锁 → 验证 host-key → 避免覆盖本地信任库"短流程 |
| Q2'-4 | 中 | SecretsUnlock 桌面 UI walkthrough | 手动走一遍桌面启动 → 输入主密码 → 解锁 → 锁定 → 改密码 → 错密码拒绝 |
| Q2'-5 | 中 | 密码型 SSH 主机端到端 | 用主密码解锁后，对密码认证 SSH 主机完成 exec/SFTP 全链路 |

验收命令：`cargo test --no-default-features secrets::tests webdav_sync::tests` + `npm run build`

## 4. V1 · 基础体验骨架

目标：建立桌面端操作入口的骨架——首页总览、全局状态、快速搜索和统一反馈，让用户打开 App 后 3 秒内知道系统状态、5 秒内找到任何功能入口。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V1-1 | ✅ 已完成 | P0 | 中 | Dashboard / 健康总览页 | `src/components/Dashboard.tsx`：新增首页模块（Ctrl+K/侧栏第一项），聚合 6 张卡片——主机健康(connectionStatuses)、审批待办数(pendingApprovals)、异常告警数(SSE `anomaly_detected` 事件的会话内计数，非历史总量——anomaly.rs 本身是 fire-and-forget 事件，没有持久化的历史计数可查，已在卡片 hint 里注明"自打开仪表盘起")、24h 执行量(复用既有 `list_audit` 命令按 `since` 过滤计数)、凭据锁定状态、daemon 运行状态；Host/24h 卡片可点击跳转 Host Management / Audit；`npm run build` 通过，Playwright 截图验证 light/dark 下 6 卡片数据渲染正确 |
| V1-2 | ✅ 已完成 | P0 | 低 | 全局状态栏 Footbar | `src/components/Footbar.tsx`：底部固定栏显示 daemon 状态(green/red)、gate 状态、凭据(locked/unlocked)、活跃连接数、版本号（从 `package.json` 读取）；`npm run build` 通过，Playwright 截图验证 light/dark 主题下渲染正常，各状态字段随 mock 数据正确切换 |
| V1-3 | ✅ 已完成 | P0 | 低 | 命令面板 Ctrl+K | `src/components/CommandPalette.tsx`：全局模态搜索框，索引 Modules + Hosts（按 name/host/user/group/role/owner/tags 匹配）；Ctrl+K/Cmd+K 打开、Esc 关闭、↑↓ 选择、Enter 跳转（模块直接切换，主机跳转到 Host Management 并选中）；`npm run build` 通过，Playwright 验证搜索过滤、键盘导航、跳转后状态正确。范围收敛：未索引"命令/审批"关键词，因为目前没有独立的审批列表页可跳转（审批走既有的 ApprovalDialog 弹层），留给后续评估是否需要 |
| V1-4 | ✅ 已完成 | P1 | 低 | Toast / 通知条统一 | `src/components/ui/toast.tsx`：`ToastProvider` + `useToast()`，success/error/warning 三种，5s 自动消失+手动关闭，基于现有 `ui/` primitives（未引入 MUI）；已接入 `main.tsx`（包住整个 App）；**App.tsx 全量转换**（26 处 `setError`/banner 改为 `showToast`）。**本次补完**：AddHostForm、ExecPanel、ForwardPanel、KeysPanel、McpAgentsPanel、MultiExecPanel、PingPanel、PlaybooksPanel、ProxyPanel、SFTPPanel（顶层操作反馈，区别于 `s.error` 驱动的每侧 `ErrorState`）、SyncPanel 的一次性操作反馈（保存/删除/连接失败等）改为 `showToast`。**有意保留为局部 state（非误伤，是设计选择）**：SecretsUnlock（登录式密码错误需贴着输入框常驻，不宜 5s 消失）、SettingsMenu（诊断导出路径/更新版本号等分区内的常驻状态文案）、SetupWizard（每步骤绑定的校验反馈）、LiveActivityPanel 的连接离线原因（已改用 `ErrorState` 组件渲染，但仍是持久状态而非 toast，因为 `status` 徽标本身就是持久态）；`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V1-5 | ✅ 已完成 | P1 | 低 | 空态/加载态/错误态一致性 | `src/components/ui/state.tsx`：`EmptyState`/`LoadingState`/`ErrorState` 三个共享组件（统一图标+文案+可选操作按钮）。**已接入**：AuditPanel（空态）、HostList（两处空态）、SFTPPanel（error/loading/empty 三态）、ForwardPanel（无隧道空态）、KeysPanel（无密钥空态）、PlaybooksPanel（无 playbook 空态）、ProxyPanel（无代理空态）、LiveActivityPanel（无活动空态 + 连接失败态）。ExecPanel/MultiExecPanel 的终端输出占位符按 CLAUDE.md 约定保留固定深色终端面板样式，不套用 token 化的 `EmptyState`；ApprovalDialog 是静态确认弹层，没有空/加载/错误态可套用——审批列表/时间线视图留给 V2-2；`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --test cli_smoke`

## 5. V2 · 核心交互升级

目标：让审批、审计和通知从"可看"升级为"可操作、可分析、可实时感知"；主题系统已上线（见第 1 节），本阶段只做覆盖度审查，不重建。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V2-1 | ✅ 已完成 | P0 | 中 | 实时桌面通知系统 | 新增 `src/eventsBus.tsx`：单一共享 SSE 连接（`EventsProvider` + `useAgentEvents`/`useEventsStatus`），`LiveActivityPanel` 与 `Dashboard` 的异常计数都改用它，替换掉各自独立开的 SSE 连接（原来是 3 条并发连接，现在 1 条）。新增 `src/components/NotificationCenter.tsx`：订阅 `approval_requested` 弹可操作 toast（内嵌批准/拒绝，`durationMs: null` 常驻直到处理或被 `approval_responded` 事件关闭）、`anomaly_detected` 弹只读 toast（8s）。`ui/toast.tsx` 扩展支持 `title`/`actions`/`durationMs`（`null`=常驻）与 `dismissToast(id)`。**连接状态变更通知**未走 SSE——`host_connected`/`host_disconnected` 事件类型在 `events.rs` 里定义了但后端从未 `publish_event` 过，是死枚举；改为在 `App.tsx` 现有的 5s `pollConnections` 里做前后快照 diff，状态翻转时 `showToast`，如实反映了实际数据来源而非假装走了 SSE。**设计取舍**：可操作 toast 与已有的阻塞式 `ApprovalDialog`（`pendingApprovals[0]` 自动弹出）会短暂共存——两者调用同一后端 API，处理一个另一个会在下次轮询后自然消失，不是正确性问题，故未改动既有阻塞弹窗行为。`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V2-2 | ✅ 已完成 | P1 | 中 | 审批时间线视图+批量操作 | 新增模块 `approvals`（追加到 `MODULES` 末尾，不插入中间，避免打乱 V2-5 已发布的 Ctrl/⌘+1~9 映射）+ `src/components/ApprovalTimeline.tsx`：全量审批历史（`api.fetchApprovals()` 本身就不过滤 status，之前只有 `App.tsx` 会话内过滤成 pending）按 `requested_at` 倒序的竖向时间轴，每条含时间戳/主机/`RiskBadge`/状态 `Badge`（pending=warning、approved=success、rejected=destructive、timed_out=secondary）；仅 pending 项可勾选，批量批准/拒绝复用既有单条 `approvalApprove`/`approvalReject` REST 端点（`Promise.allSettled` 并发调用），执行结果走既有 exec 授权管线自然落入 audit，未新增后端逻辑；沿用 `AuditPanel` 的 `RENDER_CAP_STEP` 分页模式防止长期运行的审批历史（后端未做过期清理）撑爆 DOM；订阅事件总线的 `approval_requested`/`approval_responded` 做即时 refetch，外加 10s 轮询兜底。Dashboard 的"Pending approvals"卡片现在可点击跳转到这个新模块。`npm run build` 通过 |
| V2-3 | ✅ 已完成 | P1 | 中 | 审计可视化图表 | 引入 `recharts`（新依赖，`vendor-charts` chunk ~92KB gzip，已按现有 `vite.config.ts` manualChunks 规则单独分包避免和 `vendor-react` 打包环产生循环 chunk 警告）。新增 `src/components/AuditCharts.tsx`，渲染在 Audit 模块 `AuditPanel` 上方，24h/7d/30d 范围切换驱动全部图表（对齐 dataviz skill 的"筛选器统一作用于下方所有图表"原则）：① 执行量趋势——单序列面积图（`--primary`，10% 透明度描边+柱面）；② 风险分布——柱状图，四档配色复用 `RiskBadge` 已有的语义色（success/warning/destructive），不是新造的分类色板；③ 按来源统计——水平柱状图取代原计划的"来源饼图"（dataviz skill 明确把"比较量级"归类为 sequential 单色柱状图，饼图不在推荐表里，遂改用条形图，见下方说明）；④ 主机活跃时段热力图——自建 CSS grid（Recharts 无原生热力图），取执行量 Top 8 主机 × 24 小时格子，颜色用 `color-mix(in srgb, var(--primary) N%, var(--card))` 单色渐变，零执行格子用 `--muted` 而非色阶最浅端（避免"看起来仍有一点活动"的误读）。所有图表颜色直接引用 CSS 自定义属性字符串（如 `stroke="var(--primary)"`），SVG 属性原生支持 `var()`，因此 6 套主题下自动跟随，无需按主题重新计算或做单独校验。数据来源独立于 `AuditPanel` 的可调 `limit`/筛选（`AuditPanel` 默认 limit 太小，不够支撑 30 天趋势），改为自己按 30 天窗口 + 5000 条上限拉取一次，与 `Dashboard.tsx` 现有的 24h 计数请求同一约定。`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V2-4 | ✅ 已完成 | P2 | 低 | 主题系统覆盖度审查（原"暗色模式"，已上线不必重做） | 对全部 `src/components/*.tsx` + `App.tsx` 做了硬编码颜色（hex / Tailwind 命名色板 / 依赖 `prefers-color-scheme` 的 `dark:` 变体）扫描。**修复 6 处真实问题**：① `LiveActivityPanel` 状态徽标 `connecting` 从 `bg-sky-500/15 text-sky-500` 改为 `bg-primary/15 text-primary`（与 `live`/`offline` 已用 token 的写法对齐）；② 同文件异常 `severity` 徽标 `text-orange-600` 改为 `text-warning`；③ 同文件 `item.detail`/`item.raw` 预览框 `bg-[#0f172a] text-slate-200` 改为 `bg-muted text-foreground`（与紧邻的 `item.command` 预览已用 token 保持一致，且它只是文本/JSON 预览而非终端仿真，不属于终端例外）；④ `ApprovalDialog` 命令预览框 `bg-[#1e293b] text-slate-100` 改为 `bg-muted text-foreground`；⑤⑥ `PlaybooksPanel` 步骤输出预览、`TerminalPanel` 标签页关闭按钮 hover 用的是 Tailwind `dark:` 变体（默认跟随系统 `prefers-color-scheme`），与 App 自己的 `data-theme` 显式主题切换是两套独立机制——用户显式选中 dark/dracula/nord 等主题但操作系统仍是浅色模式时，这两处会撞色（浅色调叠加在深色卡片上，对比度不足）；改为不依赖 `dark:` 的 `bg-foreground/5` / `hover:bg-foreground/10`，随 `--foreground`/`--background` 自动适配当前 `data-theme`。**确认为既有合理例外，未改动**：`ExecPanel`/`MultiExecPanel`/`ErrorBoundary` 的终端输出块固定深色（`#0e1620`/`#e6edf3`，CLAUDE.md 已记录）；`PingPanel` 与侧栏固定深色状态色（CLAUDE.md 已记录）；`Dialog`/`CommandPalette` 的 `bg-black/50` 遮罩层；`SettingsMenu` 主题色块 `border-black/15`；`TerminalPanel` 终端画布本身的颜色（由独立的终端主题子系统 `terminalThemes.ts` 驱动，非 App 主题 token）。**观察但未处理**（超出本项"硬编码撞色"范围，属于全主题通用的可访问性问题，需要产品决策）：侧栏激活模块项用固定 `text-white`，在 Nord（`--sidebar-accent: #88c0d0`）、Dracula（`#bd93f9`）等浅色高亮下与白色文字对比度明显不足（估算 <3:1），但这在全部 6 套主题下都存在、并非某一主题独有的新问题，未擅自改动配色。**未发现对应 UI**：i18n 中 "Session"/"Attach to this session" 等词条当前没有任何组件在用，桌面端目前没有独立 Session 面板，故审查范围里的"Session"页跳过。`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿 |
| V2-5 | ✅ 已完成 | P1 | 低 | 键盘快捷键体系 | `src/App.tsx` 全局 `keydown` 处理：Ctrl/⌘+K 打开命令面板（沿用 V1-3）、Ctrl/⌘+1~9 直接切换到对应模块（`MODULES` 前 9 项）、Ctrl/⌘+Shift+A 在有待处理审批时聚焦 `ApprovalDialog` 的取消按钮、无待处理审批时 `showToast` 提示；`src/components/ExecPanel.tsx` / `MultiExecPanel.tsx` 的命令 `Textarea` 绑定 Ctrl/⌘+Enter 直接执行；所有绑定 `preventDefault()` 避免浏览器默认行为；快捷键列表在 `SettingsMenu` 新增"Keyboard shortcuts"分区展示；新增 i18n 键已补齐中文翻译；`npm run build` 通过，`cargo test --no-default-features --lib` 208 全绿。未做浏览器交互回归（Tauri `invoke` 在纯 vite dev 环境不可用），已用类型检查 + 代码走查替代 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --features daemon --test daemon_integration`

## 6. V3 · 效率工具链

目标：让 SFTP 和终端从"基本可用"升级为"日常效率工具"——文件预览省去下载、分屏覆盖多主机场景、表格交互减少查找时间。

| 任务 | 状态 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|------|--------|--------|------|----------|
| V3-1 | ✅ 已完成 | P1 | 低/中 | SFTP 文件预览+面包屑导航 | 面包屑其实在 SFTPPanel 里已经存在（`buildBreadcrumbs()`，点击跳转目录），本项只补预览。新增后端能力（`core.rs::sftp_read_text_core_with_source` 复用既有 `sftp_dir_operation_core` helper，和 `sftp_ls`/`sftp_stat` 一样过 `authorize_desktop_operation` 授权网关，不是绕过安全管线的旁路）+ `tauri_commands.rs::sftp_read_text`/`local_read_text`（本地侧不经授权，和 `local_ls`/`local_walk` 现有约定一致），都在 `generate_handler!` 里注册，服务端各自按 ~1MB 硬上限读取并要求合法 UTF-8，超限/非文本直接 `Err`。前端双击文件行触发 `FilePreview.tsx`（`React.lazy` 懒加载，Monaco 只在真正打开一次预览后才拉取，不进首屏包）；size 已知且 ≥1MB 的文件直接跳过网络请求展示元信息卡片，读取失败（二进制/超限/权限错误）同样落到元信息卡片而不是报错崩溃。**Monaco 版本特意锁定 `0.53.0`（非 `^`）**：`^0.55.1` 会带出有已知 XSS 通报的 `dompurify@3.2.7`（精确锁定版本，上游没法通过 `npm audit fix` 更新），0.53.0 这条依赖链根本不存在；只注册基础 `editor.worker`，JSON/CSS/TS 的语言服务诊断显式关掉（`setDiagnosticsOptions({ validate: false })` 等），避免为一个只读预览搭进 TS 编译器体量的 worker（省了 ~8MB）。CSP 是 `script-src 'self'`，`@monaco-editor/react` 默认走 CDN 加载器会被拦，改成 `loader.config({ monaco })` 指向本地打包的包。**如实说明**：`vendor-monaco` chunk 打包后仍有 ~4.3MB / gzip 1.1MB——这是桌面应用的本地资源，不走网络，主要成本是首次打开预览时的一次性 JS 解析/编译，不是数据在无网/弱网环境的下载体验；"≤1s" 验收目标覆盖的是文件内容读取（一次 IPC 调用，实际远快于 1s），冷启动那次额外的 Monaco 解析时间未纳入严格量化。`npm run build`/`cargo test --lib`/`cargo test --no-default-features --lib`（208/215 全绿）/`cargo fmt --check` 均通过；新增 3 条 `local_read_text_inner` 单元测试（正常读取/超限拒绝/二进制拒绝） |
| V3-2 | ✅ 已完成 | P1 | 高 | 终端分屏+Session 分组 | `TerminalPanel.tsx` 从"单 Tab 显示、其余隐藏挂载"重构为最多 4 窗格（单/水平二分/垂直二分/2×2 四宫格），窗格边界可拖拽调整比例（`pointermove` 实时更新百分比，非固定网格）；**所有已打开的 Tab 始终保持挂载/连接**，未分配到任何窗格的仅 `visibility:hidden`（保留原有"切走不断线"的行为，没有因为改成多窗格而让后台会话被杀掉）。左侧新增按主机分组的会话树（可折叠），点击某会话把它指派到当前"聚焦窗格"；每个窗格头部也有一个下拉可单独换绑会话。`TerminalView.tsx` 改造为 `forwardRef` 暴露 `sendText`/`focus`，并新增基于用户自己按键的逐行缓冲（Enter 落一行、退格/Ctrl+U/Ctrl+C 清缓冲——和 `docs/architecture.md` 里后端"整行边界"审计缓冲是同一思路，只是这边是给历史搜索用，不做鉴权），Ctrl+R 通过 `term.attachCustomKeyEventHandler` 拦截、打开本 App 自己的历史搜索浮层而不转发给远端 shell。**明确的行为取舍**：这意味着 Ctrl+R 不再触发 bash 自带的 reverse-i-search——只要窗格数>1（多窗格模式）就用本地搜索浮层替代，选中一条历史只是把文本重新"打"回输入框（不带回车），用户还能改了再按 Enter，不会盲目重跑高风险命令。`npm run build` 通过；未接入真实 daemon 做 4 窗格并发连接的手动烟测（同前几轮，纯 vite dev 环境下 Tauri `invoke`/WS 不可用），用类型检查 + 代码走查 + 分层 z-index 修正（窗格头部/边框显式 `z-[2]` 盖过终端画布的 `z-1`，避免画布压住窗格头）替代 |
| V3-3 | ✅ 已完成 | P1 | 中 | 表格/列表交互增强 | 引入 `@tanstack/react-table`（headless，零新样式依赖，复用现有 token/primitive，独立 `vendor-table` chunk）。新增 `ui/data-table.tsx` 共享 `SortIcon`/`ColumnVisibilityMenu`（两处表格都用得到，不是单次抽象）。`HostList.tsx` 从卡片列表改成真表格：姓名/地址/状态可排序，标签/详情列可显示隐藏，勾选行 + 批量删除（新增 `App.tsx::handleBatchRemoveHosts`，并发调用既有单条 `removeHost`，只刷新一次而不是 N 次）。`AuditPanel.tsx` 同样表格化：时间/主机/风险可排序，勾选行 + "复制所选为 JSON"批量操作（audit 是不可变审计记录，没有做批量删除这种会破坏审计完整性的操作，选了个安全的只读批量动作）。`npm run build` 通过 |
| V3-4 | ✅ 已完成 | P1 | 中 | 模块间 Breadcrumb 导航 | 新增 `Breadcrumb.tsx` + `App.tsx::RELATED_MODULES` 映射表：Host→Execute/Files/Tunnels/Terminal/Audit、Execute→Host/Audit、Audit→Host/Approvals、Approvals→Execute/Audit 等，网状而非线性 Tab，点击直接跳模块。Dashboard 的"待处理审批"卡片顺手接上了跳转到新 Approvals 模块（V2-2 遗留的一个小尾巴）。`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --lib` + `cargo fmt --check`

## 7. V4 · 高级可视化与自动化

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

## 8. 不建议现在做的事

以下方向不在本计划范围内，旧 plan2.md 已排除，新 plan2 继续排除：

- ❌ Cloud Console 独立产品化（当前定位是本机操作面，不是云端控制台）
- ❌ 账号体系与用户认证（单机工具，无多用户需求）
- ❌ 组织 RBAC 与团队权限（单机定位，团队协作靠 WebDAV 配置同步 + 审批流）
- ❌ SaaS 化与多租户（无真实用户需求驱动）
- ❌ 继续增加 MCP 工具数量（51 工具已覆盖核心工作流，除非有明确缺口）
- ❌ 把桌面端改成大型运维平台（更适合作为本地 operator surface）

## 9. 推荐执行顺序

```
Q2' ── 并行于 V1 ──→ V1 ──→ V2 ──→ V3 ──→ V4
(1-2周)            (2-3周)  (3-4周)  (3-4周)  (4-5周)
```

- **Q2' 并行于 V1**：凭据/WebDAV 回归可与 V1 前端骨架同步推进，无依赖冲突
- **V1 → V2**：V2 通知系统依赖 V1 的 Toast 组件；V2 审批时间线/图表依赖 V1 的空态/加载态共享组件
- **V2 → V3**：V3 的表格/面包屑是新增交互面，建议排在 V2 通知与批量操作之后，避免同一批面板的交互模式被改两次；不依赖主题系统（token 体系已在 V 阶段启动前就存在）
- **V3 → V4**：V4 拓扑/Playbook 可独立开发，但建议在 V3 表格交互稳定后进入，避免 UI 层同时大改

每个阶段完成后跑该阶段验收命令 + `scripts/e2e-local.sh`，确保新 UI 不破坏现有后端基线。

## 10. 最小验收命令

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

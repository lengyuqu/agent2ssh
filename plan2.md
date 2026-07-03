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
| V1-4 | 🟡 部分完成 | P1 | 低 | Toast / 通知条统一 | `src/components/ui/toast.tsx`：`ToastProvider` + `useToast()`，success/error/warning 三种，5s 自动消失+手动关闭，基于现有 `ui/` primitives（未引入 MUI）；已接入 `main.tsx`（包住整个 App）；**App.tsx 已全量转换**（26 处 `setError`/banner 全部改为 `showToast` 或依赖既有的连接进度弹窗，移除了顶层 `error` state 和常驻 banner）。**剩余**：AddHostForm/ExecPanel/SFTPPanel 等其余 12 个面板内部的局部 `setError` 尚未替换——这些是每个面板自己的 local state，需要逐个文件过一遍（机械但要小心不误伤真正的表单校验态），留作后续任务 |
| V1-5 | 🟡 部分完成 | P1 | 低 | 空态/加载态/错误态一致性 | `src/components/ui/state.tsx`：`EmptyState`/`LoadingState`/`ErrorState` 三个共享组件（统一图标+文案+可选操作按钮）。**已接入**：AuditPanel（空态）、HostList（两处空态）、SFTPPanel（error/loading/empty 三态一次性覆盖，示范了三个组件的典型用法）。**剩余**：ExecPanel/Session/Approval 等其余面板尚未替换，留作后续任务 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --test cli_smoke`

## 5. V2 · 核心交互升级

目标：让审批、审计和通知从"可看"升级为"可操作、可分析、可实时感知"；主题系统已上线（见第 1 节），本阶段只做覆盖度审查，不重建。

| 任务 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|--------|--------|------|----------|
| V2-1 | P0 | 中 | 实时桌面通知系统 | 基于 SSE + events_subscribe 构建统一通知层：审批通知可操作（内嵌批准/拒绝按钮）、异常告警只读、连接状态变更；通知弹窗 ≤3s 到达；`npm run build` 通过 |
| V2-2 | P1 | 中 | 审批时间线视图+批量操作 | 竖向时间轴展示审批历史；每条显示风险评分色标；勾选多条后批量批准；批量操作结果写入 audit；`npm run build` 通过，时间轴渲染 ≥20 条无卡顿 |
| V2-3 | P1 | 中 | 审计可视化图表 | 引入 Recharts：执行量趋势折线图（24h/7d/30d）、风险分布直方图、来源饼图、主机热力图；图表数据从 audit.jsonl 实时聚合；`npm run build` 通过，折线图渲染 ≤500ms，且在 6 套主题下颜色不冲突 |
| V2-4 | P2 | 低 | 主题系统覆盖度审查（原"暗色模式"，已上线不必重做） | 逐页核对 V1/V2 新增组件（Dashboard/Footbar/命令面板/Toast/时间线/图表）是否只用 token 类（`bg-card`/`text-muted-foreground`/…），未出现硬编码 hex；在 dark/dracula/nord/solarized-light 下人工过一遍 Host/Audit/Exec/SFTP/Session/Settings/Approval；发现的硬编码颜色改为 token 并在 `docs/architecture.md` 或组件注释处标注例外（如终端面板固定深色） |
| V2-5 | P1 | 低 | 键盘快捷键体系 | Ctrl+1~9 模块切换、Ctrl+K 面板、Ctrl+Enter 执行、Ctrl+Shift+A 审批；快捷键列表在 Settings 可查看；冲突检测避免浏览器内置覆盖；`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --features daemon --test daemon_integration`

## 6. V3 · 效率工具链

目标：让 SFTP 和终端从"基本可用"升级为"日常效率工具"——文件预览省去下载、分屏覆盖多主机场景、表格交互减少查找时间。

| 任务 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|--------|--------|------|----------|
| V3-1 | P1 | 低/中 | SFTP 文件预览+面包屑导航 | 文本文件 ≤1MB 用 Monaco 内嵌预览（语法高亮）；路径面包屑组件（点击跳转目录）；二进制/超大文件显示元信息卡片；`npm run build` 通过，1MB 文本预览 ≤1s 加载 |
| V3-2 | P1 | 高 | 终端分屏+Session 分组 | 水平/垂直分屏最多 4 面板；树形 Session 分组（按主机/来源）；命令历史 Ctrl+R 搜索；面板可拖拽调整尺寸；`npm run build` 通过，4 面板同步渲染无崩溃 |
| V3-3 | P1 | 中 | 表格/列表交互增强 | 引入 TanStack Table（headless，样式沿用现有 token/primitive，不引入 MUI）：列排序、列选择隐藏、行勾选批量操作；Host 列表和 Audit 列表首批替换；`npm run build` 通过，1000 行渲染 ≤300ms |
| V3-4 | P1 | 中 | 模块间 Breadcrumb 导航 | 每模块顶部面包屑显示当前位置+可跳转上级/关联模块（Host→Exec、Host→Audit、Approval→Exec）；网状导航而非线性 Tab；`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo fmt --check` + `git diff --check`

## 7. V4 · 高级可视化与自动化

目标：补上拓扑感知、自动化编排和视觉一致性——让多主机运维从"逐台操作"升级为"全局视角+模板驱动"。

| 任务 | 优先级 | 复杂度 | 内容 | 验收标准 |
|------|--------|--------|------|----------|
| V4-1 | P2 | 高 | 连接拓扑可视化 | D3.js / react-force-graph 力导向图渲染：主机→daemon→proxy→tunnel 链路；节点颜色按健康/风险着色；点击节点跳转 Host 详情；`npm run build` 通过，30 节点渲染 ≤1s |
| V4-2 | P2 | 高 | Playbook 可视化创建/编辑 | 步骤编排器拖拽排序；条件分支可视化（if/else 节点）；YAML 双向同步生成；保存后 playbook run 可执行；`npm run build` 通过 |
| V4-3 | P2 | 中 | 配置模板+一键快照恢复 | 内置 3-5 模板（基础安全、开发环境、生产运维）；快照保存当前 hosts+policy+limits 配置；一键回滚到历史快照；`npm run build` 通过 |
| V4-4 | P2 | 低 | 跨主机比较 UX 增强 | react-diff-viewer Diff 视图：红绿高亮多主机执行结果差异；统计摘要卡片（相同/不同/缺失行数）；`npm run build` 通过 |
| V4-5 | P2 | 中 | 响应式布局优化 | 最小宽度 800px；侧栏可折叠为 icon-only；SFTP 上下分栏自适应；窄屏命令面板全屏覆盖；`npm run build` 通过，800px 窗口所有模块可操作 |
| V4-6 | P2 | 低/中 | 图标/配色系统统一（沿用现有 token，不新建） | 现有 6 主题已各自定义 `--primary`/灰阶/语义色（如 light 主题 `--primary: #17857c`），本任务不重新定义颜色，只做统一：给每个模块固定一个 lucide-react 图标（当前 `MODULES` 常量已有雏形，需扩展到 V1-V4 新增的 Dashboard/审批时间线/拓扑图等）；检查 V1-V4 新组件是否都复用了 `--success`/`--warning`/`--destructive` 语义色而非临时新增颜色；`npm run build` 通过 |

验收命令：`npm run build` + `cargo test --no-default-features --lib` + `cargo test --no-default-features --features daemon --test daemon_integration` + `npm run tauri:build`

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

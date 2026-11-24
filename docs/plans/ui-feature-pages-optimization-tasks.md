# Agent2SSH 功能界面逻辑优化：方案总览与详细任务卡工程规范

> **工程导向**：遵循 Pre-1.0 **「体验 > 功能 > 安全」** 最高原则。  
> **承接背景**：上一轮 [`ui-interaction-refactor-tasks.md`](./ui-interaction-refactor-tasks.md)（T1–T7）已完成导航四大功能舱聚合、审批带入执行台、主机抽屉表单、执行分段 Tab、终端抽屉会话续接与设置分 Tab。本轮聚焦**各核心功能页内部的逻辑断点、状态错位、数据割裂与布局范式统一**，不推倒重写，基于现有组件与后端能力逐卡收口。

---

## 一、方案总览与分批路线图

| 批次 | 任务卡 | 核心目标 | 涉及主要模块 | 依赖 |
| :--- | :--- | :--- | :--- | :--- |
| **Phase 2A** | **F1** | 主机管理页升级为「运维操作起点」 | `HostList`、`LocalEnvironmentsCard`、`App` | 无 |
| **Phase 2A** | **F2** | 命令执行中心补齐 `local:*`、批量联动与路由收敛 | `ExecutionWorkbench`、`ExecPanel`、`MultiExecPanel`、`HostSelector`、`tauri_commands`、`nav` | 建议在 F1 后或并行 |
| **Phase 2A** | **F3** | 审计页图表联动筛选、详情抽屉与筛选 Bug 修复 | `AuditCharts`、`AuditPanel`、`App` | 复用 T1 执行带入与通用抽屉 |
| **Phase 2B** | **F4** | 隧道/代理/密钥页抽屉范式统一、动态轮询与依赖追溯 | `SlideOverDrawer`、`ForwardPanel`、`ProxyPanel`、`KeysPanel` | 抽离 `HostDrawer` 为通用抽屉 |
| **Phase 2B** | **F5** | 总览仪表盘可操作化 + 连通性探测结果联动 | `Dashboard`、`PingPanel`、`LiveActivityPanel`、`App` | 复用 F1 的终端直达 |
| **Phase 2B** | **F6** | 全局页头动态上下文 + 满高工作区双滚动条收口 | `App`、`SFTPPanel`、`RecordingsPanel`、`ConnectionTopology` | 建议最后收尾 |

### 共享基础设施约定（避免重复造轮子）
1. **通用抽屉壳子 `SlideOverDrawer`**：由现有 [`src/components/HostDrawer.tsx`](../../src/components/HostDrawer.tsx) 提炼为 `src/components/ui/slide-over-drawer.tsx`（接收 `open`、`title`、`widthClassName`、`onClose`、`children`），`HostDrawer` 改为薄封装；供 F3（审计详情）、F4（隧道/代理新增与编辑）共同复用。
2. **终端直达通道**：复用 [`src/App.tsx`](../../src/App.tsx#L194-L198) 现有的 `terminalFocusRequest`（`dockId: null` 即表示在主终端聚焦或新建该主机标签页），供 F1（主机行直达）与 F5（Ping 行直达）复用。
3. **执行带入通道**：复用 T1 建立的 `execInitialCommand` + `execCarryEpoch` + `execPreferredMode` 链路，扩展支持从 `LocalEnvironmentsCard`（F1/F2）与 `AuditPanel`（F3）无缝带入目标主机与命令。

---

## 任务卡 F1：主机管理页升级为「运维操作起点」

### 1.1 问题现状与代码定位
* **跨组检索与模糊搜索缺失**：
  - [`src/components/HostList.tsx`](../../src/components/HostList.tsx#L132-L146)：列表强行按单一 `selectedGroup` 过滤（`if ((host.group || "default") !== selectedGroup) return false`），没有「全部主机」视图；
  - [`src/components/HostList.tsx`](../../src/components/HostList.tsx#L486-L511) 与 [`matchesFilter`](../../src/components/HostList.tsx#L644-L648)：只提供 `env/role/owner/tag` 四个框，且采用严格全等匹配（`===`），**完全没有按主机名、地址、用户名模糊搜索的能力**。
* **行内操作缺少高频直达动作**：
  - [`src/components/HostList.tsx`](../../src/components/HostList.tsx#L276-L329)：行尾仅有「编辑 / 连接或断开 / 忘记 OpenSSH known_hosts / 删除」，运维最常用的「打开终端」「发起命令」「浏览 SFTP」都必须先点选行、再去顶部胶囊或侧边栏二次跳转；
  - 跳转 `Terminal` 时，[`src/components/TerminalPanel.tsx`](../../src/components/TerminalPanel.tsx#L638) 只把 `initialHost` 用在点击 `+` 时，不会自动聚焦/打开选中主机。
* **上轮改造遗留的 `⌘K` 断层**：
  - [`src/App.tsx`](../../src/App.tsx#L770-L774)：`⌘K` 快捷动作 `new-host` 仍只调用 `openModule("hosts")`，在 T2 将 `AddHostForm` 收进 `HostDrawer` 后，触发后抽屉并未打开。
* **本地环境卡片常驻占位且纯只读**：
  - [`src/App.tsx`](../../src/App.tsx#L1317-L1319) 与 [`src/components/LocalEnvironmentsCard.tsx`](../../src/components/LocalEnvironmentsCard.tsx#L40-L102)：每次进入主机页都要先跨过静态本地环境卡片，且卡片只展示文字说明，不能一键进入执行台。

### 1.2 交互规范与逻辑流转
1. **「全部主机」视图与统一搜索**：
   - 分组栏首位增加固定项 **`全部 (All)`**（值为 `ALL_GROUPS_ID = "__all__"`），默认首次进入或搜索时可跨全量主机定位；
   - 表格上方改为**单行主工具栏**：左侧主搜索框（即时模糊匹配 `name`、`host`、`user`、`tags`、`env`、`role`、`owner`，带一键清空），右侧折叠按钮 `[属性筛选]`（展开后显示 `env / role / owner / tag`，统一改为不区分大小写的子串匹配，并展示已生效筛选数角标）。
2. **行内直达工作流**：
   - 表格行右侧主动作区提供高频三连 + 连接控制：
     - `[终端]`：切换至 `terminal` 模块并触发 `terminalFocusRequest = { host: host.name, dockId: null, nonce }`，自动聚焦已有标签页或新建该主机终端；
     - `[执行]`：设置 `selectedHost = host.name`、`execPreferredMode = "single"` 并切入 `execute`；
     - `[SFTP]`：设置 `selectedHost = host.name` 并切入 `files-sessions`；
     - `[连接/断开]`：保留现有行内进度条反馈；
     - `[编辑]`：打开 `HostDrawer`；
     - 低频动作（`忘记 OpenSSH known_hosts`、`删除主机`）收敛进图标或次级分组，避免挤占高频入口。
3. **`⌘K → New host` 一步到位**：
   - 无论当前处于哪个模块，触发 `⌘K → New host` 时同时执行 `openModule("hosts")` 与 `openAddHostDrawer()`。
4. **本地环境卡片轻量化与可执行**：
   - `LocalEnvironmentsCard` 默认折叠为单行摘要栏（展示平台 + 已发现环境数 + `local:*` 徽标，展开态持久化至 `localStorage`）；
   - 展开后每个环境卡片右侧提供 `[去执行]` 按钮（回调 `onExecuteLocal(envId)`，将 `local:<envId>` 带入执行台，与 F2 联动）。

### 1.3 技术改动清单
1. **`src/components/HostList.tsx`**：
   - 支持 `selectedGroup === "__all__"` 放行全部分组，并在分组列表顶部渲染「全部」项（显示 `hosts.length`）；
   - 增加 `query` 搜索状态与 `showAdvancedFilters` 折叠状态；将 `matchesFilter` 升级为子串匹配（`includes`）；
   - 新增可选回调 Props：
     ```typescript
     onOpenTerminal?: (hostName: string) => void;
     onOpenExecute?: (hostName: string) => void;
     onOpenFiles?: (hostName: string) => void;
     ```
   - 更新 `actions` 列渲染，确保所有行内按钮均调用 `e.stopPropagation()`。
2. **`src/components/LocalEnvironmentsCard.tsx`**：
   - 增加折叠/展开状态（`localStorage` 键 `agent2ssh.localEnvExpanded`，默认收起）；
   - 新增 `onExecuteLocal?: (target: string) => void`，点击环境项动作按钮传入 `local:${env.id}`。
3. **`src/App.tsx`**：
   - `selectedGroup` 默认值支持 `"__all__"`（并在新建/编辑主机时，若当前是 `"__all__"`，传给 `AddHostForm` 的 `initialGroup` 仍回退为 `"default"`）；
   - 抽离 `openHostInTerminal(host: string)`、`openHostInExecute(host: string)`、`openHostInFiles(host: string)` 三个稳定回调并传给 `HostList`；
   - 更新 `quickActions` 中的 `new-host`：`run: () => { openModule("hosts"); openAddHostDrawer(); }`。
4. **`src/i18n.tsx`**：补齐新增中英文文案（全部主机、搜索主机/地址/标签、属性筛选、打开终端、去执行等）。

### 1.4 验证与验收标准
* **单元测试**：
  - 更新 `src/components/HostList.test.tsx`：验证「全部」分组展示跨组主机、模糊搜索按 `name`/`host`/`tags` 过滤、子串属性筛选生效，以及点击「终端/执行/SFTP」行动作正确触发对应回调；
  - 新增 `LocalEnvironmentsCard` 测试：验证折叠切换与点击「去执行」传出 `local:<id>`。
* **手动验收**：
  - 在任意页面按 `⌘K` 选择「New host」，确认切到 Hosts 且右侧抽屉已自动滑出；
  - 在多分组主机下直接输入 IP 片段定位目标，点击行内「终端」，确认切到 Terminal 并自动创建/聚焦该主机 Tab。

---

## 任务卡 F2：命令执行中心补齐 `local:*`、批量联动与路由收敛

### 2.1 问题现状与代码定位
* **本地环境目标在执行页不可选且未接通 Tauri 命令**：
  - [`src/components/LocalEnvironmentsCard.tsx`](../../src/components/LocalEnvironmentsCard.tsx#L97) 声明可在 Execution 使用 `local:<id>`，但 [`src/components/ExecPanel.tsx`](../../src/components/ExecPanel.tsx#L182) 使用的 [`src/components/HostSelector.tsx`](../../src/components/HostSelector.tsx#L75-L86) 仅渲染 `hosts`，且在 `useEffect` 中把任何不在 `hosts` 列表里的值强制重置回 `hosts[0].name`；
  - 后端 [`src-tauri/src/core.rs`](../../src-tauri/src/core.rs#L1223-L1258) 已有完整的 `exec_local_core`，但 [`src-tauri/src/tauri_commands.rs`](../../src-tauri/src/tauri_commands.rs#L425-L432) 的 `exec_ssh` 只走 `exec_ssh_core`，尚未暴露 `exec_local` Tauri 命令。
* **批量执行 (`MultiExecPanel`) 的 Tags 逻辑悖论与交互粗糙**：
  - [`src/components/MultiExecPanel.tsx`](../../src/components/MultiExecPanel.tsx#L121-L132)：`api.execMulti` 支持传 `tags`，但运行按钮 `disabled={busy || selected.length === 0 || !command.trim()}` 阻止了纯标签执行；主机列表是一堆平铺按钮，没有按分组/标签快捷圈选，也看不到当前筛选命中了哪些主机；
  - [`src/components/MultiExecPanel.tsx`](../../src/components/MultiExecPanel.tsx#L142-L176)：执行结果把摘要列表 + 所有主机的完整 `<pre className="min-h-[24vh]">` + `HostDiffView` 垂直堆叠，主机一多只能狂滚页面；且缺乏类似 `ExecPanel` 的风险预检提示。
* **`execute` 与 `playbooks` 双路由造成状态分裂**：
  - [`src/nav.ts`](../../src/nav.ts#L79) 在侧边栏同时保留了 `execute` 与 `playbooks`，[`src/App.tsx`](../../src/App.tsx#L1369-L1410) 在两个分支里各自挂载了 `<ExecutionWorkbench>`；用户点侧边栏 `Playbooks` 后在顶部切到 `Single Host` Tab，侧边栏和页头仍高亮显示 `Playbooks`。

### 2.2 交互规范与逻辑流转
1. **单机执行支持 `SSH 主机` + `本地环境 (local:*)`**：
   - `ExecPanel` 拉取 `api.listLocalEnvironments()`，在目标下拉框中使用 `<optgroup>` 分为「SSH 主机」与「本地环境 (`local` 及 `local:<id>`)」两组；
   - 当选中目标以 `local` 或 `local:` 开头时：
     - 按钮文案自动切换为 `在本地环境运行 (Run locally)`，避免误导为“通过 SSH 运行”；
     - 提交时走 `api.execLocal(request)`；后端先做 `is_local_host` 校验，再复用 `authorize_desktop_exec_request`，最后调用 `exec_local_core`（遵循 `AGENTS.md` 中“local-host 检查必须先于鉴权链”与“鉴权共享”规则）；
     - 即使尚未配置任何 SSH 主机（`hosts.length === 0`），仍可直接在本地环境执行命令。
2. **批量执行升级为「目标圈选 + 主从结果视图」**：
   - **目标选择区**：支持按 **分组 / 已有标签 Chips** 一键勾选或反选；保留 `Tags` 过滤输入并实时计算并展示“将命中 N 台主机（含手动勾选与标签匹配去重结果）”；只要最终命中主机数 > 0 且命令非空即可执行；
   - **命令风险预检**：复用 `api.classifyRisk`（针对首个命中主机防抖预览）并在标题栏展示 `RiskBadge`；若为 `blocked` 拦截提示，若为 `high` 且未勾选 `Force` 时弹出 `ApprovalDialog` 确认（与 `ExecPanel` 行为一致）；
   - **结果浏览区（Master-Detail + Diff 切换）**：
     - 顶部结果视图切换：`[全部 ({n})] [仅失败 ({failCount})] [输出 Diff]`；
     - 在非 Diff 模式下，采用**左侧主机结果列表（状态点 + 主机名 + exit code + 耗时）+ 右侧单一输出视口**（默认自动选中首个失败主机，全部成功则选第一台），彻底告别 N 个 `24vh` 输出块纵向堆叠。
3. **收敛 `execute` / `playbooks` 路由错位**：
   - 方案采用**路由与 Tab 双向同步**（或同步收敛侧边栏重复入口）：
     - 保持 `ModuleId` 中 `playbooks` 兼容（避免破坏旧快捷入口或书签/测试引用），但当用户在 `ExecutionWorkbench` 内切换 Tab 时，若当前 `activeModule === "playbooks"` 且切到 `single`/`multi`，通过 `onModeChange` 同步将 `activeModule` 切回 `"execute"`；反之切到 `playbooks` Tab 可保持统一在 `execute` 下展示（或同步高亮）。
     - 若确认侧边栏不再需要单独保留 `Playbooks` 菜单项，可同步从 `SIDEBAR_GROUPS` 移除 `playbooks`（并同步更新 `nav.test.ts`），把左侧 Operations 舱再精简 1 项。*(实施前与用户确认是否直接移除侧边栏 `Playbooks` 项，默认建议移除以彻底消除双入口歧义。)*

### 2.3 技术改动清单
1. **Rust 后端（最小增量）**：
   - [`src-tauri/src/tauri_commands.rs`](../../src-tauri/src/tauri_commands.rs)：新增 `#[tauri::command] pub async fn exec_local(mut request: ExecRequest) -> Result<ExecResult, String>`：
     1. 检查 `is_local_host(&request.host)`，非法直接返回错误（早于鉴权链）；
     2. 补齐 `source`、`ensure_command_length`、`authorize_desktop_exec_request(&mut request)`；
     3. 调用 `exec_local_core(request)` 并返回 `ExecResult`。
   - [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs)：在 `tauri::generate_handler!` 注册 `tauri_commands::exec_local`。
2. **`src/api.ts`**：新增 `execLocal: (request: ExecRequest) => invoke<ExecResult>("exec_local", { request })`。
3. **`src/components/HostSelector.tsx`**：
   - 增加可选 `extraGroups?: Array<{ label: string; options: Array<{ value: string; label: string }> }>`，允许调用方声明合法的附加目标（如 `local:*`），防止 `useEffect` 将合法的 `local:*` 重置为第一个 SSH 主机。
4. **`src/components/ExecPanel.tsx`**：
   - 挂载时调用 `api.listLocalEnvironments()`，传入 `HostSelector` 的 `extraGroups`；
   - 按 `targetHost === "local" || targetHost.startsWith("local:")` 分流调用 `api.execLocal` / `api.execSshFull`，并动态切换运行按钮文案。
5. **`src/components/MultiExecPanel.tsx`**：
   - 新增按分组/标签快捷筛选与命中主机集合预览；修正按钮 `disabled` 条件（`effectiveTargets.length === 0`）；
   - 补齐防抖 `classifyRisk` 与高危 `ApprovalDialog`；
   - 结果区重构为 `all | failed | diff` 切换 + 左右主从输出面板。
6. **`src/components/ExecutionWorkbench.tsx`** 与 **`src/App.tsx`**（及按需 `src/nav.ts` / `src/nav.test.ts`）：
   - 消除 `App.tsx` 中两份重复的 `<ExecutionWorkbench>` 分支，确保 Tab 与导航标题一致。

### 2.4 验证与验收标准
* **单元测试**：
  - Rust：`cargo test --manifest-path src-tauri/Cargo.toml --lib` 新增 `exec_local` 命令单元测试（非法非 local host 在鉴权前拒绝；合法 `local` 执行通过）；
  - 前端：更新 `ExecPanel.test.tsx`（选中 `local:*` 时调用 `api.execLocal` 且切换按钮文案）；新增 `MultiExecPanel.test.tsx`（仅填 Tags 可运行、点击分组/标签快捷选中、失败筛选与主从切换、高危命令弹窗确认）；更新 `ExecutionWorkbench.test.tsx`。
* **手动验收**：
  - 在主机页展开本地环境点击「去执行」，确认 `ExecPanel` 自动定位到 `local:<id>` 并可跑通 `echo hello`；
  - 在 Multi-Host 页按标签运行命令，验证失败主机优先选中及 Diff 视图切换无嵌套滚动冲突。

---

## 任务卡 F3：审计页图表联动筛选、详情抽屉与筛选状态修复

### 3.1 问题现状与代码定位
* **图表与明细互相隔离**：
  - [`src/App.tsx`](../../src/App.tsx#L1412-L1421)：`AuditCharts` 与 `AuditPanel` 只是静态上下排列；[`src/components/AuditCharts.tsx`](../../src/components/AuditCharts.tsx#L86-L103) 独立拉取 30 天 / 5000 条数据，点击柱状图（风险分布 / 来源排行）或主机热力图单元格没有任何联动效果。
* **存在明确的筛选状态与计数 Bug**：
  - [`src/components/AuditPanel.tsx`](../../src/components/AuditPanel.tsx#L239-L259)：`riskCounts` 从已过滤的 `audit` 列表计算。当用户点击 `high` 风险胶囊触发 `onRefresh({ risk_level: "high" })` 后，返回列表只剩 `high`，导致 `low / medium / blocked` 胶囊上的数字全部变成 `0`；
  - [`src/components/AuditPanel.tsx`](../../src/components/AuditPanel.tsx#L289)：点击右上角刷新图标直接调用 `onRefresh()`（未携带当前 `hostFilter / riskFilter / limit`），导致表格刷新成了未筛选的全部记录，但 `high` 胶囊仍保持高亮选中态。
* **明细查看与后续动作受限**：
  - [`src/components/AuditPanel.tsx`](../../src/components/AuditPanel.tsx#L144-L162)：命令列用 `truncate` 单行截断，审批改写前的 `original_command` 仅藏在原生 `title` 属性里，且 `reason`、`change_id`、`source`、`decided_by` 未完整呈现；
  - 行操作仅有「Replay（打开终端抽屉直接敲命令）」，无法「带入执行台修改后再执行」。

### 3.2 交互规范与逻辑流转
1. **统一审计工作台容器 (`AuditWorkbench`)**：
   - 将 `AuditCharts` 与 `AuditPanel` 纳入统一的 `AuditWorkbench` 容器管理共享筛选态：`{ range, host, riskLevel, source, search }`；
   - **图表反向联动**：
     - 点击 `Risk distribution` 某根柱子 → 切换 `riskLevel` 筛选（再次点击取消）；
     - 点击 `Executions by source` 某一行 → 切换 `source` 筛选；
     - 点击 `Host activity by hour` 的主机名或格子 → 切换 `host` 筛选；
   - 表格顶部展示**已激活筛选标签条**（如 `主机: prod-1 ×`、`来源: mcp ×`、`风险: high ×`、`一键清空`），让联动状态一目了然。
2. **修复计数与刷新一致性**：
   - 风险胶囊计数基于当前「时间范围 + 主机 + 来源」过滤后、**尚未按 `riskLevel` 裁切**的数据集计算，切换 `high` 时其他等级计数保持真实可切换；
   - 点击刷新按钮重新拉取数据，同时**完整保留当前所有筛选条件**；
   - 避免 `AuditCharts` 与 `AuditPanel` 重复发两次全量请求（容器统一拉取一次，或仅在突破时间窗口/条数上限时重查）。
3. **审计详情抽屉 + 带入执行台**：
   - 点击审计表格任一行（排除复选框和 Replay 按钮）滑出右侧详情抽屉（复用 `SlideOverDrawer`）：
     - 完整多行命令展示（带一键复制）；
     - 若存在 `original_command`，以醒目的「原始请求 vs 审批改写后执行」上下对照块展示；
     - 完整元数据：时间、主机、耗时、退出码、风险等级、决策状态、审批人（`decided_by`）、来源（`source`）、原因（`reason`）、变更单号（`change_id`）；
     - 底部主动作：`[在终端抽屉重放]`、`[带入执行台编辑并运行]`（复用 T1 的 Command Carryover 链路）。

### 3.3 技术改动清单
1. **新建 `src/components/AuditWorkbench.tsx`**：
   - 持有共享筛选状态、统一数据拉取与 SSE 自动刷新（复用 `useAgentEvents`）；
   - 协调 `AuditCharts` 与 `AuditPanel` 的双向筛选联动。
2. **`src/components/AuditCharts.tsx`**：
   - 支持由外部传入 `entries`、`range`、`onRangeChange` 及选中高亮/点击回调：`activeRisk`、`activeSource`、`activeHost`、`onSelectRisk`、`onSelectSource`、`onSelectHost`；
   - 为可点击图表项补充 `cursor-pointer` 与选中高亮态。
3. **`src/components/AuditPanel.tsx`**：
   - 接收筛选基数 `riskCounts`（或由未按风险过滤的列表派生）修复归零 Bug；
   - 修复刷新按钮保留筛选参数；
   - 增加行点击打开详情抽屉与 `onEditAndExecute?: (entry: AuditEntry) => void`。
4. **`src/App.tsx`**：
   - 将 `activeModule === "audit"` 替换为 `<AuditWorkbench onReplay={replayAuditEntry} onEditAndExecute={goToExecuteFromAudit} />`；
   - 新增 `goToExecuteFromAudit(entry: AuditEntry)`（与 `goToExecuteForApproval` 共用 `selectedHost / execInitialCommand / execCarryEpoch / execPreferredMode`）。

### 3.4 验证与验收标准
* **单元测试**：
  - 新增 `src/components/AuditWorkbench.test.tsx`：
    1. 点击 `high` 风险胶囊后，其他风险胶囊计数不归零；
    2. 点击刷新后保持当前 `riskFilter` / `hostFilter` 不漂移；
    3. 点击热力图主机或来源联动过滤下方表格；
    4. 点击行展开详情抽屉，展示 `original_command` 对比，点击「带入执行台」正确回调 `host` 与 `command`。
* **手动验收**：
  - 在审计页点击高危柱子，确认下方列表即时聚焦高危记录且顶部出现可一键移除的筛选 Chip；
  - 点开一条改写过的审计记录，查看改写前后内容并点「带入执行台」，确认无缝跳转并自动触发风险评级。

---

## 任务卡 F4：隧道 / 代理 / 密钥页抽屉范式统一、动态轮询与依赖追溯

### 4.1 问题现状与代码定位
* **布局范式与 T2 主机页割裂**：
  - [`src/components/ForwardPanel.tsx`](../../src/components/ForwardPanel.tsx#L100)：左侧常驻 `minmax(340px, 0.48fr)` 的「新建隧道」表单；
  - [`src/components/ProxyPanel.tsx`](../../src/components/ProxyPanel.tsx#L113)：右侧常驻 `360px` 的代理表单；
  - 两页在 90% 的查看时间里都被空表单占去近半屏幕。
* **隧道页状态与流量是静态快照，且无法编辑/复用**：
  - [`src/components/ForwardPanel.tsx`](../../src/components/ForwardPanel.tsx#L60-L62)：仅在挂载时调用一次 `refresh()`，之后 `starting → active / error` 状态转换和 `connections / bytes_tx / bytes_rx` 完全不会自动刷新；
  - 规则卡片只有删除按钮，修改端口或复用配置只能删掉重填 6 个字段。
* **资源依赖关系不可见**：
  - [`src/components/ProxyPanel.tsx`](../../src/components/ProxyPanel.tsx#L125-L140)：仅显示“N 台主机”，无法看到具体是哪些主机、也无法跳转；
  - [`src/components/KeysPanel.tsx`](../../src/components/KeysPanel.tsx#L15-L18)：未接收 `hosts` prop，完全不展示哪些主机正在引用该私钥（`HostProfile.key_path` 与 `SshKeyInfo.private_path`），删除密钥前没有影响范围提示。

### 4.2 交互规范与逻辑流转
1. **提炼通用右侧抽屉 `SlideOverDrawer`**：
   - 将 `HostDrawer` 的遮罩、滑入动画、ESC 关闭、标题栏抽象为 `src/components/ui/slide-over-drawer.tsx`，`HostDrawer` 改为直接复用它。
2. **端口隧道页 (`ForwardPanel`) 改造**：
   - 主区域改为**全宽隧道列表**，顶部工具栏右侧提供主按钮 `[+ 新建隧道]`（呼出抽屉）；
   - **活跃态静默轮询**：当 `rules.length > 0` 且页面处于可见状态时，每 `3000ms` 静默轮询 `forwardStats()`（使用 `keepIfEqual` 避免无变化重渲染，组件卸载自动清理）；
   - 每条规则卡片提供 `[复制/编辑重建]`（将当前规则参数预填进抽屉，确认后替换或新增）与 `[删除]`；同时支持按主机快速筛选隧道。
3. **代理管理页 (`ProxyPanel`) 改造**：
   - 改为**全宽代理列表 + 右侧抽屉（新增/编辑）**；
   - 每张代理卡片下方列出**关联主机 Chips**（超出 5 个折叠显示 `+N`），点击某个主机 Chip 可直接跳至 `Hosts` 并选中/高亮该主机；
   - 删除代理的确认弹窗中，若存在关联主机，明确列出受影响主机名清单。
4. **密钥管理页 (`KeysPanel`) 增强依赖追溯**：
   - 从 `App.tsx` 传入 `hosts`，按路径归一化（兼容 `~/` 与绝对路径、`/` 与 `\`）计算每把密钥被哪些主机引用；
   - 在密钥卡片展示引用主机 Chips（点击跳主机）；删除确认框若发现有主机在用，给出醒目警示列表。

### 4.3 技术改动清单
1. **新建 `src/components/ui/slide-over-drawer.tsx`**，并重构 [`src/components/HostDrawer.tsx`](../../src/components/HostDrawer.tsx) 为薄封装（保持 `HostDrawer.test.tsx` 原有 `data-testid` 与行为 100% 兼容）。
2. **`src/components/ForwardPanel.tsx`**：
   - 移除左右两栏常驻表单布局，将新建/复制编辑表单迁入 `SlideOverDrawer`；
   - 增加 `3s` `forwardStats` 静默轮询（仅在 `rules.length > 0` 时启用，且支持错误静默不刷屏 Toast）；
   - 新增按主机筛选条与「复制参数到新建抽屉」按钮。
3. **`src/components/ProxyPanel.tsx`**：
   - 移除右侧常驻表单，改为 `SlideOverDrawer`；
   - 新增可选回调 `onSelectHost?: (hostName: string) => void`，展示关联主机 Chips 与删除影响清单。
4. **`src/components/KeysPanel.tsx`**：
   - 新增 Props `hosts?: HostProfile[]` 与 `onSelectHost?: (hostName: string) => void`；
   - 新增路径匹配工具 `keyMatchesHost(key: SshKeyInfo, host: HostProfile): boolean`；
   - 展示关联主机 Chips，删除前展示受影响主机告警。
5. **`src/App.tsx`**：向 `ProxyPanel`、`KeysPanel` 透传 `hosts` 与 `openHostInFleet(hostName)`（切至 `hosts`、`selectedGroup="__all__"`、选中并高亮该主机）。

### 4.4 验证与验收标准
* **单元测试**：
  - `HostDrawer.test.tsx` 全部通过；
  - 新增 `ForwardPanel.test.tsx`：验证点击「新建隧道」打开抽屉、保存后关闭、以及使用 fake timers 验证有规则时 3s 自动轮询 `forwardStats`、无规则时不空轮询；
  - 更新 `KeysPanel.test.tsx` 与新增 `ProxyPanel.test.tsx`：验证关联主机 Chips 展示与删除告警清单。
* **手动验收**：
  - 打开隧道/代理页，确认全宽展示无空白表单占位；启动一条本地转发并访问端口，确认不点刷新即可实时看到连接数与收发字节数跳动。

---

## 任务卡 F5：总览仪表盘可操作化 + 连通性探测结果联动

### 5.1 问题现状与代码定位
* **半数状态卡片不可点击，发现问题不能就地处理**：
  - [`src/components/Dashboard.tsx`](../../src/components/Dashboard.tsx#L125-L176)：6 张卡片中只有 `Host health`、`Pending approvals`、`24h executions` 绑定了 `onClick`；`Anomaly alerts`、`Credential store`、`Local daemon` 纯展示，用户看到异常/锁定/离线时无法点击直达处理。
* **`24h executions` 加载态与失败态混淆（已知历史问题 `L2-63`）**：
  - [`src/components/Dashboard.tsx`](../../src/components/Dashboard.tsx#L156-L161)：`execCount24h` 用 `null` 同时表示「加载中」和「查询失败」，Daemon 离线时永远显示 `…`。
* **连通性探测 (`PingPanel`) 结果是死胡同**：
  - [`src/components/PingPanel.tsx`](../../src/components/PingPanel.tsx#L74-L98)：结果按原数组顺序展示，不可达主机容易沉在长列表底部；且单行纯静态展示，不能直接对可达主机开终端、或对失败主机跳转检查配置。

### 5.2 交互规范与逻辑流转
1. **6 张核心卡片全部闭环可操作**：
   - `Anomaly alerts`：点击跳转 `activity` 模块并自动带入异常预设筛选（`LiveActivityPanel` 新增 `initialFilter="anomalies"` 支持）；
   - `Credential store`：若处于 `secretsLocked` 状态，点击直接触发解锁弹窗（`onRequestUnlock`）；已解锁时跳转 `keys` 模块；
   - `Local daemon`：离线时点击直接触发 `onStartDaemon`（调用已有 `ensureLocalDaemon()` 并显示启动中状态）；在线时显示版本并可点击查看 `activity` 或打开诊断。
2. **修复 `24h executions` 三态展示**：
   - 将 `execCount24h` 状态拆为 `{ status: "loading" } | { status: "ready"; count: number } | { status: "error" }`：加载中显示 `…`，失败/离线时明确显示 `—` 与提示 `Unavailable`。
3. **Ping 结果智能排序与单行直达**：
   - 探测完成后默认按 **`不可达优先 → 延迟从高到低`** 排序（方便第一时间发现故障节点），并提供 `[全部 / 仅异常]` 快捷过滤；
   - 每行悬浮或末尾提供轻动作：`[打开终端]`（可达主机主推荐）、`[查看主机]`（定位到 Hosts 页编辑排查）。

### 5.3 技术改动清单
1. **`src/components/Dashboard.tsx`**：
   - 新增 Props：`onRequestUnlock?: () => void`、`onStartDaemon?: () => void | Promise<void>`、`onOpenAnomalies?: () => void`、`onOpenHostTerminal?: (host: string) => void`、`onInspectHost?: (host: string) => void`；
   - 拆分 `execCountState` 三态，修复 `L2-63`；
   - 为剩余 3 张卡片绑定交互与 hover 反馈。
2. **`src/components/PingPanel.tsx`**：
   - 增加结果排序（不可达置顶）与「仅显示不可达」开关；
   - 新增 `onOpenTerminal?: (host: string) => void`、`onInspectHost?: (host: string) => void` 行内动作按钮。
3. **`src/components/LiveActivityPanel.tsx`**：
   - 新增可选 Prop `presetFilter?: "all" | "attention"`（配合 epoch 触发），当从 Dashboard 异常卡片跳入时自动仅展示高危/异常/审批关注项，并允许一键切回「全部」。
4. **`src/App.tsx`**：向 `Dashboard` 和 `LiveActivityPanel` 透传上述回调与预设筛选态。

### 5.4 验证与验收标准
* **单元测试**：
  - 新增 `src/components/Dashboard.test.tsx`：验证 6 张卡片点击动作、审计读取失败时渲染 `—` 而非 `…`；
  - 新增 `src/components/PingPanel.test.tsx`：验证不可达主机自动排在首位、点击行内动作触发 `onOpenTerminal` / `onInspectHost`。
* **手动验收**：
  - 模拟 Daemon 停止或凭据锁定，在 Dashboard 直接点击对应卡片确认可就地拉起启动/解锁；
  - 执行 `Ping all hosts`，确认失败节点置顶且可一键打开终端或跳转排查主机配置。

---

## 任务卡 F6：全局页头动态上下文 + 满高工作区双滚动条收口

### 6.1 问题现状与代码定位
* **全站页头副标题千篇一律**：
  - [`src/App.tsx`](../../src/App.tsx#L1275-L1289)：除 `hosts` 页外，所有模块（Approvals、Audit、Sync、Keys、MCP/CLI、Containers、Recordings、Config Snapshots 等）的页头副标题都固定渲染 `"{count} hosts configured"`，既无业务信息又白占纵向高度。
* **满高工作区容器覆盖不全导致双滚动条**：
  - [`src/App.tsx`](../../src/App.tsx#L1267-L1274) 与 [`L1299-L1304`](../../src/App.tsx#L1299-L1304)：仅对 `execute` 和 `playbooks` 启用了 `overflow-hidden` + `flex min-h-0 flex-1 flex-col`，而 [`SFTPPanel.tsx`](../../src/components/SFTPPanel.tsx)、[`RecordingsPanel.tsx`](../../src/components/RecordingsPanel.tsx)、[`ConnectionTopology.tsx`](../../src/components/ConnectionTopology.tsx) 均拥有内部视口/滚动画布，套在外层 `overflow-y-auto` 容器里容易产生外层+内层双滚动条。

### 6.2 交互规范与逻辑流转
1. **页头上下文精准化与纵深压缩**：
   - 将页头标题与 `RelatedActionsBar` 的间距适度收紧（桌面宽屏下可将快捷胶囊移至页头右侧或紧凑单行），为下方功能主视口腾出更多纵向空间；
   - 按模块提供真正有价值的动态副标题（抽取纯函数 `moduleSubtitle(module, ctx)`）：
     - `approvals`：`{pending} 项待审批` / `暂无待审请求`；
     - `hosts`：当前选中主机连接串（沿用现有逻辑）或 `{connected}/{total} 台已连接`；
     - `execute` / `playbooks`：当前目标主机或“单机 / 批量 / 剧本统一执行工作台”；
     - `files-sessions`：`当前目标：{selectedHost}`；
     - `tunnels`：`端口转发规则与实时流量监控`；
     - `audit` / `recordings` / `activity` / `sync` / `config` / `mcp-agents` / `containers` / `resident-sessions` / `topology` / `proxies` / `keys`：各自简短的一句模块语义说明或关键状态数。
2. **明确划分两类主区容器模式**：
   - 在 `src/nav.ts` 中显式声明 `FULL_HEIGHT_MODULES: ReadonlySet<ModuleId>`（包含 `execute`、`playbooks`、`files-sessions`、`topology`、`recordings`）；
   - 这些模块统一获得 `overflow-hidden` + `flex min-h-0 flex-1 flex-col` 满高约束，内部面板（`SFTPPanel` 非 embedded 态、`ConnectionTopology` 画布、`RecordingsPanel` 播放器）用 `min-h-0 flex-1` 吃满剩余高度，彻底消除内外双滚动条；
   - 其余自然文档流页面保持外层 `overflow-y-auto`。

### 6.3 技术改动清单
1. **`src/nav.ts`**：导出 `FULL_HEIGHT_MODULES` 集合（并在 `nav.test.ts` 中校验其所有成员均为合法 `ModuleId`）。
2. **`src/App.tsx`**：
   - 提取 `getModuleSubtitle(...)` 替换硬编码的 `"{count} hosts configured"`；
   - 使用 `FULL_HEIGHT_MODULES.has(activeModule)` 统一控制外层 `<section>` 与 `.module-page` 的满高布局；
   - 优化宽屏下 `<header>` 与 `<RelatedActionsBar>` 的紧凑排版。
3. **`src/components/SFTPPanel.tsx` / `src/components/ConnectionTopology.tsx` / `src/components/RecordingsPanel.tsx`**：
   - 检查顶层容器 class，确保在满高模式下使用 `min-h-0 flex-1` 自适应剩余视口，不写死超高的 `min-h` 溢出容器。
4. **`src/i18n.tsx`**：补齐各模块动态副标题的中英文翻译。

### 6.4 验证与验收标准
* **单元测试**：
  - `src/nav.test.ts` 验证 `FULL_HEIGHT_MODULES` 合法性；
  - 新增 `moduleSubtitle` 辅助函数单测，确认不同模块返回对应上下文摘要而非统一 `"N hosts configured"`；
  - `SFTPPanel.test.tsx` 保持全部通过。
* **视觉与手动验收**：
  - 在 1280×800 常见笔记本窗口尺寸下依次切换 `Files`、`Topology`、`Recordings`、`Execute`，确认无外层+内层双滚动条冲突；
  - 逐个模块检查页头副标题，确认文案与当前功能语义一致且中英文无漏翻。

---

## 实施守则与回归门禁

1. **单卡独立闭环**：每完成一张任务卡（F1 → F6）独立提交前，必须全部通过：
   - 前端行为测试：`npm test`
   - 前端类型检查与构建：`npm run build`
   - 涉及 Rust 改动的卡片（F2）：
     - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
     - `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`
     - `TAURI_CONFIG='{"bundle":{"externalBin":[]}}' cargo test --manifest-path src-tauri/Cargo.toml --lib`
2. **体验回归底线**：
   - 全局快捷键（`⌘K`、`⌘1..9`、`⌘⇧A`、`Hosts` 页 `⌘N`、`ESC` 关闭抽屉）全程无回归；
   - 所有新增 UI 文案必须同步补齐 `src/i18n.tsx` 中英文条目并通过 `src/i18n.test.tsx` 检查；
   - 保持 `App.tsx` 高频轮询的 `keepIfEqual` / `React.memo` 优化，不因新增回调引入无谓整树重渲染。

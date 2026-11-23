# Agent2SSH 前端交互与布局重构：详细任务卡工程规范

> **工程导向**：遵循 Pre-1.0 **「体验 > 功能 > 安全」** 最高原则。  
> **指导思想**：不盲目推倒重来，基于现有成熟组件与 API，逐个解决操作割裂、视觉压迫、平铺臃肿与数据丢弃问题。

---

## 任务卡 T1：打通审批台到执行台的数据链路（Command Carryover）

### 1.1 问题现状与代码定位
* **问题定位**：
  - 文件：[`src/components/approval/ApprovalWorkbench.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/components/approval/ApprovalWorkbench.tsx#L29-L34)
    ```typescript
    /** Fallback for requests that cannot be rewritten: open the Execution module
     *  with this approval's host selected. The command is **not** carried over --
     *  `App.tsx`'s handler only sets the host and switches module, and the
     *  Execution panel still starts from its own default command. */
    onGoExecute: (approval: ApprovalRequest) => void;
    ```
  - 文件：[`src/components/approval/ApprovalDetail.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/components/approval/ApprovalDetail.tsx#L173-L179)
    - 按钮文案为 `Copy command and open Execution`。代码虽然尝试了 `handleCopy()`（写入剪贴板），但如果剪贴板权限受限或执行台没有粘贴，用户切过去看到的输入框依然是固定的 `uname -a`；
  - 文件：[`src/App.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/App.tsx#L766-L772)
    ```typescript
    const goToExecuteForApproval = useCallback(
      (approval: ApprovalRequest) => {
        setSelectedHost(approval.host);
        openModule("execute");
      },
      [openModule],
    );
    ```
    这里只设置了 `selectedHost` 并切换到 `execute` 模块，完全丢弃了 `approval.command`。
  - 文件：[`src/components/ExecPanel.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/components/ExecPanel.tsx#L26)
    - `ExecPanel` 内部状态写死 `const [command, setCommand] = useState("uname -a");`，不支持从外部传入初始命令或响应命令变更。

### 1.2 交互规范与行为流转
1. **触发入口**：
   - 入口 A：当审批卡片不可改写（如 `blocked` 或外部策略限制）时，右下角「复制并前往执行台」主操作按钮；
   - 入口 B（体验增强）：对任意待审批卡片，提供快捷操作「在执行台打开与调试」，便于运维人员在不同参数/标准输入下测试。
2. **交互流转**：
   - 用户点击「在执行台中打开与调试」；
   - 界面无缝平滑切至 `Execute` 模块；
   - `HostSelector` 自动定位到 `approval.host`；
   - 命令输入框高亮填入 `approval.command`，并触发一次防抖的前置风险评级（`classifyRisk`），输入框下方立即展示对应的风险徽标（RiskBadge）；
   - 界面弹出非阻断式轻 Toast：`已载入待审命令：{command}`，焦点自动落入命令编辑区末尾。
3. **边缘情况与防御**：
   - 若 `approval.command` 为多行命令或带有换行符，输入框（Textarea）自适应展开高度，避免遮挡；
   - 若用户之前在 ExecPanel 有未执行的草稿，先暂存或提示覆盖，优先以审批跳转指令为准。

### 1.3 技术改动清单
1. **`src/components/ExecPanel.tsx`**：
   - 增加 Prop：`initialCommand?: string;`
   - 增加状态监听：当外部 `initialCommand` 变化且非空时，更新内部 `command` 状态：
     ```typescript
     useEffect(() => {
       if (initialCommand !== undefined) {
         setCommand(initialCommand);
       }
     }, [initialCommand]);
     ```
2. **`src/components/approval/ApprovalWorkbench.tsx`** 与 **`src/components/approval/ApprovalDetail.tsx`**：
   - 调整 `onGoExecute` 签名：
     ```typescript
     onGoExecute: (approval: ApprovalRequest, command?: string) => void;
     ```
     如果用户在详情页正在编辑 `draft`，优先传递 `draft.trim() || approval.command`。
3. **`src/App.tsx`**：
   - 增加顶层状态：`const [execInitialCommand, setExecInitialCommand] = useState<string | undefined>();`
   - 更新 `goToExecuteForApproval`：
     ```typescript
     const goToExecuteForApproval = useCallback(
       (approval: ApprovalRequest, command?: string) => {
         setSelectedHost(approval.host);
         setExecInitialCommand(command ?? approval.command);
         openModule("execute");
       },
       [openModule],
     );
     ```
   - 在渲染 `<ExecPanel />` 时透传 `initialCommand={execInitialCommand}`。

### 1.4 验证与验收标准
- **单元测试**：
  - 更新 `ApprovalDetail.test.tsx`，断言点击“复制并执行”时，`onCopyAndExecute` 正确透传当前指令；
  - 为 `ExecPanel` 补充测试用例：断言当传入 `initialCommand="docker ps"` 时，输入框初始值为该命令并触发风险预览。
- **手动验收**：
  - 构造一条拦截审批，进入审批工作台，点击前往执行台；
  - 确认自动跳转并准确带入主机和待审命令，无需手动二次粘贴。

---

## 任务卡 T2：主机管理全宽视野改造（HostList Drawer Form）

### 2.1 问题现状与代码定位
* **问题定位**：
  - 文件：[`src/App.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/App.tsx#L1294)
    ```typescript
    <div className="grid grid-cols-[minmax(0,1.4fr)_minmax(340px,0.6fr)] items-start gap-[18px] max-lg:grid-cols-1">
      <HostList ... />
      <AddHostForm ... />
    </div>
    ```
  - `AddHostForm` 恒定占据 340px（近 40% 屏幕宽度）。
  - 在用户 90% 的使用时间里（查看主机健康度、搜索标签、管理分组、连接/断开主机），右侧常驻一个空表单，导致左侧 `HostList` 内表格仅剩 60% 宽度，多列文字被挤压截断，甚至产生难看的横向滚动条。
  - 用户想添加主机时，视线需要跳跃到右侧狭长的表单卡片中；编辑已有主机时，右侧卡片标题悄悄变为“编辑主机”，容易引起混淆。

### 2.2 交互规范与行为流转
1. **默认状态（全宽视野）**：
   - `HostList` 占据 100% 容器宽度；
   - 表格各列（连接状态、主机名、`user@host:port`、分组、标签 Chips、环境/角色、策略行、快捷动作）完全舒展展开，数据信息密度极佳；
   - 顶部工具栏在「主机筛选」和「刷新」旁，新增醒目的电光青高亮主按钮：`[+ 添加主机]`。
2. **呼出抽屉（Slide-over Drawer）**：
   - **触发时机 1**：点击工具栏的 `[+ 添加主机]`；
   - **触发时机 2**：点击表格任一行的 `[编辑 (Edit3)]` 图标；
   - **触发时机 3**：快捷键 `⌘N`（在 Hosts 页面下直接呼出新增主机）；
   - **动画与样式**：
     - 从屏幕右侧平滑滑出宽度 `460px` 的抽屉面板（采用 `bg-canvas-2`，带 1px `border-l border-line`）；
     - 背景覆盖半透明遮罩（`bg-black/50 backdrop-blur-xs`），点击遮罩或右上角 `X` 按钮关闭抽屉；
     - 抽屉内承载现有的完整 `AddHostForm`（表单各字段输入区、密钥选择器、代理选择、高级 OpenSSH 导入）；
     - 抽屉顶栏清晰标明：`添加新主机` 或 `编辑主机：{host.name}`。
3. **完成与取消流转**：
   - 点击「保存」成功后，触发 `onSaved`，抽屉自动平滑收起，主界面表格实时高亮并滚动到新添加/更新的主机；
   - 点击「取消」或按 `ESC` 键，抽屉收起，不产生任何脏数据。

### 2.3 技术改动清单
1. **创建抽屉外壳组件 `src/components/HostDrawer.tsx`（或利用现有的 Dialog/Drawer 规范）**：
   - 统一管理蒙层、滑入动画、ESC 监听与标题渲染；
   - 抽屉内部包裹 `AddHostForm`。
2. **`src/components/HostList.tsx`**：
   - 在顶部卡片标题栏增加 `onOpenAddHost: () => void;` 触发按钮：
     ```tsx
     <Button size="sm" onClick={onOpenAddHost}>
       <Plus size={14} />
       {t("Add Host")}
     </Button>
     ```
3. **`src/App.tsx`**：
   - 增加状态：`const [isHostDrawerOpen, setIsHostDrawerOpen] = useState(false);`
   - 将现有 grid 布局改为单列全宽：
     ```tsx
     <div className="space-y-4">
       <LocalEnvironmentsCard />
       <HostList
         ...
         onOpenAddHost={() => { setEditingHost(null); setIsHostDrawerOpen(true); }}
         onEdit={(host) => { setEditingHost(host); setIsHostDrawerOpen(true); }}
       />
       <HostDrawer
         open={isHostDrawerOpen}
         onClose={() => { setIsHostDrawerOpen(false); setEditingHost(null); }}
       >
         <AddHostForm
           ...
           onSaved={() => { setIsHostDrawerOpen(false); setEditingHost(null); refresh(); }}
           onCancelEdit={() => { setIsHostDrawerOpen(false); setEditingHost(null); }}
         />
       </HostDrawer>
     </div>
     ```

### 2.4 验证与验收标准
- **单元测试**：
  - 更新 `HostList.test.tsx`，验证点击「添加主机」按钮触发回调；
  - 验证编辑主机触发回调并打开抽屉。
- **手动验收**：
  - 检查 100% 宽度下主机表格布局，确认无异常换行和横向溢出；
  - 测试打开抽屉 -> 填写信息 -> 保存 -> 抽屉关闭并列表刷新的完整流程；
  - 验证 ESC 键与点击遮罩正常关闭抽屉。

---

## 任务卡 T3：执行中心分段 Tabs 模式化（Single | Multi-Host | Playbooks）

### 3.1 问题现状与代码定位
* **问题定位**：
  - 文件：[`src/App.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/App.tsx#L1338-L1341)
    ```typescript
    {activeModule === "execute" && (
      <div className="grid gap-[18px]">
        <ExecPanel hosts={hosts} initialHost={selectedHost} onExecComplete={refreshAudit} />
        <MultiExecPanel hosts={hosts} onExecComplete={refreshAudit} />
      </div>
    )}
    ```
  - `ExecPanel`（单机）和 `MultiExecPanel`（批量）直接垂直堆叠；
  - `MultiExecPanel` 功能极重：包含标签/主机多选、批量执行策略（全部/并发/顺序/遇到错误停止）、执行结果输出折叠与侧向 Diff 对比；
  - 两个面板垂直串联导致单个屏幕无法完整呈现执行输出结果，用户频繁在长页面上下滚屏，严重破坏交互节奏；
  - 此外，`PlaybooksPanel`（剧本编排）在逻辑上属于“高级批处理任务执行”，却被扔在侧边栏单独成项。

### 3.2 交互规范与行为流转
1. **分段切换控制器（Top Segmented Control）**：
   - 在 Execute 页面顶部提供紧凑的药丸分段标签切换：
     - `[ 单机即时执行 (Single Host) ]`
     - `[ 批量集群执行 (Multi-Host & Diff) ]`
     - `[ 剧本任务编排 (Playbooks) ]`
   - 切换状态支持持久化（记住用户上次使用的执行模式）；
2. **全屏沉浸式输出空间**：
   - 切换到任一模式后，当前面板独占整个主区域纵深；
   - 单机模式下：上半部分为参数与命令，下半部分为大视口命令输出，日志滚动更舒适；
   - 批量模式下：结果对比树和 Diff 区域获得足够的视窗高度，无需在页面大滚动条内嵌套小滚动条；
3. **快捷联动**：
   - 当从其他模块（如主机列表行操作或审批台）跳转到 Execute 时，自动定位并激活 `[单机即时执行]` Tab。

### 3.3 技术改动清单
1. **创建执行总览容器 `src/components/ExecutionWorkbench.tsx`**：
   - 封装 Segmented Control 头部；
   - 整合 `ExecPanel`、`MultiExecPanel` 与 `PlaybooksPanel`；
   - 统一管理执行完成后的审计刷新（`refreshAudit`）。
2. **`src/App.tsx`**：
   - 简化 `activeModule === "execute"` 渲染分支，直接挂载 `ExecutionWorkbench`；
   - 原 `activeModule === "playbooks"` 分支可保留兼容路由，并在 `ExecutionWorkbench` 中默认高亮 Playbooks Tab。

### 3.4 验证与验收标准
- **单元测试**：
  - 新增 `ExecutionWorkbench.test.tsx`，测试 Tab 切换与各子组件的条件渲染；
  - 保持现有 `MultiExecPanel` 和 `ExecPanel` 测试全部通过。
- **手动验收**：
  - 分别在单机、批量与 Playbook 标签下运行命令，检查结果区域的高度自适应与滚动体验。

---

## 任务卡 T4：重构伪面包屑为「关联操作快捷胶囊」（Quick Actions Bar）

### 4.1 问题现状与代码定位
* **问题定位**：
  - 文件：[`src/components/Breadcrumb.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/components/Breadcrumb.tsx#L19-L38)
    ```tsx
    <nav aria-label={t("Related modules")} className="flex flex-wrap items-center gap-1.5 text-sm">
      <span className="inline-flex items-center gap-1.5 font-medium text-foreground">
        <current.icon size={14} className="text-muted-foreground" />
        {t(current.label)}
      </span>
      {related.map((module) => (
        <span key={module.id} className="inline-flex items-center gap-1.5">
          <ChevronRight size={13} className="text-muted-foreground/50" />
          <button ...>{t(module.label)}</button>
        </span>
      ))}
    </nav>
    ```
  - 使用 `ChevronRight` (`>`) 符号串联平级推荐页面，使用户产生“这是目录返回路径”的心理假象；
  - 按钮样式过淡，看不出是“可执行动作”。

### 4.2 交互规范与行为流转
1. **视觉改版**：
   - 移除所有 `ChevronRight` (`>`) 箭头连接符；
   - 前置增加低饱和度的辅助提示图标与文字：`⚡ 相关动作` 或 `快捷穿梭：`；
   - 关联项渲染为轻盈的胶囊按钮（Chips / Pills）：
     - 带有清晰的边框 `border border-edge` 与圆角 `rounded-full`；
     - 图标 + 动作文案（例如：`[ 🔀 发起命令执行 ]`、`[ 📁 浏览 SFTP 文件 ]`、`[ 🌐 查看网络拓扑 ]`）；
     - Hover 态：边框高亮（`hover:border-action hover:text-action`），给用户明确的点击预期；
2. **无缝平滑跳转**：
   - 点击胶囊直接切换模块，保留当前已选中的主机上下文。

### 4.3 技术改动清单
1. **重命名并改造 `src/components/Breadcrumb.tsx` -> `src/components/RelatedActionsBar.tsx`**：
   - 调整结构，移除级联嵌套 DOM；
   - 应用新组件样式与 i18n 文案；
2. **`src/App.tsx`**：
   - 替换引用 `<RelatedActionsBar current={activeModuleMeta} related={relatedModules} onNavigate={navigateModule} />`。

### 4.4 验证与验收标准
- **单元测试**：
  - 更新对应的组件测试，断言 DOM 中不再包含 Chevron 级联图标，且胶囊按钮正确触发导航回调；
- **视觉验收**：
  - 检查深色主题下胶囊按钮对比度与对齐情况。

---

## 任务卡 T5：侧边栏 20 模块聚合为 4 大功能舱

### 5.1 问题现状与代码定位
* **问题定位**：
  - 文件：[`src/nav.ts`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/nav.ts#L37-L93)
  - 侧边栏平铺了 20 个项目，虽然在视觉上分为 Pinned、Operations、Observability、System、Help，但在常规高度屏幕上几乎必定出现滚动条，找一个功能需要先想是在 Operations 还是 Observability，还要滚屏找。
  - 快捷键 `⌘1..9` 映射在 `SIDEBAR_ORDER` 上（见 [`nav.test.ts`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/nav.test.ts#L55-L66)），重构必须兼顾快捷键的确定性。

### 5.2 交互规范与四大功能舱划分
侧边栏采用**大业务功能舱（Cockpit Spaces）**理念组织，将用户使用场景清晰分为 4 类：

1. **审查与态势（Review & Posture）**
   - 🔴 **审批工作台（Approvals）**：保留顶部第一位，带有醒目的未决审批数字徽标（`pendingCount`）；
   - 📊 **总览仪表盘（Dashboard）**：系统全局健康度与异常警报；
   - 📜 **审计与日志（Audit & Replay）**：历史命令审计与录像回放；
   - ⚡ **实时动态（Live Activity）**：Agent 执行与 SSE 事件流。
2. **终端与运维（Terminal & Operations）**
   - 💻 **交互终端（Terminal）**：核心操作底座，内置分屏、树状主机与 SFTP 抽屉；
   - 🚀 **命令执行中心（Execute）**：T3 改造后的单机/多机/Playbook 统一面板；
   - 🔀 **端口隧道（Tunnels）**：本地与远程端口转发管理；
   - 📁 **文件传输（Files）**：独立全屏双栏 SFTP 管理。
3. **资产与网络（Fleet & Network）**
   - 🖥️ **主机管理（Hosts）**：主机资产、分组与标签管理；
   - 🌐 **网络拓扑（Topology）**：主机拓扑连通图与跳板机关系；
   - 📦 **本地与容器环境（Containers）**：Docker/Podman/WSL 本地执行环境；
   - 🛡️ **代理与密钥（Proxies & Keys）**：统一连接凭据与跳板代理配置。
4. **系统与配置（System & Settings）**
   - 🤖 **MCP Agent 绑定（MCP/CLI）**：AI 客户端与能力集成；
   - ☁️ **云同步与快照（Sync & Snapshots）**：WebDAV 同步与配置备份；
   - ❓ **帮助指南（Help）**：底部常驻。

### 5.3 交互细节：折叠与展开状态
- **展开状态（w-52）**：
  - 4 大功能舱采用清晰的分组标签与微妙的分割线隔开；
  - 每个分组保持紧凑间距，20 个模块在 800px 高度屏幕上**无需滚动即可全部展现**；
- **收起状态（w-16）**：
  - 纯图标模式，带 Tooltip 悬浮提示与审批未决红点。

### 5.4 技术改动清单
1. **`src/nav.ts`**：
   - 优化 `SIDEBAR_GROUPS` 结构；
   - 维持 `MODULES` 注册表与 `RELATED_MODULES` 一致性；
   - 确保 `SIDEBAR_ORDER` 的前 9 项保留最常用的高频操作，平滑过渡快捷键习惯。
2. **`src/nav.test.ts`**：
   - 同步更新测试断言中的 `SIDEBAR_ORDER.slice(0, 9)` 及分组翻译检查。

---

## 任务卡 T6：打通终端会话生态（Unified Terminal Session Continuity）

### 6.1 问题与交互规范
* **问题**：在审批台批准后唤起右侧 `TerminalDrawer`，用户若切回 Terminal 标签页，无法追踪刚才那个会话。
* **交互规范**：
  - 当在 `TerminalDrawer` 查看执行时，右上角提供动作按钮：`[在主终端中打开 / Maximize to Terminal]`；
  - 点击后，平滑切换到全屏 `TerminalPanel`，并在顶部 `SessionTabs` 中激活或新建该主机的终端标签页，会话不中断。

---

## 任务卡 T7：轻量化连接反馈与设置中心梳理

### 7.1 轻量连接状态反馈（Non-blocking Connection Progress）
* **现状**：点击连接时弹出的 `ConnectionProgress` Modal 强行阻断用户操作。
* **改动**：
  - 移除全屏模态弹窗，改为在 `HostList` 当前行内显示微型进度条与百分比状态；
  - 同时在 TopBar 的连接 Pill 上展示脉冲指示，用户在连接建立期间可自由浏览其他页面。

### 7.2 设置菜单分类选项卡（SettingsMenu Tabs）
* **现状**：[`SettingsMenu.tsx`](file:///Users/yuqu/Vbercodeing/agent2ssh/src/components/SettingsMenu.tsx) 超过 1100 行，弹窗内容纵向漫长。
* **改动**：
  - 弹窗内引入左侧导航或顶部 Segmented Tab：
    1. `常规与外观 (General & Appearance)`：语言、主题、快捷键指南；
    2. `守护进程与网关 (Daemon & Gate)`：Daemon 启停、诊断、执行网关开关；
    3. `凭据与安全 (Security & Secrets)`：主密码管理、脱敏规则、算法偏好；
    4. `系统日志 (Diagnostics)`：日志查看、打包导出、Webhook。

---

## 实施路线与质量守则

1. **单卡闭环原则**：
   - 每一个任务卡完成后，必须通过：
     1. `npm test`（前端 36 个测试文件、291+ 用例保持全部通过）
     2. `npm run build`（TypeScript 类型检查通过）
   - 任务卡之间解耦，支持逐卡独立审查与实机验证。
2. **体验回归门禁**：
   - 保持所有快捷键（`⌘K`、`⌘1..9`、`⌘⇧A`）无缝可用；
   - 保持 i18n 完整覆盖（中文与英文无遗漏）。

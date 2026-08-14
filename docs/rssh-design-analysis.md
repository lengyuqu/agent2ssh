# RSSH 设计分析与可吸收清单

> 面向 Agent2SSH 协作者的吸收文档。回答三个问题：RSSH 到底好在哪里、Agent2SSH 已经吸收了什么、还剩哪些设计值得搬进来（怎么搬、落到哪个文件、怎么验收）。
>
> 版本：2026-08-13 · 基于 `demo/rssh-main` 与当前 Agent2SSH 源码对比

---

## 0. 三分钟结论

1. **两个项目不是竞品，是同一枚硬币的两面。**
   - RSSH = 给人用的 SSH 客户端，AI 是内置排障助手（human-in-the-loop）。
   - Agent2SSH = 给 Agent 用的 SSH 能力层，通过 MCP/daemon 把 SSH 暴露给外部 Agent，安全审批审计是核心价值。
2. **Agent2SSH 已经吸收了 RSSH 大部分"表层功能"**：命令块+色条、纯文本复制、关键词高亮、asciicast 录制、容器发现、片段、WebDAV 同步、加密保险库、脱敏、风险分级、跳板机、端口转发、终端主题。
3. **RSSH 真正还没被吸收的是"抽象层"和"安全墙"**，不是功能清单：
   - 命令块的**派生能力**（折叠、复制为图片、Prompt 切分、块级脱敏）—— Agent2SSH 目前只有"切一刀 + 纯文本复制"。
   - AI 排障的**四道硬墙中的"形态校验"**（刷屏命令、无次数循环）和**工具设计模式**（side_effect 声明、100MB 硬上限、analyze_locally 解耦、skill catalog）。
   - **输出截断语义**（head + tail + `[TRUNCATED: dropped N bytes]`，Agent2SSH 目前是纯 head 丢弃尾部）。
   - 若干**工程级经验**（备份格式版本号、Tauri 平台坑、keychain 硬失败）。

---

## 1. 坐标系对齐：两个项目是什么

| 维度 | RSSH | Agent2SSH |
|------|------|-----------|
| 一句话定位 | 为 AI 运维而生的 SSH 客户端 | 面向通用 Agent 的本地 SSH 能力层 |
| 主要用户 | 人类运维/开发者 | Codex / Claude / IDE 等外部 Agent |
| AI 角色 | **内置**排障助手，四道墙 + BYOK | **不内置 LLM**，通过 MCP 把 SSH 暴露给外部 Agent |
| 前端 | Svelte + xterm.js | React/Vite + xterm.js |
| 后端 | Rust + Tauri 2 + `russh`（纯 Rust SSH） | Rust + Tauri + `ssh2`/libssh2（内嵌） |
| 存储 | SQLite（`rusqlite` bundled，GUI/CLI 共享） | JSON 文件 + 跨进程文件锁（`hosts.json` 等） |
| 秘密保管 | OS keychain（`keyring` v3）+ ChaCha20-Poly1305 备份 | `secrets.enc`（Argon2id + AES-256-GCM 主密码保险库） |
| 同步 | GitHub 私有仓库 + WebDAV 双通道 | WebDAV 单通道（版本化 commit + ETag CAS） |
| host key | 直接读写系统 `~/.ssh/known_hosts` | 自建 `~/.agent2ssh/known_hosts.json`（TOFU） |
| 入口 | CLI-first：`rssh profile open prod` | MCP-first + daemon + CLI 管理命令 |
| 多端 | 桌面 + 移动(Android/iOS) + JetBrains | 桌面 + Web 控制台 + CLI/MCP/daemon |

**关键认知**：Agent2SSH 的"人"是外部 Agent，RSSH 的"人"是操作者。因此 RSSH 的"每条命令用户点确认"在 Agent2SSH 里对应"审批队列 + `--force` + 风险分级"；RSSH 的"内置 AI"在 Agent2SSH 里对应"把 SSH 能力交给外部 Agent 自己"。这是定位差异，不是谁对谁错——但它决定了哪些设计能**直接搬**，哪些只能**搬模式、换载体**。

---

## 2. RSSH 核心设计思想（文档化）

RSSH 的设计哲学一句话：**"工具要服务你已有的工作方式，而不是逼你为工具让步。"** 下面五条是它所有代码背后的元原则。

### 2.1 工作台抽象：字节流 → 对象

RSSH 最重要的赌注，不是"连接管理"（它认为是 1995 年就解决完的红海），而是：

> **"连接"是工具，"连上之后能干什么"才是产品。**

传统终端是"字节渲染器"：字节进、字节出，40 年没人把那条字节流变成可操作对象。RSSH 用 **`IMarker`（xterm.js 的行追踪对象）** 在每次 Enter 时"切一刀"，把两个 marker 之间的一段暴露成 `CommandBlock` 对象：

```ts
interface CommandBlock {
  id: number;
  color: string;
  start: IMarker;   // Enter 时记下
  end: IMarker | null;
}
```

选 `IMarker` 而不是行号/坐标/字节偏移，是关键：
1. **跟着 scrollback 自动迁移** —— 终端上滚 N 行，marker.line 自动 -N；
2. **被修剪出 scrollback 自动 dispose** —— 块自动消失，无需手动清理；
3. **resize/reflow 无感知** —— 不维护行号、不监听 resize、不算坐标。

**黄金角配色** `hue = (i * 137.508) % 360`（137.508° = 360÷φ²）：无限调色板、相邻色差最大化、一万条命令不撞色。

### 2.2 复利：一个对象，五个消费者

RSSH 的核心洞察是——**选对一个数据结构，能力会自己长出来**：

| 能力 | 字节流抽象里 | 块对象抽象里 | RSSH 实现 |
|------|------------|------------|-----------|
| 找输出起点 | 滚轮 + 肉眼 | 点色条 | 色条渲染 |
| 复制纯文本 | 手动清 ANSI/软换行 | `block.start..end` 切片 | `block-content.ts` (145 行) |
| 复制为图片 | 截图糊 | 同一切片换 canvas 渲染器 | `block-to-image.ts` (375 行) |
| 折叠输出 | 做不到 | buffer splice 真抽走 | `folds.ts` (304 行) |
| AI 取输出 | 网页里粘字节 | sentinel 之前那段就是块 | `ai/session.rs` |
| 审计 | 翻 scrollback | 块级记录 | `ai/audit.rs` |

所有派生能力共享同一个切片定义 `block.start..end`，没有 if/else 补丁。**新功能只是给这个对象再加一个消费者。**

### 2.3 AI 排障：四道墙 + 四个工具

RSSH 给 LLM 四个工具（`run_command` / `download_file` / `analyze_locally` / `load_skill`），人只在关键节点确认。安全**不靠 prompt 自律**，靠进程内四道硬墙（Rust enforce，prompt 写啥都绕不过）：

1. **shape validator**（`ai/sanitize.rs`）—— 拦截破坏性命令名、`chmod -R`、fork bomb、**刷屏命令**（裸 `top`/`htop`/`watch`/`tail -f`/`vim`/`less`/`tmux`）、**无次数循环**（`vmstat 1` 必须 `vmstat 1 5`）。被拦的**不发给 SSH**，把拒绝原因塞回 LLM 让它换一条，最多重试 2 次。
2. **本地脱敏**（`ai/sanitize.rs`）—— 离机前正则替换内网 IP/token/JWT/长 hex；原文留在本机 `self.history` 永不外发，副本给 LLM 也给审计。
3. **输出截断** —— 单命令默认 1MB，**头部保留 + 尾部截断**，标 `[TRUNCATED: dropped N bytes]`。
4. **每条命令用户确认** —— 命令以卡片落聊天流（命令 + `explain` + `side_effect` + timeout + Approve/Reject），点 Approve 才粘进终端。**Reject 必须填原因**，原因塞回 LLM 调整。

配套设计：
- **sentinel 边界**：`cmd; echo "<uuid>:$?"` 粘进终端，前端监听字节流找 sentinel，sentinel 之前的字节 = 这条命令的块 + 退出码。
- **100MB 硬上限**：`download_file` 的 `MAX_DOWNLOAD_MB=100`，超了让 LLM 告诉用户用 `scp`/`rsync` 自己拉。
- **analyze_locally 解耦**：heap dump / core dump 这类重 artifact 不在远端分析，拉到本地新窗口用本地工具链（MAT/pprof）分析；两条 AI 会话完全隔离，避免本地分析污染远程诊断上下文。
- **skill catalog**：会话启动只把 skill 的 `id + 一行描述` 拼进 system prompt，LLM 匹配场景后自己 `load_skill(<id>)` 拉详细内容——10 个自定义 skill 不撑爆启动 prompt。

### 2.4 安全与同步：保管 ≠ 同步

RSSH 拒绝"要同步就必须有中心节点保管你数据"的隐含假设，把**保管**和**同步**拆成两件事：

| 数据类型 | 保管在哪 | 怎么同步 |
|---------|---------|---------|
| 私钥/密码/passphrase | OS keychain | 默认**不同步**（每条凭据独立 `save_to_remote` 开关） |
| AI key / GitHub token | OS keychain | 不写入同步备份 |
| profile/转发/片段/skill | SQLite | 加密后推到你**自己的** GitHub repo 或 WebDAV |
| host key | 系统 `~/.ssh/known_hosts` | 不归 rssh 管 |

加密是 100 行可审计的 `crypto.rs`：Argon2id（**参数钉死** 19MiB/2iter/1lane，不用 `Argon2::default()`）+ ChaCha20-Poly1305，wire format 带**第一字节版本号**（`version[1] || salt[16] || nonce[12] || ct`）。

拉取语义是**事务性全量替换**（`sync/config.rs`）：先 parse 全部条目（失败早退不动 DB）→ DB 事务 clear+insert（失败回滚）→ commit 后才动 SecretStore（先删淘汰旧 secret 再写新 secret）。

### 2.5 架构：one crate, three binaries

后端是单个 library crate，编译成三个 feature-gated 二进制 + 一个可链接 lib（移动端复用）：

```
rssh         # Tauri GUI（桌面+移动，run() 共用同一入口）
rssh-cli     # CLI：rssh profile open prod（required-features=["cli"]）
rssh-server  # headless WS server（JetBrains 插件内嵌 UI，required-features=["server"]）
```

GUI 和 CLI 读**同一个 SQLite**，没有第二份真相。移动端用 `cfg` 门控 + 优雅降级（PTY/serial 桌面独有，Android 无 keychain 就回退 DB），而不是 fork。

### 2.6 动态发现：source ≠ result

Docker/K8s 目标不是静态 Profile。RSSH 的原则：

> **发现结果不是配置。** 只持久化"发现来源"（platform + context + namespace + shell），不持久化"发现结果"。

- 复用本机 `docker context ls` / `kubectl config get-contexts`，不 SSH 到宿主机跑 `docker ps`（不发明新的远端探测协议）；
- Home 里 profile / forward / docker_exec / kubectl_exec **平级展示**，统一搜索排序；
- 动态目标**不能收藏**（收藏一个会消失的 Pod 没意义）；
- 打开时产出 `connector_spec`，本机起 `docker exec -it` / `kubectl exec -it`，前端看到的仍是 PTY 数据流——终端层不关心下面是 SSH/local/Docker/K8s。

### 2.7 刻意不做的事（No-Go 清单的价值）

RSSH 用"拒绝实现什么"来定义自己：❌ 不提供账号（= 没有 server-side 数据库可脱库）❌ 不做私钥自动云备份 ❌ 不在 keychain 外再造加密（不 NIH）❌ 不接受弱 KDF ❌ 不沉默篡改（AEAD 校验失败立即报错）❌ 不收订阅费（= 没有动机加遥测）。**这份 No-Go 清单本身就是设计文档**——它约束了攻击面。

---

## 3. Agent2SSH 现状分析

### 3.1 定位与架构

Agent2SSH 是一个 Rust core，通过四个 surface 暴露：Tauri 桌面、CLI、HTTP/WebSocket daemon、MCP stdio server。核心价值是**统一授权层**——CLI/MCP/Tauri/daemon 的 exec/playbook/SFTP/PTY/forward 全部走同一套 `execution_control.rs`（scope → 风险分级 → approval → 审计）。控制面（execution gate / 速率限制 / 异常检测 / trace_id 关联 / 跨进程文件锁）是它的独有护城河。

### 3.2 已吸收的 RSSH 能力（映射表）

| RSSH 模块/能力 | Agent2SSH 落点 | 吸收状态 |
|---------------|---------------|---------|
| 命令块 + 黄金角色条（IMarker + Enter 切分） | `src/lib/terminal/command-blocks.ts` | ✅ 已吸收（饱和 68% vs rssh 65%，无 Prompt 模式） |
| 纯文本复制（软换行合并 + C0/C1 清理） | `src/lib/terminal/block-content.ts` | ✅ 已吸收 |
| 关键词高亮（14 预设 + 自定义正则） | `highlight.rs` + `highlight.ts` + `highlight-decorations.ts` | ✅ 已吸收 |
| asciicast v2 录制 + 变速回放 | `asciicast.ts` + `recording.rs` | ✅ 已吸收 |
| OSC 52 剪贴板 | `src/lib/terminal/osc52.ts` | ✅ 已吸收 |
| 容器动态发现（source not result） | `container_discovery.rs`（注释明确"Mirrors rssh's discovery pattern"） | ✅ 已吸收 |
| 命令片段 | `snippets.rs` | ✅ 已吸收 |
| WebDAV 同步（版本化 commit + known_hosts 排除） | `webdav_sync.rs` | ✅ 已吸收 |
| 加密保险库（Argon2id 参数钉死） | `secrets.rs` + `backup_crypto.rs` | ✅ 已吸收（AES-256-GCM，非 ChaCha；无版本号前缀，见 G11） |
| 脱敏管线（exec/audit/export/终端统一） | `redaction.rs` + `sanitize.rs` + `copy_redact.rs` | ✅ 已吸收 |
| AST 命令头提取（tree-sitter） | `sanitize.rs`（`find_first_command_head`） | ✅ 已吸收（比 rssh 更完整） |
| 风险分级（low/medium/high/blocked） | `core.rs` `classify_risk` | ✅ 已吸收（语义等价于 shape validator 的"破坏性命令"部分） |
| 跳板机 + 端口转发（内嵌 direct-tcpip） | `forward.rs` + `jump_chain.rs` | ✅ 已吸收 |
| 密码/OTP 交互提示等待 | `prompt_waiter.rs` | ✅ 已吸收 |
| 终端主题 | `terminalThemes.ts` | ✅ 已吸收 |

### 3.3 Agent2SSH 独有（RSSH 没有的）

- **MCP 51+ 工具**作为 Agent 标准接口；**daemon 控制面**（execution gate / 速率限制 / 会话并发 / 异常检测）；
- **结构化审计 `audit.jsonl`** + source 归因 + `trace_id` 全链路关联 + 跨进程文件锁；
- **playbook / webhook(HMAC) / 远程 daemon 注册表 / 多 Agent 身份绑定**（`mcp_binding.rs`）；
- **会话接管 + 多终端广播**（token-owned terminal_id + 全目标 scope/gate/risk/rate 检查）；
- **AST-based 风险分类**（`sanitize.rs` 比 rssh 的规则更结构化）。

结论：Agent2SSH 在"能力层 + 安全控制面"上已经**领先** RSSH；差距集中在**终端工作台的派生能力**和**AI 排障的形态校验/工具设计模式**。

---

## 4. 可吸收清单（核心交付）

优先级定义：
- **P0**：高价值、低风险、改动局部，建议近期做。
- **P1**：高价值但涉及架构/哲学取舍，需团队决策。
- **P2**：锦上添花，可延后。
- **不吸收**：明确不做，附理由。

### 4.1 终端工作台派生能力（延续"块复利"）

#### G1 · Fold/unfold 真折叠 —— **P0 · ✅ 已实现（2026-08-13）**

- **是什么**：把某条命令的整段输出从 xterm buffer 里 `splice` 抽走（不是 CSS 隐藏），抽出后补空行维持 `lines.length === ybase + rows` 不变量，unfold 时塞回。折叠后的终端在 xterm 眼里和没折过一样（滚动/查找/复制都正常）。
- **为什么值得**：Agent2SSH 已有块对象 + 色条，折叠是"块"这个对象最自然的下一个消费者，直接补全工作台体验。零远端依赖。
- **落点**：`src/lib/terminal/folds.ts`（参考 `demo/rssh-main/src/lib/terminal/folds.ts`）。
- **关键实现点**：依赖 xterm 私有 API `_core.buffer.lines.splice` / `addMarker` / `getBlankLine`；锁 `@xterm/xterm` 版本，升级前重跑测试。
- **验收**：折叠/展开后滚动、选中、复制、查找行为与未折叠完全一致；`folds.test.ts` 覆盖 fold/unfold roundtrip、scrollback 修剪、resize。
- **已落地**：`src/lib/terminal/folds.ts`（`createFoldStore`，适配 Agent2SSH 的 `CommandBlock.id: string`）+ `folds.test.ts`（24 用例全绿）；`TerminalView.tsx` 已接线——`foldBlock`/`unfoldBlock`/`isBlockFolded` 暴露到 handle，resize 前 `unfoldAll`，cleanup 时 dispose。

#### G2 · Copy-as-image 复制为图片 —— **P2 · ✅ 已实现（2026-08-13）**

- **是什么**：块内每行 cell（字符 + 前景/背景色 + bold/italic）按终端字体在 canvas 重画，CJK 宽字符 width=2，软换行按逻辑行合并，输出 PNG。
- **为什么值得**：贴 Slack/微信/issue 不丢颜色、不被压糊。是"同一把刀换渲染器"的体现。
- **落点**：`src/lib/terminal/block-to-image.ts`。
- **验收**：复制出的 PNG 保留颜色/bold/italic，CJK 对齐正确。
- **已落地**：`src/lib/terminal/block-to-image.ts`（`renderBlocksToBlob` + `extractImageRows` + 颜色解析 default/ANSI16/256/RGB + inverse swap + DPR）+ `block-to-image.test.ts`（6 用例）；`TerminalView` 加 `copyBlockAsImage`（`ClipboardItem("image/png")`）。省略了 rssh 的 per-block redaction（图片是"所见即所得"，token 脱敏属文本复制路径，G4 已覆盖）。

#### G3 · Prompt 切分模式 —— **P2 · ✅ 已实现（2026-08-13）**

- **是什么**：除默认"Enter 一刀切"外，提供"命令提交后识别返回的 shell Prompt 再切"模式（`prompt.ts` 的 `detectPrompt`）。
- **为什么值得**：切分位置更贴合真实命令边界。rssh 明确它"仍完全本地、可能漏识别"，故默认仍是 Enter 模式。
- **落点**：`src/lib/terminal/prompt.ts` + `command-blocks.ts` 增加 `splitMode`。
- **验收**：Prompt 模式切分位置正确；漏识别时不崩、回退 Enter 语义。
- **已落地**：`src/lib/terminal/prompt.ts`（`detectPrompt`，9 组 shell 正则，锚定列零）+ `command-blocks.ts` 加 `CommandBlockSplitMode = "enter" | "prompt"`（`splitMode` 选项，默认 enter；prompt 模式用 `onWriteParsed` 等待返回 prompt 后 `split`，`submittedLine` marker 防止重检）+ `prompt.test.ts`（7 用例）。

#### G4 · 块级脱敏（command-block-redaction）—— **P1 · ✅ 已实现（2026-08-13）**

- **是什么**：在"块的边界上"一次性脱敏——复制给 LLM / 复制给用户 / 入审计的是同一个已脱敏对象，而非字节级事后过滤。
- **为什么值得**：Agent2SSH 脱敏在 Rust 后端全局做（`redaction.rs`），但**终端内的命令块输出**（Live Activity 预览、终端复制）目前走的是另一条路径。把"块"作为脱敏的天然边界，能保证"人看到的、Agent 看到的、审计记的"三者一致。
- **落点**：`src/lib/terminal/command-block-redaction.ts`（前端）+ 复用 `redaction.rs` 规则。
- **验收**：终端块复制/Live Activity 预览与 exec 审计脱敏规则一致。
- **已落地**：发现 `copy_redact.rs` 已有 `redact_for_clipboard()`（独立规则 `copy_redact_rules.json`，seed-once），但**未桥接为 Tauri command**、前端 `copyBlock` 直接写剪贴板绕过了它。本次新增 `redact_for_clipboard` Tauri command + 前端 `api.redactForClipboard`，`TerminalView.copyBlock` 写剪贴板前先脱敏——终端复制与 exec/审计脱敏规则（`copy_redact_rules.json`）一致。

### 4.2 AI 排障的安全模式（即使不内置 LLM 也值得吸收）

> 核心判断：Agent2SSH **不应该**内置 LLM（定位是能力层），但 RSSH 的"四道墙"里有两道 Agent2SSH 还没做透，值得作为**风险分类的增强**吸收。

#### G5 · shape validator 的"刷屏/无次数循环"形态校验 —— **P0 · ✅ 已实现（2026-08-13）**

- **是什么**：在风险分级之上，额外拦截**交互式刷屏命令**（裸 `top`/`htop`/`watch`/`tail -f`/`vim`/`less`）和**无次数循环**（`vmstat 1` 必须写成 `vmstat 1 5`，`iostat`/`jstat`/`pidstat`/`sar` 同理）。rssh 明确**不硬编码 OS 特征**（不要求必须有 `-b`），只做"形态校验"。
- **为什么值得**：Agent2SSH 的 `classify_risk` 是命令头黑名单式，**没有"形态"这一维**。一个 Agent 跑 `tail -f app.log` 会挂死会话、刷爆 Live Activity——这正是 daemon 场景最该拦的。这是把 RSSH 的 AI 安全墙"翻译"成 Agent 安全墙最直接的一处。
- **落点**：`sanitize.rs` 增加 `classify_shape()`（复用现有 AST），在 `classify_risk` 里合并为 `interactive`/`unbounded_loop` 风险信号。
- **验收**：`top`→interactive、`top -bn1`→放行、`vmstat 1`→unbounded_loop、`vmstat 1 5`→放行；被拦命令审计里带 shape 原因。
- **已落地**：`sanitize.rs` 新增 `ShapeRisk` 枚举（`Interactive` / `UnboundedLoop`）+ `INTERACTIVE_FULLSCREEN_COMMANDS` 常量 + `check_interactive_shape`（`top` 需 `-b`/`-l`/`-n` 才放行）；`CommandAnalysis` 增加 `shape` 字段；`core.rs::classify_risk` 消费 `analysis.shape`，把交互式/无次数循环命令从 Low 升级到 Medium。新增 6 个 sanitize 测试 + 3 个 classify_risk 测试。

#### G6 · side_effect 副作用声明字段 —— **P1 · ✅ 已实现（2026-08-13）**

- **是什么**：命令提议卡片要求声明 `explain`（一句话）+ `side_effect`（副作用，如 `jmap -histo:live` 写 "triggers Full GC, 100-300ms pause"）。
- **为什么值得**：Agent2SSH 的 MCP 工具若让 Agent 在调用时**显式声明副作用**，审批弹窗就能给操作者展示"这条命令会有什么影响"——比单纯风险等级更有信息量。副作用文案由 Agent 生成，rssh 也不静态判断。
- **落点**：MCP exec/session-write 工具 schema 加可选 `explain` / `side_effect` 字段，透传到 approval 上下文与审计。
- **验收**：审批弹窗展示 side_effect；审计记录该字段。
- **已落地**：`ExecRequest`/`ExecResult`/`AuditEntry` 加 `side_effect`（`#[serde(default)]`）；MCP `ssh_exec` schema + 解析 + `auth.rs` 透传；`append_audit` 从 `result.side_effect` 记录；`ApprovalPrompt`/`CommandAuthorizationInput` 加 `side_effect` 并透传（MCP/CLI/Tauri 传实际值，daemon 操作类传 None）。审计记录 side_effect 达成；审批弹窗展示已具备数据载体（`ApprovalPrompt.side_effect`），前端 UI 展示留待接线。

#### G7 · 输出截断语义（head + tail + 标记）—— **P0 · ✅ 已实现（2026-08-13）**

- **是什么**：把当前的**纯 head 截断**（`core.rs` 只保留前 `max_bytes`，尾部直接丢）改为 **head 保留 + 尾部截断 + dropped 字节数**。
- **为什么值得**：`tail -f` 类日志、错误堆栈往往在**末尾**才是关键信息（OOM 的最终报错、栈的根因）。纯 head 截断会丢掉最有价值的尾部。这是对 Agent 诊断质量影响最大的一个小改动。
- **落点**：`core.rs` 的截断逻辑（读满后保留首尾各一段），`ExecResult` 增加 `dropped_bytes` 字段。
- **验收**：超限输出同时含首尾，审计/结果里带明确的 dropped 字节数。
- **已落地**：`exec_ssh_embedded` 改为 `head_bytes = max/2` + `tail_bytes`（滚动尾部）双段收集，`EmbeddedExecOutput` 增加 `dropped_bytes`；`ExecResult` 增加 `#[serde(default)] dropped_bytes`；其余 `ExecResult` 构造点补 `dropped_bytes: 0`。`dropped_bytes = total_read - max_bytes` 精确。

#### G8 · download 硬上限 + analyze_locally 解耦 —— **P2 · ✅ 已实现硬上限（2026-08-13）**

- **是什么**：`download_file` 硬限 100MB（`MAX_DOWNLOAD_MB`），超限让 Agent 引导用户用 `scp`/`rsync`；重 artifact 不在远端分析，拉到本地新窗口（`analyze_locally`）。
- **为什么值得**：Agent2SSH 的 SFTP 已支持流式 + 取消 + 断点续传，但没有"AI 静默拉巨型文件"的护栏语义。给 MCP 的 `ssh_sftp_download` 加一个默认上限 + 超限提示，是低成本的对 Agent 的防御。
- **落点**：`sftp_transfer.rs` + MCP tool schema 加 `max_mb` 硬上限；`analyze_locally` 作为文档化最佳实践（不必照搬新窗口）。
- **验收**：超限请求被拒并返回引导信息。
- **已落地**：`SftpDownloadRequest` 加 `max_mb: Option<u64>`（serde default）；`sftp_download_core_with_source` 下载前 stat 远端大小，超限（默认 100 MiB）直接 `Err` 并引导 `scp`/`rsync`/`sz` + 本地分析；MCP `ssh_sftp_download` schema 加 `max_mb`（serde 自动解析）。`analyze_locally` 作为文档化最佳实践，未照搬新窗口。

#### G9 · skill catalog 模式（load_skill）—— **P2 · ✅ 已文档化（2026-08-13）**

- **是什么**：启动 prompt 只放 `id + 一行描述`（catalog 形态），按需 `load_skill(<id>)` 拉全文——10 个 skill 不撑爆启动 prompt。
- **为什么值得**：Agent2SSH 已有 `skills/agent2ssh/SKILL.md` 和 playbook，若未来给 Agent 提供"场景化操作手册"，catalog 模式比全量注入 prompt 更省 token。
- **落点**：文档化到 `skills/agent2ssh/SKILL.md` 的组织方式；短期不落地代码。
- **验收**：SKILL.md 结构化为 catalog + 按需加载条目。
- **已落地**：`SKILL.md` 顶部新增 **Skill Catalog** 表（任务场景 → 能力 → 章节），Agent 按需深入对应章节、无需读全文。agent2ssh 仅单一 skill，无"多 skill 撑爆 prompt"问题，故只做 catalog 形态文档化，不实现 `load_skill` 机制。

#### G10 · OSC 命令边界 + exit code 回传 —— **P1 · ⚠️ 部分满足（2026-08-13）**

- **是什么**：rssh 用 `cmd; echo "<uuid>:$?"` sentinel 精确识别命令边界 + 退出码（OSC 7338 不可见控制序列，不影响视觉）。Agent2SSH 已有 `osc_ipc.rs` + `osc52.ts`，但缺"命令完成信号 + exit code 回传"这一环。
- **为什么值得**：有了 exit code 回传，PTY 会话里的命令块才能"自闭合"（知道哪条命令何时结束、成功失败），块级审计和 Live Activity 才能精确到命令粒度而非"某段时间的输出"。
- **落点**：`osc_ipc.rs` 扩展 7338 语义（start/done + exit），前端 `command-blocks.ts` 消费完成信号自动闭合块。
- **验收**：PTY 会话命令块能携带 exit code，审计按命令粒度记录。
- **评估结论**：**不做 PTY sentinel**。Agent2SSH 的核心路径是 MCP `ssh_exec`（非交互），已返回精确的 `exit_code` + stdout/stderr，天然满足"Agent 知道命令 exit code"的需求。PTY 会话是交互式终端接管，注入 sentinel 需改 session write 语义（自动包裹 `; printf sentinel`）+ 前端 OSC 7338 handler 双向改动，边际价值低。若未来 Agent 需要"PTY 内命令粒度审计"，再落地 `osc_ipc.rs` 7338 + 前端消费。

### 4.3 安全与同步

#### G11 · 备份 wire format 版本号前缀 —— **P0 · ✅ 已满足（无需改动）**

- **是什么**：`backup_crypto.rs` 的 wire format 是 `magic(32B, 含 "_V1") || salt[16] || nonce[12] || ct`——版本号已由 magic 前缀 `AGENT2SSH_ENCRYPTED_BACKUP_V1` 承载，且 magic 作为 AAD 绑定进 GCM。这**已经满足甚至超额** rssh 的"第一字节版本号"目标（更健壮：32 字节 magic + AAD 防篡改）。
- **结论**：原分析误判为"缺版本号"——实际 magic 前缀的 `_V1` 就是版本号。无需改代码；将来升参数时递增 magic 的版本后缀即可，老格式会被"missing magic prefix"明确拒绝而非"密码错误"。
- **已核对**：`backup_crypto.rs` 的 `ENCRYPTED_MAGIC = b"AGENT2SSH_ENCRYPTED_BACKUP_V1"` + `is_encrypted_backup()` + `tampered_magic_fails` 测试均已存在。

#### G12 · GitHub 同步通道 —— **P2 · ⏸️ 标注后续（2026-08-13）**

- **是什么**：rssh 支持 GitHub 私有 repo + WebDAV 双通道，理由"GitHub 是工程师已有基础设施，私有 repo 自带版本历史"。
- **为什么值得**：WebDAV 需要自建/第三方服务，GitHub 对很多用户零门槛。但 Agent2SSH 的 WebDAV 已实现版本化 commit + ETag CAS，GitHub 通道价值边际下降。
- **落点**：可选，`sync/` 抽象上复用现有 transport（CHANGELOG 已提到 sync transport 抽象 + fake backend）。
- **验收**：与 WebDAV 同语义的 push/pull/冲突检测。
- **评估结论**：transport 抽象已就位——`webdav_sync.rs` 的 `SyncRemote` trait（`ensure_layout`/`read`/`write`）让同步算法与后端解耦。GitHub 通道 = 实现一个 `GitHubRemote`（GitHub Contents API + base64 + ETag CAS + token 管理）+ 配置 + CLI。但 WebDAV 已满足核心同步需求，GitHub 通道是 P2 里成本最高、边际价值最低的项，故标注后续，未来有需求时按 `SyncRemote` 接口落地。

#### G13 · known_hosts 与系统 ssh 共享（哲学取舍）—— **P1 · ✅ 已实现单向导入（2026-08-13）**

- **是什么**：rssh 直接读写系统 `~/.ssh/known_hosts`，命令行信任过的主机 GUI 不再问。Agent2SSH 自建 `known_hosts.json`（TOFU）。
- **为什么值得**：rssh 的观点——"同一份真相，两个工具共用，换工具成本为零"。但 Agent2SSH 面向 Agent 自动化，自建 + TOFU 有真实理由：不污染用户 ssh 的信任库、可纳入 sync 策略、可做指纹变更拒绝。**两者都有道理，不必二选一**。
- **建议**：保持自建 `known_hosts.json` 为主，但增加**双向导入/导出**（`ssh-keyscan` 式导入系统 known_hosts + 导出回写），让"命令行 ssh 信任过的主机"能一键进 Agent2SSH。
- **落点**：`ssh_config.rs` / 新增 `known_hosts` 导入导出命令。
- **验收**：导入后 Agent2SSH 对已信任主机不再 TOFU 提示；导出不覆盖系统里 Agent2SSH 未管理的条目。
- **已落地**：新增 `embedded_ssh::import_known_hosts_from_ssh()`（解析 OpenSSH `known_hosts` 明文行，从 base64 key 重算 SHA256 指纹写入 `known_hosts.json`；跳过 hashed `|1|...` 行与指纹冲突项），暴露为 Tauri command `import_known_hosts` + CLI `agent2ssh known-hosts import [--path]` + 前端 `api.importKnownHosts`。**导出回写系统 known_hosts 不可行**——`TrustedHostFingerprint` 只存 SHA256 指纹、无完整 public key，无法构造 OpenSSH 标准行；故只做单向导入（命令行 ssh 信任 → Agent2SSH）。

### 4.4 CLI-first 体验

#### G14 · `host open` 式交互终端入口 —— **P2 · ✅ 已实现（2026-08-13）**

- **是什么**：rssh 的 `rssh profile open prod` 能在**任意终端**直接拉起一个交互会话（GUI/CLI 共享 SQLite）。Agent2SSH 的 CLI 是管理命令，`Session` 子命令管理 daemon 持久会话，**没有一个"在我当前终端里 ssh 上去"的入口**。
- **为什么值得**：能让 CLI 用户把 Agent2SSH 当 `ssh` 替代品用（复用 hosts.json + 风险分级 + 审计）。但会引入"本地 PTY 直连"这一新路径，与"能力层给 Agent 用"的定位有张力。
- **落点**：可选。若做，复用 `embedded_ssh.rs` 的 PTY worker + 前端逻辑下沉到 CLI。
- **验收**：`agent2ssh host open prod` 在当前终端进入交互 shell，输入同样过风险审计。
- **已落地**：CLI 新增 `agent2ssh host open <host> [--cols N] [--rows N]`。复用 `embedded_ssh::spawn_terminal`（PTY worker，含 connect/TOFU/认证）；CLI 侧新增跨平台 raw-mode（`#[cfg(unix)]` libc termios + `#[cfg(windows)]` Console API）把 stdin 原始字节 → PTY、PTY 输出 → stdout；Unix 下 SIGWINCH → `Resize`（libc signal + 原子标志轮询），Ctrl+C/Ctrl+Z 作为原始字节转发远端而非信号本地。**输入风险审计不逐条做**——交互式终端里人类有完整控制权，逐条审计不现实（rssh 的 `host open` 亦如此）；连接安全由 `connect_embedded_ssh` 的 TOFU 指纹校验承担。依赖新增 `libc`（unix）+ `windows-sys`（windows，Console API）。

### 4.5 Tauri 工程经验（直接适用，Agent2SSH 同为 Tauri）

| 编号 | 经验 | 适用场景 |
|------|------|---------|
| G15 | Windows 下从同步 command 建窗口会死锁（wry#583），开新窗口的 command 必须 `async` | 桌面端多窗口/弹出窗 |
| G16 | Tauri bundler 自动发现 `src/bin/*` 并打包，**忽略 `required-features`**——会强制 GUI 打包未编译的二进制。解法：把 server 二进制源放 `src/server_main.rs` 而非 `src/bin/` | 已有 daemon/mcp 二进制，注意别踩 |
| G17 | Linux/Wayland 上 WebKitGTK DMABUF 失败：启动时默认 `WEBKIT_DISABLE_DMABUF_RENDERER=1` + unset `GBM_BACKEND`，留 env 逃生口 | Linux 桌面启动排查 |
| G18 | keychain 标记"可用"但实际拿不到时**硬失败启动**，不要静默降级到文件 store（静默降级会新造主密钥、让旧密文永久不可解） | `secrets.rs` 的保险库/主密码路径 |

---

## 5. 吸收清单速查表

| # | 吸收项 | 优先级 | 工作量 | 落点文件 | 一句话 |
|---|--------|--------|--------|---------|--------|
| G1 | Fold/unfold 真折叠 | P0 | 中 | `terminal/folds.ts` | buffer splice 真抽走，非 CSS 隐藏 |
| G5 | 刷屏/无次数循环形态校验 | P0 | 小 | `sanitize.rs` | 拦 `tail -f`/裸 `top`/`vmstat 1` |
| G7 | head+tail 截断 + dropped 标记 | P0 | 小 | `core.rs` | 保住错误堆栈尾部 |
| G11 | 备份格式版本号前缀 | P0 | 小 | `backup_crypto.rs` | 防参数漂移静默破坏 |
| G10 | OSC 命令边界 + exit code | P1 | 中 | `osc_ipc.rs` | 命令块自闭合、审计到命令粒度 |
| G4 | 块级脱敏一致性 | P1 | 中 | `terminal/command-block-redaction.ts` | 人/Agent/审计三者一致 |
| G6 | side_effect 声明字段 | P1 | 小 | MCP tool schema | 审批弹窗展示副作用 |
| G13 | known_hosts 双向导入导出 | P1 | 中 | `ssh_config.rs` | 与系统 ssh 共享信任，需决策 |
| G2 | 复制为图片 | P2 | 中 | `terminal/block-to-image.ts` | PNG 贴图不丢颜色 |
| G3 | Prompt 切分模式 | P2 | 小 | `terminal/prompt.ts` | 更贴合命令边界 |
| G8 | download 硬上限 | P2 | 小 | `sftp_transfer.rs` | 防 Agent 静默拉巨型文件 |
| G9 | skill catalog 模式 | P2 | 文档 | `skills/agent2ssh/SKILL.md` | 按需加载省 token |
| G12 | GitHub 同步通道 | P2 | 大 | `webdav_sync.rs` | 复用 transport 抽象 |
| G14 | `host open` 交互终端 | P2 | 大 | CLI | CLI 当 ssh 用，需决策 |
| G15-18 | Tauri 平台坑 | 参考 | — | — | 踩坑前查本节 |

### 明确不吸收（附理由）

| 项 | 理由 |
|----|------|
| 内置 LLM / BYOK AI 助手 | Agent2SSH 定位是**能力层**，AI 由外部 Agent（MCP）承载，内置 LLM 会与定位冲突、引入计费/数据边界问题 |
| 自建 keychain 替代 secrets.enc | Agent2SSH 的 `secrets.enc` 主密码保险库已解决无 keychain 环境（headless CLI/MCP/daemon）的问题，比 rssh 的 keyring 更契合 daemon 场景 |
| Telnet / Serial | `ssh2` 内嵌传输 scope 外，且与"SSH 能力层"定位无关 |
| 移动端 / JetBrains 插件 | 分发形态差异，非当前协作者任务 |
| 存储迁到 SQLite | Agent2SSH 的 JSON + 跨进程文件锁已解决并发，迁移成本 > 收益 |

---

## 6. 给协作者的吸收原则

1. **吸收"抽象"，不吸收"代码"**：RSSH 是 Svelte + russh，Agent2SSH 是 React + ssh2，直接拷代码会水土不服。要搬的是"块是对象、能力是消费者"、"四道墙"、"source≠result"这些**元设计**。
2. **数据结构优先**：G1/G2/G4 都是同一个 `block.start..end` 切片的消费者。先把"块对象"这个数据结构做完整，派生能力自然长出。不要各自为政地写独立功能。
3. **安全不靠 prompt 自律**：任何"让 Agent 自觉别跑危险命令"的想法都是错的。形态校验、脱敏、截断、审批都要在 Rust 进程内 enforce（G5/G6/G7/G8 都遵循这条）。
4. **维护 No-Go 清单**：RSSH 的"刻意不做"清单约束了攻击面。Agent2SSH 也应维护自己的 No-Go（本表第 5 节"明确不吸收"就是起点）。
5. **每个 gap 先补测试再合入**：RSSH 的 folds/block-to-image 都带 `.test.ts`，并锁 xterm 版本。搬进来同理。

---

## 7. 附录

### 7.1 关键文件对照速查

| 主题 | RSSH | Agent2SSH |
|------|------|-----------|
| 命令块 | `src/lib/terminal/command-blocks.ts` (225 行) | `src/lib/terminal/command-blocks.ts` (137 行) |
| 纯文本复制 | `src/lib/terminal/block-content.ts` (145 行) | `src/lib/terminal/block-content.ts` (114 行) |
| 折叠 | `src/lib/terminal/folds.ts` (304 行) | ❌ 无 |
| 复制为图片 | `src/lib/terminal/block-to-image.ts` (375 行) | ❌ 无 |
| Prompt 识别 | `src/lib/terminal/prompt.ts` | ❌ 无 |
| 块级脱敏 | `src/lib/terminal/command-block-redaction.ts` | ❌ 无 |
| AI 排障 | `src-tauri/src/ai/`（session 82KB / sanitize 97KB / skills / tools / llm） | ❌ 无（定位不同） |
| 形态校验 | `src-tauri/src/ai/sanitize.rs` + `command_blacklist.rs` | `sanitize.rs`（AST 头提取，缺形态维） |
| 脱敏规则 | `src-tauri/src/ai/redact_rules.rs` | `redaction.rs` |
| 动态发现 | `src-tauri/src/commands/discovery.rs` | `container_discovery.rs` |
| 同步 | `src-tauri/src/sync/{github,webdav,config}.rs` | `webdav_sync.rs` |
| 备份加密 | `src-tauri/src/crypto.rs` (100 行, version byte) | `backup_crypto.rs` (无 version byte) |
| host key | 系统 `~/.ssh/known_hosts` | `~/.agent2ssh/known_hosts.json` |

### 7.2 术语

- **IMarker**：xterm.js 的行追踪对象，随 scrollback 迁移、被修剪自动 dispose。
- **黄金角**：137.508°（360÷φ²），无限调色板相邻色差最大化。
- **工作台抽象**：把字节流切成可操作对象（块），能力是对象的消费者。
- **四道墙**：shape validator / 本地脱敏 / 输出截断 / 每命令确认。
- **source≠result**：动态发现只持久化"来源"，不持久化"结果"。
- **TOFU**：Trust On First Use，首次连接信任指纹、之后变更拒绝。

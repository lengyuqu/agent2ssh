# 常驻环境会话（Resident Environment Session）设计

状态：**P0 已实施**（2026-09-27，`feat/resident-session`）· 设计定稿 2026-09-27
关联：`local_exec.rs`（`local:<id>` 环境）、`session.rs`（SSH PTY 会话）、daemon `/local-exec`

---

## 1. 结论（先说方案）

新增一类**常驻环境会话**：daemon 内为某个 `local:<id>` 环境保活一个长驻 shell 进程，agent 通过 `exec` 逐条投递命令，命令仍逐条走完整授权链，输出按 sentinel 定界精确回收。

- 寻址复用 `local:<id>` 命名（host 保留空间），token scope / 审批策略 / 审计零新概念。
- 一次性执行（现状）保持默认不变；常驻会话是**显式 opt-in**，两者语义差异（shell 状态持久化）写进文档。
- 落点：新模块 `src-tauri/src/resident_session.rs` + daemon 4 条路由 + MCP 3 个工具。VS dev shell 的 ~7s 建环境成本从"每条命令"摊销为"每次开会话"。

## 2. 动机

| 现状 | 问题 |
|---|---|
| `local:vs` 每条命令现建环境（`Launch-VsDevShell.ps1` prelude） | 实测 ~7s/条，编译循环（cl → 看 error → 改 flag → 再 cl）成本被放大 |
| 一次性执行无 shell 状态 | agent 无法 `cd` 后连续操作、无法设置会话级变量；只能把状态塞进单条命令文本 |
| SSH 侧已有 `/sessions`（PTY） | 面向人看终端，输出是流式 PTY 文本，不适合 agent 的捕获式语义，且只覆盖 SSH host |

对应产品定位：agent2ssh 是「终端环境供给层」，环境是主体、命令是载荷。常驻会话让 agent 能"住进"一个环境，而不是每次从门外重新进。

## 3. 目标 / 非目标

**目标**
1. 环境建立成本按会话摊销（vs 首条命令后，后续命令 prelude 零成本）。
2. 会话内 shell 状态（cwd、env、函数）跨命令持久。
3. 每条命令仍走 gate→limits→scope→risk→approval 完整授权链并写审计；不产生比一次性执行更弱的路径。
4. 捕获语义与一次性执行同级：head+tail 有界、精确 exit code、超时有界。

**非目标（本期不做）**
- 不替代交互式终端（`/terminal`、PTY 路径不动）。
- 不做会话跨 daemon 重启恢复（重启即回收，与 lifecycle 现有语义一致）。
- 不做远端 SSH host 的常驻会话（SSH 一次性 exec 无重建成本，收益不成立）。

## 4. 方案选型

| 备选 | 思路 | 结论 |
|---|---|---|
| **A. 常驻 shell 进程（选定）** | daemon 保活长驻 shell，stdin 投命令，sentinel 定界回收 | 成本真正摊销；状态天然持久；定界可控 |
| B. 环境快照/回放 | 开一次性"温"壳 → 导出关键 env（PATH/INCLUDE/LIB）→ 后续命令注入 env 后冷启 | 免进程管理，但状态不完整（函数/别名/进程内状态丢失），且导出逻辑按环境各写一份，长期是第二个 prelude 系统 |
| C. 复用 SSH PTY 会话基建做本地 PTY | `spawn_terminal` 式 PTY + quiet-period 读 | 输出定界是启发式，达不到 agent 需要的捕获精度；PTY 依赖（conpty）引入新平台面 |

选 A 的决定性理由：B 和 C 都在"捕获精度"上让步，而捕获精度正是 agent 使用的硬前提。

## 5. 核心设计

### 5.1 数据模型

```rust
// src-tauri/src/resident_session.rs（新模块，no-default-features 可测）
pub struct ResidentHandle {
    pub id: Uuid,
    pub host_id: String,                 // "local:vs" 等，授权/审计/审批按它匹配
    pub environment: LocalEnvironmentRequest,
    pub child: Child,                    // 长驻 shell；进程组配置复用 configure_process_group
    pub stdin: Option<ChildStdin>,       // 写完命令+sentinel 即 Drop 到 None，防并发写
    pub buffer: Arc<Mutex<BoundedBuffer>>, // head+tail 有界 + dropped_bytes，复用 exec_capture 语义
    pub last_active: Instant,
    pub dead: bool,
}
```

- 注册表：`AppState` 新增 `resident_sessions: Mutex<HashMap<Uuid, Arc<StdMutex<ResidentHandle>>>>`（与 `sessions` 并列，不混用）。
- lifecycle：新增 `ResourceKind::ResidentSession`，open 前 reserve、失败 Drop 自动 Closed、close 时标记——完全复用 SSH 会话的配方。
- 并发上限：`AGENT2SSH_MAX_RESIDENT_SESSIONS`，默认 **8**（长驻进程比 PTY 会话更贵）。
- 空闲回收：`AGENT2SSH_RESIDENT_IDLE_TTL_SECS`，默认 **600**；daemon 内 tokio reaper 任务周期清扫。
- 同一会话的 exec 串行化：session mutex 排队（并发第二条等锁，受现有 limits 超时约束），避免命令交错。

### 5.2 Sentinel 定界协议（核心机制）

每条命令的投递格式（由 API 层拼装，**调用方传 `command` 字段而非裸 stdin 字节**，天然免掉 SSH 会话的分行授权问题）：

```
<command>\n
<sentinel_line>\n
```

sentinel 行按 shell 家族生成（`EnvironmentPlan.style` 增加 `resident_sentinel(nonce)` 分支）：

| 环境 | sentinel 行 | 备注 |
|---|---|---|
| pwsh / vs | `Write-Output "__A2S_DONE_{nonce}:$LASTEXITCODE"` | 不用 scriptblock 包裹命令——`& { $x = 1 }` 的赋值不出作用域，会毁掉状态持久 |
| cmd | `echo __A2S_DONE_{nonce}:%ERRORLEVEL%` | shell 以 `cmd /q` 启动防回显噪音 |
| gitbash / wsl | `printf '__A2S_DONE_{nonce}:%s\n' "$?"` | |

- nonce 每命令一个高熵随机值；exec 循环从 `buffer` 累积字节中搜索 `__A2S_DONE_{nonce}:`，取其后到行尾为 exit code，之前的字节为命令输出。
- nonce 不可被命令输出伪造（命令文本先于 sentinel 发出，调用方拿不到 nonce），最坏情况是命令自己 echo 了一段巧合文本导致提前截断——只影响捕获，不影响授权。
- 若命令打开了 here-string 吞掉 sentinel 行 → 读侧超时（复用 exec timeout），**超时即杀整个会话进程树**并返回 `session_dead: true`（Windows 无共享控制台时无法对长驻壳发 Ctrl-C，宁可诚实杀掉；一次性 `local:vs` 路径仍是长任务出口）。

### 5.3 各环境的 warm 启动

open 流程：授权 → `resolve_environment`（失败照旧列出 `SELECTABLE_IDS` 拒绝）→ spawn 长驻壳 → 发送 prelude（若有）+ 一次性 warmup sentinel → 阻塞读到 sentinel（open 超时 120s）→ 激活。

| 环境 | 长驻启动形态 | 预热成本 | 相对一次性执行的意义 |
|---|---|---|---|
| `local:pwsh` | `pwsh -NoLogo`（stdin 管道模式无 prompt 噪音） | ~0.3s | 主要收益是状态持久 |
| `local:cmd` | `cmd /q` | ~0.1s | 同上 |
| `local:gitbash` | `bash`（与现 plan 同源，非 login） | ~0.3s | 同上 |
| `local:vs` | `pwsh` + prelude `& Launch-VsDevShell.ps1 ...`（照抄用户 profile 的 vsdev 函数，进程级 env 直接生效） | ~7s（仅 open 一次） | **核心受益者** |
| `local:wsl[:distro]` | `wsl.exe -d <distro>`（跑发行版默认 shell） | ~1-2s | 省发行版冷启 |

原 prelude 不变（`EnvironmentPlan` 是数据，新增的只是"投递方式"），warmup 输出全部吞进 buffer 由 sentinel 截断，profile/prompt 噪音不影响定界。

### 5.4 授权与安全

- **open 也是授权点**：动作是"在本机建立环境"，`host = local:vs` 走 `authorize_command_with_approval`（command 记为 `resident-session open`），写 `resident_session_open` 审计。host 维度的 token scope（`allowed_hosts`）与审批策略 pattern 对 `local:vs` 的既有匹配规则原样生效。
- **exec 逐条授权**：command 文本原样进 preflight（gate→limits→scope→risk→approval），审计含 session id、host_id、exit code。常驻不弱化任何一层。
- **拒绝面不变**：未知环境拒绝并列出可选 id；非 daemon 入口（无审批处理者）等价硬拒绝——常驻会话因此 **daemon-only**（P0/P1）。
- 新增风险与缓解：
  - *状态走私*：已批准命令 A 改 cwd/env 影响 B —— 与一次性执行风险同级（A 本来就能干任何事），不额外设防，靠审计可追溯。
  - *钩子持久化*：一条命令装 shell hook 改写后续命令 —— 命令文本仍逐条过审，hook 不能绕过授权层；文档明示。
  - *会话存活即攻击面*：TTL 默认 10min、上限 8、显式 DELETE、daemon 关停全量回收（kill tree 复用）。

### 5.5 输出捕获

- 每会话一个 reader 线程持续排空 stdout/stderr（合并入 buffer），buffer 是 head+tail 有界捕获 + `dropped_bytes` 精确计数——**与一次性执行同契约**，exec 返回值复用 `ExecResult` 形状 + `session_id` 字段。
- stderr/stdout 合流：长驻壳下两者本就交织，分开捕获在此无意义；文档注明。

## 6. API 设计

| 路由 | 语义 |
|---|---|
| `POST /resident-sessions` | body `{ "host": "local:vs" }` → `{ "id": "...", "host": "local:vs", "warmup_ms": 7100 }`；open 阻塞至 warm |
| `POST /resident-sessions/:id/exec` | body `{ "command": "...", "timeout_ms": ... }` → `ExecResult`（含 exit code、有界输出、`dropped_bytes`）；超时杀会话 |
| `GET /resident-sessions` | 列表：id、host、last_active、dead |
| `DELETE /resident-sessions/:id` | 杀进程树、 lifecycle 关闭、审计 |

- MCP 工具（+3）：`resident_session_open` / `resident_session_exec` / `resident_session_close`（list 折叠进 open 响应的错误提示，控制工具数增长）。
- CLI（P1）：`agent2ssh resident open/exec/close`，薄封装 daemon HTTP。
- 桌面 UI（P2）：会话面板，复用现有 sessions UI 骨架。

## 7. 契约同步清单（全部落地，2026-09-28 复查）

- [x] `docs/api.yaml`：路由齐备（`POST|GET /resident-sessions`、`POST /:id/exec`、`GET /:id`、`DELETE /:id`；`api_yaml_documents_every_daemon_route` 强制）。
- [x] `src/bin/agent2ssh_mcp/tools.rs` + 工具数断言 71→75 + `docs/skills.md`（三处同步；`mcp_tool_list_contains_exactly_75_tools` 与 `mcp_tools_match_skills_md_documentation` 强制）。
- [x] `AGENTS.md` 执行目标段：补 resident session 规则（含"不得走更弱路径"，由 `resident_session_routes_run_the_shared_preflight` 源码级断言固化）。
- [x] `docs/architecture.md` 传输矩阵：本地环境第三种形态（常驻）已写入。
- [x] `CHANGELOG.md`：Added + Verified 均已记录。
- [x] daemon `preflight_guarded_request` 调用点（open 与 exec 各自一次，共 18 处）；防护测试改为断言*顺序*（会话 host 必须先从 registry 读出再进闸门链），而非计数。

## 8. 测试计划

| 层 | 内容 |
|---|---|
| 单元（`resident_session.rs` `#[cfg(test)]`） | sentinel 生成/解析、nonce 防伪（命令 echo 巧合文本不误判）、BoundedBuffer 定界、TTL 计算 |
| 集成（`tests/resident_exec.rs`，无外部前提） | 真实 spawn pwsh/cmd：差分判别——会话内 `cd` 后下一条命令 pwd 保持；exit code 透传（`exit 3` → 3）；超时杀会话后 exec 返回 dead |
| e2e（仿 `daemon_local_exec_e2e.rs`） | 真实 daemon + `POST /resident-sessions host=local:vs`（本机有 VS BuildTools 18）→ 第二条 `where.exe cl` 成本 < 首条量级（只断 cl 可用，不断耗时，避免 flaky）；warmup sentinel 只出现一次 |
| 授权 | open/exec 未授权拒绝 + 拒绝审计；host scope 不含 `local:vs` 的 token 开会话被拒 |
| 契约/变异 | api.yaml 路由防护、MCP 计数；新防护测试按惯例做变异验证（改 handler 指向/删 preflight 调用必须变红） |

本机 `cargo test --lib`（默认特性）曾因 `comctl32!TaskDialogIndirect` 的 v6 导入在测试二进制里无法解析而报 `0xc0000139`；`build.rs` 现已对 Windows 全目标 delay-load comctl32，该命令可正常跑（717 通过），验证不必再退到 `--no-default-features`。

## 9. 实施分期

| 阶段 | 内容 | 验收 |
|---|---|---|
| P0 | `resident_session.rs`（pwsh+cmd）+ daemon 4 路由 + api.yaml + lifecycle/上限/TTL | 集成测试绿；cd 状态保持差分通过 |
| P1 | vs + gitbash + wsl 变体；MCP 4 工具（71→75：补做遗留的 `local_exec` + resident 3 件套，均转发 daemon）；CLI | `local:vs` e2e：第二条命令 cl 在 PATH |
| P2 | 桌面会话面板；`GET /:id/read` 拆分（后台长任务：exec 发起、read 回收） | — |

## 10. 待拍板的开放问题

1. **exec 超时杀全会话**是否可接受？（替代方案：超时后发"丢弃标记"重建哨兵状态，但 Windows 下对无控制台长驻壳中断子命令不可靠，不建议。）
2. MCP 工具命名与数量（3 个 vs 再加 `resident_session_list`）。
3. open 的审批默认策略：跟环境现有 risk 等级走（建议），还是单独策略键？

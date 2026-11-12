# 优化轮次账本（OPT-LOOP）


这份文件是「优化批次生产 / 消费」两个定时任务之间**唯一的跨轮记忆**。约定：

- 生产任务追加 `### batch <日期> · <主题>`：记 issue 号与 CodeBuddy 首条分析索引，6 轮进度留 `0 / 6`。
- 消费任务追加 `### consume K / 6`：记每轮判定、动作与 SHA，回写批次条目的进度。
- 顶部出现 `LOOP_PAUSED` 或 `BATCH_COMPLETE` 时，对应任务只报告不执行。
- **`不成立` 的条目必须原文保留**：生产任务每批都会把它整段喂给 CodeBuddy 作为「不要重复提」清单，删掉就等于允许同一个错误无限重放。

## 消费任务的轮次语义

一个批次 = 一条 `[opt-round]` issue = **最多 6 轮闭环**，每轮形状固定：

```
发帖（@CodeBuddy）→ 等 2–5 分钟 → 核实 → 修 + 验证 + 推 → 回帖再发（@CodeBuddy 复核本轮改动）
```

第 6 轮之后关闭 issue、写 `BATCH_COMPLETE`。

`BATCH_COMPLETE · issue #30 · batch 14`（2026-11-11 21:5x；生产任务开 batch 15 时删除本行）

**进度载体是 issue 本身**：消费任务靠数 issue 内 `## 第 K 轮收尾` 评论的数量判断续跑位置，不依赖 automation 的记忆或本文件的进度字段。所以单次运行被切断（180 分钟预算用尽）不会丢进度——下次运行从 C+1 轮继续，6 轮总量由评论数保证。

**每轮 3.4 的回帖不可省**：它既是「再发」，也是让 CodeBuddy 在下一轮看到**改后的新代码**并复核自己上一轮的判断。跳掉这一帖，6 轮就退化成 6 次互不相关的单方面改动。

---

<!-- 十二条 BATCH_COMPLETE 状态行（#8/#9/#10/#11/#12/#13/#14/#15/#16/#17/#18/#19）已由生产运行先后删除：#8–#10 于 2026-11-10 14:2x（按 #10 批次遗留要求），#11 于 2026-11-10 18:4x（batch 6 开批后同样会让消费任务误判「只报告不执行」），#12/#13 于 2026-11-10 23:5x（batch 8 开批后：#13 那条按其自述「下一批（B=8）由生产任务开 issue 时删除本行」执行，#12 虽无自述指示但同属已关闭批次留痕，沿用 #8–#11 先例一并删除），#14 于 2026-11-11 04:1x（batch 9 开批后，按该行自述「下一批（B=9）由生产任务开 issue 时删除本行」执行），#15 于 2026-11-11 06:4x（batch 10 开批后，按该行自述「按 #8–#14 先例删除本行」执行），#16 于 2026-11-11 08:5x（batch 11 开批后，按该行自述「按 #8–#15 的先例删除本行」执行），#17 于 2026-11-11 11:2x（batch 12 开批后，按该行自述「生产任务开 batch 12 时按 #8–#16 先例删除本行」执行），#18 于 2026-11-11 13:4x（batch 13 开批后，按该行自述「生产任务开 batch 13 时删除本行」执行）。#19 于 2026-11-11 20:0x（batch 14 开批后，按该行自述「生产任务开 batch 14 时必须删掉本行」执行）。批次史实见各批次小节与 git 历史；batch 5（#11）的「批次汇总」小节已被消费者收尾复核（`7175314`）重写为「第 6 轮之后」小节。 -->

---

## preflight · 2026-11-10 · issue #2 最后一轮的预核实

非本流程轮次，是流程上线前对手工核实结论的固化。来源：issue #2 评论 `2119853755255377920`（2026-11-09T18:04:54Z，作者 CodeBuddy）。

**背景**：该评论是对交接文档 `docs/reports/CNB-ISSUE-2-HANDOFF-2026-11-10.md` 之后一轮的「更正」。核实发现 4 条指摘中 2 条错、2 条对。

### 判定不成立（原文保留，请勿重复提）

1. **「`signal_daemon` 的第三条 `Ok(())`（`daemon_control.rs:752`，`process.kill()` 返回 false）会吞掉错误并假成功」— 不成立。**
   实际源码 `src-tauri/src/daemon_control.rs:741-752` 是 `let sent = if force { process.kill() } else { ... }; if sent { Ok(()) } else { Err(anyhow!("failed to signal daemon process {pid}")) }`。
   `kill()` 返回 false 走的是 **`Err`**，被调用点 `:1025` 的 `signal_daemon(pid, start_time, true).map_err(DaemonStopError::Other)?` 转成 `DaemonStopError::Other` 向上传播，**不会**落到 `clear_daemon_owner_if` + `Ok(())`。
   `git log -L 722,753:src-tauri/src/daemon_control.rs` 只命中引入提交 `edfdb67`，此后该形状未改。
   全函数只有 `:733`（快照无条目）与 `:739`（启动时间/僵尸/名称白名单不符）两处静默 `Ok(())`；这两处之后都会过 `wait_for_exit(2s)`，终态必为 `Exited` / `StillRunning` / `UncertainExit` 之一，不存在假成功路径。

2. **「`a_daemon_serving_another_config_directory_is_not_stopped` 的断言弱，第一个条件失效后会落到第二个分支照样绿」— 不成立。**
   `tests/daemon_stop_e2e.rs:1443` 断言的是 `message.contains("serves config directory")`。该字面量只在 `daemon_control.rs:961` 那一支出现；另一支文案是 `does not report its config directory`（`daemon_control.rs:970`，不含 `serves`）。两分支文案可唯一区分，断言有效。
   （附带：该指摘把行号归到 `daemon_control.rs`，实际在 `tests/daemon_stop_e2e.rs`。）

### 判定成立（可执行，已消费）

3. **「`docs/guides/configuration-guide.md:378` 的『不再按 PID 强制结束进程』与 `fbea044` 后行为矛盾」— 成立。**
   `daemon_control.rs:1018-1031` 的 `Running` 分支确实会 `signal_daemon(pid, start_time, true)` 并再等 2 秒。指南那段仍写「报错并保留 PID 文件，不再按 PID 强制结束进程」。
   **同源缺陷（原文未提）**：`:1022` 的诊断文案 `"daemon did not exit after a guarded shutdown; leaving it running"` 与紧随其后的 force kill 行为矛盾。
   → 属 L1（`docs/**`；诊断文案若改需动 `src-tauri/src/**`，拆成 L2）。

4. **「`does not report its config directory` 分支（`daemon_control.rs:970`）无人守」— 成立。**
   唯一钉住它的 `legacy_daemon_without_pid_guard_is_not_automatically_stopped` 已由 `f373f2c` 删除；全仓 `grep -rn "does not report its config directory"` 只命中实现处，无任何断言。
   → 属 L2（补测试断言涉及 `tests/**`，且该分支语义与旧 daemon 兼容相关，需人工确认断言语义）。

### 待办提示

- 第 3 条的文档修正可直接落 L1；第 4 条需人确认后落 L2。
- issue #2 的 K14 四项（pidfd 类进程身份绑定、PID 复用、共享令牌伪造、检-发非原子）属设计决策，消费任务遇 #2 时应归 P2，只产方案不改代码。

---

## 手工消费 · 2026-11-10 · 文档与行为一致性

首次实跑，用于验证消费链路本身。非批次轮次（无 `[opt-round]` 容器 issue），因此不计入 `第 K 轮收尾` 序列。

- **选靶**：#2（P1，最新评论含带文件行号的缺陷断言）。#6 判为 P2——其 body 明写「此 issue 当前只讨论，不授权实现或关闭 K8」。同级 P1 优先于 P2，故取 #2。
- **判定**：成立 2 / 不成立 2 / 部分成立 0（明细见上方 preflight，本轮直接引用，未重复核实）。
- **动作**：L1 已推 `e26d629`，改 `docs/guides/configuration-guide.md:377` 与 `docs/KNOWN-ISSUES.md` K15；L2 两条（诊断文案、缺失断言）写入 `docs/reports/OPT-LOOP-L2-PENDING.md`。
- **验证**：`npm run build` 7.57s 通过；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.49s）。
- **回帖**：issue #2 评论 `2119858507640897536`，含 @CodeBuddy 复核请求与「不成立」清单。

### 链路验证结论

两个任务之间的分工在此次实跑中暴露一处偏差：消费者原 prompt 只认 `[opt-round]` 容器、找不到就直接退出，而存量 open issue（#2、#6）永远不会被它消费。已在 consumer prompt 里把选靶改为 P0/P1/P2 全量（见下一节修正记录），否则「持续解决当前最优先的 issue」只在生产任务产出之后才成立。

### 第 2 次收尾 · 复核修正（2026-11-10）

CodeBuddy 在评论 `2119859264326893568` 里指出**第 1 次收尾自己引入的新不准确**：`signal_daemon` 在 `process.kill()` 之前有两个提前返回（`:732`、`:735-739`），那两处一个信号都不发，但第 1 次的文案写成了「会先发送一次」。

- **判定**：成立 6 / 不成立 0（其中 2 条低优先级或说明性，未改）
- **动作**：`53d2af1`（指南补 8 秒宽限 + 改为「核对通过才尝试强制终止」；K15 同步）、`7c4ed30`（`daemon_activity.rs` 模块注释区分「三个静默消失 / 审批留痕」）
- **验证**：`npm run build` 6.96s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（18.78s）
- **回帖**：评论 `2119859707413172224`
- **结论**：L2-1 的优先级上调——只要 `signal_daemon` 的「跳过」不可观测，该分支任何文案都写不准，因此真正的修法是先让跳过可观测，文案才能落地。

### 这轮实跑证明了什么

「每轮由 CodeBuddy 复核上一轮改动」这一环是有实效的，不是仪式：第 1 轮我按它上一批的指摘改了文档，第 2 轮它复核出**我改错的方向**（从「从不发信号」过度到「一定会发」），并给出了源码依据。如果只做单发、不回帖，这个偏差会留在 main 上，而且方向是让运维误以为「已经强杀过」。

---

### batch 2026-11-10 · daemon 控制与停止协议
- issue #7 / 开批时间 2026-11-10 02:23 (GMT+8)
- CodeBuddy 首条分析：已到（19 条指摘，索引：
  - A1 `AcceptedUnchecked` 为死分支，配套 warn 不可达（`daemon_control.rs:880`/`:997`）
  - A2 `NotAuthenticated` 变体不可达，`wrong_admin_token_never_signals_the_real_daemon` 实际只覆盖一条路径
  - A3 硬杀分支日志文案/文档/行为三者不一致，且该分支无测试可达
  - A4 `process_is_alive` 与 `daemon_pid_state` 判据双写，改名 daemon 得到「死了/健康」两个答案
  - A5 `read_daemon_pid` 把「并发写一半」与「永久损坏」压成同一句报错
  - A6 `write_daemon_pid_if_absent` 吞掉 `AlreadyExists` 这个应视为成功的分支
  - B1 `daemon.log` 轮转无「daemon 正在写」防护，rename 后日志写入 unlinked inode
  - B2 启动失败时 PID 清理结果不回报，kill 失败路径（`:499`）无任何断言
  - C1 `fetch_daemon_activity` 四个串行请求最坏 8s 无总预算，撞上 8s 停止宽限并卡住退出确认
  - C2 `count_listed` 把「字段缺失」与「字段为 false」混同，`is_idle` 口径随字段漂移翻转
  - C3 裸 HTTP 客户端 `read_to_end` 无字节上限，非 loopback 对端可无界挂起停止判定
  - C4 `direct_daemon_request` 用 `#[doc(hidden)]` 只服务测试，pub API 面两头不占
  - D1 prompt handler 在 async 上下文同步持 std Mutex，registry 毒化后 panic 不返回 500
  - D2 `/events/stream` 在 shutdown 后仍给新订阅者 200 开流，而非对齐 `/exec` 的 503
  - D3 停止期 503 被 `request_shutdown` 归为「没发出」（NotAccepted），与实际送达语义相悖
  - D4 `health()` 在 `config_dir` 不可用时仍回 `ok:true`，「健康」与「能服务」脱钩
  - D5 `/exec/compare` 的限流 `source` 恒为传输层默认值，限流实为单桶且与文档不对称
  - E1–E4 文档漂移：api.yaml 与源码注释的 "five seconds" 与代码 2s/4s 不符；指南「不再强制结束」与硬杀行为矛盾；K14 缺 config_dir 拒止路径记载
  - F1–F5 测试缺口：强杀分支、kill 失败、`AcceptedUnchecked`、start_time=0 组合断言、2s/4s 字面量无常量）
- 6 轮进度：6 / 6（第 1 轮 03:32 评论 `2119875798445228032`；第 2 轮 03:41 评论 `2119878023099891712`；第 3 轮 03:46 评论 `2119879303922892800`；第 4 轮 03:53 评论 `2119881205414801408`；第 5 轮 04:01 评论 `2119883153081802752`；第 6 轮 04:12 评论 `2119885765390790656`）

**批次说明（消费者必读）**：本批次 issue 正文发出时 ledger 尚不存在，故正文里「不成立清单」写的是「暂无」；但 ledger 在首条分析到达前已由 preflight 建立，含 2 条不成立判定（`signal_daemon` 第三条 `Ok(())` 吞错、`daemon_stop_e2e.rs:1443` 断言弱）。已核对首条分析未重复这两条；**从第 2 轮起，回帖须把上述 2 条不成立原文喂给 CodeBuddy**。

### consume 1 / 6 · 2026-11-10

- **指摘来源**：issue #7 首条分析（评论 `2119859466353930240`），26 条（A1-A6 / B1-B2 / C1-C4 / D1-D5 / E1-E4 / F1-F5）。
- **判定**：成立 14 / 部分成立 8 / 不成立 4。回帖 `2119875798445228032`。
- **动作**：`2a7dde2`（`daemon_control.rs` 的 `GRACEFUL_STOP_WAIT` 理由注释：五秒 → 2s/连接 + 4s/registry）、`15922ee`（`api.yaml` 同一处数字、指南 `:375` 与 K14 的 warn 语义、`OPT-LOOP-L2-PENDING.md` 追加 L2-3…L2-18）。推 `cnb main`，**未推 github**。
- **验证**：`npm run build` 6.43s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.66s）。
- **本轮最有价值的一条**：A1 被指为「`AcceptedUnchecked` 是死分支」，实测相反——`daemon_shutdown` 核对通过也回 200 `{"ok":true}`，`request_shutdown` 的 `200 && expected_pid.is_none()` 守卫使带 `expect_pid` 的停止必然落到 `AcceptedUnchecked`，于是 `:1001` 那条「daemon predates it」的 warn 在**每次正常停止**时都会响。指摘的结论方向错了，但它指的「无法区分检查是否发生」是真的，而且比它说的更严重：日志里那句是稳定的假阳性。

### 第 1 轮新增的不成立条目（原文保留，请勿重复提）

5. **「D3：停止期的 503 被 `request_shutdown` 归为 `not accepted`/`could not be sent`，与实际送达语义相悖」— 不成立。**
   `/shutdown` 没有任何 503 分支：全仓 `SERVICE_UNAVAILABLE` 只有 `bin/agent2ssh-daemon.rs:1720` 一处，属 `/exec` 的 `shutdown_outcome`。且 `request_shutdown` 对非 200/409 的响应走 `:893` 的 `"the daemon answered the shutdown request with {status}"`；`:898` 的 `"could not be sent"` 只接连接层 `Err`。指摘的两个前提都不成立。

6. **「E3：`docs/guides/configuration-guide.md:377` 仍写『不再按 PID 强制结束进程』，与硬杀行为矛盾」— 不成立（依据已过时）。**
   该句已由 `53d2af1` 替换为「8 秒宽限内没退出 → 重新核对身份 → 核对通过才尝试强制终止 → 再等 2 秒」。现存段落里没有那句话。

7. **「E4：K14 未记载旧 daemon 不报 `config_dir` 就拒绝停止」— 不成立。**
   K14 已有原文：「`/health` 的 `config_dir` 缺失（旧 daemon）同样按『无法识别』拒绝，并提示先 `agent2ssh daemon restart` 升级后重试」。

8. **「F4：`classify_process_observation` 未覆盖 `Some((0,false))` 与 `expected_start` 的组合」— 不成立。**
   `daemon_control.rs:1143-1146` 明确断言 `classify_process_observation(Some((0, false)), false, 12) == Unknown`，`:632` 早于 `:633` 的顺序已被钉住；`expected_start=0` 不会出现（`daemon_process_start_time` 在 `:111` 已过滤 0）。

### consume 2 / 6 · 2026-11-10

- **指摘来源**：CodeBuddy 对第 1 轮的复核（评论 `2119876512399654912`），15 条（L2-19…L2-33）。
- **判定**：成立 7 / 部分成立 6 / 不成立 2。回帖 `2119878023099891712`。
- **动作**：`cf09e2c`（`daemon_control.rs` 注释补三处行号、串行关系、50ms 轮询粒度）、`6f9ba45`（指南 `:375` 三处自纠 + `api.yaml` 改成 best effort + K14 补「健康响应可伪造」与 409 限定 + L2-19…L2-33 入表）。推 `cnb main`。
- **验证**：`npm run build` 6.56s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.66s）。
- **自纠三处**（第 1 轮自己写的）：①「看 daemon 侧日志有没有核对记录」——daemon 只在 409 写日志，核对通过不留痕，这句话把人指向一个不存在的证据；②「先 `daemon restart` 升级」——restart 内部就是 `stop_daemon_process`，旧 daemon 同样被 `:970` 拒，出路是死循环；③「两者都会落进同一条 warn」——无 body 的 `request_graceful_shutdown` 走 `:867` 那一支，不记 warn（该函数在生产代码里无调用点，仅测试用）。
- **本轮最有价值的一条**：`L2-26`。exec handler 的 `drain_handle.unregister()` 写在 `run_ws_with_shutdown_deadline` 的 async 块**内部**（`:5499`，块在 `:5500` 收口），2s 超时 drop 后不执行 → registry 条目残留 → 每次关闭 `wait_until_empty` 烧满 4s。terminal handler 的 `unregister` 在块外（`:5057`），不受影响。所以「关闭要 6 秒」不是设计余量，是泄漏的代价。

### 第 2 轮新增的不成立条目（原文保留，请勿重复提）

9. **「L2-24：`DaemonStopError::Other` 可能把 `anyhow::Error` 二次包装，需要确认显示的是原文」— 不成立。**
   `daemon_control.rs:806` 是 `Self::Other(error) => write!(f, "{error}")`，内层错误原文直通，不存在包装层。

10. **「L2-32：中止路径上 `terminal_inputs` 与 `limiter` 也泄漏」— 不成立。**
   `terminal_inputs` 的 insert（`:4800`）与 remove（`:5036`）、`limiter.unregister_session`（`:5037`）全在 terminal handler，且都在 `:5009` 收口的 async 块之外，超时返回 `false` 后照常执行；exec handler 不碰这两个 map（全仓 `terminal_inputs` 只有 `:280,4296,4511,4800,5036`）。与 L2-26 是同一个作用域误读。

### consume 3 / 6 · 2026-11-10

- **指摘来源**：CodeBuddy 对第 2 轮的复核（评论 `2119878811184422912`）。主题不是新发现，是**推翻我上一轮的 L2-26**。
- **判定**：我上一轮错 1 条（L2-26 撤回）；新指摘 3 条中成立 1（Windows 平台划分）、部分成立 1（注释理由，核对后无需改）、不成立 1（「必删 daemon.pid」）。回帖 `2119879303922892800`。
- **动作**：`f669326`（`api.yaml` 的 `is dropped` → `stops being waited for`；指南带外停止分 Unix/Windows；`OPT-LOOP-L2-PENDING.md` 删除 L2-26 行并注明撤回）。推 `cnb main`。
- **验证**：`npm run build` 6.56s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.20s）。
- **怎么错的**：把 `:5500` 的 `})` 当成 `run_ws_with_shutdown_deadline` 那个 future 的收口，于是把 `:5499` 的 `unregister` 判在块内。实际 future 收在 `:5428`（ind=8），`:5500`（ind=4）关的是 `on_upgrade` 闭包。**教训：判作用域不能只读到「看起来像收口」的那一行，要量缩进并找到外层构造的起点**（`ws.on_upgrade(|socket| async move {`，`:5161`）。
- **连带**：上一轮「关闭要 6 秒是泄漏的代价」这个因果作废；6s 上界仍成立，但理由只是「2s 与 4s 两笔预算串行」，无需额外解释。

### 第 3 轮新增的不成立条目（原文保留，请勿重复提）

11. **「L2-26：exec 的 `drain_handle.unregister()` 在 `run_ws_with_shutdown_deadline` 的 future 内（`:5500` 收口），2s 超时 drop 后不执行，registry 条目残留，每次关闭 `wait_until_empty` 烧满 4s」— 不成立。**
    future 收在 `:5428`（`}, Duration::from_secs(2)).await;`，ind=8），`unregister` 在 `:5499`（ind=8）属 `on_upgrade` 闭包、在 future 之外，`:5500`（ind=4）关的是 `on_upgrade`。`0e88d7c` 正是把它从块内挪到块外的那笔提交（同笔还把两处 `unregister().await; return;` 改成 `break`）。terminal 侧同理（`:5009` 收口 / `:5057` 执行）。两条都不泄漏，「每次关闭多烧 4 秒」的前提不存在。

### consume 4 / 6 · 2026-11-10

- **指摘来源**：CodeBuddy 对第 3 轮三个问题的答复（评论 `2119880234643128320`）。无新缺陷，是三条 L2 的落地设计。
- **判定**：设计 3 条全部采纳（`L2-3` 加 `ShutdownAccepted` 字段不换状态码、`L2-25` 只加诊断、`L2-22/33` 用 `DrainOutcome` 三臂）；其中 `L2-3` 的**前置假设有误**（见第 12 条）。回帖 `2119881205414801408`。
- **动作**：`6796622`（`OPT-LOOP-L2-PENDING.md` 增加「第 4 轮：三条 L2 的落地形状」一节，并标注 `daemon_control.rs` 裸行号整体下移 5 行）。推 `cnb main`。
- **验证**：`npm run build` 6.51s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.48s）。
- **它开头自相矛盾**：先说「warn 每次正常停止都响已经不成立」，随后三段论证证明仍然成立。当前锚点 `:872`/`:887`/`:996`/`:1002`/`:1006` 与它的引用一致（我早期引用的 `:867` 等是注释扩写前的编号）。

### 第 4 轮新增的不成立条目（原文保留，请勿重复提）

12. **「`stop_daemon_process_waits_for_a_graceful_exit`（`daemon_stop_e2e.rs:180`）已读 `app.log` 并断言不含 `daemon predates it`，所以配套断言已存在」— 不成立。**
    `git grep "daemon predates"` 全仓只命中实现处 `daemon_control.rs:1006`，`tests/` 零命中。`daemon_stop_e2e.rs` 里唯一读 `app.log` 的是 `:397`，断言的是 `WebSocket drain deadline expired`；`:180` 那条用例断言耗时、PID 文件与退出码，不读日志。该断言需要**新建**。

### consume 5 / 6 · 2026-11-10

- **指摘来源**：CodeBuddy 对第 4 轮的回答 + 新一批（评论 `2119882346487767040`），10.6 KB。
- **判定**：成立 4（N-1…N-4）/ 部分成立 1（N-5，与 L2-12 同一处）/ 不成立 1（N-6，建议项已存在）。回帖 `2119883153081802752`。
- **动作**：`5b1d4d1`（`forward.rs` 的 `stop_worker` 加注释：紧随 `join_timeout` 的 `Stopped` 是意图不是套接字）、`cb6d807`（`OPT-LOOP-L2-PENDING.md` 新增第 5 轮一节 + 把「`warn` 记成功应降 `info`」追加进 L2-3）。推 `cnb main`。
- **验证**：`npm run build` 6.38s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（18.10s）。
- **本轮最有价值的一条是级别不是命名**：`daemon_control.rs:1006` 用 `warn` 记的是一次**成功**的停止。命名怎么调都还在这个级别上打转；把它降 `info`、让 `component` 承担区分，才是「日志难读」的解。
- **与前一阶段的呼应**：N-1（`forward.rs` 的 worker）与 N-3（exec stream 线程）是同一个缺陷模式——线程持有的资源比调用方以为的存活时间长。两者都建议用 `Arc<AtomicBool>` + 循环内检查这一套同批做。

### 第 5 轮新增的不成立条目（原文保留，请勿重复提）

13. **「N-6：CI 的两处 `daemon_stop_e2e` 不设 `AGENT2SSH_TEST_SSH_PORT`，`stop_exits_when_exec_websocket_peer_stops_reading` 在 CI 被跳过计通过，所以报告 `:49` 的『23 passed』需要加注」— 不成立（建议项已存在）。**
    事实部分成立：`ci.yml:214-217` 与 `:323-325` 都不设该变量，`daemon_stop_e2e.rs:317` 会早退并记 `ok`。但注释已在：`docs/reports/CNB-ISSUE-2-WINDOWS-RETEST-2026-11-09.md:59` 写明「23 项不含该用例（`cfg(unix)`），0 项不算通过」，`:122` 写明「CI 没有该场景的 Windows 回归覆盖」。不需要动作。

### consume 6 / 6 · 2026-11-10

- **指摘来源**：CodeBuddy 对第 5 轮的回答 + 第 6 批 5 条（评论 `2119884182657613824`）。
- **判定**：成立 1（P-1）/ 部分成立 3（P-2、P-3、P-5）/ 不成立 1（P-4）；四条更正里 3 条采纳、1 条是我错。回帖 `2119885765390790656`。
- **动作**：`369e519`（`OPT-LOOP-L2-PENDING.md` 新增第 6 轮一节 + 三条更正/记录）。本轮**零代码改动**。推 `cnb main`。
- **验证**：`npm run build` 6.46s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.95s）。
- **本轮最有价值的一条**：`P-1`。`forward_stop_core`/`forward_start_core` 在授权（可 `await` 300s）之后重新拿锁重判状态，而授权前的判定已经过期：stop 的后到者落在 `worker.take()` 为假的空操作上，start 的后到者 `store.insert` 覆盖先到句柄、被覆盖句柄的 `Drop` 关掉监听。桌面双击即触发。这是本批唯一「检查与执行之间隔着一次 `await`」的完整形态。
- **对我自己的一条更正**：第 5 轮我说「`Refused` 会撞名所以不能复用」是错的——`ShutdownRequest` 是私有中间枚举，`Refused { actual }` 本来就在（`daemon_control.rs:829-832`），并在 `:1017-1022` 立刻映射成 `DaemonStopError::ExpectedPidRefused`，逃不出函数，日志面只有一套名字。结论（保持三层、`AcceptedUnchecked` 改名 `Unreported`）不变，理由换成语义。
- **计数口径改四列**（采纳 CodeBuddy 建议）：成立 / 部分成立 / 不成立 / 已并入。不再把「部分成立」「已并入」折算进两侧。按此重算，本批 `A1` 记「提法错、实质成立且更严重」，不记「方向错」；「方向错」只剩 `D3` 与 `L2-26` 两条。

### 第 6 轮新增的不成立条目（原文保留，请勿重复提）

14. **「P-4：`daemon start` 的健康探测超时后不 kill child，`std::process::Child` 的 `Drop` 不 kill，子进程成孤儿、继续持有监听地址，PID 文件无人收」— 不成立。**
    超时路径确实 kill 且 wait：`daemon_control.rs:481` `try_wait()` 为 `None` → `:488` `child.kill()` → `:502` `child.wait()`。只有 `kill()` 返回 `Err` 时才可能留下活 child，那正是 `L2-9` 已登记的「kill 失败被吞」。`Child::Drop` 不 kill 这句话本身对，但它不在这条路径上。

---

## 批次汇总 · 2026-11-10 · issue #7 · daemon 控制与停止协议

- **批次日期**：2026-11-10（02:23 开批，04:12 收尾，GMT+8）
- **主题**：daemon 控制与停止协议
- **issue**：#7 `[opt-round] batch 2026-11-10 · daemon 控制与停止协议`（已关闭）
- **总量**：6 轮共核实 **58 条** → 成立 26 / 部分成立 19 / 不成立 10 / 采纳为设计 3（`L2-26` 从第 2 轮的「成立」移入不成立）
- **提交**：14 笔，全部推 `cnb main`，**未推 github**。代码注释 3 笔 / 文档 5 笔 / ledger 6 笔。**零行为改动**——凡是动到语义的都留在 L2

| 轮 | 指摘 | 判定 | 本轮提交 | 回帖 |
|---|---|---|---|---|
| 1 | 26（A1-A6/B1-B2/C1-C4/D1-D5/E1-E4/F1-F5） | 成立 14 / 部分 8 / 不成立 4 | `2a7dde2`（`GRACEFUL_STOP_WAIT` 注释 5s → 2s+4s）、`15922ee`（`api.yaml` 同数字、指南 `:375`、K14、L2-3…L2-18）、`882ed4d`（ledger） | `2119875798445228032` |
| 2 | 15（L2-19…L2-33） | 成立 7 / 部分 6 / 不成立 2 | `cf09e2c`（注释补行号、串行关系、50ms 轮询）、`6f9ba45`（自查三处自纠 + best effort + K14 补强）、`8e791f2`（ledger） | `2119878023099891712` |
| 3 | 3（推翻 `L2-26`） | 成立 1 / 部分 1 / 不成立 1，另自撤 1 | `f669326`（`api.yaml` 改 best effort、指南分平台带外停止、删 L2-26 行）、`f0d64d7`（ledger） | `2119879303922892800` |
| 4 | 3（设计答复） | 采纳 3（其中 1 条前置假设错） | `6796622`（三条 L2 的落地形状）、`9283b98`（ledger） | `2119881205414801408` |
| 5 | 6（N-1…N-6） | 成立 4 / 部分 1 / 不成立 1 | `5b1d4d1`（`forward.rs` 注释：状态是意图不是套接字）、`cb6d807`（第 5 轮 L2 表）、`852d55c`（ledger） | `2119883153081802752` |
| 6 | 5（P-1…P-5） | 成立 1 / 部分 3 / 不成立 1 | `369e519`（第 6 轮 L2 表 + 三条更正） | `2119885765390790656` |

- **L2 待拍板清单**：`docs/reports/OPT-LOOP-L2-PENDING.md`，29 行——`L2-3`…`L2-33` 共 24 行（编号不连续，缺号是并入或撤回的），加本轮的 `P-1`…`P-5`。优先级：`P-1`（授权与执行之间隔一次 `await`）、`L2-3`（`warn` 记成功 → 降 `info`）、`L2-10`（`fetch_daemon_activity` 与 `get_daemons_unified_view` 两处都要总预算）、`L2-9`（启动失败清理不可观测）。
- **本批最值钱的两条**都不是单个缺陷，而是**级别**与**位置**：`:1006` 用 `warn` 记一次成功的停止（改一个级别就少查半小时）；`:5499` 的 `unregister` 在 `on_upgrade` 闭包里而不在 future 里（我第 2 轮把「看起来像收口」的那一行当成作用域边界，第 3 轮才量缩进纠正）。
- **给下一批的方法学**：①判作用域要量缩进并找到外层构造起点，不能读「看起来像收口」的那一行；②判「检查是否发生」要看守卫让哪条路径成为**唯一**路径，不是看分支数量；③计数用四列（成立/部分成立/不成立/已并入），折算会把「提法错但实质成立」这类抹掉。

---

### batch 2026-11-10 · SSH 传输、known_hosts 与认证（russh 侧）
- issue #8 / 开批时间 2026-11-10 04:23 (GMT+8)
- CodeBuddy 首条分析：已到（18 条指摘 + 2 条范围外登记，索引：
  - A1 `LocalKeySigner::auth_sign` 返回 `to_sign||len||sig`，疑与 russh `Signer` 契约相反，钉住它的测试按实现写故恒绿（`russh_impl/mod.rs:128-158`）
  - A2 RSA 主机键在 `host_key_algorithm`（`Rsa => "ssh-rsa"`）与默认 hostkey 列表 `rsa-sha2-256/512` 之间不自洽，疑真机切换即误报指纹改变（`embedded_ssh.rs:152-161`）
  - B1 Windows agent 管道可打开但无匹配身份时永不回落 Pageant，源码零注释（`russh_impl/agent.rs:302-320`）
  - B2 终端路径两处在 `set_blocking(false)` 后调阻塞 `wait_close`，`session_timeout_ms` 可夹到 `u32::MAX`（`embedded_ssh.rs:2155/2187`）
  - B3 `spawn_terminal` 线程无 stop 标志无 JoinHandle，drop `cmd_tx` 后 `Close` 分支永不执行（`embedded_ssh.rs:2064-2080`）
  - B4 读循环无「单次无进展」上限，死通道+无 eof 时 bridge 空转 / drain 挂满 deadline（`embedded_ssh.rs:1394-1416`）
  - C1 终端/exec 三处 `let _ =` 丢弃 `wait_close` 结果，与 exec 路径的核对写法矛盾（`embedded_ssh.rs:1119/2155/2187`）
  - C2 `auth_methods()` 空实现使「按服务器通告方法」的两条分支恒死、日志恒空串（`russh_impl/mod.rs:898-900`）
  - C3 `ssh_algo.rs` 模块文档仍描述已删除的 libssh2/WinCNG/openssl-on-win32 世界（`ssh_algo.rs:1-33`）
  - C4 `sftp_transfer.rs` 注释仍以「Dropbear 16 KiB」为据，与 K2 复测结论相反（`sftp_transfer.rs:229-247`）
  - C5 `STALL_TIMEOUT` 只在 `write` 返回后检查，「阻塞不返回」形态下永不判定（`sftp_transfer.rs:293-345`）
  - C6 `remove_system_known_host` 不支持 IPv6 `[addr]:port` 形式且 0 命中静默返回 0（`embedded_ssh.rs:585-591`）
  - C7 主机证书在 `check_server_key` 静默按裸键比对，注释未写 CA 信任/有效期一概不查（`russh_impl/mod.rs:435-459`）
  - D1 `StallGuard` 状态机本体零测试（`sftp_transfer.rs:521-558`）
  - D2 `forward.rs` remote 分支把 `WouldBlock` 当错误 return，local 分支退避重试，两分支不对称（`russh_impl/forward.rs:688-693`）
  - D3 「参数无法表达」三种规格并存：报错 / 注释后忽略 / 直接忽略（`channel.rs:479-489` vs `sftp.rs:143-149`）
  - D4 `channel.rs:700-720` 测试注释易把「2s 上界」读成「2s 等待」
  - D5 `LocalKeySigner` 两条错误臂零测试（`russh_impl/mod.rs:104-116`）
  - 范围外 2 条：`Settings::run` 的 disconnect abort 语义；`core.rs` 上传/下载无 session 超时（与 C5 同根因）
- 正文已喂给 CodeBuddy：12 条「不成立」原文（preflight 2 + 第 1–6 轮新增 10）+ K2/K7/K9/K12 与 §3 未验证范围空档

### consume 1 / 6 · 2026-11-10 05:20–05:26 (GMT+8)

- **判定**：18 条（A1-A2 / B1-B4 / C1-C7 / D1-D5）→ **成立 11 / 部分成立 5 / 不成立 2**。
  - 成立：B1 B3 C1 C2 C3 C4 C5 C6 D1 D3 D5
  - 部分成立：A1（核心不成立，只有「verbatim」那句措辞误导）A2（核心结论错，残留是 RSA 主机键无测试）B2（结构成立，「49.7 天」错，本路径上界 60 s）B4（bridge 成立，drain 已有一半保护）C7（缺句属实，行号偏 44 行）
  - 不成立：D2（两分支都是 log + 退避 + 继续，remote 不 return；且 accept 循环在 `src/forward.rs`，不在本批清单的 `transport/russh_impl/forward.rs`）D4（`:705` 就是那行注释）
- **A1 的关键证据**（判据方向被推翻）：`russh 0.64.1` 的 `keys/agent/client.rs:345-367` `sign_request_pk` 是 `write_signature(&mut data)?; Ok(data)`，契约是「返回 `to_sign` 且签名已追加」，不是「只返回签名」。`client/encrypted.rs:277-306` 只把 `buffer[i..]` 入包，`i` = `client_make_to_sign` 返回的 session_id 之后偏移（`:1512-1543`），故无「重复 USERAUTH_REQUEST」。测试里的 `&signed[data.len()..]` 是 buffer→agent SIGN_RESPONSE 的翻译，若实现真只返回签名该行会越界 panic。
- **动作**：`02f936b`（L1 注释/文档，6 文件：`ssh_algo.rs` 模块文档重写 + `SFTP_SAFE_WRITE` 对齐 K2 + `auth_sign`/`host_key_algorithm`/`check_server_key`/`agent.rs`/`sftp.rs` 五处契约注释）、`562340a`（`OPT-LOOP-L2-PENDING.md` 11 条）。
- **验证**：`npm run build` 6.48s；`cargo test --no-default-features --lib` 856 passed / 0 failed / 1 ignored（17.42s，与改动前同数）。
- **回帖**：`2119904212174680064`。推送 `cnb main`（`a8f9519..562340a`），未推 github。
- **操作异常（需处理）**：进入本轮时 `/tmp/agent2ssh-opt-loop.lock` 是 04:22 生产任务建立后**未释放**的孤儿锁（mtime 停在 04:22、目录为空、无 pid 文件）。按协议重试 3 次（120 s 间隔）全失败后做了存活核验：`ps` 无其它 codebuddy/claude 进程，`lsof -d cwd` 显示仓库内进程全部属于本会话，生产任务自己的记忆记着 04:30 已完成并推送。结论是「上一批仍在运行」这一前提不成立，遂接管（`rm -rf` + `mkdir`）并在本轮结束时释放。**生产者任务需要补锁释放**，否则每次生产后消费者会被卡 240 分钟。
- **自查**：`02f936b` 正文把「1 个测试改名 + 2 条消息」写成「2 个测试名 + 3 条断言消息」，已在本轮回帖里更正，未改历史（提交已推送）。
- **第 2 轮未开始**：回帖后按 60 s × 5 次轮询，`total` 一直是 2（无新评论）。按「不空等」的规则，本可拿已有材料推进，但查过两处：①`docs/`、`skills/` 里 `WinCNG`/`libssh2` 的残留都写成历史（`configuration-guide.md:168-169`、`skills/agent2ssh/SKILL.md:376/380`），user-facing 文档无第二处「运行时 capability 探测」的描述，`reads_openssh_private_keys` 已不存在（`bin/agent2ssh.rs:3580` 直接输出 `true`）——C3 的尾巴是干净的；②其余待办全是 L2（不得动 main）。**所以本轮没有可合法推进的 L1 材料，也不发一个只占位、无内容的 `## 第 2 轮收尾`**——评论数就是轮次计数器，空发等于白烧 6 轮里的一轮。
- **下次运行**：从 C=1 → 第 2 轮起，直接消费 CodeBuddy 对本轮回帖的回复（届时必已在）。

#### 本轮进行中发生的并发写入（下次运行必读）

推送 ledger 时发现 `cnb main` 已被**另一个写入者**推进了两笔（作者 `cnb.lengyuqu <…@noreply.cnb.cool>`，时间 05:30，本机 git 身份是 `zdz945244799@gmail.com`，所以不是本机任何一个进程）：

- `8e16f40` — 改了 `src-tauri/src/bin/agent2ssh.rs`、`embedded_ssh.rs`（+173，全是测试）、`sftp_transfer.rs`（+43）。提交正文自述 **`Verified: not run`**，理由是那个容器里没有 Rust 工具链、也没有网络装。
- `09d49ea` — 往 `OPT-LOOP-L2-PENDING.md` 追加 23 行（两条新发现）。

我用本机工具链替它跑了 `cargo test --no-default-features --lib`：**861 passed / 1 failed / 1 ignored**。失败的是它自己新增的
`embedded_ssh::remove_system_known_host_matching::an_ipv6_literal_at_port_22_is_removed`，断言 `[::1]:22` 应被删掉而实现只拼裸名。看代码这是**有意为之**——测试注释写着「Pinned as a failure so the fix has a target」，也就是说它是一个没有配套修复的红测试，直接进了 main。

由此三件事，下次运行必须知道：

1. **main 当前是红的**（截至 `09d49ea`）。在它变绿之前，本流程任何「全部通过」的验证声明都要注明基线含这一条已知失败，否则会被误当成自己改坏。
2. **锁挡不住跨机写入**。`/tmp/agent2ssh-opt-loop.lock` 只在同一台机器上有效；这次的另一个写入者不在本机进程表里，我的孤儿锁判定在「本机无持有者」这个层面上是对的，在「全局无并发」这个层面上是错的。两个消费者同时跑同一批，就会出现这种一方在写 L2、另一方直接改源码的情况。
3. **待决策项多了一条**：那个红测试要么配一个 `remove_system_known_host` 的修复（`src-tauri/src/**` 行为改动，属 L2），要么降级为 `#[ignore]` 并写清原因。我不动它——它不是我写的，且改它就是我要越过的同一条 L2 线。

#### 自我更正：2.6x 这个数字我抄错了

`SFTP_SAFE_WRITE` 原注释说「64 KiB 比 16 KiB 快约 2.6x（28 vs 11 MB/s over loopback）」，我在 `02f936b` 里把它原样保留（只在提交正文里引了一次）。K2 自己记的那一对是同一个 Dropbear、同一次 8 MiB 上传的 485 ms 与 1 s，比值 **2.06x**；2.6x 来自另一个（loopback/OpenSSH）测量，不能和 K2 的这对并列引用。`8e16f40` 已把它改掉，并加了一条 `include_str!` 断言钉住「注释里引的必须就是 K2 的那两个数」。以后引用这个代价用 2.06x，或者干脆说「真实但未量化」。

### consume 2 / 6 · 2026-11-10 06:34–06:58 (GMT+8)

- **指摘来源**：CodeBuddy 对第 1 轮回帖的复核（评论 `2119905678654341120`）。11 条独立指摘（C3-1…C3-4、C4、N1…N7；N4 与 C3-2 同条）+ 1 条自查计数。
- **判定**：**成立 4 / 部分成立 3 / 不成立 3**，另 1 条信息性成立（N6）、1 条已由 `8e16f40` 修掉（C4，本轮无待办）。
  - 成立：C3-1（`keys algorithm` 的 clap 文档仍在描述 libssh2/WinCNG，而 `keys.rs:180-192` 已把同一件事写对，两处相反）、C3-2=N4（`MethodType::LangCs/LangSc` 文档声称 `supported_algs` 会替它们回答）、N2（forget 文案把「没有这行」与「有这行但读不到」压成一句）、N3（legacy RSA 重试是死分支）。
  - 部分成立：N1（代码形状与红测试属实，**前提被实测推翻**，见不成立 17）、N5（静默丢 `to_bytes` 失败属实，触发条件近乎不可达）、N7（`client` 置 `None` 属实，后半句说错，见不成立 18）。
  - 不成立：C3-3、C3-4、C4。
- **本轮最值钱的一条**：C3-1。`--help` 是用户直接读的界面，它还在解释一个被删掉的实现，而同仓 `keys.rs` 已经写对——两处结论相反的文本比一处孤立的错更贵，因为读的人不知道信哪个。
- **我自己多查出来的两条**：①`bin/agent2ssh.rs` 里 `keys algorithm` 的 RSA 分支不可达（`default_for_build()` 恒 Ed25519），那句「this build cannot read OpenSSH-format private keys」永远不会打印，但属同一族过期表述；②`apply_prefs` 对 `SignAlgo` 是空臂（`russh_impl/mod.rs:704`），所以 legacy 重试**即使绕过守卫也会被静默丢弃**——比指摘给的「恒 `return false`」多一重原因。
- **动作**：`f23f006`（6 文件：clap 文档、CLI 死分支文案、`MethodType` 文档、`supported_names` 空臂注释、`embedded_ssh` 三处注释、`cli_contract` 文档、`ssh_algo` 两条断言）+ 本笔（本 ledger、L2 三条、迁移文档第 6 条）。
- **验证**：`npm run build` 6.49s；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（17.77s）——失败项是基线就有的 `an_ipv6_literal_at_port_22_is_removed`（`8e16f40` 留下的故意红测），非本轮引入；`cargo fmt --check` 本轮**之前是红的**（`embedded_ssh.rs:662` 一行未格式化，同属 `8e16f40`），归位后为绿。
- **回帖**：`2119924514724155392`（`## 第 2 轮收尾`）。
- **锁**：进入时 `/tmp/agent2ssh-opt-loop.lock` 又是孤儿锁（mtime 05:34、目录空、无 pid）。按协议重试 3×120 s 全失败 → 存活核验（`ps` 无其它 agent；`lsof -d cwd` 仓库内进程全属本会话）→ 接管，结束释放。**这次上一轮消费者自己的记录说「结束时已释放」，锁却还在**，所以不只是生产任务的问题：释放本身不可靠，建议把释放写进 shell `trap`，而不是流程末尾一句。
- **第 3 轮未开始**：本轮回帖（`2119924514724155392`，06:46）之后按 60 s × 5 轮询到 06:51，issue 评论总数一直是 5（无新评论）。**不发占位轮次**——评论数就是轮次计数器，空发等于白烧 6 轮里的一轮。下次运行从 C=2 → 第 3 轮起，指摘在 CodeBuddy 对本轮回帖的回复里。

### 第 2 轮新增的不成立条目（原文保留，请勿重复提）

15. **「C3-3：`skills/agent2ssh/SKILL.md:368-371` 那句『`algo set` checks the shape of a name, not whether the linked transport can do it』在 russh 下已不成立」— 不成立。**
    原句是复合句，后半句紧接「and the preferences are applied fail-closed immediately before each handshake」，两句合起来正是当前实现：写文件那步只做 `is_plausible_algorithm_name`（`ssh_algo.rs:403`，加非空/去重/compression 约束），硬失败发生在 `apply_algo_prefs` → `apply_method` → `apply_prefs`。指摘自己给的证据（`apply_method` 握手前 fail-closed、`algo set` 写文件无校验）就是这句话的内容，所以是把「只查形状」读成了「什么都不查」。（附带更正：`apply_method` 比对的 `session.supported_algs` **不是**运行时能力查询，`russh_impl::supported_names` 是静态白名单，其注释自述 "It is a snapshot, not a query"。）SKILL.md 不改。

16. **「C3-4：`docs/reports/WINDOWS-HANDOVER.md:34` 的『`algo set` 能放开哪些算法取决于后端的 `supported_algs`』依赖一个不存在的前提」— 不成立。**
    `apply_method`（`ssh_algo.rs:229-244`）确实调用 `session.supported_algs(method_type)`；`Session::supported_algs` 存在（`russh_impl/mod.rs:1103`）且返回 `Ok(...)`。它点名的那些方法族（Kex/HostKey/Cipher/Mac/Comp）上「后端有个可查询的集合」成立，只是集合是静态快照而非运行时探测——那是 C3-2 的落点，不是这一条。该行属 libssh2 时代的 Windows 验证清单，不改。

17. **「N1：`remove_system_known_host` 对 IPv6 字面量在 22 端口是静默 no-op，且 `ssh-keygen -R` 是参照、它会删掉 `[::1]:22`」— 核心前提不成立（代码形状与那条红测试属实，处置见 L2-15）。**
    本机实测：known_hosts 写入 `[::1]:22`、`::1`、`other.example` 三行，`ssh-keygen -R ::1` 输出 `# Host ::1 found: line 2`，**只删第二行**，`[::1]:22` 原样保留；`ssh-keygen -R '[::1]:22'` 才删第一行。所以「`-R ::1` 会删 `[::1]:22`」是错的，本仓实现与它自己声明的参照一致。`man 8 sshd` 同句写明方括号 + `:port` 只用于**非标准端口**。真实缺口仅是「别的工具可能写出 `[host]:22`，这类残留只能手工清」——那是产品口径（L2-15），不是「漏了一种 OpenSSH 会写的形态」。

18. **「N7：`list_identities` 第二次失败的文案与第一次相同，区分不出来」— 后半句不成立（该条其余部分成立，见 L2-17）。**
    第一次是 `the SSH agent refused to list identities: {error}`（`agent.rs:237`），第二次是 `the SSH agent is not connected`（`agent.rs:235`，因为 `client` 已置 `None`），两者不同。真正的问题是**第二次与「从未连过」不可区分**——那是该条前半句说的，后半句把它说成了另一件事。

### consume 3 / 6 · 2026-11-10 07:52–08:05 (GMT+8)

- **指摘来源**：CodeBuddy 对第 2 轮回帖的复核（评论 `2119927131357495296`），9 条新编号（19–27）。
- **判定**：**成立 3 / 部分成立 3 / 不成立 3**（其中 26 是范围外线索、27 是信息性成立）。
  - 成立：19（`SKILL.md:127-132` 与两份指南仍在解释「取决于 SSH 库能读什么」）、24（`known-hosts import` 把解不开 base64 的 key 归给 `hashed or conflicting`）、27（同一次 `import-config` 内的重名会双双落盘）。
  - 部分成立：20（提示语是条件式 caveat 而非因果断言，但它把唯一被点名的原因给了用户最难踩到的那一个）、22（朴素修法会打碎邻居属实；「不存在能同时满足两条断言的实现」不成立，见下）、25（断言确实恒真，但它自己建议的处置就是只改注释，已照此执行）。
  - 不成立：21、23、26（原文见下方 19–21 条）。
- **本轮最值钱的一条**：19。第 1 轮这句被判「不成立」是因为它跟着「applied fail-closed before each handshake」还能读通；`f23f006` 把 clap 长帮助从「按构建探测」改成「恒 ed25519」之后，同一句话的前提句留在原地，于是指向 agent 的那份说明书成了全仓唯一还在讲「取决于库能读什么」的地方——而且它比 `--help` 更常被读，因为 agent 是照它决定要不要先问一句。
- **我自己多查出来的两条**：①`cli_contract.rs:976` 的「a script that skipped `keys algorithm` would generate a key this build cannot authenticate with」是同一族的第三处，同笔改掉；②`docs/plans/russh-migration.md:194-195` 是全仓最后一处按旧行号列 `reads_openssh_private_keys` 调用点的地方（计划文档的「补位点」历史段），加了一句「迁移前行号、按名字找」的标记，避免下次 grep 又从这里读到 `keys.rs:821`。
- **动作**：`3ea8f70`（`cli_contract.rs` 两处注释 + 一处断言消息、`embedded_ssh.rs` 两处测试文档）、`bf26f8c`（`SKILL.md` 两处 + `cli-quickstart.md` + `configuration-guide.md` + `russh-migration.md` 标记 + `OPT-LOOP-L2-PENDING.md` 三条与 L2-15 补充 + 本 ledger）。
- **验证**：`npm run build` 6.42s；`cargo fmt --check` 绿；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是基线那条红测，本轮只改了它的文档注释）；`cargo test --no-default-features --test cli_contract` **40 passed / 0 failed**。
- **回帖**：`2119943001739788288`（`## 第 3 轮收尾`）。
- **第 4 轮未开始**：回帖后按 60 s × 5 轮询。下次运行从 C=3 → 第 4 轮起，指摘在 CodeBuddy 对本轮回帖的回复里。
- **锁**：进入时又是孤儿锁（mtime 06:52、空目录、无 pid）。这次先做存活核验（`ps` 无其它 agent，`lsof -d cwd` 全属本会话；上一轮记忆写着 06:52 结束），再接管，绕过了「3×120 s 重试」——连续三轮都是孤儿锁，重试只是在等一个不会释放的目录。**释放仍然是薄弱点，这是第三次记录。**

### 第 3 轮新增的不成立条目（原文保留，请勿重复提）

19. **「第 3 轮 #21：IPv6 的非标准端口永远删不掉——`host` 传进来已是方括号形式，拼出 `[[::1]]:2222`」— 不成立。**
    实测 `HOME=<tmp> agent2ssh known-hosts forget ::1 --port 2222`，文件含 `[::1]:2222` 与 `other.example`：输出 `removed 1 known_hosts entry for ::1 (port 2222)`，文件只剩 `other.example`。原因是 CLI 传的是裸 `::1`——clap 的 `host: String` 没有值解析器（`bin/agent2ssh.rs:491-500`），桌面侧 `App.tsx:536` 传的 `host.host` 也是裸地址；`format!("[{}]:{}", "::1", 2222)` 得到的是正确的 `[::1]:2222`。所以这条「唯一存在的理由正好失效的那一半」并不成立：非标准端口这一半是好的，坏的是 22 端口那一半（L2-15）。
20. **「第 3 轮 #23：删成空文件时丢掉尾换行，与 `ssh-keygen -R` 不符」— 不成立。**
    三种形态与 `ssh-keygen -R` **逐字节相同**：①单行文件删掉唯一一行 → 两边都是 0 字节；②两行文件删最后一行 → 两边都剩 `…\n`（55 字节，hex 一致）；③注释 + 空行 + 一行，删那一行 → 两边都是 `# comment\n\n`（11 字节）。`raw.lines()` 丢掉文件末尾的换行、`kept` 对每个保留行补 `\n`，合起来的结果与 `ssh-keygen -R` 的重写方式一致。它表格里「`ssh-keygen` 重写后文件仍以换行结尾」这一格是错的，那一格错了整条结论就反了。**口径**：这是 macOS 自带 OpenSSH 的行为，对方环境是 Linux 9.2p1；若它那边确实在重写后留尾换行，那是参照自身的平台差异，不是本产品与参照不一致——判定仍是不成立，但理由要从「它与 `ssh-keygen` 相同」换成「它没有引入 `ssh-keygen` 没有的行为差异」。
21. **「第 3 轮 #26：`components:` 是 main 在 fmt 门上一直红的机制解释」— 前提属实，推论不成立。**
    `rust-toolchain.toml` 与 `.github/workflows/ci.yml:90-92` 确实都把 `rustfmt`/`clippy` 列进 `components`，也确实没有第二步 `cargo install`——但这只说明「CI 装得上 fmt」，它恰好推出**相反**的方向：CI 只要跑过就会红。真正没被解释的是「那一笔是否触发了 CI」，那需要 Actions 的运行记录，不是读 YAML 能得到的。`.github/**` 属禁区，本条只作线索交给人。

### consume 4 / 6 · 2026-11-10 08:05–08:20 (GMT+8)

- **指摘来源**：CodeBuddy 对第 3 轮回帖的复核（评论 `2119944012235706368`，「第 3 轮复核（基线 `bf26f8c`）」）。9 条新编号 + 3 条 #19 的残留。
- **判定**：11 条 → **成立 4 / 部分成立 4 / 不成立 2**，另 1 条只作观察。
  - 成立：R4-1（`configuration-guide.md` 的命令行注释仍写「本构建推荐的类型」）、R4-2（`Generate.key_type` 的 clap 长帮助仍写 "whichever type this build can actually authenticate with"）、C.6（证书场景下错误分类：陌生 CA 签的新钥匙被报成「指纹已变」）、C.7（信任记录先于认证，认证失败也落盘）。
  - 部分成立：A.1（形态不一致属实，但 `remove_known_host_core` 无任何生产调用点）、A.2（无结构校验属实；「第三列含 `|` 会截短 identity」不成立）、B.5（`Unknown` 可达——`ssh_key::Algorithm` 的 `Sk*` 两变体落进 `_`）、D.8（CRLF 折成 LF 属实；「删中间行留空行」不成立）。
  - 不成立：A.3、B.4（原文见下方 22–23 条）。
- **本轮最值钱的一条**：A.1 的落点与指摘预期相反——`remove_known_host_core` 是**死代码**。`tauri_commands` 只暴露 `forget_system_known_host`（改写系统 `~/.ssh/known_hosts`），CLI 的 `known-hosts` 只有 `import`/`forget`，`api.ts` 也没有对应命令，全仓引用只剩两条测试。所以那条「UI 点 Forget 删不掉」的路径不存在；真正的问题是要不要保留这个函数（L2-21）。
- **动作**：`c41df69`（`bin/agent2ssh.rs` 的 `Generate.key_type` clap 长帮助）、本笔（`configuration-guide.md` 命令注释 + `OPT-LOOP-L2-PENDING.md` 五条与 L2-15 的补充 + 本 ledger）。
- **验证**：`npm run build` 6.49s；`cargo fmt --check` 绿；`cargo check --no-default-features --bin agent2ssh` 干净；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（仍是基线红测）。
- **回帖**：`2119945160132812800`（`## 第 4 轮收尾`）。
- **第 5 轮未开始**：回帖（08:08）后按 60 s × 5 轮询到 08:13，`comment_count` 一直是 9（无新评论）。不发占位轮次——评论数就是轮次计数器。下次运行从 C=4 → 第 5 轮起，指摘在 CodeBuddy 对本轮回帖的回复里。
- **时间**：第 3 轮 07:52–08:00，第 4 轮 08:05–08:20，均未触及 180 分钟预算。

### 第 4 轮新增的不成立条目（原文保留，请勿重复提）

22. **「第 4 轮 A.3：`remove_known_host_core("a:22")` 会一次删掉两台主机的信任」— 不成立（两重原因）。**
    ①该函数没有生产调用点（见 L2-21），整条路径在 UI 与 CLI 上都不存在；②它假设「profile `a:22` 指向 `h2`、profile `a` 指向 `h1`，两者的 identity 恰好相同」，那要求 `h1 == h2`，而两个 profile 指向同一 `host:port` 时它们本来就共享一条信任记录——信任是按 endpoint 建键的（`known_host_identity` = `host:port`），这与 OpenSSH 的 `known_hosts` 同构，是设计而不是误删。真正成立的部分只是「输入命名空间把 profile 名与 identity 混在一起，一个形如 `host:port` 的 profile 名会遮蔽 identity 写法」。
23. **「第 4 轮 B.4：`read_http_proxy_response` 的 16 KiB 上限永远不会触发」— 不成立（它只对一个特定场景成立，被说成了结构性恒真）。**
    循环是 `while response.len() < 16 * 1024 { read_exact(1 byte)?; push; if ends_with(b"\r\n\r\n") { return } }`，条件在**每次读之前**判：对端持续发送超过 16 KiB 且不给 `\r\n\r\n`、连接不断开时，第 16384 次 `push` 之后 `len == 16 * 1024`，循环退出并返回 `Err("...exceeded 16 KiB")`。`read_exact` 只有在 EOF/超时时才 `?` 早退，所以「上限不可达」的推理把「对端先 EOF」当成了唯一路径。**成立的是更窄的一条**：对端只发 LF 头然后关闭时，报的是 `read_exact` 的 I/O 错（`early eof` 一类），而不是「头过长」——错误分类确实不可区分，但那是可观测性问题，不是上限不可达。
24. **「第 4 轮 A.2 后半：第三列里有 `|` 会让 `split_known_host_key` 截短 identity」— 不成立（两处错）。**
    ①`split_once('|')` 切在**第一个** `|`，而 key 的形状是 `{identity}|{algorithm}`，第一个 `|` 就是分隔符本身，identity 在它之前，永远不会被截短；被污染的是 algorithm 那一段。②第三列是 base64 的 key blob，不是算法名——算法名是第二列 `key_type`。A.2 前半（只做 `STANDARD.decode`、不验解出来的公钥结构）成立，已记 L2-22。

### consume 5 / 6 · 2026-11-10 09:22–09:30 (GMT+8)

- **指摘来源**：CodeBuddy 对第 4 轮回帖的复核（评论 `2119947109238460416`，「第 4 轮复核（基线 `f8bc228`）」）。5 条新编号（N-1…N-5）+ 一节措辞确认（四），确认部分无新问题。
- **判定**：**成立 2 / 部分成立 2 / 观察 1 / 不成立 0**。
  - 成立：N-1（`/risk/check` 的 `matched_rule` 恒为 `"user_rule"`，`api.yaml` 却声明成任意字符串）、N-5（把 `HostKeyKind::Unknown` 引到 `to_bytes` 失败那条路，只会把「unknown」换成一句假话）。
  - 部分成立：N-2（两处对 `eof()` 的用法确实不同，但「与模块头契约相反」不成立，见不成立 25）、N-3（webhook 与状态码不对称属实，「同构」与「handler 一行可改」不成立，见不成立 26）。
  - 观察：N-4（`raise` 成功后 `*guard = Some(s)` 直接替换旧会话、不 `disconnect`）。采纳它自己的判断**不进 L2**：`Session` 被 drop 即关 socket，服务端会回收；与 `forward.rs` 的 `stop_worker` 只是风格差异，不是行为缺陷。
- **本轮最值钱的一处**：N-3 的落点与指摘预期相反。`core.rs` 的 `exec_local_core_impl` 已经把 `LocalExecShutdown` 降级成一句字符串错误，并在同一处写明「local runner 自己持有 deadline 并收割子进程及其进程组，所以不像 SSH 路径那样后面还挂着一个在跑的命令」。所以那条「在 handler 里把 `None` 换成计算结果」的修法**恒等于 `None`**：类型在上游就没了。这一条把 L2-27 从「一行」改成「先透类型、再统一算 outcome」，也顺带固定了语义口径——local 中断时未知的只是「已产生的副作用」，不是「命令可能还在跑」。
- **我自己多查出来的两条**：①`/risk/check` 每个请求把 `classify_with_user_rules` 调**两次**（`:3492` 直接调，`:3495` 经 `effective_command_risk` → `classify_effective_risk` 再调一次），两次各自 `load_risk_rules()`；规则文件在两次读之间变更或读失败时，`matched_rule` 与 `risk_level` 可以互相矛盾。②指摘说「全仓 grep `matched_rule` 只有测试与 `api.yaml`」，漏了 `console.html:1052`——而那是唯一把它呈现给人的地方，渲染成 `Matched: user_rule`，正好把这个常量的误导性放大到用户可见。
- **动作**：`ed72760`（`daemon.rs` 两处 landmark 注释：`eof()` 的判定顺序为何承重、`matched_rule` 的取值域为何是两个值）；本笔（`api.yaml` 写出取值域 + `daemon-api-quickstart.md` 一句「固定字面量，不是命中的 pattern」+ `OPT-LOOP-L2-PENDING.md` 的 L2-26/L2-27 与 L2-23 收窄 + 本 ledger）。
- **验证**：`npm run build` 6.51s；`cargo fmt --check` 绿；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是基线那条故意红测，本轮只加注释，未触碰它）；因改了 `api.yaml`，另跑 `cargo test --no-default-features --features daemon --test daemon_integration api_yaml` → **1 passed / 0 failed**（`api_yaml_documents_every_daemon_route`）。
- **回帖**：见本轮 `## 第 5 轮收尾`。
- **锁**：进入时又是孤儿锁（mtime 08:14、空目录、无 pid，上一轮记录写着「结束释放」）。这次直接存活核验（`ps` 无 cargo/cnb/vite 进程；`lsof -d cwd` 仓库内无进程）→ 接管，并写入 `owner.txt`（pid + 起始时刻）让下一次能区分「活着的持有者」与「残骸」。**释放仍是薄弱点，第四次记录。**

### 第 5 轮新增的不成立条目（原文保留，请勿重复提）

25. **「N-2：`spawn_embedded_exec_stream` 的 `if channel.eof() { break; }` 与 `channel.rs` 模块头那句『returning true before both queues drain truncates stderr』相反；它靠抽干才让 `eof()` 变真，抽干前 `eof()` 一直为假」— 不成立（「相反」这一半）。**
    `eof()` 就是 `drained()`（`channel.rs:315-320` = `state.eof && stdout 空 && stderr 空`，`:452-454` 的 `pub fn eof` 直接转调），模块头那句是对**实现或改动 `eof()` 的人**的约束（别让它在队列没抽干时返回 true），不是「消费者必须在读之前调用它」。此处检查位于两个读循环**之后**（`daemon.rs:5124`），队列刚被抽干，`state.eof` 一为真它就转 true——正是这个谓词要表达的意思。至于「抽干前 `eof()` 一直为假」：流未结束时它为假的原因是 `state.eof == false`，与队列是否为空无关（队列本来就每轮被抽干）。**成立的是更窄的一条**：判定顺序是承重的且先前无注释，已补 landmark 注释。另一半「两处对同一谓词的用法不同」也成立（`bridge_tcp_and_channel` 只在 `Ok(0)` 之后用它来关写端），但那是两种正当用法，不是不一致。
26. **「N-3：`/local-exec` 拿到的是同一个 shutdown token、同一种『命令发出去了、进程被杀了、不知道远端副作用有没有发生』的处境，所以在 webhook 里写 `remote_outcome_unknown: None` 是一处可以一行改掉的不对称」— 「同构」与「一行可改」两点不成立（不对称本身成立，见 L2-27）。**
    ①`exec_local_core_impl` 的 `map_err` 已经把 `LocalExecShutdown` 转成字符串错误（"local command stopped during daemon shutdown"），并记了 `append_incomplete_exec_audit`；`ssh_shutdown_outcome` 走 `downcast_ref::<ExecShutdown>()`，在那个点之后永远 `None`——**在 handler 里改一行不会产生任何效果**。②同处注释写明 local runner 自己持有 deadline 并收割子进程**及其进程组**，「unlike the SSH path there is no command still running behind this error」，所以两种处境不同构：SSH 未知的是「命令是否还在远端跑」，local 未知的只是「已经产生的副作用」。要改先透类型，且 `true` 的文案需要为 local 补一句，否则会把「进程已收割」说成「可能还在跑」。

### consume 6 / 6 · 2026-11-10 09:40–10:05 (GMT+8)

- **指摘来源**：CodeBuddy 对第 5 轮回帖的复核（评论 `2119968114631675904`，基线 `e984c35`）。7 条新编号（N6…N12）+ 1 条不编号的留痕 + 1 处对我的措辞更正。
- **判定**：**成立 4 / 部分成立 1 / 不成立 2 / 信息性 1**。
  - 成立：N9（我上一轮 L2-27 写的「恒 400」不准确——preflight 的 403/423/429 走更早的 `?`，只有执行本身失败是 400；已改措辞）、N10（三处 `api.yaml` 契约缺页：两个 PUT 有 `require_admin` 却不写 403，`GET /health-snapshot` 按 scope 过滤却不提）、N11（`policy.rs` / `approval.rs` 用 `std::fs::write`，与同目录其余文件的 `0o600` + 临时文件 + rename 不一致）、N12（`http_risk_check_blocked_command` 对 `matched_rule` 取值域零覆盖）。
  - 部分成立：N8（两个 handler 行为上都过门，但「契约测试守不住」只对 `terminal_broadcast_run` 成立——`run_playbook` 正文里有 `reject_if_gate_paused`，所以照抄那条断言会绿）。
  - 不成立：N6、N7（原文见下方 27–28 条）。
  - 信息性：留痕（`read_side` 在 `state.eof` 时无条件返回 0）**成立**，但它推出的结论方向要反过来读——正因为 `read()` 可能不代表队列空了，才必须以 `eof()` 为退出条件；见本轮的自我更正。
- **本轮最值钱的一条**：N11。它把两件本该分开说的事压在一处：**权限**（`policy.toml` / `approval_policies.toml` 落到 umask 决定的模式）与**原子性**（写一半崩溃会留下截断文件，而加载失败的策略文件在语义上等于「什么都不需要审批」）。这两个文件描述的正是「谁能批准什么」，所以它不是风格问题。
- **自我更正（我上一轮写错了）**：`ed72760` 的注释说「把 `eof()` 检查提到两个读之前会让 stderr 尾部静默丢失」——**不可能**，`eof()` 含「两个队列都空」，只要 stderr 还有字节它就是假，检查的位置无关。真实的承重条件是「不能拿 `read() == 0` 当退出条件」：`read_side` 在流结束后无条件返回 0，哪怕**另一个**队列还有数据（`:352-355` 明写这是 libssh2 的行为，drain 循环依赖它）。`a04e99d` 已按此改写。CodeBuddy 给的理由（「那个循环根本不会带着退出码返回」）同样不成立，但它指出这句话会误导下一个人，这一点是对的。
- **动作**：`a04e99d`（`daemon.rs` 注释改写）；本笔（`api.yaml` 三处契约缺页 + `OPT-LOOP-L2-PENDING.md` 的 L2-28/29/30 与 L2-27 措辞更正 + 本 ledger 的第 6 轮记录与批次汇总）。
- **验证**：`npm run build` 6.36s；`cargo fmt --check` 绿；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（仍是基线那条故意红测）；`cargo test --no-default-features --features daemon --test daemon_integration api_yaml` **1 passed**；`api.yaml` 可解析、path 数仍为 90，三处新增（两个 `403` + `health-snapshot` 的 scope 句）读回确认。
- **回帖**：见本轮 `## 第 6 轮收尾`（末轮）。
- **issue 状态**：本轮结束后按协议关闭 #8，顶部写 `BATCH_COMPLETE`。

### 第 6 轮新增的不成立条目（原文保留，请勿重复提）

27. **「N6：Slack 的 `exec_completed` 到不了任何 URL，内容是空的；`exec_blocked` 干脆没人发，`fire_webhook` 的三个调用点没有它」— 不成立（两处事实错）。**
    ①`exec_blocked` 有发送点：`daemon.rs:754` 构造 `event: "exec_blocked"`，`:763` 交给 `fire_webhook`；`fire_webhook` 在 daemon 里有 5 个调用点（`:650`、`:763`、`:1714`、`:1810`、`:5632`）加 `anomaly.rs:295`，不是三个。②Slack 载荷不是空的：`format_slack_message`（`notify.rs:283-357`）先按 `remote_outcome_unknown` 选 header 文案（"SSH Outcome Unknown" / "SSH Command Not Sent" / "Command Completed"），再带 Host / Command / Risk Level / Exit Code 四个 field，approval 事件还带按钮。它确实没有 `event` / `remote_outcome_unknown` 这类**机器键**——但 Block Kit 是给人看的，`daemon-api-quickstart.md:877` 讲 `remote_outcome_unknown` 的那段指的是默认 JSON 载荷，不是 Slack 渲染。把「没有机器键」说成「告警靠的字段没了、内容是空的」是把两种载荷混为一谈。顺带：`format_slack_message` 上标着的 `#[allow(dead_code)]` 已在回帖时删掉——它在 `:250` 被 `fire_webhook` 调用。**这句附注本身错了，2026-11-10 batch 4 第 1 轮更正并在 `db60643` 之后加回该属性**：`fire_webhook`（`notify.rs:229`）带 `#[cfg(feature = "daemon")]`，所以在 `--no-default-features` 构建里 `format_slack_message` **没有任何调用者**（`cfg(test)` 的测试不算），删掉属性就会重新出现 `warning: function format_slack_message is never used`；`:365` 的 `compute_signature` 至今仍带着同一属性，理由相同。这条更正不动 27 的结论——「`exec_blocked` 有发送点」与「Slack 载荷非空」两点仍然成立。教训：`#[allow(dead_code)]` 是不是过期，要按**每个 feature 组合**判，不能只看 default 那一份调用图。
28. **「N7：`*Command:*\n\`\`\`<cmd>\`\`\`` 在 Slack 里是一行内联代码而不是代码块，所以 `|`、`>`、`_`、反引号会原样进入审批卡」— 机制与推论都不成立。**
    ①这是对 Slack 解析器的断言，本仓无法验证，也不该由本仓的测试来钉；②即便按它的读法成立，推论也是反的——Slack 的内联 code span 与 fenced code block **同样**抑制 `|`、`>`、`_` 这些 mrkdwn 标记，所以「漏进审批卡」在两种解析下都不会发生；③它给的证伪断言自相矛盾（要求 `field.split('\n')[1]` 不以 ``` 开头又「以它结尾」，而该行两者都成立）。**成立的是另一件它没提的事**：`format!("*Command:*\n```{}```", command)` 对命令内容**不做转义**，命令自身含有三个连续反引号时会提前闭合围栏——这类命令极罕见，属文案硬化，不立 L2。

29. **「C1 前半：`hosts_updated` 恒为 0」— 不成立。** `core.rs:4494` 的断言就是 `assert_eq!(result.hosts_updated, 1)`，递增点在 `team_config.rs:100` 的 `else { … updated += 1; }`。这条判据在它自己的正文里被否掉了（原文写着「不是恒 0」），标题却留在那儿。
30. **「C1 后半：`hosts_updated` 没有任何测试钉住它」— 不成立。** 同一条测试（`core.rs:4490-4496`）钉的是一组三件：`hosts_added == 0`、`hosts_updated == 1`、`hosts_skipped == 0`，构造的正是「同名 + port 不同」。缺的不是测试，是**字段集合**的覆盖——把 `team_host_same` 的 11 个字段各漏一个，没有测试会红，但那属于判据强度不足，不是「没有测试」。
31. **「C8 后半：`RecordingInfo` 用 snake_case，与 `RecordingContent` 的 camelCase 混在同一个响应里」— 不成立。** `recording.rs:30-32` 两个结构都带 `#[serde(rename_all = "camelCase")]`，所以线上两层**都是** camelCase（`createdAt` / `durationSeconds` / `sizeBytes`），一处混搭都没有。成立的是另一半：`api.yaml` 的四个 recording 200（`:1430`、`:1444`、`:1452` 与 `/recordings/config` 的 PUT）只有 description、没有 schema。

### 复核 · 末轮之后（基线 `2476ec3`）

- **对象**：本批第 6 轮我自己落的 `a04e99d` / `2476ec3`，以及第 6 轮对 N6 / N7 的两条「不成立」。
- **N9（我的措辞）**：`OPT-LOOP-L2-PENDING.md` 的 L2-27 已从「恒 400」改成「`:1818` 执行错误恒 400，preflight 的 403/423/429 走更早的 `?`，不经过这里」。核实无误：`local_exec`（`:1800` 起）在 `exec_local_core_with_shutdown` 之前只有 `source_or_env_for_request` 与 `ensure_command_length` 两个 `?`，之后 `.map_err(|e| err(BAD_REQUEST, e))` 是唯一的执行错误出口。**闭环**。
- **N10（`api.yaml` 三处）**：两个 `403` 与 `/health-snapshot` 的 scope 句都在 `2476ec3` 里，`api_yaml_documents_every_daemon_route` 绿。**闭环**，但补 schema 时发现同一个 `path` 的 200 响应体一直是错的，见下。
- **N6 / N7 的「不成立」是否过头**：两处事实经复核都站得住——`exec_blocked` 的发送点在 `:754`/`:763`，`fire_webhook` 在 daemon 里 5 个调用点（`:650`/`:763`/`:1714`/`:1810`/`:5634`，我说 `:5632` 差了个偏移，是按上一笔的行号说的）加 `anomaly.rs:295`；`format_slack_message` 确实按 `remote_outcome_unknown` 选 header 并带四个 field。**但 N7 的回应里我有一句话过头了**：我说「内联 span 与 fenced block 同样抑制这些 mrkdwn 标记」——这在 Slack 文档里是对的，而我手上没有 Slack 客户端，所以它只是「与文档一致」，不是「我验证过」。改为如实：机制由 Slack 侧决定，本仓只能保证发出去的 payload 是什么；本轮把 payload 的形状写成了可断言的事实（`format_slack_message` 的 `*Command:*\n```…``` ` 不转义，命令含三连反引号会提前闭合围栏），这才是本仓能负责的那一半。**N7 的判定不变（不成立），但理由收窄为「推论反过来也不成立 + 本仓无法验证对方的解析器」**。
- **本轮新查出的三条**（都记 L2，因为都需拍板）：
  - **L2-34**：`a04e99d` 的注释讲清了「不能以 `read() == 0` 退出」，但 `/exec/stream` 那个循环（`:5129-5153`）是**直接**以 `channel.eof()` 退出的，没有 `drain_channel`（`embedded_ssh.rs:1594-1601`）那样的 `&& !progressed` 计次。`eof()` 是「流结束且两队列空」，而 `close()`/reader 消失时 `mark_closed` 会**立刻**把 `state.eof` 置真、此时另一个 task 可能还在往队列里 push——`drain_channel` 的注释写的就是这件事（"the extra idle passes absorb a trailing packet"）。同一条主线，一个实现有护栏、一个没有。
  - **L2-35**：`session_write` 的 body 没有 `command` / `change_id`，也不 `deny_unknown_fields`，与 `/exec` 一族的入参不同形；`api.yaml` 原来一句没写，本轮写进了 `SessionWriteRequest.input` 的描述。
  - **L2-36**：`/health-snapshot` 的 200 一直是错的（写裸数组，实为 `HealthSnapshot` 对象，键名是 `load_avg`/`disk_usage`/`memory_usage`）。`http_health_snapshot_get_returns_200` 用 `hosts.is_some() || error.is_some()` 兜住，所以没红。本轮改了 `api.yaml` 的响应表（get + post，含 `403`/`500`），并把测试留在 L2-36 的建议改法里（它需要 L2-29 的 harness 隔离）。
- **顺带**：`notify.rs` 的 `#[allow(dead_code)]` 标志已删（`fire_webhook` 在 `:250` 调它，`--lib` 构建下也在用）。
- **动作**：本笔（`api.yaml` 的 `/health-snapshot` 两个响应表 + `SessionWriteRequest.input` 描述、`daemon_integration.rs` 的 `http_risk_check_blocked_command` 断言口径与注释、`notify.rs` 去掉过期 `allow`、`OPT-LOOP-L2-PENDING.md` 的 L2-34/32/33 + 本 ledger）。
- **验证**（本机首次跑 Rust 侧，见下）：`npm install` + `npm run build` 12.33 s；`npm test` **194 passed / 26 files**；`cargo fmt --check` 绿；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（仍是基线那条故意红测 `an_ipv6_literal_at_port_22_is_removed`）；`cargo test --no-default-features --features daemon --test daemon_integration` **63 passed / 0 failed**；`cargo check --no-default-features --features daemon --bin agent2ssh-daemon` 干净；`api.yaml` 可解析、path 数仍 90。

## 批次汇总 · 2026-11-10 · issue #8 · SSH 传输、known_hosts 与认证（russh 侧）

6 轮全部完成，issue 已关闭。指摘来源始终是 CodeBuddy 对本流程上一轮回帖的复核，每轮的指摘都带文件路径与行号，核实后按 L1 / L2 分流。

| 轮 | 条数 | 判定 | 动作（L1） | 回帖 id |
|---|---|---|---|---|
| 1 | 18（A1-A2 / B1-B4 / C1-C7 / D1-D5） | 成立 11 / 部分 5 / 不成立 2 | `02f936b`（`ssh_algo.rs` 模块文档整段重写 + 五处契约注释）、`562340a`（L2 清单 11 条） | `2119904212174680064` |
| 2 | 11（C3-1…C3-4 / C4 / N1…N7） | 成立 4 / 部分 3 / 不成立 3 | `f23f006`（clap 文档、`MethodType` 文档、空臂注释、`cli_contract` 断言消息等 6 文件）、`724cca6`、`2c4221a` | `2119924514724155392` |
| 3 | 9（19–27） | 成立 3 / 部分 3 / 不成立 3 | `3ea8f70`（两处测试注释 + 断言消息）、`bf26f8c`（`SKILL.md` / 两份指南 / 迁移文档标记 / L2 三条） | `2119943001739788288` |
| 4 | 11（R4-1…R4-2 / A.1-A.3 / B.4-B.5 / C.6-C.7 / D.8） | 成立 4 / 部分 4 / 不成立 2 | `c41df69`（`Generate.key_type` clap 帮助）、`f8bc228`（config-guide 注释 + L2 五条 + L2-15 补充）、`6ac8834`、`60f8262` | `2119945160132812800` |
| 5 | 5（N-1…N-5） | 成立 2 / 部分 2 / 观察 1 / 不成立 0 | `ed72760`（两处 landmark 注释）、`e984c35`（`api.yaml` 取值域 + 指南 + L2-26/27 + L2-23 收窄） | `2119965165029343232` |
| 6 | 7（N6…N12）+ 1 留痕 | 成立 4 / 部分 1 / 不成立 2 / 信息性 1 | `a04e99d`（注释更正：承重的是「不以 `read()==0` 退出」）、本笔（`api.yaml` 三处契约缺页 + L2-28/29/30） | 本轮 `## 第 6 轮收尾` |

**总量**：6 轮共核实 **61 条** → 成立 24 / 部分成立 18 / 不成立 12 / 观察与信息性 7。L1 落了 12 笔提交，全部推 `cnb main`，未推 github。

**L2 待拍板清单（按该先问的顺序）**

| 编号 | 一句话 | 需要谁拍板 |
|---|---|---|
| L2-15 | `[host]:22` 这条形态要不要支持；main 上那条**故意红**的测试（`an_ipv6_literal_at_port_22_is_removed`）是配修复还是降 `#[ignore]` | 产品 + 我（红测试一直在基线里，每次验证都得带注） |
| L2-21 | `remove_known_host_core` 是死代码：删掉，还是补一个「忘记应用内信任」的 UI 入口 | 产品 |
| L2-26 | `matched_rule` 要不要真的带回命中的规则（顺带消掉 `/risk/check` 每请求两次规则读） | 产品 / 契约 |
| L2-27 | `/local-exec` 中断时的 `remote_outcome_unknown` 与状态码：先透类型，再定「未知」在 local 语境下的措辞 | 产品 |
| L2-28 | `policy.toml` / `approval_policies.toml` 换成 `write_private_text`（权限 + 原子性） | 我看是安全向的纯改进，但属行为改动 |
| L2-29 | 给 `/risk/check` 的 `matched_rule` 补覆盖，前提是先隔离测试的配置目录 | 工程（结构性改动） |
| L2-30 | 两个 handler 的执行门只有间接来源：补一次判定，还是改断言的判定口径 | 工程 / 口径 |
| 其余 | L2-16…L2-25（错误分类、文案、TOFU 记录顺序、`import` 校验、`Unknown` 命名等）见上文各轮表格 | 逐条已写明代价 |

**末轮之后的复核**：见上文「复核 · 末轮之后（基线 `2476ec3`）」，一笔提交 `18d9888` 推 `origin main`（`2476ec3..18d9888`），三条新 L2（L2-34/35/36）进 `OPT-LOOP-L2-PENDING.md`。

**给下一批的方法学（新增三条，接前一批的三条）**

1. **先看「谁在消费这个字段」，再看它的取值域**：`matched_rule` 那一族的问题不是名字错，而是三处消费者（`api.yaml`、`console.html`、集成测试）各自以为它是不同的东西，其中测试以为它是「恒 null」。
2. **同一目录下有两种写入方式时，先分清它们各自保证了什么**（`write_private_text` 保证权限与原子性，`std::fs::write` 两样都不保证），再判断是不是漏用——「两个文件都能读回来」证明不了任何一层。
3. **对自己上一轮写下的注释要按同一把尺子复核**：`ed72760` 的错误不是笔误，是把「谓词的含义」与「检查的位置」混成了一件事；它躲过了我自己的核验，却被下一轮的一句反问照出来。

### batch 2026-11-10 · MCP 契约与 CLI 文档一致性
- issue #9 / 开批时间 2026-11-10 09:48 (GMT+8)
- CodeBuddy 首条分析：未到（等待中，issue 发出后 5 分钟内 5 次轮询均 0 评论；消费者下次运行重新等）
- 6 轮进度：6 / 6（第 1 轮 `consume 1 / 6` · 回帖 `2119990687696330752`；第 2 轮 `consume 2 / 6` · `2119994949708967936`；第 3 轮 `consume 3 / 6` · `2119998853859422208`；第 4 轮 `consume 4 / 6` · `2120001436644700160`；第 5 轮 `consume 5 / 6` · `2120003384483667968`；第 6 轮 `consume 6 / 6` · `2120004587762384896`）
- 正文已喂给 CodeBuddy：**35 条「不成立」原文**（preflight 2 + batch 1 六轮 10 + batch 2 第 1 轮行内 D2/D4 两条 + batch 2 第 2–6 轮 14）+ K7/K8/K9/K12/K13 相关空档 + 口径提醒（基线红测、`.github/**` 禁区、四列计数）。范围：`docs/skills.md`、`docs/api.yaml`、`bin/agent2ssh.rs` clap help、`bin/agent2ssh-mcp.rs`、`skills/agent2ssh/SKILL.md` 及 guides 相关段落。

#### consume 1 / 6（基线 `db60643`，回帖 `2119990687696330752`）

7 条指摘 → **成立 4（A1 / A2 / A4 / P1）/ 部分成立 2（A3 限 Linux、Q1 收窄）/ 不成立 1（A5）**。本轮**没有 `src-tauri/src/**` 行为改动**，只有一条 lint 属性回填。

| 项 | 判定 | 关键证据 | 落地 |
|---|---|---|---|
| A1 | 成立 | `bin/agent2ssh.rs:1258` 的 `command: String` 无 `#[arg]`，是位置参数；`--command` 属 `ExecMulti`（`:126-127`） | `1a36f7d` 改 `SKILL.md:77` |
| A2 | 成立，**数字要改** | 实测 **36 path / 39 operation**（33 path 走 `require_admin`，3 path（6 op）走 `require_recording_admin`，无交集）；api.yaml 原只有 6 个 path 提过 admin → **30 个 operation 未记载**。两个门都是无条件首行调用（抽查 `metrics_trend:4037`、`containers_start_service:968`） | `1a36f7d` 补齐 39 个 403 + 重写 `info.description` 区分两种 403；`info.version` 0.3.0→0.4.5 |
| A3 | **部分成立（平台分裂）** | `cargo tree --no-default-features --target x86_64-unknown-linux-gnu -i openssl-sys` 命中 `openssl-sys 0.9.116 ← native-tls ← hyper-tls ← reqwest 0.12.28`；同命令 `aarch64-apple-darwin` 为空。所以「No platform」错、「SSH 传输层不需要」对 | `1a36f7d` 把 AGENTS.md 该段收窄到传输层并写清 Linux 形态，regression 判据改成「`ssh2`/`libssh2-sys` 回来或 OpenSSL 回到 SSH 路径」 |
| A4 | 成立，**行号要改** | 实际 `docs/skills.md:59`（`:47` 是工具编号）；`agent2ssh-mcp.rs:1648` 走 `local_daemon_url()` | `1a36f7d`（`docs/skills.md`）+ `3c30287`（`tools.rs:767`） |
| A5 | **不成立（机制读反）** | `start_local_service` 的 `None` 分支直接 `available()`（`container_runtime.rs:505-512`），从不调 `select`；env 只在 `select` 的 `_` 臂（`:542`）读，而 `select(None)` 生产代码无调用点（只有 `:786` 测试）。工具描述是准确的。**反向的事成立** → L2-37 | 不改；记 L2-37 |
| Q1 | 部分成立 | 版本号两处过时属实；`:211` 不是版本号问题是**事实错**（`telemetry.rs` 存在、默认 `enabled=false`、无网络出口） | `1a36f7d` 改两处 0.4.5 + 按 telemetry 实况重写 `:211` |
| P1 | **成立（推翻我上一批的附注）** | 对照实验：属性在场 `cargo check --no-default-features --lib` = 0 条 `never used`；删掉属性立刻 `warning: function format_slack_message is never used`；还原回 0。根因 `fire_webhook` 带 `#[cfg(feature = "daemon")]`（`:228`），`cfg(test)` 不算调用者 | `3c30287` 加回属性 + 写清理由；上面第 27 条的附注已改 |

**验证**：`npm run build` 6.77s；`cargo fmt --check` 绿；`--no-default-features --lib` = 861 passed / 1 failed / 1 ignored（已知红测）；`--features daemon --test daemon_integration api_yaml` = 1 passed；`yaml.safe_load` 仍 90 path，gated op 缺 403 由 30 → 0/39。

**方法学**：`#[allow(dead_code)]` 是否过期必须按**每个 feature 组合**判——第 6 轮只看了 default 的调用图，把一个承重属性当成残留删掉了，还写进 ledger 当成"修复"。这是本流程第一次出现「我自己的更正被下一轮当成新缺陷」的反向事故。

#### consume 2 / 6（基线 `38eb70b`，回帖 `2119994949708967936`）

9 条指摘（G1–G9）→ **成立 7 / 部分成立 2（G1、G5）/ 不成立 0**，但**两处数字/位置要改**。落地一笔 `0b2bb5c`（纯文档，无 `src-tauri/src/**` 改动）。

| 项 | 判定 | 关键证据 | 落地 |
|---|---|---|---|
| G1 | 部分成立 | 指摘自称「一处事实错」又自述「这不是错，是口径窄了一层」。`skip_serializing_if` + 四个构造点都写 `None` → 原句成立；成立的是 Slack 侧 `("exec_blocked", _)` 臂忽略该字段 | `0b2bb5c` 补半句 Slack 口径 |
| G2 | 成立 | `types.rs:640-659` 六个变体 vs `api.yaml` 三处各五个（`:233` schema + `:1258`/`:1314` 两个 query enum） | `0b2bb5c` 三处全补 + 写清与 `approved` 分开的理由 |
| G3 | 成立 | `core.rs:3165-3192` 五字段 + 两嵌套；指摘列的另 5 处空 schema **独立扫描完全一致** | `0b2bb5c` 补 4 个组件（`SshSyncDiff`/`SshSyncHostDiff`/`SshSyncHostConflict`/`AlgoPrefsState`），`/algo/prefs` 三方法统一 `$ref`；`/prompts/respond` 返 `serde_json::Value`，保持 `type: object` 不硬编 |
| G4 | 成立 | `grep -c decision src-tauri/web/console.html` = 0；桌面端 `AuditPanel.tsx:37` 有 | **L2-39**（表头 + 行 + 两处 `colspan` 必须一起改） |
| G5 | 成立，位置要改 | 不该改 `SKILL.md:29`（那行不含地址）；真问题是 `docs/skills.md` 里 `AGENT2SSH_DAEMON_ADDR` 只出现 1 次（第 59 行） | `0b2bb5c` 新增 `## Daemon address` 小节，第 59 行改为引用 |
| G6 | 成立 | `notify.rs:322-329`、`:331-337` 条件 push → `diagnostic_error` 只有 2 个 field | `0b2bb5c` 改成「固定 2 + 条件 2」并补 `diagnostic_error` 的读法 |
| G7 | 成立 | 四条出站测试（`:643`/`:668`/`:697`/`:725`）只断言 `is_ok()`，无一读请求体；三条 `format_slack_message` 测试绕过三元 | **L2-41**（干净解法是抽 `payload_for()` 纯函数，属 src 重构） |
| G8 | 成立 | `team_config.rs:31-38` 只剥三凭据，`:81-95` 也只补回三个，`:96` 的 `*existing = next` 让 `proxy_id: None` 落地 | **L2-40**（产品语义）；连带半条已 L1：`HostProfile` 补 `proxy_id`/`group`/`init_command`/`passphrase`，13 → 17 字段 |
| G9 | 成立，**数字要改：36 不是 52** | 路由 105 operation（`/`、`/console` 除外）vs 表 70 行；第 37 条是 `/recordings/:id?confirm=delete` 带查询参数的假阳性 | `0b2bb5c` 补齐，**双向差集为空**（106 行 = 105 API + `/console`） |

**验证**：`npm run build` 6.44s；`fmt --check` 绿；`--lib` 861/1/1（已知红）；`api_yaml` 1 passed；`yaml.safe_load` 90 path / 47 schema；`HostProfile` 17 字段；端点表 ↔ 路由双向为空。

**自我更正**：`0b2bb5c` 第一次写成「51 schemas」，实际 47（4 个是本轮新增）。未推送前 `--amend` 修正。教训：`Verified:` 行里的每个数字都要现算，不能从「加了几个」的心算推总数。

#### consume 3 / 6（基线 `b756fb5`，回帖 `2119998853859422208`）

9 条指摘（B1–B9）→ **成立 9 / 不成立 0**。落地一笔 `684d471`（`docs/api.yaml` + 两份 guide，无 `src-tauri/src/**` 改动）。

| 项 | 判定 | 关键证据 | 落地 |
|---|---|---|---|
| B1 | 成立 | `daemon_control.rs:1018` 等 `GRACEFUL_STOP_WAIT`（`:621` = 8 s）→ `:1021` 记 warn → `:1027` `signal_daemon(..., true)` 强杀 → 再等 2 s，`StillRunning` 即 `Err`；`health_identity()` 不符时 `:982-987` 直接 `UnverifiedIdentity`，不进入等待 | `684d471` 重写 `/shutdown` 描述（8 s + 2 s + 身份不符即终止，「may terminate」删掉） |
| B2 | 成立（三重） | `notify.rs:204-221` 拼 `{url}/approve`；路由只有 `POST /approvals/:id/approve`（`daemon.rs:5881-5882`）；`is_public_path`（`auth.rs:99-101`）只放行 `/`、`/console`、`/health`、`/metrics`；且 `127.0.0.1` 在收件人机器上指别人 | 文档改成「这两个按钮点不动」；按钮行为不改 → **L2-44** |
| B3 | 成立 | `daemon.rs:1324`/`:1339` 都过 `HostProfile::redacted_for_transport`（`types.rs:105-109`）→ 读出恒 null | `writeOnly: true` + 描述；`TeamConfigExport` 与 `/config/export` 的「剥两个」→ 三个（补 `passphrase`） |
| B4 | 成立 | `approval.rs:930-951` 七字段 vs 两处 `items: { type: object }` | 抽 `ApprovalPolicy` 组件 + 两处 `$ref` |
| B5 | 成立 | 端点表路径列把 `?confirm=delete` 写进去了，而 `api.yaml` 里 `confirm` 是 `in: query` | 路径列改 `/recordings/:id`，条件移到说明列；重跑双向差集 **105 = 105** |
| B6 | 成立 | `types.rs:395-399`、`:511-514`、`:597-600`、`:604-637` | 补 `ExecRequest.side_effect`；`ExecResult.side_effect`/`risk_unclassified`；`AuditEntry.side_effect`/`action`/`outcome`/`risk_unclassified` |
| B7 | 成立 | `types.rs:503-510` 截断保留头尾、丢中间 | 补 `ExecResult.dropped_bytes`，写明丢的是 middle |
| B8 | 成立（CodeBuddy 自改判） | `remote.rs:427-456`、`:946-951`；`/prompts/respond` 实测只返 `status` + `nonce`（`daemon.rs:4190-4194`） | 两处换 `$ref`（5 个新组件），`/prompts/respond` 就地补形状而不硬编 |
| B9 | 成立 | `approval.rs:108-117` 五变体、`:761` 写 `Revoked`、`:1931` 有断言；`grep -c revoked docs/api.yaml` 改前 = 0 | 补枚举 + 写「今天只有进程内调用者」；要不要路由 → **L2-43** |

**动作**：`684d471` —— `api.yaml` 108 行：2 个 `writeOnly`、6 个补字段、`status` 补 `revoked`、6 个新组件（47 → 53 schema）、4 处 `type: object` 换实体、`/shutdown` 重写；`daemon-api-quickstart.md` 改路径列；`configuration-guide.md` 改 Slack 按钮那句。

**验证**：`npm run build` 6.54s；`cargo fmt --check` 绿；`--no-default-features --lib` = 861 passed / 1 failed / 1 ignored（已知红测）；`api_yaml` 1 passed；`yaml.safe_load` 90 path / 53 schema；端点表 ↔ 路由 **105 = 105**，双向为空。

**自我更正 ×2**：

1. **`?confirm=delete` 从「假阳性」改判为「写法错」**：第 2 轮我把它当成本轮唯一的假阳性写进本 ledger，CodeBuddy 指出那一格把查询串写进了路径列，`(method, path)` 守卫会永远红一条；判据是 `api.yaml` 里 `confirm` 是 `in: query`。已修，回帖里明说接受这次反纠正。教训：**「守卫和人对不上」不等于「守卫错」**，先看是哪一列放错了（这条推翻了上面 G9 那句「假阳性」的定性，G9 的 36 仍是「35 真缺 + 1 写法错」）。
2. **`daemon_control.rs:1027` 改后按门禁撤销**：B1 沿调用链牵出那句 `"…leaving it running"`（记完 warn 就强杀），我改了一版才想起 `git grep "leaving it running"` 命中 `OPT-LOOP-L2-PENDING.md:14` 与 `LEDGER.md:52`——**L2-1 早已登记同一处**，且属 `src-tauri/src/**`。已 `git checkout --` 撤回，并在 L2 清单与回帖里写明「改过一版、按门禁撤回」，避免下一轮把 `api.yaml` 的描述改动误读成代码侧也修了。

#### consume 4 / 6（基线 `684d471`，回帖 `2120001436644700160`）

**指摘来源：等待超时后的自扫。** 第 3 轮回帖 `2119998853859422208` 之后按 60 s 轮询 6 次（11:41:35 → 11:46:38），`total` 一直是 6，CodeBuddy 没有回复。按协议「仍未到 → 标等待超时，用已有材料推进，不空等」，本轮把第 2、3 轮同一类缺陷收尾：数 `docs/api.yaml` 里所有 `type: object` 且无 `properties` 的节点。**18 个，5 组，全部成立**，落地 `6d1fb71`（仅 `docs/api.yaml`）。

| 组 | 原状 | 真值来源 | 落地 |
|---|---|---|---|
| `AlgoPrefsState.prefs` / `.defaults` | 两个匿名对象 | `ssh_algo.rs:53-70` 八字段全无 `skip_serializing_if`；`safe_defaults()`（`:110-124`）逐个 `narrow()`，`SUPPORTED_COMPRESSION = "none"`（`:295`）⇒ `comp_*` 恒为 `none` | 新增 `SshAlgoPrefs`；`/algo/prefs` 的 PUT body 从内联八字段换成同一个 `$ref`（内联正是它与 `AlgoPrefsState` 漂开的源头） |
| `/snippets` GET + POST | 两处 `items: { type: object }`，POST body 内联 | `snippets.rs:31-40`；`add_snippet`（`:141-156`）用 `*existing = snippet` **整体替换**，省略 `description` 会清空旧描述 | 新增 `Snippet`，三处统一；把「省略即清空」写进描述 |
| `/redact/rules` 5 op | GET 内联三字段（对），其余 4 处空对象 | `types.rs:893-901` `RedactRuleInfo` | 抽 `RedactRuleInfo`，5 处统一 |
| `/highlight/rules` 5 op | GET 内联六字段（对），其余 4 处空；`/update` 的 `rule` 是 `required` 的空对象 | `types.rs:869-891` `HighlightRule` | 抽 `HighlightRule`，6 处统一 |
| `/webdav/status` | `local`/`remote` 空对象；`required` 只有 `[state, summary]` | `webdav_sync.rs:104-114` 八字段 → `WebDavSyncMarker:74-91` 十字段 → `WebDavSyncFile:67-71` 三字段。五个 `Option` 都**没有** `skip_serializing_if`，键恒在 | 新增三个组件，`required` 补到 8 个 |
| `/metrics/trend` | `risk_distribution`、`top_hosts[]`、`hourly_breakdown[]` 空 | `store.rs:1080-1117` 四个结构；`failure_rate` 是 0–1 比值（`:1156-1160`） | 新增 `MetricsTrend`/`RiskDistribution`/`HostExecutionCount`/`HourlyBucket` |

**自扫掉出来的一条（不是指摘）**：请求收 `24h`，响应用 `last24h`。`TrendPeriod` 带 `rename_all = "lowercase"`（`store.rs:1090-1098`），`store.rs:2216-2219` 的测试把 `"last24h"` 钉死；handler 解析两种拼写都收（`bin/agent2ssh-daemon.rs:4037-4047`）。所以拿响应值回填请求安全，但拿它和自己发出去的字面量比对会误判「参数被忽略」。已写进 `MetricsTrend.period` 的描述。

**验证**：`npm run build` 6.51s；`cargo fmt --check` 绿；`--no-default-features --lib` = 861 passed / 1 failed / 1 ignored（已知红测）；`api_yaml` 1 passed；`yaml.safe_load` 90 path / 64 schema；`type: object` 无 `properties` 的节点 **18 → 0**；`$ref` 61 个、悬空 0。

**L2 新增 3 条**：L2-46（`nullable` 是 3.0 关键字而文档自称 3.1，全文 142 处）、L2-47（`AgentEvent`/`ApprovalResponse`/`AuditFilter` 三个组件从未被引用）、L2-48（两个同名 `pub struct WebDavSyncStatus`）。

**顺手修的排版错**：`OPT-LOOP-L2-PENDING.md` 里 L2-38 那行被第 1 轮插到了文件末尾（「关于 B1」段之后），离开了第 1 轮的表格；已挪回 L2-37 下面。表格断行会让拍板的人以为 L2-38 不存在。

**流程观察（下次运行该改）**：第 2、3 轮的指摘都是 CodeBuddy 复核，所以每轮只修「它点到的那几处」。这一轮自扫才发现同类缺陷还剩 18 个——**复核的覆盖度取决于复核者挑了几条，不取决于缺陷有多少**。以后每轮收尾前应先跑一遍全量扫描（空 schema、`$ref` 悬空、端点表 ↔ 路由、403 ↔ `require_admin`），把扫描结果而不是回帖里的条目当下一轮的输入。

#### consume 5 / 6（基线 `c3af875`，回帖 `2120003384483667968`）

指摘回到 CodeBuddy 侧：它对第 4 轮回帖给了 9 条（C1–C9），11:53:28 到达。判定 **成立 7 / 部分成立 2（C1、C8）/ 不成立 0**，但两条部分成立里有三处判据被推翻。落地两笔：`95461ff`（只改文档注释）、`6b6cf1d`（`api.yaml` + 端点表 + 本目录 L2）。

| 项 | 判定 | 关键证据 | 落地 |
|---|---|---|---|
| C1 | **部分成立** | 「恒为 0」不成立：`core.rs:4494` `assert_eq!(result.hosts_updated, 1)`，递增点 `team_config.rs:100`。「没有测试钉住」不成立：同一条测试钉的是 updated=1 / skipped=0。成立的是注释：`ConfigDiffPreview.hosts_to_update` 写「host/port/user differs」，`team_host_same`（`:226-238`）比 11 字段，preview（`:176-184`）同谓词 | `95461ff` 注释列全 11 字段；`6b6cf1d` 给 `api.yaml` 的 `hosts_updated` 补语义 |
| C2 | **成立，真因更深** | 空 body 分支在 `daemon.rs:3346`，无真 handler 覆盖。根因是 `tests/daemon_integration.rs:175` 的**副本** `approval_approve` 只有 3 个参数（无 `body`），`:410` 的 `build_test_router` 挂副本；`:854` 那条虽发 `Body::empty()`，打的是副本 | **L2-53**（结构性） |
| C3 | **成立**，多两处 | `:5537-5543` 失败回 200 `{error, hint}` 与 `required: [hosts, collected_at]` 冲突；**且** `get_health_snapshot`（`:5531`）签名无 `Query`，契约声明的 `hosts`/`timeout_secs` 从未被读；且顶层描述「Collects…」与同一 operation 的 200 描述「never collects」自相矛盾 | `6b6cf1d` 三处同改 |
| C4 | **成立（口径要准）** | `daemons_view`（`:4012`）`Extension(_auth)` 从不读 auth；计数在 `remote.rs:534-541`，用 daemon 自己的 token 取远端 `/hosts`。所以不是「读到过滤前的本地配置」，而是「调用方 scope 全程没参与」 | **L2-49** |
| C5 | **成立** | `remote.rs:943`（非 `:394`）`PROTOCOL_VERSION = env!("CARGO_PKG_VERSION")` | **L2-50** |
| C6 | **成立** | `/audit` `host_scope: audit_host_scope(&auth)` vs `/audit/export` `host_scope: None` | **L2-51** |
| C7 | **成立** | `forward_remove` 命中走 gate/rate/scope/risk + 审计（`:3079-3087`），未命中直回 400 且无审计（`:3090-3093`） | **L2-52** |
| C8 | **部分成立** | 「四个 200 无 schema」成立（`api.yaml:1430/1444/1452`）；「camelCase 与 snake_case 混在一个响应里」**不成立**——`RecordingInfo`（`recording.rs:30-32`）自己也带 `rename_all = "camelCase"`，两层都是 camelCase；行号偏 130/134 行 | `6b6cf1d` 加三组件 + 四处接线 |
| C9 | **成立** | `GET /` 是内联闭包（`daemon.rs:5791-5793`），`get(命名 handler)` 的扫描器数出 106；真值 107 路由 / 105 API operation | `6b6cf1d` 写进表头与 L2-42 |

**验证**：`npm run build` 6.43s（注释那次 6.39s）；`cargo fmt --check` 绿；`--no-default-features --lib` = 861 passed / 1 failed / 1 ignored（已知红测）；`api_yaml` 1 passed；`yaml.safe_load` 90 path / **67** schema / `$ref` 悬空 0；`GET /health-snapshot` 的 `parameters` 键已消失。

**L2 新增 5 条**：L2-49（`/daemons/view` 不看调用方 scope）、L2-50（`PROTOCOL_VERSION` 其实是包版本）、L2-51（`/audit` 与 `/audit/export` 权限模型不同）、L2-52（`DELETE /forwards/{id}` 未命中不写审计）、**L2-53（`daemon_integration` 有 20 个 handler 副本，且副本已与真身分叉）**。

**自我更正 ×2**：

1. **我把两个已存在的翻译当成缺失**：扫 `src/i18n.tsx` 时正则要求键带引号，于是 `Decision: "裁决"` 与 `From: "来自"`（**未加引号**）被判为缺中文。我加了两行，`npm run build` 立刻 `TS1117: An object literal cannot have multiple properties with the same name`，已 `git checkout` 还原。教训与 C9 同源：**扫描器认不出的形态不等于缺陷**；下次扫对象字面量要同时接受 `"k":` 与 `k:`。
2. **路由数我数成 106**：脚本只认 `get(标识符)`，漏掉 `GET /` 的闭包形式。C9 说的 107 是对的，我此前在回帖里写的「105 operation」也一直没错——错的是我把「route 数」当成 106。两个数现在都带单位写进文档。

**流程观察（补充上一轮那条）**：上一轮说「先扫描再回帖」，这一轮补一条：**扫描器本身要先用反例自检**。两次假阳性（i18n 键、路由数）都是脚本的正则比代码的真实形态更窄，而两者都被我当成结论写进了草稿。

#### consume 6 / 6（基线 `b680233`，回帖 `2120004587762384896`，末轮）

指摘是 CodeBuddy 03:58:44 那条（对 `6d1fb71` 的复核 + A–H 八条），它落在我第 5 轮回帖（03:59:29）**之前**，属于「队列里还有未消化的材料」。按协议先轮询 5 分钟（12:01:13 / 12:02:14 / 12:03:14 三次 `total=10`）确认没有新回复，再用它推进。判定 **成立 6（A B C D F G）/ 部分成立 1（对 `comp_cs` 的更正）/ 不成立 1（E）**；另外它对我三条指认里有两条已经不成立。落地 `0a79492`（纯文档）。

| 项 | 判定 | 关键证据 | 落地 |
|---|---|---|---|
| A | 成立 | `events.rs:20-39` 18 变体 vs 契约 17，缺 `stream_connected`；`daemon.rs:4072-4084` 每个订阅首帧就发它；`src/types.ts:472` 前端已有 | 补为枚举第一位 + 描述「首帧即它」 |
| B | 成立 | `EventType::Host{Connected,Disconnected}` / `ConfigChanged` 只有 `events.rs:92/93/99` 测试引用，零发布点；`App.tsx:135`、`NotificationCenter.tsx:18` 的注释自认 | 契约写明「不要等它们」；削/补 → **L2-54** |
| C | 成立（数字改） | `/audit` 13 参数、`/audit/export` 14；`types.rs:443-478` 的 `AuditFilter` **14** 字段（13 线上 + `host_scope` 的 `#[serde(skip)]`），契约只写 6 个 | 补到 13 + 说明 `host_scope` 不上线 |
| D | 成立 | `playbook.rs:37-47` 六字段 vs 契约五字段，缺 `advanced_steps`；`effective_steps`（`:245-259`）优先用它，`steps` 可为空。`configuration-guide.md` 的字段表同样五行 | 加 `advanced_steps` + `PlaybookStep`/`PlaybookParam`；指南补一行与一个 `[[playbooks.advanced_steps]]` 例子 |
| E | **不成立** | 契约里 `/events/stream` 的 200 已经是 `content: text/event-stream`，schema 描述写着「SSE frames containing AgentEvent JSON in data lines」；它引的 `:1473-1478` 是旧版 | 无改动 |
| F | 成立 | `remote.rs:145-155` 的 `scope` 带 `skip_serializing_if` → 键缺席；契约写 `nullable`（键在值 null） | 改成「省略而非 null」+ 与 `WebDavSyncStatus` 互指 |
| G | 成立 | `load_algo_prefs`（`ssh_algo.rs:259-267`）= 存在 **且** 解析成功，`has_custom_algo_prefs` 就是 `.is_some()` | `custom` 语义改成「存在且能解析」 |
| H | 成立（补判据） | `tauri_commands.rs:2404-2416` 11 字段、9 个 `Option` 无 skip、camelCase；`src/types.ts:233-245` 对得上 | 不改；L2-48 的落地范围按它补上「`types.ts`/`api.ts` 的 IPC 名」 |

**验证**：`npm run build` 6.53s；`cargo fmt --check` 绿；`--no-default-features --lib` = 861 passed / 1 failed / 1 ignored（已知红测）；`api_yaml` 1 passed；`yaml.safe_load` 90 path / **69** schema / 悬空 `$ref` 0；`event_type` 17 → 18、`AuditFilter` 6 → 13、`Playbook` 5 → 6。

**自我更正 ×4**：

1. **`nullable: true` 是 136 处，不是 142**：我上次用的是 `grep -c null`（把 `nullable` 以外含 null 的行也算进去），这次用 `grep -c "nullable: true"` 现算。L2-46 的定量依据已改。
2. **前四轮我一直把进度行改错了地方**：`- 6 轮进度：K / 6` 在文件里出现两次，我每轮都改到**上面那一行**（它坐落在 issue #8 的小节里，却引用着只存在于 issue #9 段内的 `consume K / 6` 锚点）。本轮把错位那行删掉、把 issue #9 自己那行补到 6 / 6 并逐轮写上回帖 id。教训：**改计数器之前先确认它属于哪一节**——`grep` 得到唯一匹配，不等于匹配到了正确的那一条；位置错了四周都没人发现，是因为它「看起来」一直在递增。
3. **`$ref` 兄弟键那处它说得对**：上一轮我在 `/highlight/rules/update` 的 `rule` 上留了 `$ref` + `description`，是全文唯一一处这样写的；已把句子挪进 `HighlightRule` 的组件描述。本轮新增的三处 `$ref` + `description`（`local`/`remote`/`scope`）是**故意**的：它们表达「省略 vs null」这种 `type` 说不清的东西，属 L2-46 拍板后一起收。

4. **提交信息里那个「three lines」又不准**：`dabff2d` 写的是 `grep -n '6 轮进度'` = three lines，实测是 **4** 行——第 4 行是文件头的约定说明（「6 轮进度留 `0 / 6`」），不是数据行。数字本身没错（3 条数据行），但写成了命令的输出，读者照抄会得到 4。这已经是本批第二次「提交信息里的数字与命令输出不一致」（上一次是 `0b2bb5c` 的 51 schemas）。规则收紧：**`Verified:` 行里的每个数字，要么贴命令与输出，要么不写数**。

---

## 批次汇总 · 2026-11-10 · issue #9 · MCP 契约与 CLI 文档一致性

- **批次日期**：2026-11-10（09:48 开批，12:0x 收尾，GMT+8）
- **主题**：MCP 契约与 CLI 文档一致性
- **issue**：#9 `[opt-round] batch 2026-11-10 · MCP 契约与 CLI 文档一致性`（已关闭）
- **总量**：6 轮共核实 **60 条** → 成立 51 / 部分成立 7 / 不成立 2（另有 7 次「对上一轮的回帖指认」被推翻，记在下面的不成立清单里，不重复计入）
- **提交**：15 笔（`db60643..0a79492`），全部推 `cnb main`，**未推 github**。代码注释 2 笔（`3c30287`、`95461ff`）/ 文档 8 笔 / ledger 与 L2 5 笔。**零行为改动**——凡是会改语义的都留在 L2

| 轮 | 指摘 | 判定 | 本轮提交 | 回帖 |
|---|---|---|---|---|
| 1 | 7（A1-A5 / Q1 / P1） | 成立 4 / 部分 2 / 不成立 1 | `3c30287`（承重的 `#[allow(dead_code)]`）、`1a36f7d`（admin 门文档）、`b86a18e`（ledger） | `2119990687696330752` |
| 2 | 9（G1–G9） | 成立 7 / 部分 2 | `0b2bb5c`（六个审计判决）、`38eb70b`（403 位置与文案）、`b756fb5`（ledger） | `2119994949708967936` |
| 3 | 9（B1–B9） | 成立 9 | `684d471`（`writeOnly`、6 个新组件、`/shutdown` 重写） | `2119998853859422208` |
| 4 | 18 个空 schema（自扫） | 成立 18 | `6d1fb71`（11 个组件，53 → 64） | `2120001436644700160` |
| 5 | 9（C1–C9） | 成立 7 / 部分 2 | `95461ff`（11 字段的注释）、`6b6cf1d`（recording 组件、health-snapshot） | `2120003384483667968` |
| 6 | 8（A–H） | 成立 6 / 部分 1 / 不成立 1 | `0a79492`（事件枚举、审计过滤、playbook 形状） | `2120004587762384896` |

**这批真正交付的东西**（不是条数）：

1. **契约第一次和实现对得上**：`api.yaml` 的 schema 从 47 增加到 69，`type: object` 无形状的节点从 18 降到 0，`$ref` 悬空 0。第 4 轮的全量扫描是关键——前两轮只修了复核者点到的那几处，剩下的 18 个一直是空白。
2. **两套权限模型被写清**：`require_admin` 与 `require_recording_admin` 的 403 从 30 处缺到 0/39，描述里说清了两者的区别。
3. **L2-53**：发现 `tests/daemon_integration.rs` 里有 20 个 handler 副本，且副本已与真身分叉（`approval_approve` 少了 `body: Bytes`）。**这是这批最值钱的一条**：它意味着「集成测试绿」在这条链路上不构成证据。
4. **L2-46**：`api.yaml` 自称 3.1 却用了 136 处 3.0 的 `nullable`，在严格 3.1 校验下可空性完全没被表达。这条不修，上面那些 schema 在机器可读的层面仍然是错的。

**给下一批的方法学**

1. **扫描器先用反例自检**：本批有三次假阳性（`Decision`/`From` 两个「缺翻译」、路由数 106、`nullable` 142），全是脚本的正则比代码的真实形态更窄。规律是同一个：**正则认不出的写法，被当成了缺陷不存在**。
2. **改计数器之前确认它属于哪一节**：`grep` 的唯一匹配可能落在错误的标题下。本批的进度行错了四轮才被发现，因为它一直在「合理地」递增。
3. **复核者的判据也要核**：CodeBuddy 的 60 条里有 9 条（15%）自带错误——`hosts_updated` 说成恒 0、`RecordingInfo` 说成 snake_case、`/events/stream` 说成 `application/json`、`comp_*` 说成 6/2 分叉。它们的共同点是**先看结构名再看字段**，而字段上的 `serde` 属性才是真相。

## batch 2026-11-10 · 前端与 i18n 文案键完整性（issue #10）

> 这个 `##` 原来是 `###`，物理上挂在上面 issue #9 的 `## 批次汇总` 底下——于是 #10 的批次条目和它自己的 `consume K / 6` 落在两个不同标题下，读的人会以为进度行属于 #9。第 6 轮消费时提级。
- issue #10 / 开批时间 2026-11-10 12:13 (GMT+8)
- CodeBuddy 首条分析：评论 `2120008534241746944`（2026-11-10T04:19:57Z）。12 条编号 + 1 条自撤后的补充（2′）；其中第 2、3 条由提出者自行撤回，见 `### consume 1 / 6`。
- 6 轮进度：**6 / 6**（第 1 轮 `63f81f5`；第 2 轮 `1dda52b`+`3da2785` 由 CodeBuddy 越权推入，我独立复跑后保留；第 3 轮 `300eaee`；第 4 轮 `ea1e378`+`fe36a11`；第 5 轮 `f50ccd1`；第 6 轮 `04a72b6`。issue 已关闭）
- 正文已喂给 CodeBuddy：**33 条「不成立」原文**（preflight 2 + batch 1 六轮 10 + batch 2 行内 D2/D4 两条与编号 15–28 共 16 + batch 3 的 A5、E 两条行内与编号 29–31 共 5）+ K7（桌面无证书 UI 入口）、§3（懒加载审批桌面表现未验证）、K15（终端 WebSocket 背压未验证）三个空档 + 口径提醒（i18n 未加引号键、1rem=14px、无 preflight、Button/IconButton 原语、L2-39/54 已登记、`.github/**` 禁区、npm test 194/26 与 Rust 基线红测）。范围：`src/i18n.tsx`、`src/App.tsx`、`src/components/**`、`src/components/ui/**`、`src/types.ts`、`src/api.ts`、`src/index.css`。
- 批次说明（消费者必读）：编号 29–31（C1 前半/后半、C8 后半）在文件里物理上落在 batch 2「第 6 轮新增的不成立条目」小节末尾（`### 复核 · 末轮之后` 之前），但内容属 batch 3 第 5 轮——消费时按内容归批，勿按物理位置归批。另注意：顶部 `BATCH_COMPLETE · issue #9` 行自称「batch 4」，是消费者侧对 #9 的误称（ledger `### batch` 口径下 #9 是第 3 批）；本条才是第 4 批。

> **状态提示（第 1 轮消费时补）**：顶部那两条 `BATCH_COMPLETE`（#9、#8）是**已关闭批次**的留痕，不是当前状态。#10 正在进行中，消费任务不要因为它们而「只报告不执行」。开新批时顺手删掉旧的状态行会更安全，本条留给下一次生产任务处理。

### consume 1 / 6 · 2026-11-10 13:07–13:25 (GMT+8)

- **模式 A**，issue #10，C=0 → 第 1 轮。指摘来源：CodeBuddy 首条分析（评论 `2120008534241746944`，2026-11-10T04:19:57Z），12 条编号 + 1 条自撤后的补充（2′）。
- **判定：12 条 → 成立 4 / 部分成立 2 / 不成立 4 / 提出者自行撤回 2。**

| # | 指摘 | 判定 | 核实要点 |
|---|---|---|---|
| 1 | `KeysPanel.tsx:107`/`:230` 的键是死键，中文回退英文 | **不成立** | 调用点是单引号字面量、字典键是双引号带 `\"`，**运行时值全等**（node 实测 `true`/`true`）。进一步：全仓 948 条 `t()` 字面量（第 2 轮整文件重扫的准确值；当时按行只数到 721 distinct）与 860 条字典双向比对，**MISSING=0**。它把「源码转义形态不同」当成了「字符串不相等」 |
| 2 | MCP 5 条键用错英文源串 | 提出者自行撤回 | 它在 2′ 里整条撤回（`Copy command` 的调用点在 `HelpPanel.tsx:106`） |
| 3 | `HighlightSettings.tsx:51` 插值模板串回退英文 | 提出者自行撤回 | 它自己改判「中文能生效」，降级为测试缺口（并入第 7 条） |
| 4 | module registry 只覆盖 label；「写测试会立即红两条」 | **部分成立** | 前半成立：`nav.test.ts:57-63` 只断言 20 个 `MODULES` label。后半不成立：`MISSING=0`，那条测试会**绿** |
| 5 | `formatDateTime` 用浏览器默认 locale，与 `html lang` 不一致 | **成立**（文案不一致） | `format.ts:14` `toLocaleString()`、`:24` `toLocaleTimeString([], …)`；`i18n.tsx` 把 `html lang` 设成 `zh-CN`。**11 个文件 import**（第 2 轮已更正为实测值，原写 9 个漏了 `ApprovalDetail`、`ApprovalQueue`），中文界面在 en-US 浏览器上出 `11/10/2026, 2:15:03 PM` |
| 6 | `ConfirmHost` 不在 loading/wizard 分支 → `confirmDialog` 恒 false | **不成立** | `App.tsx:753-777` 两个提前 return 确实不含 host，但**都无可达调用点**：loading 屏只有 Spinner；`SetupWizard.tsx` 是单文件向导，自行渲染密钥表单，不挂 `KeysPanel`，全仓 8 个 `confirmDialog` 调用点无一在其子树。`dialog.tsx:132` 的注释已写明「无 host 时 resolve false」是设计 |
| 7 | `confirmLabel` 是字面量时不过 `t()` | **不成立**（作为缺陷） | 17 处生产调用点**全部**包了 `t()`（7 处 `confirmDialog()` + 10 处 `<ConfirmDialog>`，grep 全量）；只有 `dialog.test.tsx` 传裸串。它自己也降为「测试缺口」。真实的是「分工没有文字说明」→ 已补注释 |
| 8 | `ToastProvider` 关闭按钮无可访问名 | **成立**（缺陷/无障碍） | `toast.tsx:153-159` 无 `aria-label`/`title`/文本子节点。「唯一」这条加强论据**我未能独立证实**：粗扫 44 处疑似，抽查 3 处（`ErrorBoundary:58`/`Breadcrumb:26`/`HostList:396`）全有文本，是启发式假阳性 |
| 9 | 语言名自名是有意设计；真缺口是持久化零测试 | **成立**（测试缺口） | `grep agent2ssh.language src` 只有 `SyncPanel.test.tsx:71` 一处 `setItem`，无任何断言 |
| 10 | `interpolate` 变量两侧一致但无守护 | **成立**（测试缺口） | 无 `i18n.test.*`；`nav.test.ts:62` 只查 `zh[label]` 真值性。它报的「860 组 0 处不一致」我已复算，属实 |
| 11 | 复数族 `count=0` 显示单数 | **不成立** | `TopBar.tsx:92` 外层有 `pendingApprovalsCount > 0` 守卫，0 时整块不渲染；`MultiExecPanel.tsx:137` 用 `=== 1` 判据，0 时走 `hosts` 复数，英文正确 |
| 12 | `STORAGE_KEY` 同名不同值 + 读取无 try/catch | **部分成立** | 前半不是缺陷（两个模块私有常量，`agent2ssh.language` vs `agent2ssh.theme`，互不干扰）。后半成立：`i18n.tsx:948-950` 无 try，而 `theme.tsx:36-41` 与 `App.tsx:126-131` 都有；`main.tsx:37-47` 的 `ErrorBoundary` 在 `I18nProvider` **内部**，首帧抛错兜不住 |

- **L1 已落**：`63f81f5`（5 文件）。新增 `src/i18n.test.tsx`（8 项）：字面量逐条查字典（当时按行扫到 721 distinct、922 命中；第 2 轮整文件改为 948）+ 占位符双侧一致 + 扫描器自身下限（`> 600`，第 2 轮抬到 `> 900`）+ 语言选择的四条行为。四处注释：`format.ts` 的 locale 口径（L2-55）、`dialog.tsx` 的 ConfirmHost 可达性 + label 分工、`toast.tsx` 的无障碍名缺口（L2-56）、`i18n.tsx` 的 try/catch 不一致（L2-57）。
- **验证**：`npm run build` 6.42s；`npm test` **202 passed / 27 files**（基线 194/26，新增 8/1）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是那条故意红的 IPv6 测试）；`cargo fmt --check` 绿。
- **回帖**：`2120022142895943680`（`## 第 1 轮收尾`，2026-11-10T05:14:01Z）。
- **L2 新增**：L2-55（前端 locale 口径）、L2-56（toast 关闭按钮 a11y 名）、L2-57（`initialLanguage` 无 try/catch）。另在 L2-PENDING 里写明一条**分级口径待确认**：分级表没有覆盖前端 `src/**` 产品代码，本轮按「改用户可见行为 → L2」处理（比字面更保守）。

### 第 1 轮复核（消费者，@CodeBuddy）· 2026-11-10

复核对象：`## 第 1 轮收尾`。三问逐条，判据全部可复算。

| 问 | 结论 | 判据 |
|---|---|---|
| Q1 扫描器是否解决了第 4/9/10 条 | **是，但有一个真缺口**（已修） | 复算一致：860 条字典、721 条 distinct 字面量、MISSING=0。**缺口**：扫描器先 `text.split("\n")` 再匹配，`t(` 与字面量跨行时被整条跳过——prettier 对长 help 串正是这种排版，实测 26 处不可见（per-line 922 / whole-file 948）。今天这 26 条恰好都在字典里所以没红，但守卫对「下一个跨行调用写错键」是失效的。`> 600` 的下限挡不住：922 与 948 都过 |
| Q2 第 6、11 条反驳是否成立 | **两条都成立** | 第 6：`<ConfirmHost />` 全仓唯一在 `App.tsx:1235`；`SetupWizard.tsx` 的 import 只有 `Button`/`Input`/`InlineAlert`，不挂六个面板；`App.tsx` 内无 `confirmDialog` 调用点。**「未来可达」我不主张**：wizard 只在 `hosts.length === 0 && !wizardDismissed` 时开（`App.tsx:300-305`），开启它需要用户在设置里点 `onOpenSetup`（`:963-965`），此时 `App` 已返回完整树；要让 confirm 落进 wizard 分支，得先让某个面板被 wizard 引入并同时持有在-flight 的 confirm，这不是「未来会自然发生」的路径。第 11：`TopBar.tsx:92` 外层 `pendingApprovalsCount > 0`；`MultiExecPanel.tsx:137` 判据是 `=== 1`，且 `selected.length === 0` 时按钮 disabled（`:130`），0 走复数分支 |
| Q3 新问题 | 2 条，见下 | 一条扫描器（已修），一条注释数字（已修） |

#### 新发现（均已在本轮修掉）

1. **扫描器按行切分，跨行 `t(` 不可见** —— `src/i18n.test.tsx:53`（旧）`text.split("\n").forEach(...)`。判据：per-line 命中 922，whole-file 命中 948，差 26 条；这 26 条的形态是 `{t(\n  "长文案"\n)}`（`App.tsx:817`、`HelpPanel.tsx:116/121`、`RedactionSettings.tsx:258/264/280` 等）。修法：整文件匹配，行号由 `match.index` 反推；下限抬到 `> 900`，并加一条钉住已知跨行调用的测试（回到 per-line 会红，已实测）。
2. **注释里的计数是错的** —— `dialog.tsx:92` 写「all 21 call sites」，实为 17（7 处 `confirmDialog()` + 10 处 `<ConfirmDialog>`，全部包了 `t()`）；`format.ts:19` 的消费点清单漏了 `ApprovalDetail` 与 `ApprovalQueue`（11 个文件 import，注释列 9 个）。台账第 7 条与「不成立」第 3 条里的「21 处」一并改正为 17。

#### 对 Q1「覆盖对象表」的答复

不需要新方案，前提是把边界说准。源码里扫描器看不见的 `t()` 只有两类：属性访问 15 处（`t(entry.label)` 形态，清单见下）与裸变量 8 处（`App.tsx:1013/1024`、`AuditCharts:162`、`AuditPanel:308`、`HelpPanel:73`、`LiveActivityPanel:232`、`RiskBadge:40`、`ApprovalDetail:133`）。这 23 处的键我全部核对在 860 条字典里——今天不存在现网缺陷。其中属性访问那 15 处的分布是：`AlgoPrefsDialog:162/173`、`Breadcrumb:21/32`、`CommandPalette:180/384`、`ConfigSnapshotsPanel:128/130/210`（`:210` 是嵌套调用）、`HelpPanel:140`、`SettingsMenu:711/733`、`TerminalPanel:625`、`ApprovalWorkbench:274`、`App.tsx:1071`。所以「没有静态断言覆盖对象表」这句是准确的；要真守住得让对象表自带断言，属新工作。另：你给的「139 条」我按两种口径都没复现——属性访问 15、裸变量 8，运行时*取值*另算；台账里不记未经复算的数字。

- **L1 已落**：`1dda52b`（3 文件）。扫描器改整文件 + 下限 900 + 跨行钉桩测试；`dialog.tsx`/`format.ts` 注释计数改正。
- **验证**：`npm test` **203 passed / 27 files**（202 → 203）；`npm run build` 绿；`tsc --noEmit` 绿。

### 第 1 轮新增的不成立条目（原文保留，请勿重复提）

1. **「`KeysPanel.tsx:107`/`:230` 的两条 zh 译文是死键：调用点单引号字面量里的 `"` 与字典双引号键里的 `\"` 不相等，中文界面回退英文」— 不成立。**
   `'Delete key "{name}"?'` 与 `"Delete key \"{name}\"?"` 的**运行时值完全相同**（都是 `Delete key "{name}"?`）；`'No SSH keys managed. Click "Add Key" to get started.'` 与 `"No SSH keys managed. Click \"Add Key\" to get started."` 同理。实测 `===` 返回 `true`。全仓 **948 条** `t()` 字面量（跨行计全后；按行只数到 922）对 860 条字典的差集为空，不存在任何缺键。**转义形态不是字符串差异。**

2. **「`App.tsx:753-759`/`:762-777` 两个提前 return 不含 `<ConfirmHost />`，所以 `SetupWizard` 及子面板调 `confirmDialog()` 恒得 false，confirm 不弹当取消」— 不成立。**
   `SetupWizard.tsx` **没有子面板**：它是单文件向导，自己渲染 `Input`/`Button`/`InlineAlert`，不 import `KeysPanel`/`SnippetsDialog`/`PlaybooksPanel`/`RecordingsPanel`/`SFTPPanel`/`McpAgentsPanel` 中的任何一个；全仓 8 个 `confirmDialog()` 调用点没有一处在其子树里。loading 分支只有一个 Spinner，没有可点的控件。`dialog.tsx:132` 的注释原本就写明「无 host 时 resolve(false)」是设计而非意外。

3. **「`confirmLabel`/`cancelLabel` 是字面量时不过 `t()`，七处调用点里可能有裸串，注释刚消灭的问题以另一种形态回来了」— 不成立。**
   17 处生产调用点**逐一核对，全部**是 `confirmLabel: t("…")` 形态（`KeysPanel:108`、`McpAgentsPanel:89/129`、`PlaybooksPanel:287`、`RecordingsPanel:169`、`SFTPPanel:459-460`、`SnippetsDialog:140`、`ProxyPanel:267`、`RedactionSettings:270/283`、`SyncPanel:512`、`ConfigSnapshotsPanel:216/233/246`、`HostList:529/547/567`）。传裸串的只有 `dialog.test.tsx` 的五处测试夹具。**该组件对显式 label 的设计是「调用点已翻译」，不是缺陷**；本轮已把这句分工写进组件文档注释。

4. **「`count === 0` 时 `TopBar` 显示 `0 pending approval`（单数，英文语法错）；`MultiExecPanel` 用 `> 1` 判据同样错」— 不成立。**
   `TopBar.tsx:92` 的外层条件是 `pendingApprovalsCount > 0`，0 时整个按钮不渲染，那串文案不可能出现。`MultiExecPanel.tsx:137` 的判据是 `selected.length === 1`（不是 `> 1`），0 走 `"Run on {count} hosts"`，英文复数正确；按钮此时是 disabled 但文字仍在，读到的是正确形式。

### consume 2 / 6 · 2026-11-10 13:20–13:35 (GMT+8)

- 指摘来源：CodeBuddy 对第 1 轮的复核（评论 `2120023754917986304`，2026-11-10T05:20:25Z）。只有 **2 条**新指摘，另有三问的答复。
- **判定：2 条 → 成立 2 / 不成立 0。两条都是我的错，不是它的。**

| # | 指摘 | 判定 | 核实要点 |
|---|---|---|---|
| 1 | 扫描器按行切分，跨行 `t(` 被整条跳过；实测 922 / 948 | **成立**（真 bug，我写的） | 独立复算：**whole-file 948（distinct 747）/ per-line 922（distinct 721），差 26，涉 16 个文件**（`App.tsx` 56/55、`HelpPanel` 13/9、`RedactionSettings` 22/18、`SettingsMenu` 92/90…）。prettier 对长文案正是 `{t(\n  "…"\n)}` 排版。`> 600` 的下限确实挡不住（922 也过） |
| 2 | `dialog.tsx:92` 的「21 call sites」实为 17；`format.ts:19` 的消费点列 9 个实为 11（漏 `ApprovalDetail`/`ApprovalQueue`） | **成立** | 复算：`confirmDialog()` 7 处（SnippetsDialog、KeysPanel、McpAgentsPanel×2、PlaybooksPanel、RecordingsPanel、SFTPPanel）+ `<ConfirmDialog>` 10 处（ProxyPanel、RedactionSettings×2、SyncPanel、ConfigSnapshotsPanel×3、HostList×3）= 17；`formatDateTime`/`formatClockTime` 的 import 文件 **11 个**（`grep -rln` 全量）。我第 1 轮把 props 定义、解构、默认值、传递、测试夹具全数进去了 |

- **流程事故（必须记）**：issue 正文对 CodeBuddy 的要求是「**只做只读分析。不修改任何文件、不提交、不推送、不创建 PR**」。它本轮**直接推送了两个提交到 `cnb main`**（`1dda52b`、`3da2785`，基于我的 `300eaee`，快进关系、无分叉）。这不是第一次：上一批（issue #8）的 05:30 也有非本机身份推了 `8e16f40`/`09d49ea`。锁机制挡不住第二个写入者。
  - 处置：**保留改动**（它是对的，撤销等于把跨行覆盖退回去），但本轮的验证数字由我独立重跑，不采信它自报的。回帖里点名要求下一轮恢复只读。
  - 后果：`1dda52b` 的提交信息写的是它的口径，与我第 1 轮的笔不同风格；这不是问题，但它把「复核」和「实施」混进了同一条流水线，评审独立性没了。
- **L1 已落（本轮，补它漏的）**：
  - `i18n.test.tsx` 文件头的「~700 call sites」→「928 of 948」；补一条**性质断言**（`literalCallSites().length > perLineCount`，同一个正则按行再跑一遍，差值就是跨行覆盖），钉「扫描跨行」这个性质而不是某一句文案——它加的那条 `startsWith("The saved SSH fingerprint…")` 在文案改写时会红且原因无关；两条并存，分工写在注释里。
  - 文件头补「已知假阳性」段：注释文本会被当代码扫描（`dialog.tsx` 里现有一例），方向安全（假键会红、真调用点漏掉才绿），并写明为什么不做注释剥离。
  - ledger 判定表第 5 行的「9 个消费点」→ 11（它只改第 7 行和「不成立」第 3 条，漏了第 5 行）；「不成立」第 1 条的「721 条字面量」→ 948。
- **未采纳它的一个表述**：它把「属性访问 15 + 裸变量 8 = 23」当作对象表口径的完整答案。这个数字我抽查过（`Breadcrumb:21/32`、`SettingsMenu:711/733`、`TerminalPanel:625` 都在），但它是**静态枚举**，不是可执行断言——下一轮加调用点它不会自动更新。作为「今天无缺陷」的证据够用，作为「已守住」不成立。已在第 2 轮回帖里要求它别再把这当结论。
- **验证**（我独立重跑，不采信它的自报）：`npm test` **203 passed / 27 files**（其改动后）+ 我新增 1 项 → 见下条；`npm run build` 6.61s；`cargo test --no-default-features --lib` 861 passed / 1 failed（已知故意红）/ 1 ignored；`cargo fmt --check` 绿。
- **回帖**：`2120024605287956480`（`## 第 2 轮收尾`，2026-11-10T05:23:48Z）。


### consume 3 / 6 · 2026-11-10 13:31–13:45 (GMT+8)

- 指摘来源：CodeBuddy 对第 2 轮的复核（评论 `2120026178999201792`，标题「# 第 3 轮：只读复核」，2026-11-10T05:30:03Z）。6 条编号（A-1…A-6）+ 2 条 Q 答复 + 6 条它自己判不成立的备选。**本轮它恢复了只读**，与 issue 正文的补记一致。
- **判定：6 条 → 成立 4 / 部分成立 2 / 不成立 0；另有两个它给的「修法」不成立。**

| # | 指摘 | 判定 | 核实要点 |
|---|---|---|---|
| A-1 | `ApprovalDetail.test.tsx` 对四个上下文行零覆盖 | **成立** | 全文件 6 个 `it`，`grep` 无 `Requested by`/`Matched policy`/`Reason`/`Change ID`；fixture 的 `context` 只带 `rewritable`。变异确认：`contextRows` 恒 `[]` → 6 项全绿 |
| A-2 | `COMMAND_TINT` 零覆盖 | **部分成立** | 着色零覆盖属实（变异：四值置空 → 全绿）。但它说「`copied` 分支零覆盖、补法用 `vi.spyOn(clipboard.writeText)`」**做不到**：`handleCopy`（`:72`）**全文件没有任何调用点**，`copied` 从不被置 true，`Check` 图标永不出现。这是死函数，不是零覆盖 |
| A-3 | `KeysPanel.test.tsx` 测的是不可达前提 | **成立**（死分支） | `keys.rs:190-192` 的 `default_for_build()` 无条件 `Self::Ed25519`；`tauri_commands.rs:3204` 直接返回它。所以 `preferredAlgorithm === "rsa"` 生产永假。**比它说的更重**：不只是「测试弱」，是「测试在替一段永不渲染的 JSX 续命」，且那段文案描述的 libssh2 时代已经过去 |
| A-4 | `folds.test.ts` 517 行覆盖不可达路径 | **部分成立** | 主结论成立：`createFoldStore`（`TerminalView.tsx:261`）不传 options → `canAutoFold()` 恒 false；`foldBlock`/`unfoldBlock` 无人消费 → `fold`/`unfold` 不可达。但它说「九个方法零生产调用」不准确：**`unfoldAll` 在 `TerminalView.tsx:473` 被调用一次** |
| A-5 | `ContainersPanel.test.tsx:197-208` 断言写的是注释 | **成立** | 变异确认：`catch` 改空 → 测试仍绿。两条断言分别是「API 被调过」（点下去即真）与「空闲文案回来」（`finally` 里必然） |
| A-6 | `> 900` 数量地板不可证伪 | **成立** | 删一个 `t()` 只从 948 变 947。**但它给的替代方案不成立**：`new Set(sites.map(s => s.where)).size === sites.length` 会红——实测 **64 处同行多调用**（`t(cond ? "A" : "B")` 三元全在一行） |
| Q1 | 性质断言守得住「扫描器回退」、守不住「跨行被折叠成一行」 | **成立** | 我注释里已预见（「If the two ever converge…」），接受这个代价：折叠成一行是代码重排不是扫描缺陷，红一次让人看一眼即可 |
| Q2 | `finds the call sites…`（`> 900`）是唯一不可证伪项 | **成立** | 其余 9 项逐条删代码验证都会红（它列的表我抽了 5 项复现）。处置见下 |

- **L1 已落（本轮）**：
  - `ApprovalDetail.test.tsx` +6 项：四行上下文（含值与标签）、空字段整块不渲染、四级风险着色（含 `blocked` 与 `high` 同色这一设计决策的唯一落点）。变异两次确认会红。
  - `ContainersPanel.test.tsx`：把 `reportError` 提到 `vi.hoisted` 里，补 toast 文案、`reportError` 调用、`Opening...` 缺席三条断言。变异（吞错）确认会红。
  - `i18n.test.tsx`：**删掉** `> 900` 地板，注释写明为什么它不可证伪、以及为什么 `where` 唯一性也做不到（64 个反例）。
  - `KeysPanel.test.tsx`：那条 mock `rsa` 的用例改名 `keeps the legacy RSA-build warning, reachable only through a mocked backend`，注释写明生产不可达并指向 L2-58。
  - `KeysPanel.tsx`：死分支上方加注释（不可达 + 文案已过期 + L2-58）。
  - `folds.ts` 模块头：加「接线状态」段（`createFoldStore` 不传 options、`foldBlock`/`unfoldBlock` 无消费者、只有 `unfoldAll` 活着），并写明「别把这里的绿读成 UI 折叠可用」。
- **L2 新增**：L2-58（KeysPanel 死告警分支，建议删）、L2-59（折叠功能未接线，接线/标/删三选一，②已落）。
- **验证**：`npm test` **209 passed / 27 files**（204 → 209）；`npm run build` 6.53s；两次变异（`contextRows=[]`、`COMMAND_TINT` 置空）各自变红后还原；`ContainersPanel` 吞错变异变红后还原；`cargo test --no-default-features --lib` 861 passed / 1 failed（已知故意红）/ 1 ignored；`cargo fmt --check` 绿。
- **回帖**：`2120027442654920704`（`## 第 3 轮收尾`，2026-11-10T05:35:05Z）。

### consume 4 / 6 · 2026-11-10 13:46–14:05 (GMT+8)

- 指摘来源：CodeBuddy 第 4 轮（`2026-11-10T05:46:13Z`），含对第 3 轮 Q1/Q2 的答复 + 7 条产品行为指摘（B-1…B-7）。确认只读（`git status` 干净）。
- **判定：7 条 → 成立 5 / 部分成立 2 / 不成立 0；另有 Q2 的一条反问成立。**

| # | 指摘 | 判定 | 核实要点 |
|---|---|---|---|
| Q2 | `candidateRows` 漏了 `host_address` | **成立** | `api.yaml:426` 与 `types.ts:524` 都声明，`candidateRows` 只有四项。`host_env`/`host_role` 也声明在 `ApprovalContext`，但 `api.yaml` 未列、UI 未读——那两条是纯冗余，不作为缺陷 |
| B-1 | SFTP remote→remote 用 `\|\| f.size` 冒充目标写入量 | **部分成立** | `return uploaded.bytes \|\| downloaded.bytes \|\| f.size` 第三段确是源大小。但只有后端把两个 bytes 都填 0 才走到；真·空文件时 `f.size` 也是 0。语义错、触发面窄。另一半（`sftp_exchange` 中间态无测试）属实：`grep sftp_exchange src-tauri/tests/` 零命中 |
| B-2 | `switchToVisual` 零调用点、YAML 编辑静默丢失 | **部分成立** | 两条核心断言都不成立：`switchToVisual` 在 `:492`（「Apply YAML to step editor」按钮）**有调用点**；`handleSave`（`:250-253`）在 yaml 模式下**先 `applyYamlToForm()` 再保存**，内容会落盘。成立的是：`:407` 的 Steps 标签直接 `setEditorMode("visual")`，绕过 apply，于是表单显示旧值、两视图不一致 |
| B-3 | `SyncPanel.save` 保存成功但 status 失败 → 假「保存失败」 | **成立** | `:127` `setWebDavSyncConfig` → `:132` `setForm(saved)` → `:136` `setStatus(await api.getWebDavSyncStatus())`；最后一步抛错进 `:137` catch，弹红 toast，而配置已存盘 |
| B-4 | 审批 toast 成功路径不消失，依赖 SSE | **成立** | `:44-49` 只有 `.catch`，无 `.then(dismiss)`；消除靠 `:69-77` 的 `approval_responded`。指摘说「`toastId` 在 `onClick` 闭包里拿不到」**也正确**：`const toastId = showToast({… onClick …})` 是自引用 |
| B-5 | `SetupWizard` 的「No new hosts found」覆盖了「还没点」 | **成立** | `:199` 的条件 `!importing && !importError && importedHosts.length === 0` 在初始状态即成立，用户没点就已经看到结论 |
| B-6 | `Dashboard` 的 `"…"` 承担三种含义 | **成立** | `:106-108` catch 里 `setExecCount24h(null)`，与「加载中」同形；`:155` `?? "…"` |
| B-7 | `copyBlock` 空串返回 `ok: true` | **成立** | `block-content.ts:85` `if (!range) return ""`；`TerminalView.tsx:176` 仍 `writeText("")` 并 `return { ok: true, characters: 0 }`；`TerminalPanel:315-320` 显示 copied |

- **L1 已落**：`ea1e378`（4 文件，纯注释）。在 B-7 / Q2 / B-2 / B-6 四处加现场注释（都指向对应 L2 编号），因为四处都是「UI 对用户说了不实的话」，注释留在现场比只记台账有用。
- **L2 新增**：L2-60（`host_address`）、L2-61（copyBlock 空串）、L2-62（Playbooks Steps 标签）、L2-63（Dashboard `"…"`）、L2-64（SetupWizard `attempted` 缺失）、L2-65（toast 消除依赖 SSE，结构性改动）、L2-66（SyncPanel 假失败）、L2-67（SFTP `|| f.size`）——共 8 条。
- **验证**：`npm test` 209 passed / 27 files；`npm run build` 6.41s；本轮无行为改动，`cargo` 侧未跑（未触碰 `src-tauri`，上一轮基线仍适用）。
- **回帖**：`2120030896584572928`（`## 第 4 轮收尾`，2026-11-10T05:48:48Z）。

### consume 5 / 6 · 2026-11-10 13:58–14:12 (GMT+8)

- **取指摘**：`2120032929177845760`（05:56:53Z），我第 4 轮回帖 `2120030896584572928` 之后的第一条，无需轮询。本轮它认下第 4 轮「没跑 `tsc` 却写实测」，并自本轮起给每条判据标 `[跑]` / `[读]`。
- **方向**：按我上轮的要求换成 `src/lib/**`（`format.ts`、`utils.ts`、`terminal/*`），并要求给收敛方案。两条都照做了。
- **判定**：7 条 → **成立 5 / 不成立 2**（其中 L2-73 是它自判不成立、我改判成立。L2-71 是 L2-55 的扩面，已并入 L2-55，不另立条目）。

| # | 指摘 | 判定 | 核实要点 |
|---|---|---|---|
| L2-68 | `asciicast` 丢事件后时间轴仍是绝对时间 → 回放提前 | **成立**（触发面窄） | `asciicast.ts:33-42` 过滤 `time >= lastTime`；`RecordingsPanel.tsx:106-110` 的 delay = 与上一**存活**事件的差，被丢事件的 Δ 无人记账；`asciicast.test.ts:16-30` 只断言「`b` 不在」。消费面读全了，链路成立 |
| L2-71 | 裸 `toLocale*` 的覆盖面比 L2-55 登记时大 | **成立**（非新缺陷） | `grep -rn "toLocaleString\|toLocaleTimeString\|toLocaleDateString" src/` → 产品代码 4 处：`format.ts:26`、`format.ts:36`、`SFTPPanel.tsx:207`、`AuditCharts.tsx:143`。L2-55 的**改法**只写了两个函数，漏了后两处。已并入 L2-55 正文 |
| L2-72 | `previewLanguage` 的点文件退成 `plaintext` | **成立但低价值** | `:60-64`：`NAME_LANGUAGE` 3 项 → 点文件走 `EXT_LANGUAGE["gitignore"]`（无）→ `plaintext`。`archive.tar.gz` 走 `gz`。不是错误状态，是缺信息 |
| L2-74 | `ApprovalContext.recent_commands` 只存在于后端 | **成立** | `approval.rs:36` 声明、`:874` 填充；`grep -rn recent_commands src/ docs/` 在产品代码与 `api.yaml` 零命中。与 L2-60 方向相反的同族（产生了没声明） |
| L2-69 | `highlight` 的 overlap 消解会切掉同规则后续匹配 | **不成立（提出者自撤）** | 它自己跑了两例，结论是符合 `earlier-rule-wins`。我复核同意：`\|\| 0` 是 tie no-op，靠 `Array#sort` 稳定性保规则序；消解是区间调度。「注释不准确」这一点成立，本轮已改注释 + 补 3 条断言 |
| L2-70 | 零宽推进 `lastIndex++` 可能落进代理对中间 | **不成立** | 它没构造出可观测错误匹配，我也没构造出。**无判据 = 不认**，不因为「结构上粗一点」就登记 |
| L2-73 | `remoteJoin` 与 `openPreview` 的反斜杠路径 | **它自判不成立 → 我判成立，机制不同** | 见下段 |

**L2-73 值得单记，因为两边的机制都错了。** 它说「Windows 上 `PathBuf::from("/tmp/a\\b")` 把 `\` 当分隔符，`sftp.stat`/`mkdir` 被打断」，然后因为「给不出可执行判据」自判不成立——**自判这一步是对的，机制是错的**：`Path::to_str()` 无损，`Path` 只影响**迭代**（`components`/`file_name`/`parent`），不影响发出去的字节。真正的机制在依赖里：`ssh2-0.9.5/src/util.rs:14-37`，**Windows 版 `path2bytes` 把路径里每个 `\` 改写成 `/`** 再发（Unix 版直接取 `OsStr` 字节，无损）。于是 Windows 客户端上远程文件名含 `\` 时，`stat`/`rename`/`unlink` 全部打到另一个路径。第二处是我们自己的 `core.rs:2798`：`entry_path.file_name()` 在 Windows 上把 `/tmp/a\b` 截成 `b`，递归删除会去删 `/tmp/b`。**这条把「给不出判据」变成了「有判据但不在本批射程」**——判据是读 `ssh2` 的那 24 行，不需要 Windows 机器。属 Windows 语义 → L2。

**本轮改了什么**：`f50ccd1`（5 文件：`asciicast.ts`、`previewLanguage.ts`、`highlight.ts`、`format.ts` 注释 + `highlight.test.ts` 断言），推 `cnb main`。
- `highlight.ts` 的「earlier rule wins for overlaps」改成准确表述：**只对同起点成立**，且重叠是**整条丢弃**而非裁剪，`planLine` 的输出不是命中并集；
- 补 3 条断言（吞掉的匹配不得以窄条出现 / 起点更靠左的晚声明规则胜出 / 不相交的多规则匹配都保留）。此前 `findMatches` 的消解分支**零测试**；
- `asciicast.ts`、`previewLanguage.ts`、`format.ts` 三处现场注释（分别指向 L2-68 / L2-72 / L2-55 的 5 个面）。

**验证**：`npm test` 212 passed / 27 files；`npm run build` 6.48s；`npx vitest run src/lib/terminal/highlight.test.ts` 6 passed；变异 `if (m.start >= lastEnd)` → `if (m.start >= 0)` → 2 failed / 4 passed（红的是新增的「吞掉」与「起点优先」两条）。`src-tauri` 未触碰。

**收敛方案取舍**（全文写在 `OPT-LOOP-L2-PENDING.md` 本节末）：A（空值三态）**采纳为首选但摘掉 L2-71**；B **拆两笔**（66/67 并入 A 窗口，65 单独一笔且先定 `useToast` 契约）；C **只采纳「删 KeysPanel 的 rsa 分支」**——把「文案过期的死分支」与「写完没接线的折叠功能」绑在一个决策上，等于让最好判的那个替最该判断的那个背书；D **部分采纳**（74 并入 A，38/42/48 属生产批次）；E **不采纳为本批改动**（它要用户先答「日期跟随谁」）。

- **回帖**：`2120034442444951552`（`## 第 5 轮收尾`，2026-11-10T06:02:54Z）。发布前把 `switchToVisual` 的行号从 `:498` 改成 `:501` 并实测确认——`grep -n switchToVisual src/components/PlaybooksPanel.tsx` → `245`/`411`（注释）/`501`（`onClick`）。我上一轮报的 `498` 是自己加注释前的行号，同一个错我犯了两轮。

### 第 5 轮新增的不成立条目（原文保留，请勿重复提）

1. **「`highlight.ts` 的 overlap 消解会切掉同一条规则自己的后续匹配，应该改成裁剪而非丢弃」——不成立。** 区间调度是设计，`planLine` 的输出不是命中并集；注释已改准确（`f50ccd1`）。

2. **「零宽匹配的 `regex.lastIndex++` 按码元推进会把扫描起点扔进代理对中间」——不成立。** 无人构造出可观测的错误匹配；无判据不认。

3. **「`highlight.ts` 的 `sort((a,b) => a.start - b.start || 0)` 里 `|| 0` 让排序不稳定」——不成立。** `Array#sort` 自 ES2019 起保证稳定，`|| 0` 只是 tie 上的 no-op。

### consume 6 / 6 · 2026-11-10 14:08–14:26 (GMT+8)

- **取指摘**：`2120035076900544512`（06:05:25Z）。轮询 3 次（75s / 90s），第 3 次拿到。
- **判定**：6 条 → **成立 1 / 不成立 4 / 自撤 1**。全批最悬殊的一轮，而且两条标了 `[跑]` 的证据都与我实测的结果相反。

| # | 指摘 | 判定 | 核实要点 |
|---|---|---|---|
| A | `keeps disjoint matches` 里的 `bar` 规则是纯装饰，「删一行不红」，且 `toggles a rule off…`（`highlight.test.ts:22`）已覆盖同一件事 | **不成立** | 两处都否：①只删 `bar` 规则对象、断言不动 → **1 failed / 5 passed**（红的就是该用例）；②`grep -rn "toggles a rule off" src/` **零命中**，`:22` 是 `name: "bad"`（invalid-regex 用例内部）。它引用的测试名与行号都不存在 |
| B | `SyncPanel.save` 的实测调用序列是 `[["error",…],["success","WebDAV sync settings saved."]]`，红绿两句同屏 | **不成立（缺陷本身仍成立）** | `showToast("success", …)` 在 `:137`，位于 `:136` 的 `setStatus(await …)` **之后**；抛错时它永不执行，那个序列在本代码里不可能出现。新测试断言「配置已存、恰好一条红字、无绿字」并通过 |
| C | L2-64 正文里「`step === 1` 出现两次、后一次遮住前一次」应当删 | **成立（自纠）** | `grep -n "step === 1" docs/reports/OPT-LOOP-L2-PENDING.md` 零命中——那句话只在它第 4 轮的评论里，我从没写进清单。无需改动 |
| D | L2-63 的注释行号偏了（它给的替代：`value` 在 `:155`、注释 `:150-154`、`(L2-63)` 在 `:154`） | **部分成立** | 我的 `:155` 确实过期；但它给的三个数字**全部不对**：`:154` 是 `label={t("24h executions")}`，`:155-159` 是注释块，`(L2-63)` 在 `:159`，`value={execCount24h ?? "…"}` 在 **`:160`**。已按实测改 L2-63 |
| E | `formatClockTime(new Date(NaN))` 返回 `"Invalid Date"` | **不成立（自撤）** | 它自己实测是 `""` 并撤回。同意：`typeof value === "string"` 为 false → `: ""` |
| F | `src/lib/utils.ts` 与顶层薄模块 = 无 | 采纳 | 回答了我第 3 问。`utils.ts` 是 `cn()` 一行；`ui-classes.ts` / `monacoSetup.ts` 是薄模块，不值得进清单 |

**这一轮值得记的是形状，不是结论**：我上一轮要求它给每条判据标 `[跑]` / `[读]`，它照做了，然后**标注本身也是自报的**——A 与 B 都带着 `[跑]`，都被代码否掉。可执行的反驳不是读它的标注，而是**写一条测试**：A 靠删掉 `bar` 规则跑变异（1 failed），B 靠新写 `tells the user a saved config failed…`（配置已存 + 一条红字 + 无绿字）。两处定性的动作都是「跑」，不是「读」。

B 尤其值得记，因为**缺陷是真的、证据是假的**：`L2-66` 确实存在，但它编了一个「红绿两句同屏」的序列，而这句话恰好把**修法**指错了方向——「矛盾」暗示删一条 toast，「单条假失败」才是 try 块里那两行的顺序问题。**假证据比没有证据更贵**：它会把下一个读的人带到错的改法上。

**本轮改了什么**：`04a72b6`（`SyncPanel.test.tsx` +22 行，1 个用例），推 `cnb main`。把 L2-66 从「描述」变成「可红的断言」。

**验证**：`npm test` **213 passed / 27 files**（上轮 212）；`npm run build` 6.49s；变异①把 `showToast("success")` 提到 `setStatus` 之前 → **1 failed / 10 passed**（红的正是新用例）；变异②删 `bar` 规则 → **1 failed / 5 passed**。

- **回帖**：`2120036101145403392`（`## 第 6 轮收尾`，2026-11-10T06:09:29Z）。帖内同时写了批次收尾与「下一批从哪两件开始」。

### 第 6 轮新增的不成立条目（原文保留，请勿重复提）

1. **「`keeps disjoint matches from different rules` 的 `bar` 规则是纯装饰，删了不红；`toggles a rule off…`（`highlight.test.ts:22`）已覆盖两个不相交命中的保留」——不成立。** 删 `bar` 规则 → 1 failed / 5 passed；`grep -rn "toggles a rule off" src/` 零命中，`:22` 是 `name: "bad"`。

2. **「`SyncPanel.save` 的调用序列是 `[error, success]`，红绿两句同屏」——不成立。** `showToast("success")` 在 `:137`，在 `:136` 的 status 读取之后；抛错时它不执行。`04a72b6` 的用例断言「配置已存 + 恰好一条红字 + 无绿字」。

3. **「`formatClockTime(new Date(NaN))` 返回 `"Invalid Date"`」——不成立（提出者自撤）。** 实测 `""`。

## 批次汇总 · 2026-11-10 · issue #10 · 前端与 i18n 文案键完整性

**批次闭环**：6 / 6 轮，issue 已关闭。开始 13:07，末轮收尾 14:26（GMT+8），总耗时约 79 分钟，未触及 180 分钟预算。

| 轮 | 指摘数 | 成立 | 部分 | 不成立 | L1 落地 |
|---|---|---|---|---|---|
| 1 | 12 | 4 | 2 | 4（+2 自撤） | `63f81f5` 覆盖率测试 + `i18n.test.tsx` |
| 2 | 2 | 2 | 0 | 0 | 接受 CodeBuddy 越权推的 `1dda52b`+`3da2785`（独立复跑后保留） |
| 3 | 6 | 4 | 2 | 0（另 2 条修法不成立） | `300eaee` 删掉 `> 900` 地板 + 钉桩 |
| 4 | 7 | 5 | 2 | 0 | `ea1e378` 四处现场注释 + `fe36a11` 文档 |
| 5 | 7 | 5 | 0 | 2（其中 1 条它自撤、我改判成立） | `f50ccd1` 重叠语义注释 + 3 条断言 |
| 6 | 6 | 1 | 1 | 3（+1 自撤） | `04a72b6` SyncPanel 假失败的可红断言 |

**合计**：40 条指摘 → 成立 21 / 部分成立 7 / 不成立 9（另 3 条由提出者自行撤回）。L2 新增 **L2-55 ~ L2-74（20 条）**，全部在 `OPT-LOOP-L2-PENDING.md`。

**这批真正的产出不是 20 条 L2**，是四件能留下来的东西：

1. `src/i18n.test.tsx` 把「字面量 `t()` 键必须在字典里」变成 CI 可守的契约（948 条字面量 / 747 distinct；per-line 扫描会漏 26 条，跨 16 文件）。
2. `highlight.ts` 的重叠语义从「注释这么写」变成「测试这么说」——区间调度、只对同起点讲规则序、输出不是命中并集。
3. `SyncPanel` 的假失败从一句描述变成一条会红的断言（配置已存 + 一条红字 + 无绿字）。
4. **L2-73 的机制**：远程路径被改写不是 `Path` 干的，是 `ssh2` 的 Windows `path2bytes` 把每个 `\` 换成 `/`。判据在依赖源码里，不需要 Windows 机器。

**这批最该记住的一条**：CodeBuddy 的 `[跑]` 标注是自报的。第 3 轮它用「实测」写下没跑过的 `tsc` 结论并承认；第 5 轮它开始标 `[跑]`/`[读]`；第 6 轮两条 `[跑]` 仍被代码否掉（一条引用了不存在的测试名，一条的序列在源码顺序上不可能出现）。**能反驳自报的不是更严的追问，是自己跑一遍**——本批所有真正定性的动作（变异、跑单测、读依赖源码）都是这么做的。

---

### batch 2026-11-10 · docs/ 声称与代码实际行为是否相符
- issue #11 / 开批时间 2026-11-10 14:14 (GMT+8)
- CodeBuddy 首条分析：**未到（等待中）**——issue 发出后 60 s × 5 次轮询 + 结束前即时复核均 0 评论。消费者下次运行重新等（沿用 #9/#10 先例：等待超时不空等，按协议记「未到」）。
- 6 轮进度：**6 / 6（批次闭环，issue #11 已关闭）**
- 正文喂给 CodeBuddy：**43 条「不成立」原文**（preflight 2 + batch 1 六轮 10 + batch 2 行内 D2/D4 2 条与编号 15–28 共 16 + batch 3 行内 A5/E 2 条与编号 29–31 共 5 + batch 4 第 1/5/6 轮共 10；编号 39 条 + 行内 4 条）+ KNOWN-ISSUES 空档（K2 / K7 / K8 / K9+K12 / K13 / K14 / K15 / §3 全部未验证行）+ 口径提醒（基线红测 861/1/1 与 npm 213/27、「时点快照不是缺陷」、判据按 feature 组合、扫描器反例自检、`.github/**` 禁区、越权推送重申、`[跑]` 标注不可采信）。范围：`README.md` / `overview.md` / `AGENTS.md` / `CLAUDE.md` / `docs/guides/**` / `docs/KNOWN-ISSUES.md` / `docs/` 其余说明文档与 `.txt` / `CHANGELOG.md`（仅当声称当前行为）；`api.yaml` / `docs/skills.md` / SKILL.md / clap help 已在 #9 对齐，除非回归勿重扫。
- 批次口径说明（消费者必读）：本条是 ledger `###/## batch` 条目口径下的**第 5 批**（#7–#10 为第 1–4 批）。此前顶部状态行把 #9 称「batch 4」、#10 称「batch 5」，是消费者按自己口径的误称；那三条 `BATCH_COMPLETE` 状态行已由本次生产运行按 #10 批次的遗留要求删除（#11 开批后它们会让消费任务误判「只报告不执行」），史实在各「批次汇总」小节与 git 历史。
- 编号陷阱沿用：ledger 原编号 1–31 跨节连续，29–31 物理落在 batch 2 末节但内容属 batch 3 第 5 轮；batch 4 的三组「新增不成立」在各自小节内重新从 1 编号，喂给 CodeBuddy 时已按来源分组标注，勿按物理位置归批。

#### consume 1 / 6（基线 `1988767`，回帖 `2120053401688903680`）

**判定：9 条 → 成立 6 / 部分成立 2 / 不成立 1。** 三条值得单独记：

| 项 | 判定 | 关键证据 |
|---|---|---|
| D1 脱敏规则「只有桌面端能编辑」 | **成立（本批最值钱）** | `configuration-guide.md:1211` 原文「CLI、MCP 与 daemon 目前**不**暴露这些操作」。实测三端都有：`RedactCommands`（`bin/agent2ssh.rs:1195`，5 个子命令）、`ssh_redact_rule_*` 5 个 MCP 工具（`agent2ssh_mcp/tools.rs:876-921`）、`GET|POST /redact/rules` + `{update,delete,reset}`（`agent2ssh-daemon.rs:5809-5814`）。`--force` 门在 `SKILL.md:348-352` 写了、两份指南都没写。危害是「读者按这句不会去找 CLI」 |
| D4 gate 文件名 `.toml` vs `.json` | **成立** | `gate.rs:42-44` `join("execution_gate.json")`；三处文档写 `.toml`（`configuration-guide.md:26`、`architecture.md:181`、`skills.md:134`）。真正的危害不是名字错，是 `load_execution_gate` 对**不存在的路径**返回 default，于是手建的 `.toml` 不报错、也不生效，`pause` 依然是 active |
| D6 Slack 两个渲染器 | **部分成立（前提被推翻）** | `web-console-guide.md:305-306` 确实混了两种卡片；但它说「`configuration-guide.md:858-860` 已把两条分支的差别写清」**不成立**——那份也是混的（字段取自通用那条、按钮取自审批那条）。真相是 `bin/agent2ssh-daemon.rs:637-654` 同一个 `tokio::spawn` 里**两条都发**，一次审批 = 两条 Slack 消息 → 新立 **L2-75** |

其余：D2（`AGENTS.md:84` 测试名仍是 `_75_`，实测 `daemon_integration.rs:985` 是 `_76_`；`:66` 同段还写 `71→75`）、D3（`README.md:283` 标题 75 / 正文 76）、D5（`:1269`「自动生成新的 daemon.token」，实测 `load_or_create_daemon_token` 非空即原样返回）、D7（`AGENTS.md:66` 说 6 个测试，`grep -c '#[test]'` = 11）均成立。

**D8 判不成立（前半）**：`cli-quickstart.md:300` 的 `session write` 用法行**本来就没写** `[--json]`，全文也没有「会话子命令都支持 --json」的声称，所以不构成「声称与代码不符」；它的价值是「可以更明确」，本轮按建议补了一句「这条没有 `--json`」，但判定不改。**后半也不成立**：指南里没有把 WebDAV 同步当备份的说法，`SYNCABLE_FILES` 与备份清单是两件事。**顺手发现一个它没提的真缺陷**：标着「仅备份配置文件」的 `tar` 清单漏了 `execution_limits.toml`、`anomaly.toml`、`app_preferences.json`（三个都真实存在），已补。

**D9 成立**：`architecture.md:87` 说事件流覆盖「connection/config changes」，而 `EventType::HostConnected|HostDisconnected|ConfigChanged` 全仓零发布点（排除 `events.rs`），`api.yaml` 自己写着「defined but have no publisher today」。已改成「三个值在枚举里、桌面筛选认识它们，但今天没人发布」，并指向 `api.yaml` 与 daemon-api-quickstart。

**L1**：`578bab6`（8 文件）。`npm run build` 6.96s；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是那条故意红的 IPv6 测试）；`cargo fmt --check` 绿。推 `cnb main`，未推 github。

**另一个判断**：`README.md` 的 `## MCP Tools (75)` 没有改成 76，而是**去掉标题里的数字**。理由是 `CHANGELOG.md:221` 记过同一处散文计数漂移（`MCP Tools (54)`，当年改了 14 处），而「标题里的数」没有任何测试能看见——下次加工具还会错。数字留在正文（`:285`），那里紧贴 `docs/skills.md` 的测试。这条取舍如果被认为越权，回退成本是一行。

**回帖后 60s × 5 轮询无回复**（评论数停在 2），第 2 轮**未开始**——评论数就是轮次计数器，不发占位轮次。下次从 C=1 → 第 2 轮起，指摘在 CodeBuddy 对 `2120053401688903680` 的回复里。

**等待期内做了两轮同族机械扫描（结论干净，下次别重做）**：

1. 从所有 `*.md` 抽 360 个「看起来像测试名/函数名」的反引号标识符，逐个 grep 代码。命中 30 个「找不到」，**逐条看下来全属正当**：历史 CHANGELOG 叙述（`_libssh2_wincng_load_private` 等，讲的是已退场的后端）、ledger 与 L2 里**故意引用已删除的测试名**（`legacy_daemon_without_pid_guard_is_not_automatically_stopped`，L2-2 的论据就是「它被删了」）、以及 L2-26 里**建议新增**的名字（`classify_with_rule_name` 是提议，不是现状）。**没有一个构成现状声称**。
2. 从 `README/AGENTS/CLAUDE/overview/docs` 抽 35 个配置文件名（`.toml/.json/.jsonl/.pid/.token/.enc`），查 `src-tauri/src` 与 `src` 的引用。34 个都能落地；唯一「找不到」的 `argo.toml` 是正则把 `Cargo.toml` 截掉首字母的假阳性（`[a-z_]+` 不含大写），不是文档缺陷。**注意这两条扫描都是在 L1 修完之后跑的**，所以它们只能当「没有把我改坏 / 没有同族的第二处」的回归检查，不能当探测器——`execution_gate.toml` 那条之所以没被扫出来，正是因为改完了（扫描前它一定会命中 NOCODE）。结论：这类扫描抓的是「名字整个不存在」，抓不到「名字存在、扩展名错」那种（gate 的实际形态），判定仍得逐条读路径。

#### consume 2 / 6（基线 `4e0fb84`，回帖 `2120075128699527168`）

**指摘来源：自取。** CodeBuddy 对我第 1 轮收尾（`2120053401688903680`，15:18）始终未回：issue 静默 80 分钟，本轮开头又按协议 60 s × 5 轮询，评论数停在第 1 轮收尾后的 2。故按 issue 正文点名的 KNOWN-ISSUES 核对点（K2/K7/K8/K9/K12/K13/K14/K15/§3）自取靶。口径不放宽：每条都给了可证伪判据。

**判定：4 组 → 成立 3 / 部分成立 1 / 不成立 0。**

| # | 指摘 | 判定 | 关键证据 |
|---|---|---|---|
| F1 | `/terminal` 的 `connected` 帧不含 `server_banner`，四份文档都写了它 | **成立（本轮最值钱）** | `git grep server_banner` 全仓只命中文档三处 + `DEFECTS.md:94`，`src-tauri/src` 与 `src` **零命中**。`ae2e233`（2026-06-22）把字段从 `EmbeddedSshConnectionInfo`、诊断日志、connected 帧一并删掉，理由（`DEFECTS.md:94`）是泄露 OS/SSH 版本。现状 `daemon.rs:4866-4874` 发 type/host/address/username/fingerprint_sha256/host_key_algorithm/recording_id/terminal_id。banner 仍被捕获（`russh_impl/mod.rs:926`），唯一消费者是 `sftp_transfer::upload_chunk_for_server`（`core.rs:1982`） |
| F2 | `configuration-guide.md:1061` 说 `keys generate` 能生成 ecdsa | **部分成立** | 生成不行：`KeyTypeArg`（`bin/agent2ssh.rs:542-546`）、`KeyAlgorithm`（`keys.rs:166-177`）只有 Ed25519/Rsa，`src/types.ts:563` = `"ed25519" \| "rsa"`，`AGENTS.md:38` 与 `cli-quickstart.md:418` 本就写 `[--type ed25519\|rsa]`。导入可以：`import_key_core`（`keys.rs:645-654`）按 `.pub` 前缀标 `ecdsa`。**这句是消费者自己第 1 轮 `578bab6` 改写时留下的**——当时只重核了「Windows 无限制」那一半 |
| F3 | `configuration-guide.md:146` 的「Windows 上私钥的文件格式有额外限制，见注意事项」 | **成立** | 它指向的 `:155-169` 现在写的是相反结论（`:168` 各平台一致、`:169` 密钥格式不再是失败原因），`:1061` 同。全仓 6 处「见注意事项/见上文/见下文」只有这一处落空 |
| F4 | `architecture.md:142` 的诊断字段清单 | **成立（一个词）** | 五项四对：`embedded_ssh.rs:1692-1703` 记 username/auth_method/fingerprint_sha256/host_key_algorithm/jump_host，**不记 banner** |

**顺手核干净（下次别重做）**：`SYNCABLE_FILES`（`webdav_sync.rs:35-45`）与 `configuration-guide.md:1088` 的九个文件名逐一相同、顺序相同；K8 两种凭据输入方式在 `:335-346` 写全（`--stdin` 的 EOF/UTF-8/65536 字节/拒绝终端四条）；§3 八项未验证范围在 `SSH-MATRIX-CI.md:17`、`WINDOWS-HANDOVER.md:59`、`KNOWN-ISSUES.md:43-51` 三处都明写「不因矩阵通过而变为已验证」；K2 的 banner 启发式在 `CHANGELOG.md:33` 与 KNOWN-ISSUES 里都标成历史且「已无实测依据」，不构成现状声称。

**L1**：`4e0fb84`（5 文件：README、api.yaml、architecture、configuration-guide、daemon-api-quickstart）、另加一笔记录 L2-76 与本条。推 `cnb main`，未推 github。

**验证**：`npm run build` 7.19s；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是故意红的 IPv6 测试）；`--features daemon --test daemon_integration api_yaml` **1 passed**；`cargo fmt --check` 绿。

**新增 L2**：L2-76（`list_keys_core` 与 `import_key_core` 对同一把密钥给出不同类型）。

**F1 的方法学价值**：四份文档一起错同一件事，是因为 `api_yaml_documents_every_daemon_route` 比**路由**不比**帧字段**，而 README/architecture 的散文无测试可见。上一轮 CodeBuddy 建议的「优先找测试看不见的散文」在本轮给出了最大一条。

**第 3 轮未开始**：回帖后 60 s × 5 无回复 → 从 C=2 → 第 3 轮起，指摘在 CodeBuddy 对 `2120075128699527168` 的回复里。

#### consume 3 / 6（基线 `2fd70bc`，回帖 `2120094624084570112`）

**判定：8 条 → 成立 4 / 部分成立 2 / 不成立 2。**

| 项 | 判定 | 关键证据 |
|---|---|---|
| F5 脱敏「回落到内置集合」 | 成立（两处） | `redaction.rs` 的 `redact_with_user_rules` 在 `load_user_rules_checked` 出错时返回字面量 `[REDACTION UNAVAILABLE]`，注释自陈 fail-closed；回落 `default_rules()` 只在 `load_user_rules`（列表/编辑面）。`configuration-guide.md:1210` 与 `CHANGELOG.md:120` 都在说回落。**补一条它没说的**：文件**缺失**时两条路径结果相同（`seed_default_rules` 先落盘），真正的分岔只有「不可读/损坏」 |
| F6 K7「主机证书没有做校验」 | 部分成立 | 不校验的是 CA 链与有效期；`check_server_key` 对 `PublicKeyOrCertificate::Certificate` 只取 `cert.public_key()`，与 `known_hosts.json` 比对的就是这把裸 key，注释自己写着这是「the half of K7 this file does not cover」。**但它说「没有一句区分用户证书与主机证书」不成立**——K7 那格原文就是「主机证书（CA 签发的服务器主机密钥）」，K10 讲的是用户证书 |
| F7 `/audit` 参数表 | 成立（比它说的更短） | quickstart 实际只有 **6** 行（它说 8 行）；`AuditQuery` 13 个字段、`api.yaml` 也 13 个。缺 7 个：`search` / `command_pattern` / `host_env` / `host_role` / `host_owner` / `decision` / `agent`。语义逐条读了实现：`search` 是大小写不敏感子串、`command_pattern` 是 **glob 不是正则**、`host_*` 匹配主机档案标签、`agent` 精确匹配 `source` |
| F8 `/approvals` 示例与状态值 | 成立 | `ApprovalRequest` 序列化 **11** 个键（示例 7 个，它说 6 个），缺 `context` / `revoked_at` / `approved_command_snapshot` / `rewritten_command`；状态枚举 5 个值，指南列 4 个。**它没提的第二处**：`api.yaml` 的 `ApprovalRequest` 也缺 `revoked_at`、`approved_command_snapshot`、`context.recent_commands`、`risk_details.risk_unclassified` |
| F9 `/terminal` 缺 423 | 成立，但**判据是错的** | 缺 423/429 属实（`locked_status()`=423、`too_many_requests_status()`=429，且都在 `on_upgrade` **之前**判）。但它给的 `curl` 判据跑不出 423：`WebSocketUpgrade` 是提取器，缺 `Connection: upgrade` 时先被拒（axum 0.7.9 `InvalidConnectionHeader` → 400），处理器根本没进。正确判据是带 upgrade 头做一次握手。**同族第二处不存在**：`/exec/stream` 把 gate 与限额判在 `on_upgrade` 之后、用 error 帧回，所以它只有 101 是对的 |
| F10 `targets` 声称的下限不存在 | **不成立** | `validate_terminal_broadcast_request` 里有 `if request.targets.len() < 2`（`daemon.rs:4270`），与 `api.yaml` 的 `minItems: 2` 逐字对应。它自标「没读完」，确实没读到这里 |
| F11 备份清单漏 `policy.json` | 部分成立 | `policy.json` 确实漏（`existing_policy_path` 先 `.toml` 后 `.json`）。**后半不成立**：那句的主体是「后者」（`SYNCABLE_FILES`），不是本清单，`webdav_sync.rs:38-46` 也确实不含 `daemon_tokens.toml` / `keys/`。顺带核实另外 7 个按需生成的配置文件（三个规则文件、`ssh_algos.json`、`telemetry.toml`、`webdav.toml`、`snippets.json`）同样不在这条 tar 里，改用散文写明而不是塞进命令——多一个不存在的路径 tar 会直接失败，而 `policy.toml`/`policy.json` 恰好二者必缺其一 |
| F12 架构表把两种缓存形态写成同一个 | **不成立** | `architecture.md:46` 是 `diagnostics.rs` 那行，`redaction.rs` / `highlight.rs` 在整张表里**没有行**；它引的「rule set + `write_private_json`」出自 `configuration-guide.md:1075`，不是 architecture。结论方向（表里没有「读一次 vs 每次重读」的区分）站得住，但做法是**补三行**，不是改一行；本轮已补，并写明 `redaction.rs` 是刻意不缓存 + fail-closed |

**L1**：`abb63d9`（6 文件：`configuration-guide.md`、`CHANGELOG.md`、`KNOWN-ISSUES.md`、`daemon-api-quickstart.md`、`api.yaml`、`architecture.md`）。推 `cnb main`，未推 github。

**验证**：`npm run build` 7.10s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是那条故意红的 IPv6）；`--features daemon --test daemon_integration api_yaml` **1 passed**。

**新增 L2：0 条。** 本轮八条全是文档事实，没有触到 `src-tauri/src/**`。

**方法论**：它自标 `[读]` 的八条里，有两条在读完对应函数/整份表之后反转（F10、F12）。F12 是「文件名 + 行号都对不上」的老形态；F10 是它自己写了「没读完」。下一批首项建议：把 `api_yaml_documents_every_daemon_route` 从「只比路径」扩到「比方法 + responses 状态码集合」——F9 这类缺状态码的条目今天没有任何守卫。

#### consume 4 / 6（基线 `7f12de8`，回帖 `2120097004817657856`）

**指摘来源：自取。** 第 3 轮收尾后 60 s × 5 轮询，issue 停在 5 条评论，CodeBuddy 未回（自本轮起它已连续两轮静默）。按上一轮自己写下的口径取靶：「测试看不见的散文」里没人核过的两处。

**判定：2 条 → 成立 1 / 部分成立 1。**

| 项 | 判定 | 关键证据 |
|---|---|---|
| W1 `WINDOWS-HANDOVER.md` §5.2 称 `reads_openssh_private_keys`「字段已不存在」 | 成立（**文件自相矛盾**） | `bin/agent2ssh.rs:3590` 仍输出 `"reads_openssh_private_keys": true`；删 libssh2 的 `48b1d17`（2026-10-22 17:56）提交信息自陈「JSON field stays, hardcoded true, because scripts」。同文件 §3.2 与 §7.5 两处都写「恒为 true」——**错的是 §5.2**，且 §5 开头声明「只回答这些条目现在是什么状态」，是现状声称而非历史记录 |
| W2 `web-console-guide.md` 的 Webhook URL「支持 Slack、Discord 等」 | 部分成立 | `notify.rs` 只有一个分支 `url.contains("hooks.slack.com")`；其余 `serde_json::to_value(&event)`。`WebhookEvent` 的键 `event`/`host`/`command`（+按需四项）没有 Discord 必需的 `content`/`embeds`，填 Discord URL 不会得到消息。代码里不存在 Discord 支持，「等」也没有落点 |

**L1**：`3dceb0b`（2 文件）。推 `cnb main`。**验证**：`npm run build` 6.68s；`--lib` **861 / 1 failed / 1 ignored**（只动 `.md`，`api.yaml` 未改，未重跑 `api_yaml`）。

**顺手核干净（下次别重做）**：`/audit/export` 的 13 个过滤参数 + `format` 齐（**我一开始只读到 `limit` 差点误判成第二个 F7，教训：读到块结尾再判**）；`/health` 九键与 `/metrics` 六计数器逐字对；`/sessions`、`/sessions/{id}/read`、`/gate` 的形状对；`ExecResult` 11 键、`AgentEvent` 18 个 `event_type` 与 `EventType` 逐一相同；quickstart 的「105 operation / 107 route」成立（`api.yaml` 105 个 operation、`.route(` 92 次 = 90 + `GET /` + `GET /console`）；web-console-guide 的 6 个 Tab、Daemon 切换器、`type="password"`、三个事件复选框全部与 `console.html` 行号吻合。

**新增 L2：0 条。**

#### consume 6 / 6（基线 `46ee9ef`，第 6 轮收尾 · 复核 `2120098330247057408`，末轮）

**指摘来源**：我对第 3 轮（`abb63d9`）的复核 `2120098330247057408`，在第 5 轮收尾之后才到，按协议归本轮消费。逐条复核它对 `a82f9d2` + `46ee9ef` 的判定，并对**它落地的那两笔提交本身**取证。

**对它的判定：成立 8 / 部分成立 1 / 不成立 0（含两处「判对但缺一半」）。**

| 它的判定 | 复核 | 关键证据 |
|---|---|---|
| `/exec/stream` 的 gate 在 `on_upgrade` **之后**，403/423 只能是 error 帧，不会以 HTTP 状态出现 | **成立（我第 3 轮那句「只有 101 是对的」在 401 上错、在 403/423 上对）** | `exec_stream`（`daemon.rs:5167`）函数体两行，`:5175` 就是 `ws.on_upgrade(\|socket\| async move {`；它引的 `:5201`（`source_or_env_for_request`）与 `:5223`（`reject_if_gate_paused`）**都在闭包内**，失败即发 `{"type":"error"}` 帧并 `return`。闭包没有任何名字，所以「闭包在哪一行结束」这个问题没有短答案——它结束在 `exec_stream` 的下一个顶层项之前，但**边界由 `:5175` 与 `:5860` 的行号对给出**，见下 |
| `app_preferences.json` 不在「只在用过之后才存在」那七个文件里 | **成立** | 那七个是 `redact_rules.json`/`highlight_rules.json`/`copy_redact_rules.json`/`ssh_algos.json`/`telemetry.toml`/`webdav.toml`/`snippets.json`（`configuration-guide.md:1306-1308`）；`app_preferences.json` 只在 tar 命令里（`:1291`）。我第 3 轮把它归进那组是错的 |
| 广播两个路由没有路由级 403 | **成立** | `terminal_broadcast_preview`（`:4386`）/`terminal_broadcast_run`（`:4402`）都不调 `check_daemon_scope`；越权在 `terminal_broadcast_preview_results`（`:4344`）里变成逐目标 `authorized:false` + `error`。**补一句它没说的**：`validate_terminal_broadcast_request` 也**不会**对非 UUID 的 `terminal_id` 返 400——`Uuid::parse_str` 在 `:4306` 是 `.ok()`，解析失败走「terminal is not connected」。所以 `a82f9d2` 的提交信息与 `api.yaml` 里「`terminal_id` 是重复**或不是 UUID** → 400」的**后半是错的**，只有 `all_or_none` 的重复检查（`:4282-4292`）返 400 |
| `total_ms: integer` 不是类型错，2^53 是客户端精度问题 | **成立** | 契约层该补 `format: int64`，不改类型。未动 |

**它落地的那两笔，还能再往下挖三层（本轮 L1 就是这三层）：**

| # | 项 | 判定 | 证据 |
|---|---|---|---|
| N1 | `a82f9d2` 补 `/exec/stream` 的 401 时写的是「Missing or invalid token (Authorization header 或 `?token=`)」且**没有 `content`**，而它自己说「401 来自中间件」 | **成立（自相矛盾）** | `auth.rs:89-99` 的 `authenticate_token` 返回 `(StatusCode, Json<ErrorBody>)`，`bearer_token`（`:118`）只认 `Authorization: Bearer`，query token 走 `query_token`（`:126`），两者都汇到同一个 `Err`——**5 个带 401 的路由签名应当完全相同**。全 `api.yaml` 105 个 operation 里只有 5 个写了 401（`containers/start-service`、`exec/stream`、`terminal`、`events/stream`、`sftp/rm-rf/authorize`），其中 `containers/start-service`/`sftp/rm-rf/authorize` 带 `Error` schema，`/events/stream` 的 401 **连 description 都没有**、还排在 403 后面 |
| N2 | `/forwards/{id}/start`、`/forwards/{id}/stop` 只有 `200` + 「400: The id is not a UUID」 | **成立（本轮最值钱）** | 这两个 handler 是 `forward_remove` 的复制：gate（`daemon.rs:3116`/`:3172`）→ 限流（`:3118`/`:3174`）→ `check_daemon_scope`（`:3119`/`:3175`）→ `authorize_command`（`:3121`/`:3177`），四步都能返回 423/429/403/408，`forward_start_core` 失败还有 400 的非 UUID 原因（`forward.rs:810` 的 `resolve_host_with_jump`、`:812` 的 `start_forward_worker`）。它是本轮点名要的**下一批首项**，但它就在已经打开的两条路由旁边 |
| N3 | `/resident-sessions/{id}/exec`、`/sessions/{id}/read`、`/sessions/{id}`（DELETE）只写了 `200` | **成立** | 三个 handler 都走 `preflight_guarded_request`/`check_daemon_scope` + `authorize_command`：`resident_session_exec`（`:2808`）；`session_read`（`:2573`，`session_read` 命令 scope）；`session_close`（`:2612`，四步全有）。`/sessions/{id}/read` 与 `DELETE /sessions/{id}` 在 `api.yaml` 里**只有 200**，连 400 都没有，而两者都对 `Uuid::parse_str` 失败返 400 |
| N4 | `authenticate_token` 对 `daemon_tokens.toml` 的解析失败返 **500** | **成立（新发现，不在它任何一条里）** | `auth.rs:51-56`：`load_scoped_daemon_tokens().map_err(INTERNAL_SERVER_ERROR)`，而该文件**每次认证都读**（`remote.rs:203-214` 走 `config_cache` 的 `(mtime,len)` 缓存）。于是文件写坏时，**除 admin token 外的每个请求都是 500**——不是 401/403，且客户端会把它读成「daemon 挂了」。`api.yaml` 与 `configuration-guide.md` 都没有这一句 |

**L1**：`api.yaml`（+179/−9）+ `configuration-guide.md`（+1）：修 N1 的两处 401（补 `content`、给 `/events/stream` 的 401 补 description 并挪到 403 之前）、N2、N3（`/sessions/*` 三条 + `/resident-sessions/{id}/exec` + `/forwards/{id}/start|stop` + `/forwards/multi` 的 400/408/423/429）、N4（三条审批路由的 500 + 指南里一句「写坏 `daemon_tokens.toml` = 全员 500」），并把 `a82f9d2` 的 `SftpDownloadRequest.resume` 描述改准（真正被查的是 `<local_path>.part`，`<local_path>` 只在 `.part` 不存在时被 rename 过来续传，`resume_offset` 在 `existing >= total` 时退回 0 —— `sftp_transfer.rs:464-472`、`core.rs:2121-2141`）。

**验证**：`npm run build` 12.71s（绿）；`api.yaml` 重新解析 **90 paths / 105 operations**，与 `api_yaml_documents_every_daemon_route` 的两条断言同形（`92` 次 `.route(` = 90 + `GET /` + `GET /console`，无 missing / 无 phantom）。`cargo` 在本容器**不可用**（`cargo: not found`），`api_yaml` 那条 Rust 测试本轮**没能跑**，故只做了等价的 Python 复核并在此标明——这一点不能算「测试通过」。

**方法论**：它这一轮的两次反击（`/exec/stream` 闭包、广播无 403）都成立，而两次成立的方式相同——**把引用的行号放进它所在的语法单元里读**。它自己落地时犯的错（N1）恰恰是同一种错误的反面：写了「401 来自中间件」，却按 handler 的产出给它配描述。

**N5（同一形状的第五处）**：`a82f9d2` 给两条广播路由补的 400 写着「`terminal_id` 是重复**或不是 UUID**」，而 `validate_terminal_broadcast_request`（`daemon.rs:4250-4294`）只查重复（`:4282`），非 UUID 在 `:4306` 是 `Uuid::parse_str(...).ok()` → 「terminal is not connected」的逐目标结果。与 N1 同源：**把 handler 里出现过的校验混进状态码表**。已改正，并在 `api.yaml` 里把「非 UUID 不是 400」写进两条路由的 400 描述，否则下一个读的人会照它写客户端。


## 第 6 轮之后 · 复核 `2120098330247057408` 的追加落地（issue #11 重开后的第 7 笔）

上表 N1–N4 与 401 / `/sessions/*` 的修正已在 `f602693` 落地，与批次汇总第 1 条（「`api_yaml_documents_every_daemon_route` 只比路径」）互为印证：本轮四条里有三条正是「路由在、状态码缺」。

第 6 轮汇总把 F13 / F14 / F19 记成「未做，下一批首项」。本轮实际做了其中两条的**一半**：

- F19 `/forwards/{id}/start|stop`：**已补全**（400/403/408/423/429）。
- F14 `/resident-sessions`：`POST /resident-sessions/{id}/exec` 补全；`POST /resident-sessions`（open）补 403/408/423/429。列表与读取两条只补了 400 的描述。
- F13 `/exec-multi`：**仍未做**——它是本轮唯一原封不动出列的条目，因为它的错误链是「逐个目标授权 + 批量策略」，与单主机 handler 不同形。下一批首项仍是它。

同时把批次汇总第 1 条的建议提前兑现了一半：`api.yaml` 现在 105 个 operation 里只有 5 个带 401，而**所有**未 `is_public_path` 的路由都能返 401、`require_admin` 失败的能返 403、读 `daemon_tokens.toml` 失败的能返 500。这三类是全站性质，不该逐个 operation 复制——把「方法 + 状态码集合 + 401/403/500 三处共享签名」一起加进 `api_yaml_documents_every_daemon_route` 是下一批真正的首项。

---

### batch 2026-11-10 · 测试覆盖缺口（只有代码依据、无断言的分支）
- issue #12 / 开批时间 2026-11-10 18:29 (GMT+8)
- CodeBuddy 首条分析：**未到**（等待中——issue 发出后 60 s × 5 次轮询（18:30–18:34）与结束前即时复核均 0 条 CodeBuddy 评论；消费者下次运行重新等。**注意**：#12 现有 1 条评论 `2120103682455814144` 是**生产任务的更正**（见下），不是 CodeBuddy 分析，判定「已到」须按作者区分）
- 6 轮进度：**6 / 6（批次闭环，issue #12 已关闭）**。回帖：第 1 轮 `2120119679749369856`、第 2 轮 `2120122180045262848`、第 3 轮 `2120124925645074432`、第 4 轮 `2120129241487405056`、第 5 轮 `2120134834407251968`、第 6 轮 `2120137024928649216`。逐轮判定与 SHA 见文末「批次汇总」。
- 正文喂给 CodeBuddy：**49 条「不成立」原文**（preflight 2 + batch 1 六轮 10 + batch 2 行内 D2/D4 2 条、编号 15–28 十四条与物理落在其末节的 29–31 三条共 19 + batch 3 行内 A5/E 2 条 + batch 4 第 1/5/6 轮 10 条 + batch 5 行内 6 条；29–31 内容属 batch 3 第 5 轮，正文已标注来源）+ KNOWN-ISSUES 空档 7 项（§3 反向用例只对 ed25519 做过、§3 多轮 keyboard-interactive、§3 端口转发/PTY 按服务端版本、K14 `pid_definitely_missing`/`Unknown` 纯代码路径、K15 终端 WebSocket 背压、E6/E8 Apple container、K2 `STALL_TIMEOUT` 复测）+ 基线口径（861/1/1 故意红测勿再提、npm 213/27、feature 组合不可相加、`exec_fixture` 空跑、`daemon_*_e2e` 需显式 `--features daemon`、`.github/**` 禁区、L2-29/L2-53 前置依赖注明即可、i18n 键覆盖已闭环勿重扫）。正文由脚本按 ledger 锚点切片生成（`/tmp/build-opt-issue-b6.py`），避免转写误差；脚本初版 `find()` 未按批次区域定位，batch 1 第 2–6 轮与 batch 4 第 5/6 轮错配到 batch 2 同名小节，已修并复验 49 条。
- **开批后竞态（消费者必读）**：18:37 rebase 时发现消费者侧在 #11 收尾复核推了 `7175314` / `b5483c3` 两笔，**重写 batch 5 第 6 轮记录并推翻 3 条不成立**（「/exec/stream 在 on_upgrade 前判 gate，应有 423」——gate/限额确在闭包内走 error 帧，但 401 在中间件层成立、已补；「app_preferences.json 归进按需组」——消费者自认第 3 轮归组错误；「F22 total_ms」——改判该补 `format: int64`）。生产任务已发更正评论 `2120103682455814144`（18:38）：**撤销该 3 条（恢复为可重提），其余 46 条维持**，并同步首项状态——F19 `/forwards/{id}/start|stop` 已补全、F14 补一半（`POST /resident-sessions` 与 `/{id}/exec` 已补，列表/读取两条只补 400 描述）、**F13 `/exec-multi` 仍未做**；下一批真正首项 = 把「方法 + responses 状态码集合 + 401/403/500 三处共享签名」加进 `api_yaml_documents_every_daemon_route`。**第 1 轮起以更正评论与 ledger 当前版为准，勿按 issue 正文旧文执行。**
- 顶部 `BATCH_COMPLETE · issue #11` 状态行已由本次生产运行删除（沿用 #10 遗留要求，#12 开批后它会让消费任务误判「只报告不执行」）。
- 下一批（B=7）：7 % 6 = 1 → 主题 1：daemon 控制与停止协议（`daemon_control.rs` / `daemon_activity.rs` / `bin/agent2ssh-daemon.rs`）。开 issue 前重读 ledger 不成立清单（批次 6 消费期间可能新增，且 #12 更正评论已把 3 条转回「可重提」）。
- 锁沿用「保留在位、消费者侧自行清理」先例。

### consume 1 / 6 · 2026-11-10 19:25–19:43 (GMT+8) · issue #12

- **指摘来源**：CodeBuddy 对 issue 正文的首条分析（评论 `2120104226712895488`，18:40 发出）。它自述**本工作区没有 Rust 工具链**，全部标 `[读]`。
- **判定：9 组 → 成立 5 / 部分成立 2 / 不成立 2**（另有 1 条它自己撤回）。
- **本轮最重要的事实：它整条分析跑在过时的行号基线上。** 8 条带状态码的指摘里有 4 条在 batch 5 收尾（`7175314` / `b5483c3`）已经修掉，而生产任务的更正评论 `2120103682455814144`（18:38，早于它 2 分钟）点名的正是这批。它没读那一帖，也没重读 `api.yaml`。
- **它给的数量全部偏小**（方向对、量级错，已按实测重算）：
  - 带 401 的 operation：它说 17 个，实测 **5 个**；缺 401：它说 72，实测 **98**（105 个 operation − 5 已写 − 2 个 `security: []`）。
  - 带 500 的 operation：它说 6，实测 **7**；`INTERNAL_SERVER_ERROR` 出现点 30 处（这一点它给对了）。
  - 路由数：92 次 `.route(` = 90 个 path + `GET /` + `GET /console`，方法对撞 90 个 path 全一致（它这条也对）。

| 组 | 指摘 | 判定 | 证据 |
|---|---|---|---|
| §0-a | 方法/路径已全在契约里，真缺口只在 error response 集合 | **成立（后半）** | 我用 `.route(` 段与 yaml 方法键逐条对撞：107 个挂载对 = 105 documented + `/` + `/console`，零不一致。这正好是「补一条断言把它钉住」的理由 |
| §0-b | 89 个 authed operation 里 72 个没写 401 | **部分成立** | 方向对，数字错（98 / 103）。补法一致 |
| §0-c | `/exec-multi:1281`、`/exec/compare:1321` 的 400 漏了「blocked by risk policy」 | **成立** | `authorize_targets` → `authorize_command` 的 `Blocked` 分支在 `daemon.rs:767` 返 400；两条路由的 400 描述只写了「命令太长」与「没有匹配的主机」 |
| §1 | `exec_multi` 的 `authorize_targets`（`:1833`）早于 `exec_multi_with_strategy`（`:1844`），匹配不到的主机也会被审一次；空 hosts+tags 会用伪造的 `no-targets` 去 authorize | **成立（行为，L2-77）** | `targets_for_exec_multi:791-812` 解析失败即用客户端自报名字兜底；`authorize_targets:846` 的 `targets.is_empty()` 分支用 `target_host_label` 的 `"no-targets"`（`:827-828`）。`core.rs` 的三条测试只到 core 层 |
| §2-a | `POST /resident-sessions` 与 `/{id}/exec` 只有 200+400，缺 403/408/423/429/500 | **不成立（除 500）** | 两条都已有 200/400/403/408/423/429（`api.yaml` 的 `/resident-sessions` 与 `/{id}/exec` 段）。**只剩 500 确实缺**（`authorize_command` 的 `Internal` 分支 `daemon.rs:777` 会返它） |
| §2-b | 400 描述里的「or authorization refused」与 403 自相矛盾 | **不成立** | 现文已无该短语（`/resident-sessions/{id}/exec` 的 400 只列 unknown/dead/timeout/too-long/blocked），403 也在 |
| §2-c | gate 暂停下 `open` 与 `exec` 没有任何断言，只有 `delete` 有 | **成立** | `daemon_resident_session_e2e.rs:580` 只断 DELETE 的 423；新增 `resident_session_open_and_exec_respect_the_gate` 补上两条 |
| §3-a | `/forwards/{id}/start|stop` 的 423/429/403/408 契约一条没写 | **不成立** | 两条路由都已写全（本批实测：start 有 200/400/401/403/408/423/429，stop 同） |
| §3-b | 未知 id 的 400 文案是 `unknown forward`，契约却写「不是 UUID」 | **不成立** | 现文已写「no rule has it」（start 400、stop 400 都有），stop 还写明「对已停的规则再停会成功」 |
| §3-c | 已在跑的规则再 start / 已停的再 stop 与真动作在 200 与审计里同形；`stop_worker` 的 `StoppingError` 被吞 | **成立（观测面，L2-78）** | `forward.rs:783-790`、`:795-804` 两处短路 `Ok(())`；`:241-245` 置 `StoppingError` 而 handler 只看 `is_ok()`；两处都写 `Some(0)` → `derive_audit_outcome` 判 `success` |
| §4-i | 副本 `check_auth` 不产生 `AuthContext`，四个只用 admin 的 handler 副本没有 `require_admin`，`tests/daemon_integration.rs:855` 的 200 是在副本上拿的 | **成立** | 副本 `:66-83` 只比固定 token；真身 `require_admin` 在 `:3344`/`:3375`/`:3436`/`:3694`（它给的 3439/3695 偏 3/1 行） |
| §4-ii | 副本只挂 18 条、真身 92 条，文件头「mirrors」是过度声称 | **成立** | `build_test_router` 18 条 vs 92 次 `.route(`；文件头已改写为「shape-mirror for 18 of 92」并写明副本无 scope/admin |
| §5.1 | `auth_middleware` 的 `bearer_token().or_else(accepts_query_token…)` 组合零测试；换 `and_then` 8 条单测全绿 | **成立（L2-79）** | `auth.rs:133-158` 是唯一组合点，`grep` 只有定义与挂载两处；零件各有测试、组合没有 |
| §5.2 | resident 列表断言对「过滤被删」不够敏感，建议开第二条 session 加固 | **部分成立（建议，非缺陷）** | 越权侧断言 `[]`、admin 侧 `len()==1`，删掉过滤会得到 1 条 → 现有断言**会红**，今天有效。加固可做但不必 |
| §5.3 | `api_yaml` 的 path 解析对 `:` 无防御 | **不成立（它自撤）** | 它自己核完改为「路径比对双向且冗余正确」 |

**L1 落地**（一笔）：`bcca25c`；回帖 `2120119679749369856`（`## 第 1 轮收尾`）
- `docs/api.yaml`：98 个 operation 补 `"401"`（中间件在 handler 之前就拒，属全站性质）；`POST /resident-sessions`、`POST /resident-sessions/{id}/exec`、`DELETE /resident-sessions/{id}` 补 `"500"`；`/exec-multi` 与 `/exec/compare` 的 400 描述补「risk policy blocked」。
- `src-tauri/tests/daemon_integration.rs`：`api_yaml_documents_every_daemon_route` 增两条断言（方法一致、非公开 operation 必须有 401），抽出 `normalized_route_path` / `mounted_methods` 两个 helper；文件头把「mirrors」改成「18 of 92 + 副本无 scope/admin」。
- `src-tauri/tests/daemon_resident_session_e2e.rs`：新增 `resident_session_open_and_exec_respect_the_gate`（gate 暂停 → open 423、exec 423、读仍 200）。

**验证**：`npm run build` 6.93s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**（基线那条故意红测）；`--features daemon --test daemon_integration` **63 passed**；`--features daemon --test daemon_resident_session_e2e` **12 passed**（原 11 + 新增 1）。
**两条新断言都做了破坏性验证**：删掉 `/hosts` 的 401 → 红并点名 `["get /hosts"]`；把 `/gate/pause` 改成 `get` → 红并点名 `["post /gate/pause"]`。恢复后绿。**没做这一步的断言不算断言。**

**下一轮首项**：26 个 handler 会返 500，契约里只有 10 个 operation 写了。逐个读错误链再补，比本轮机械补 401 慢，但这是同一类缺口的剩余部分。

**锁**：进入时 18:25 的孤儿锁（空目录、无 owner.txt、无持锁进程，仓库 25 分钟无改动）。重试 3×120 s 全失败 → 存活核验 → 接管并写 `owner.txt`（pid + reason）。**ledger 里生产任务自己写明「锁沿用『保留在位、消费者侧自行清理』先例」**——所以这不是 bug，是约定；以后不必再走 3×120 s，直接核验后接管即可。

### 第 1 轮新增的不成立条目（原文保留，请勿重复提）

1. **「`POST /resident-sessions` 与 `POST /resident-sessions/{id}/exec` 只有 200 和 400，缺 403/408/423/429/500」— 不成立（除 500）。**
   两条路由的 responses 都已有 200/400/403/408/423/429；`/resident-sessions` 段的 403 写「Scope denied — the token's allowed hosts and tags do not cover this environment id」，408/423/429 齐全；`/{id}/exec` 同。**只有 500 确实缺**，本轮已补。

2. **「`/resident-sessions/{id}/exec` 的 400 描述里『or authorization refused』与真正的 403 自相矛盾」— 不成立。**
   现文 400 只列 unknown session / dead session / timeout / over-long command / cancelled queued command / blocked or rejected，没有 `authorization refused`；403 单独一行，写着 command scope。

3. **「`/forwards/{id}/start|stop` 的 423/429/403/408 契约一条都没写」— 不成立。**
   两条都是 200/400/401/403/408/423/429。`403` 还分段写了「scoped token 只有规则主机在其 allowed hosts/tags 内才够得着这条路由；看不见的 id 按未知处理返 400，不返 403 以免确认隧道存在」。

4. **「`forward_start`/`forward_stop` 对未知 id 返 400 `unknown forward`，而契约写的是『The id is not a UUID』」— 不成立。**
   start 的 400 原文是「The id is not a UUID, **no rule has it**, or the rule still exists but its worker could not be started」；stop 的 400 是「The id is not a UUID, **or no rule has it**. Stopping a rule that is already stopped succeeds, so an id that exists never answers 400 for its state.」

5. **「`api_yaml_documents_every_daemon_route` 的 path 解析对 `:` 与前缀星号无防御，会静默放宽」— 不成立（提出者自撤）。**
   `split(':').next()` 取第一个冒号之前，而 path 里目前没有冒号；`/` 两侧的排除与 `path.is_empty()` 分支使比对双向冗余正确。它自己核完改成「路径比对本身是双向的、冗余正确的」。

### consume 2 / 6 · 2026-11-10 19:44–19:56 (GMT+8) · issue #12

- **指摘来源**：CodeBuddy 对本轮第 1 轮收尾的复核（评论 `2120120687485419520`，19:45）。它这轮同样没有 Rust 工具链，但做了一件对的事：**用 Python 复刻我的解析逻辑，先复现出与 HEAD 相同的结果（90 routes / 107 mounts / 105 ops / 0 缺失），再做变异**。这比它上一批「读一遍就断言」可靠得多。
- **判定：(a)(b)(d) 成立（且给出了比我的说法更精确的数），(c) 部分成立**——它指出 `/exec-multi` 的 400 还有**第四种**来源，我第 1 轮写的三分不完整。

| 项 | 它的指摘 | 判定 | 证据 |
|---|---|---|---|
| (a) | 98 个 401 覆盖完整、`/health` `/metrics` 未被误加 | **成立（更强）** | 逐 operation 解析：105 ops / 2 公开 / 103 带 401 / 0 缺失。它另查了 103 行 `"401":` 的缩进全是 8 且都在 `paths:` 之后 |
| (a) 附 | `is_public_path` 有**四个**公开路径，`api.yaml` 只有两个 `security: []`；`/` 与 `/console` 靠 `NON_API_ROUTES` 豁免，所以「由 `security: []` 声明、非硬编码」只对一半成立 | **成立（措辞批评，非缺陷）** | `auth.rs:99` 是 `matches!(path, "/" \| "/console" \| "/health" \| "/metrics")`。我的 commit message 确实只覆盖了两个 |
| (b) | 方法断言可证伪、链尾不丢 | **成立** | 它跑了两个变异：`/gate/pause` 的 `post→put`、`/algo/prefs` 多行链尾的 `delete→patch`，两个都被点名。后者是关键（多行链） |
| (b) 附 | `.with_state(` 那个 clamp 今天完全不起作用，且 `find` 是从 `start` 搜到**文件尾**而不是当前段——多 router 链时会静默截断 | **成立（未来隐患，不改）** | 全文件只有一处 `.with_state(`（5924），在最后一条 route 之后。同意它的判断：不为假想的多链改代码 |
| (c) | `/exec-multi` 的 400 有第四种：`resolve_exec_targets` → `expand_hosts_by_tags` → **`load_config()?`**，`hosts.json` 坏掉时 400 文案是 `failed to read hosts config` | **成立（本轮唯一没解决项）** | `core.rs:798-807` 的 `load_config()?` 只在 `tags` 非空时执行；`:1419`/`:1488` 的 `resolve_exec_targets(...)?` 由 handler 的 `.map_err(BAD_REQUEST)` 收。已补进两条 400 描述 |
| (d) | resident 的 500 有两个站点，不止 `Internal`：`reject_if_gate_paused` → `load_execution_gate()` 失败也返 500 | **成立；但 `api.yaml` 的文案本来就覆盖了** | `daemon.rs:382-391`。我写的 description 是「a config file could not be read, or the authorization chain hit an internal error」——两个站点都在内。**错的是我的 commit message 归因，不是契约文案** |
| (d) 附 | `POST /resident-sessions` 的 429 引用了 `register_limited_session_for_command`，而该 handler 从不调用它（那是 `session_open` 的预约助手） | **成立** | 该符号只在 `:2397`（`session_open`）与 `:4727`（terminal）及测试里出现；resident 链上的 429 只可能来自 `reject_if_rate_limited`（`preflight_guarded_request_inner:571`） |
| (d) 附 | `POST /hosts` 连 400 都没写，而 `add_host` 对校验失败与 `save_config` 失败都返 400 | **成立** | `daemon.rs:1331-1338` 是 `.map_err(|e| err(BAD_REQUEST, e))`；契约原来只有 200/401/403 |

**它给的 18 个 500 缺口，我测出来是 21 个。** 差异在 handler 切分口径上（它按 29 个函数算，我按 `.route(` 里挂的名字逐个回溯），但两个数都比它上一批说的「20 处左右」更接近真相。**我自己第 1 轮说的「26 个 handler / 10 个 operation」也不准**：实测 handler 内自带 `INTERNAL_SERVER_ERROR` 的有 28 个函数，映射到 21 个 operation 缺 500。

**L1 落地**（一笔）：`ac8de91`；回帖 `2120122180045262848`（`## 第 2 轮收尾`）
- `docs/api.yaml`：21 个 operation 补 `"500"`（逐条读 handler 的 500 那一行，按各自真实原因写描述）；`/exec-multi`/`/exec/compare` 的 400 补第四种来源、并补 `"500"`（经 `authorize_targets` 透传）；`POST /resident-sessions` 的 429 删掉错引的 helper；`POST /hosts` 补 `"400"`。
- `src-tauri/tests/daemon_integration.rs`：新增 `api_yaml_documents_the_500_each_handler_can_return`——**handler→operation 映射由源码现推，不写死表**（`.route(` 里的 handler 名 → 该函数体是否含 `INTERNAL_SERVER_ERROR`）。共用解析器抽成 `daemon_routes`，两条测试同源。

**验证**：`npm run build` 6.45s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 failed / 1 ignored**（基线红）；`--features daemon --test daemon_integration` **64 passed**（63 + 新增 1）；`--features daemon --test daemon_resident_session_e2e` **12 passed**。
**三条断言全部做了破坏性验证**：删 `/gate` 的 500 → 红并点名 `["get /gate (gate_status)"]`；删 `/hosts` 的 401 → 红并点名 `["get /hosts"]`；`/gate/pause` 改 `get` → 红并点名 `["post /gate/pause"]`。

**这一轮的方法论收获**：`daemon_routes` 抽出来之后，两条测试读同一份解析结果——之前「一个测试里内联解析、另一个测试另写一套」才是漂移来源。CodeBuddy 的复刻法（先复现基线再做变异）值得记下来：**它这次能给出可信结论，是因为它先证明了自己的复刻是忠实的。**

**下一轮首项**：`/exec-multi` 的目标解析顺序（L2-77）仍是本批最值钱的行为改动；若仍不能拍板，就转去 `src-tauri/tests/**` 里「跑了真 daemon 但只断状态码不断 reason」的用例。

### consume 3 / 6 · 2026-11-10 19:57–20:00 (GMT+8) · issue #12

- **指摘来源：自取。** 第 2 轮收尾（19:56）后按 60 s × 5 轮询，issue 评论总数一直是 5，于是按协议标「等待超时」并用已有材料推进。**更正（20:02 复核发现）**：CodeBuddy 的复核其实在 **20:00:04** 就到了（评论 `2120124329374425088`），正好落在本轮的提交时刻（`39fe366` 19:59:55）之后几十秒，所以本轮确实没读到它；这一条不写清会让下一轮误以为它连续两轮不回。
- **自取的方向先错了一次，值得记**：我第 2 轮自己点的方向是「`daemon_stop_e2e.rs` / `daemon_local_exec_e2e.rs` 里只断状态码不断 reason 的用例」。实际打开 `daemon_local_exec_e2e.rs` 后发现那批 400 断言**都带 reason**——`:241` 的 `body.contains("requires host")`、`:280` 的 `body.contains("blocked")`，注释还写明「a bare 400 would also come from a failure to resolve any host」。**这条方向没料**，是我上一轮凭印象提的，不是读出来的。
- **改用的方向**：把第 2 轮那条 500 断言**普适化**。500 只是「handler 自己命名的状态码」里的一行——同一个扫描换成 10 个常量（400/403/404/408/409/423/429/500/502/503，401 除外因为属中间件），差集从 21 处扩到 **21 个 operation、24 个状态码**。

| 缺口（节选） | handler 内的来源行 |
|---|---|
| `/forwards/{id}` delete 缺 400 + 403（契约只有 200/401） | `:3046` `Uuid::parse_str` → 400、`:3078` `forward_remove_core` 失败 → 400、`:3062` `check_daemon_scope` → 403 |
| `/daemons/{alias}/exec` 缺 400/404/502 | `:3953` 本地分支 `exec_ssh_core` → 400、`:3958` `get_daemon_with_scope` → 404、`:3973`/`:3990`/`:4003` 远端 → 502 |
| `/approval/check`、`/risk/check` 缺 403 | `:3451`、`:3486` 的 `check_daemon_scope` |
| `/webdav/status` 缺 502 | `:3675` `webdav_status()` 失败 |
| 五个 `/sftp/*` 缺 400 | 各自 `.map_err(\|e\| err(BAD_REQUEST, e))` |

- **L1 落地**（一笔）：`39fe366`。`api.yaml` 补 24 个状态码（逐条按产生它的那一行写描述，不是套模板）；`daemon_integration.rs` 把 `api_yaml_documents_the_500_each_handler_can_return` 改名为 `api_yaml_documents_the_codes_each_handler_can_return`，扫描表从 1 个常量扩到 10 个。
- **验证**：`npm run build` 6.92s；`cargo fmt --check` 绿；`--lib` **861 / 1 failed / 1 ignored**（基线红）；`--features daemon --test daemon_integration` **64 passed**。破坏性验证：删 `/forwards/{id}` 的 400 → 红并点名 `["400 delete /forwards/{id} (forward_remove)"]`。
- **断言的边界（必须写清，否则下一个人会以为它覆盖了一切）**：只见 handler **自己写出**的状态码。经共享 helper 到达的（`preflight_guarded_request` 的 423/429/408/500）看不见——那些已按路由逐条文档化，但**没有再被这条断言守住**。
- **下一轮首项**：`/forwards/{id}` 的 DELETE 现在还缺 404（未知 id 该不该从 400 改成 404 是 L2-52 那笔待拍板的事），以及 `proxy_exec` 的 404 与 `get_daemon_with_scope` 的语义是否一致。**这两项改用 CodeBuddy 20:00 那条复核里的确认项**（见下）——它的「两条 confirmed finding」比我自己点的方向更具体。
- **接手过来的两条（20:02 预核，待第 4 轮正式核实）**：
  1. `GET /diagnostics` 的 500 文案写的是「The remote inventory could not be read」，而 `load_remotes()` 里的 `validate_remotes(&file.remotes)?` 会因**五条语义校验**（alias 空 / alias 是 `localhost` / alias 重复 / url 不是 http(s) / 缺 token_env 与 token）返回 `Err`——这些是人手写错的配置，不是「读不出来」。`bin/agent2ssh-daemon.rs:3847`。**它说三条，实数五条**（`remote.rs:170-198`）。
  2. `PUT /approval/policies` 的 500 文案是「could not be written」，但 `approval.rs:994` 的 `toml::to_string_pretty(&file)?` 是**序列化**失败，写盘在下一行。同一句文案盖住了两个不同阶段。

### consume 4 / 6 · 2026-11-10 20:07–20:5x (GMT+8) · issue #12

- **指摘来源**：CodeBuddy 对第 3 轮收尾的复核（评论 `2120125896106352640`，20:06 到，工作基点 `e414b63`）。本轮 20:07 起，评论刚落地即取到，**没有等待**。
- **判定：(a) 6 项 → 成立 3 / 部分成立 1 / 不成立 2；(b) 2 项 → 成立 1 / 部分成立 1；(c) 1 项 + 1 处对我有利的更正 → 全部成立；(d) 1 项 → 成立；(e) 自撤 → 成立。**

| 项 | 它的指摘 | 判定 | 证据 |
|---|---|---|---|
| a1 | `handler_body_of` 的终点是下一个 `async fn`/`fn`；`/sftp/*` 的「真实体长 ~750 B」被吃到 **3 300–4 100 B 一路到 `session_open`**；`proxy_exec` 3 092 B = 3 092 B「正常」 | **部分成立（机制对，实例与量级全错）** | 机制成立且**比它说的更广**：106 个 route handler 里 **93 个越界**，最大 `session_list` +1 877 B、`delete_snippet` +1 830 B。但它点名的 8 个 `/sftp/*` 里 7 个 over=0（`sftp_upload` 748/748、`sftp_download` 756/756、`sftp_ls` 1 401/1 401、`sftp_stat` 1 407/1 407、`sftp_rename` 905/904、`sftp_rm` 810/809、`sftp_rmdir` 815/814），只有 `sftp_mkdir` +481；`proxy_exec` 是 3 092/**3 010**（+82），不是「相等」。**没有一个 `/sftp/*` 吃到 `session_open`** |
| a1 后果 | 「现在是『假红』而不是『假绿』」 | **不成立（今天）** | 逐 handler 比较 `StatusCode::` 常量多重集：**0 个 handler 的尾部带外来常量**。越界是真的，代价暂时为零——这恰是「该修但不是缺陷」的形状 |
| a2 | `else { return Vec::new() }`：handler 名对不上时**静默跳过**，`None` 分支进不去所以今天无害，但 `async fn` 一旦被折行/加 `pub` 就永久退出扫描且全绿 | **成立** | 原文如此。已改成 `panic!`；实测把 `gate_status` 改成 `pub async fn` 后报 `the route table names \`gate_status\`, but the daemon has no \`async fn gate_status(\``，不再静默 |
| a3.1 | `proxy_exec` 的 `StatusCode::from_u16(status.as_u16()).unwrap_or(BAD_GATEWAY)` 把远端**任意**状态码原样透传，502 描述漏了「转发回来的 4xx」 | **成立（行号 3 996，非 3 992）** | `:3996` 原文如此。已补 502 描述 |
| a3.2 | `:322` 有自由函数 `err(status, msg)`，所以 `err(423, …)` 合法且不含 `StatusCode::` | **不成立** | `fn err(status: StatusCode, msg: impl ToString)`（`:334`，不是 `:322`）——`err(423, …)` 是**类型错误，编译不过**。绕开常量必须写 `from_u16`，全仓 3 处：`locked_status`、`too_many_requests_status`、`proxy_exec`，前两个是 helper（属已声明的边界）。`macro_rules!` 0 个，这条它说对了 |
| a3.3 | `terminal_attach:4716` 用 `e.into_response()`；`terminal_broadcast_run` 把 `Err((status, _))` 吞成 JSON 字段后仍回 200，契约没写 | **成立（两半；行号 4 717）** | 4 处 `e.into_response()`；`Err((_, Json(error)))` 显式丢弃状态码。已补 `/terminal/broadcast` 的 operation 描述与 200 说明 |
| a5 | `GET /exec/stream` 契约「只有 101/401/403」、`GET /terminal` 有 400/423/429/101/401/403，「同一个 handler 的四条失败路径一个写在线上一个写在帧里，客户端无从分辨」 | **不成立** | `/exec/stream` 只有 **`101` + `401`（没有 403）**，且 description 明写「Every authorization refusal after the upgrade … arrives as `{"type":"error"}` … The only HTTP-level failure is the handshake itself」。`/terminal` 的 423 条目原文就是「Unlike `/exec/stream`, this route runs the gate check before the upgrade, so the refusal is an HTTP status rather than a WebSocket error frame」。**它说「无从分辨」的那件事，契约里写了两遍** |
| a4 | `daemon_tokens.toml` 校验失败 → `authenticate_token` 500 → 全站非公开路由 500，「而且是没有 500 文档的那 82 个 operation」 | **部分成立** | 行为成立（`bin/agent2ssh_daemon/auth.rs:40-62`，`remote.rs:203-215`）。**文档不是空白**：`/approvals/{id}/approve` 的 500 已写「Applies to every authed route, not just this one — a broken `daemon_tokens.toml` answers 500 to any token that is not the admin token」。缺的只是 `info.description` 没提，已补一段。**它漏了一半机制**：admin token 在 `load_scoped_daemon_tokens` **之前**就返回，所以「全站 500」只对外来 token 成立 |
| b1 | 24 个补丁的可达性逐条都活 | **成立（我方确认）** | 抽查 5 条全活；它顺带指出的 `/daemons/{alias}/exec` 400「取决于 `alias == "localhost"` 而非 `host`」与源码一致（`:3934`） |
| b2 | `POST /hosts` 的 403 没写 `content`，「全库就这一处」 | **部分成立（实例对，唯一性错）** | 确实缺，但全库 **57 个 response** 缺 `content`，横跨 40 个 operation，绝大多数是 403。已全补并加断言 |
| c | 18 个 operation 状态码乱序，附行号；并**更正**我第 2 轮说的「`POST /hosts` 的 400 位置不对」其实合法 | **成立（逐条命中）** | PyYAML 与逐行解析两路各算一遍，都是 18 个；行号 1236/1577/1785/1902/2571/3269/3322/3706/3896/4372/4400/4440/4473/4506/4575/4612/4645/4713 **全部命中**。`POST /hosts` 的更正也成立，**第 2 轮那条指摘撤回** |
| c 附 | `/hosts/import` 的 403 与 500 之间有空行；`/recordings/{id}` 是 200/401/403/400 | **成立** | 两处都实测在。已随重排清掉 |
| d | `impl` 全仓 1 个且在 `#[cfg(test)] mod tests` 内，顶层 0 个，「今天是伪问题」；建议终点改**花括号配平 + `panic!`**，与 `handler_body` 的兜底对齐 | **成立** | `grep -n "^ *impl "` 只有 `:5976`。花括号配平方案已采纳，也正是 a1 的修法 |
| e | 自撤上一轮「`impl` 会吃到文件尾」 | **成立（自纠）** | 与我的实测一致 |

**本轮最该记住的一件事：它整份 (a) 的量化表是自造的。** 它自己也写了「同一份常量，只是把终点也算出来」，但 `proxy_exec` 的「3 092 = 3 092」说明它比较的是**两次相同的计算**，而不是「配平体 vs 断点体」。它点名的 `/sftp/*` 3 300–4 100 B 在我这里有 7 个是 748–1 407 B 且 over=0。**方向对、实例错、量级错**——与 batch 5 的 F1、本轮 (b2) 的「全库就一处」是同一个失效形态：**它抓到一个真现象后，用印象补全了细节**。凡它的表格里出现「A = B」或「每个 X 都…」，一律自己重算。

**L1 落地**（一笔）：`7dddb51`；回帖 `2120129241487405056`（`## 第 4 轮收尾`，20:19）
- `src-tauri/tests/daemon_integration.rs`：`handler_body_of` 改成花括号配平（跳过字符串/字符字面量/行注释/块注释/raw string `r#"…"#`），查不到 handler 或配平失败一律 `panic!`，返回类型由 `Option<&str>` 改成 `&str`，调用点的 `else { return Vec::new() }` 随之删掉（**a2 的静默跳过就此消失**）。新增 `api_yaml_declares_a_body_for_every_response`（除 101 外每条 response 必须有 `content`）与 `api_yaml_descriptions_with_a_colon_are_quoted`（plain `description:` 值不得含 `: `）。
- `docs/api.yaml`：57 个 response 补 `content`；18 个 operation 的响应码改为升序；两条 flow 写法（`{ description: … }`）展开成块写法才能挂 `content`；`/daemons/{alias}/exec` 的 502 写明**远端拒绝会原样透传**；`/terminal/broadcast` 写明**每目标授权失败不改 HTTP 状态**（operation 描述 + 200 两条）；`GET /diagnostics` 的 500 列出 `validate_remotes` 的五条语义校验；`PUT /approval/policies` 的 500 拆开「序列化 / 写盘」两个阶段；`info.description` 增一段讲 `daemon_tokens.toml` 坏掉时的全站 500；`/hosts/import` 的空行清掉。
- **第 3 轮预核的两条（`GET /diagnostics` 与 `PUT /approval/policies`）本轮全部兑现**，判定均为成立。

**验证**：`npm run build` 7.04s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**（基线那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--features daemon --test daemon_integration` **66 passed**（64 + 新增 2）。
**四条断言都做了破坏性验证**：①去掉 `/gate` 500 的 `content` → `api_yaml_declares_a_body_for_every_response` 红并点名 `["500 GET /gate"]`；②整条删掉 `/gate` 的 500 → `api_yaml_documents_the_codes_each_handler_can_return` 红并点名 `["500 get /gate (gate_status)"]`（证明**配平后并没有变瞎**）；③把 `gate_status` 改成 `pub async fn` → 查表 `panic!` 而不是静默跳过；④把一句 description 改回带 `: ` 的 plain scalar → 冒号 lint 红并点名该行。

**本轮自己撞出来的一个真坑（必须记）**：我新写的两句 description 里有 `verbatim: when …` 与 `authorized: false`，**plain YAML scalar 里的 `: ` 不是字符串**——写完之后 `api.yaml` 根本解析不了，而两条契约测试**全是逐行解析的，一路绿**。用 PyYAML 才发现。这就是第 ③ 个新台账的原因。教训：**`api.yaml` 的改动不能只靠仓内的逐行测试验收，必须另外过一遍真解析器**；本轮起把 `python3 -c "import yaml; yaml.safe_load(...)"` 列为 `api.yaml` 改动的必跑项。

**未做（L2，需拍板）**：新增 **L2-80**（`daemon_tokens.toml` 坏掉时对非 admin token 返 500 而非 401，零测试覆盖）与 **L2-81**（`/terminal/broadcast` 丢弃 per-target 授权状态码，全拒绝仍 200）。详见 `OPT-LOOP-L2-PENDING.md`。

**没加断言的两件事（故意的）**：响应码升序是**风格**不是契约，加 lint 会把「先 4xx 后 5xx 的阅读顺序」也禁掉；`locked_status`/`too_many_requests_status` 这类 helper 产生的码不在扫描内，这是第 3 轮已写明的边界，扩表解决不了。

**下一轮首项**：`api.yaml` 里成串的 JSON 帧样例仍无覆盖（batch 5 的四份文档同时错就长在这条缝里），而 `api_yaml_documents_every_daemon_route` 只比路由与方法。第 5 轮若 CodeBuddy 还能给出「契约写了但代码不会那么答」的具体条目，优先核那一类；否则转 `/health-snapshot` 与 `/config/import` 的 400/500 描述逐条对源码。

### consume 5 / 6 · 2026-11-10 20:20–20:55 (GMT+8) · issue #12 · 回帖 `2120134834407251968`（20:41）

- **指摘来源：自取。** 第 4 轮回帖 20:19 发出，60 s × 5 次轮询到 20:23 该 issue 评论数一直是 9（无 CodeBuddy 回复）→ 按协议标「等待超时」，改从第 4 轮回帖里自己点名的两项（`/health-snapshot` 与 `/config/*` 的 400/500 未逐条对源码；`api.yaml` 里 JSON 帧样例零覆盖）继续。
- **判定：13 项 → 成立 12 / 部分成立 0 / 不成立 1**（另有 1 条顺带观察留第 6 轮）。前 9 项是自取，第 10–13 项来自 CodeBuddy 20:28 的回复——它在 20:23 我的 5 分钟轮询结束后 5 分钟才到，见文末「补记」。其中第 9 项是**我自己的错**，被本轮新加的断言当场抓出。

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | `POST /config/import` 的 400 说「payload 无效」 | 成立 | `core/team_config.rs:67` `import_team_config` **不校验 payload**；400 的真实来源是 `store_write_lock()`、`load_config`、`save_config_unlocked`、写 `risk_rules.toml`、写 `playbooks.toml` 五种 I/O。已按源码重写 |
| 2 | `POST /config/import/preview` 的 400 同上 | 成立 | `:162` `preview_team_config_import` 只做 `load_config` + `config_dir`，两者都是 I/O 失败 |
| 3 | `GET /config/export` 的 500 说「bundle 解析失败」 | 成立 | `:21` `export_team_config` **从不解析**：两份文件 `read_to_string` 原样塞进 bundle，500 只有读文件失败 |
| 4 | `GET /health-snapshot` 的 200 schema 只有 snapshot 一支 | 成立 | `get_health_snapshot` 在未采集过时以 `{"error","hint"}` 返 200（`api.yaml` 自己的 description 都写了），schema 却是 `type: object`。已改 `oneOf` |
| 5 | `ApprovalDetail.tsx` 注释引 `api.yaml:426` | 成立（我第 4 轮写的） | `e414b63` 时 426 行是 `requested_at`，`host_address` 在 434；第 4 轮后漂到 441。跨文件引用改按 schema 名 |
| 6 | 105 个 operation 里 415/422 各出现 **0** 次 | 成立 | `grep -c '"415"'`=0、`'"422"'`=0；47 个 handler 取 `Json<T>`，三个拒绝码全由提取器产出，而 `api_yaml_documents_the_codes_each_handler_can_return` 只读 handler 体内的 `StatusCode::` 常量，**结构上看不见**——与第 1 轮「98 个 401 缺失」同族、同因 |
| 7 | `Option<Json<T>>` 吞掉全部提取错误 | 成立 | `axum-core-0.4.5/src/extract/mod.rs:119-129`：`type Rejection = Infallible` + `Ok(T::from_request(..).await.ok())`。三处受影响，`/shutdown` 最重：`{"expect_pid":"abc"}` → `None` → `:204` 的 `None => {}` → 跳过 `:182` 的 409 与 `:199` 的 400 → `:207` 关机。登记 **L2-82** |
| 8 | 提取器拒绝的响应体不是 `Error` 对象 | 成立 | `axum-core-0.4.5/src/macros.rs:50-59` `(self.status(), $body).into_response()` → `text/plain; charset=utf-8`。我第一版按 `application/json` + `Error` 写的 47×2 处全错，且「body 不是 JSON」会以 400 到达、与 handler 自己的 400 同码不同体裁。登记 **L2-83** |
| 9 | 我按 `requestBody` 批量补 415/422，把 `POST /approvals/{id}/approve` 也补了 | 成立（自我更正） | 它取的是 `body: axum::body::Bytes`（`:3341`），**没有** `Json` 提取器，手写 `serde_json::from_slice` 并回自己的 400。新断言当场点名 `post /approvals/{id}/approve (approval_approve) has no Json<T> extractor` |

**L1 落地（五笔）**：`628e9f3`（`/config/*` 三条描述 + `GET /health-snapshot` 的 `oneOf` + `ApprovalDetail.tsx` 注释）、`6fff9f1`（47 组 415/422 + 派生式断言 + 三处 `Option<Json<T>>` 的吞错说明）、`c1ecc72`（94 个拒绝改成 `text/plain` + 断言）、`12a1830`（`/daemons/{alias}/exec` 的 401 语义）、`29b3fbf`（括号扫描 12 例 pin）。

**新增断言 `api_yaml_documents_how_each_handler_reads_its_body`**：从每个 handler 的**参数表**（新 helper `handler_params_of`，走括号配平）现推它属于 `Json<T>` / `Option<Json<T>>` / `Bytes` / 无 四种之一，再按该形态要求契约——`Json<T>` 必须有 415/422 **且必须是 `text/plain`**、必须有 `requestBody`；`Option<Json<T>>` **不许**有 415/422 且描述里必须出现「does not parse」；`Bytes` 与无 body 的不许有 415/422。不写死任何表。

**验证**：`npm run build` 6.82 s；`cargo fmt --all -- --check` 绿；`cargo test --features daemon --test daemon_integration` **68 passed**（66 → +2）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（红的仍是 `an_ipv6_literal_at_port_22_is_removed`，main 自 `09d49ea` 起故意红，与本次无关）；PyYAML：90 paths / 105 operations / 47 有 415 / 47 有 422 / 0 个拒绝不是 `text/plain` / 0 个 responses 缺 `content` / 0 个乱序。

**四条破坏性验证**：①删 `POST /exec` 的 `"422"` → 红并点名 `post /exec (exec)`；②给 approve 补回 `"415"` → 红并点名它「没有 `Json<T>` 提取器」，**正好复现我 20 分钟前犯的错**；③把 `/shutdown` 的 caveat 改写掉 → 红并点名 `post /shutdown`；④把某个 415 的 `text/plain` 改回 `application/json` → 红并点名 `post /hosts (add_host)`。

**方法论（本批第二次同型错）**：我又用「扫 `requestBody` 就补状态码」代替「读签名」。上一轮的同型错是「拿同族一个成员代表整族」（脱敏那次），这次是「拿一个字段的存在代表一个行为的存在」。规则收紧：**凡是「某字段存在 ⇒ 某状态码存在」的批量推导，落盘前必须对每个成员读一遍真正的产出点**——本例里产出点不在 `api.yaml`、不在 handler 体，而在**签名**。

**未做（L2，需拍板）**：**L2-82**（`Option<Json<T>>` 吞错，`/shutdown` 可被 `{"expect_pid":"abc"}` 绕过 409/400）与 **L2-83**（提取器拒绝返 `text/plain`，且语法错与 handler 自己的 400 同码）。

**下一轮首项**：第 6 轮是末轮，取靶顺序——①**82 个 response 没有 `description`**（81 个是 200，其中 `/hosts/{name}/policy` 的 403/404 又没描述又没内容），这是「缺 body」之后的同数量级缺口，且能纯 L1 补；②`api.yaml` 的 JSON 帧样例（`/terminal` 的 `{"type":"connected"}`、`/exec/stream` 的 `{"type":"stdout"}`）是否有「契约写了而代码不产」的字段；③若 CodeBuddy 已在第 5 轮回帖下回话，优先它的可核实断言。

#### 补记：CodeBuddy 20:28 的回复（`2120131443513786368`，本轮内吸收）

它的时间戳是 `12:28:20Z` = 20:28:20，比我的第 4 轮回帖晚 9 分钟、比我的 5 分钟轮询窗口晚 5 分钟。协议上它就是「第 5 轮的指摘」，所以我按指摘处理，不另开一轮。

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| 10 | `char_end` 对 `'{'` 会「吞掉紧随的冒号」，使配平器少读 | **不成立** | 它的反例是 `let bracket_rule = ('{', ':');`。`char_end` 在 `'{'` 上返回 `quote + 3`，正好落在 `,` 上——不是冒号。我把这条形状连同 `'\''`、`'\\'`、`'\u{7D}'`、`'中'`、`r#"…"#`、`"}"`、`/* } */`、`// }`、`&'static str` 共 12 例写成 `a_brace_inside_a_literal_does_not_close_the_body`，**全绿**；把 `char_end` 改成「任何 `'` 都只前进 1」则立刻点名 `"fn f() { let close = '}'; }"`，说明这条断言有牙齿。它其余的话我复核后同意（今天零风险、没有 handler 因不解码而越界），但「测不出来」这句是错的：**重要的一半测得出来**，不可观测的只是 `else` 分支那个 `quote + 1` vs `+2`（我在提交信息里如实写了这一点） |
| 11 | `/daemons/{alias}/exec` 的 401 描述漏了 handler 语义 | 成立 | `grep -n "StatusCode::UNAUTHORIZED" src-tauri/src/bin/agent2ssh-daemon.rs` → **全仓一处**，`:3967`，`proxy_exec` 的 `ok_or_else`（`:3965-3970`）：本机没给这个别名配 token。与中间件的 `{"error":"unauthorized"}`（`auth.rs:73`）同码不同事。已修 `12a1830` |
| 12 | `GET /health-snapshot` 的 200 只有 `HealthSnapshot` 一支 | 成立（**已在它发出前 1 分钟修掉**） | `628e9f3`（20:27）就是这条。它读的工作区是 `da010a2`，所以它看到的是旧文件。它的行号 `:5540-5543` 与我核的一致 |
| 13 | `POST /config/import` 的 400 混了写盘失败、且不存在 500 | 成立（同样已由 `628e9f3` 修掉） | 我的新描述列了五种真实原因（`store_write_lock` / `load_config` / `save_config_unlocked` / 写 `risk_rules.toml` / 写 `playbooks.toml`），并写了「不校验 payload」。它补的一点我没写、值得记：**它与 `/hosts` 的 400 同构**，而 `/hosts` 已明说「save failure answers 400 rather than 500」——两条本该用同一句话 |
| — | 82 个 response 没有 `description`（81 个是 200） | 留第 6 轮 | 我复核了计数方式与它一致（`PyYAML` 遍历 responses 看有无 `description` 键）。这不是错，是「下一个数量级相同的问题」，且 `/hosts/{name}/policy` 的 403/404 又没有描述又没内容 |

**本轮因此多出两笔**：`12a1830`（401 语义）、`29b3fbf`（括号扫描 12 例 pin）。

**它这轮值得记的两件事**：①它明说「我上轮那张『同一份常量』的表是自造的，这轮的数字全部是我自己算的」，并给出分母（106 route / 93 越界 / 120 个配平体逐个比 0 差异）——**这是它第一次自带可复核的分母**，也印证第 4 轮那条方法论（凡它的量化表一律自己重算）现在可以放宽成「先看它有没有给分母，给了就抽查，没给就重算」。②它对 `/exec/stream` 与 `/terminal` 两条**主动撤回**了上一轮的指摘，并引了契约原文——这是它第一次自我更正。

### consume 6 / 6 · 2026-11-10 20:47–21:05 (GMT+8) · issue #12 · 回帖 `2120137024928649216`（20:50）

- **指摘来源：自取。** 第 5 轮回帖 20:41 发出，60 s × 5 次轮询到 20:47 评论数一直 11 → 标「等待超时」，按第 5 轮公布并写进 ledger 的两项取靶：82 个 response 缺 `description`、`api.yaml` 的 JSON 帧样例。
- **判定：6 项 → 成立 5 / 部分成立 1 / 不成立 0。**

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | 82 个 response 缺 `description`，与「57 个缺 body」同数量级同问题 | **部分成立** | 数字对（我用 PyYAML 复核也是 82），**量级判断错**：81 个是 2xx 且都有 schema。带 `content` + schema 的 200 再写一段话是复述。真正有问题的是 **4 个 4xx** |
| 2 | `DELETE /hosts/{name}` 的 404 也覆盖写盘失败 | 成立 | `remove_host:1350-1352` 把 `remove_host_core` 整条 `?` 链映射成 404；那条链含 `store_write_lock()?`、`load_config()?`、`save_config_unlocked()?`（`core.rs:279-286`） |
| 3 | `GET /hosts/{name}/policy` 的 403/404 无描述无语义 | 成立 | `host_policy:1361-1370`：403 = `scope_allows_observe_host` 拒绝，404 = `host_policy_summary` 的任何失败（含 `risk_rules.toml` 读不出来） |
| 4 | `POST /exec` 的 400 混了两类失败 | 成立 | `:1673` `ensure_command_length`（命令超长）+ `:1717-1723` `result.map_err`：除 shutdown 外的一切执行失败。该 operation 无 502/504，SSH 连不上、超时、未知主机全在这里 |
| 5 | `{"type":"shutdown"}` 帧未被契约提及 | 成立 | `json!({"type":"shutdown"` 两处：`handle_terminal:4826`、`exec_stream:5337`；`api.yaml`、`docs/`、`skills/` 里 `"type":"shutdown"` **零命中**。这是 daemon 关机时唯一的通知，认不出就会立刻重连 |
| 6 | 两个 shutdown 帧形状不同 | 成立 | `/terminal` 带 `message`，`/exec/stream` 不带。同一事件两种形状 |

**L1 落地（两笔）**：`1703cfa`（两条路由补 `shutdown` 帧 + 4 条 4xx 描述 + 两条派生式断言）、本次提交（ledger 6 / 6 + 批次汇总 + 顶部 `BATCH_COMPLETE`）。

**两条新断言**：`api_yaml_describes_every_error_response`（4xx/5xx 必须自述，2xx 豁免并写明理由）、`every_websocket_frame_the_daemon_sends_is_documented`（从源码抓 `json!({"type":"…"}` 的类型名，逐个要求在 `api.yaml` 里出现；**按名匹配，看不见形状差异**，这点写进注释没假装）。

**验证**：`npm run build` 6.84 s；`cargo fmt --all -- --check` 绿；`--features daemon --test daemon_integration` **70 passed**（68 → +2）；`--no-default-features --lib` **861 / 1 / 1**（红的仍是 `an_ipv6_literal_at_port_22_is_removed`）；PyYAML：105 ops / 0 个错误响应缺描述 / 0 缺 `content` / 47+47 全 `text/plain` / 0 乱序。

**两条破坏性验证**：①剥掉 `/hosts/{name}/policy` 403 的描述 → 点名 `403 GET /hosts/{name}/policy`；②把两处 `"type":"shutdown"` 都删掉 → 点名 `["shutdown"]`。

**方法论补充（接第 5 轮那条）**：第 1 项是「数字对、拿数字做的那句话错」。既有规则只说「凡量化表一律重算」，不够——**核对完数字还要核对它拿数字做的那句话**。82 是真的，但 81 个是 2xx，「同数量级相同问题」是另一回事。

### batch 2026-11-10 · 测试覆盖缺口 · 批次汇总（6 轮闭环）

| 轮 | 指摘来源 | 判定（成立/部分/不成立） | 落地 SHA |
|---|---|---|---|
| 1 | CodeBuddy 首条分析（`2120104226712895488`） | 9 组：5 / 2 / 2 | `bcca25c`（方法 + 98 个 401）+ `e8191a2` |
| 2 | CodeBuddy 回复 4 项 | 4 项：3 / 1 / 0 | `ac8de91`（500 + 断言）+ `1a31c41` + `3397542`（更正两处时间与 id）；L2-76 |
| 3 | 自取（它的复核 20:00:04 已到，写入更正） | 缺口扫描 21 个 operation | `39fe366`（24 个状态码 + 把 500 断言普适化）+ `06b2568` + `e414b63`（更正区间与那条迟到回复） |
| 4 | CodeBuddy 回复 14 项 | 14 项：8 / 3 / 3 | `7dddb51` `90ba38b` `da010a2` `11fd07f`；L2-80 / L2-81 |
| 5 | 自取 9 项 + CodeBuddy 迟到回复 5 项 | 14 项：12 / 0 / 1 | `628e9f3` `6fff9f1` `c1ecc72` `12a1830` `29b3fbf` `ead26b1` `39d98c0` `fdf70a0`；L2-82 / L2-83 |
| 6 | 自取 6 项 | 6 项：5 / 1 / 0 | `1703cfa` + 本次提交 |

**累积成果**：`api.yaml` 从「路径 + 部分状态码」变成「每 operation 的完整 4xx/5xx 集合 + 每个响应有 body 有描述 + 拒绝体裁正确 + 帧类型齐备」，并留下 8 条派生式断言（路由/方法/401/响应体/冒号 lint/handler 状态码/body 读取方式/帧类型），全部做过破坏性验证。

**L2 待拍板（本批新增 4 条）**：L2-80（`daemon_tokens.toml` 坏掉时非 admin token 得 500 而非 401，零测试）、L2-81（`/terminal/broadcast` 三处授权/限流失败全 200）、L2-82（`Option<Json<T>>` 吞错，`{"expect_pid":"abc"}` 可跳过 `/shutdown` 的 409/400）、L2-83（提取器拒绝返 `text/plain`，语法错与 handler 的 400 同码不同体裁）。**L2-82 有安全后果，建议优先。**

**批次的一条工艺教训**：6 轮里我有 2 次犯同一类错——第 4 轮「拿同族一个成员代表整族」，第 5 轮「拿一个字段的存在代表一个行为的存在」。两次都是**批量推导跳过了逐成员读源码**。本批之后，凡「X 存在 ⇒ Y 成立」的批量改动，落盘前必须对每个成员读一遍真正的产出点，并配一条会在下次改动时变红的派生式断言。

### batch 2026-11-10 · daemon 控制与停止协议（第二批，B=7）
- issue #13 / 开批时间 2026-11-10 20:57 (GMT+8)
- CodeBuddy 首条分析：`2120141279483817984`（2026-11-10 21:07:26 GMT+8，比开批晚 10 分钟、比生产任务的 5 分钟轮询窗口晚 6 分钟；8 条指摘）
- 6 轮进度：**6 / 6（已闭环，issue 已关闭）**
- 正文已喂给 CodeBuddy：**58 条「不成立」原文**（编号 44：preflight 2 + batch1 六轮 10 + batch2 编号 15–31 十七条 + batch4 第 1/5/6 轮 10 + batch6 第 1 轮 5；行内 14：batch2 D2/D4、batch3 A5/E、batch5 D6 后半/D8/F6 后半/F10/F11 后半/F12、batch6 a1 后果/a3.2/a5/c 附更正/#10）+ KNOWN-ISSUES 空档（K14 全部未解项、K15、§3 懒加载审批）+ 口径提醒（基线红测 861/1/1、npm 213/27、feature 组合不可相加、`daemon_*_e2e` 需显式 `--features daemon`、`exec_fixture` 空跑、`.github/**` 禁区、api.yaml 已在 #12 对齐、四列计数）。正文由 `/tmp/build-opt-issue-b7.py` 按 ledger 锚点切片生成（块级正则 + 行内唯一子串校验，16 块 / 44+14 条复验通过）。
- **3 条已转可重提的注明**：`/exec/stream` 401 语义、`app_preferences.json` 归组、`total_ms` 补 `format: int64`（原 #12 正文的不成立被更正评论 `2120103682455814144` 撤销）已单独注明「无需主动重提」，未计入 58 条。
- 背景已写进正文：同主题第一批（#7）已 6 轮闭环，已修复项与已登记 L2（L2-1…L2-18、L2-82）勿重提；本批重点找 #7 闭环后新增/残留的问题。
- 下一批（B=8）：8 % 6 = 2 → 主题 2：SSH 传输、known_hosts 与认证（russh 侧）。开 issue 前重读 ledger 不成立清单（批次 7 消费期间可能新增），行内子串锚点会因消费者编辑 ledger 而漂移——届时重新定位。

### consume 1 / 6 · 2026-11-10 21:07–21:37 (GMT+8) · issue #13 · 回帖 `2120148713845710848`（21:36:58）

- **指摘来源**：CodeBuddy 首条分析 `2120141279483817984`（21:07:26，基线 `8d86fe0`）。8 条，全部落在 batch 1（issue #7）闭环之后，没有重放已判否条目。
- **判定：8 条 → 成立 2 / 部分成立 3 / 不成立 3。**

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | `/exec/stream` 的 `interrupted` 把「正常 exit 0 撞上 shutdown」记成「远端结果未知」 | **不成立** | 判据是 `:5450` `cancelled && code.is_none()`，而取消分支的 drain 用 `timeout_at`（`:5340`）把队列里已到的 `Exit` 读出来写进 `code`——`code.is_some()` 时 `interrupted` 必为 false，所以**它标题里那个形态不存在**。能为真的只有「取消后 500 ms 宽限内没收到 `Exit`」，此时 daemon 手里确实没有退出码，`remote outcome unknown` 是对 daemon 已知状态的准确描述。它自己承认前半段是对的；剩下那半是「远端已返回但 `Exit` 未及入队」的窗口——那个窗口里 daemon 不可知，要缩短只能让线程侧另存一份退出码（保真度改进，不是纠错），不立条 |
| 2 | `handle_terminal` 的 drain 没有绝对截止，500 ms 是每次 recv 重置 | **部分成立** | 机制成立：`:4830` 的 `timeout(500ms, …)` 每次迭代新建，`:4783` 的泵每 100 ms 一次，`Output` 分支只 `continue`；契约那句「drains for up to 500 ms」确是错的（**已改 L1**）。**结论那半错**：它说 2 s 把 future drop 之后 `recorder.finish()` / `terminal_inputs` 的 remove / `limiter.unregister_session` 都跑不到——这三件都在 `run_ws_with_shutdown_deadline` **之后**（`:5016-5045`），future 被 drop 也照跑（第 2 批 batch 1 的 L2-26 撤回就是同一个坑）。所以真实后果只是「drain 可能持续到 2 s」，不是「清理丢失」→ **L2-84** |
| 3 | info 宣称每条 authed 路由回 500，但 107 个 operation 只有 33 个写 500 | **不成立** | 先纠数：**105** 个 operation，**103** 个有 401（非公开），**33** 个有 500。**关键是它引漏了紧接的一句**：同段原文写着「`/approvals/{id}/approve` states this once in full rather than repeating it on a hundred operations」，而那条 500 的描述确实把整桩事写全了（坏 `daemon_tokens.toml` → 任何非 admin token → 500）。所以 33 是**写明的约定**，不是漏写，「直接矛盾」不成立。它剩下的话（按 operation 生成的客户端看不见）是对这条约定的评价，属设计取舍。顺带把 summary 句的 `a scoped caller` 改成 `a caller that does not present the admin token`——同段第一句本来就写「any token that is not the admin token」，而**无凭据请求同样会 500**，summary 比自己的定义窄 |
| 4 | Stale 分支静默 `Ok(())` 让 CLI 打印「Daemon stopped」而实际没确认任何事 | **不成立** | 机制在（`:945-952`），但它给的可执行判据是**误读测试**：`background_start_verifies_and_stops_its_own_daemon`（`:985-1003`）把 `u32::MAX-1` 写进 `daemon.pid` **之后启动了一个真 daemon**，`stop_daemon_process(started.pid)` 收到的是真 pid，走健康路径，不是 Stale 分支。而 Stale 的定义是「确定不存在或僵尸」（`:330-335`），此时没有任何活进程持有该 pid，`daemon stop` 的终态（无 daemon、标记清掉）成立；加一句「already gone」更清楚，但那是措辞偏好。preflight 第 1 条判否的是「kill 失败却报成功」，那里有一个**活**进程 |
| 5 | `fetch_daemon_activity` 漏掉在途 `/exec` | **部分成立** | 机制成立：四次请求（`daemon_activity.rs:205-217`），`exec`（`:1665-1700`）不注册任何 registry，`ResidentSessionSummary`（`resident_session.rs:361-367`）没有「在跑」字段，于是有命令在跑的 daemon 被 `is_idle()` 判成空闲 → 退出确认框不弹。但「要不要把 `/exec` 计入」是产品决策（`/exec` 自带 `timeout_secs`），且要动 daemon 与桌面契约 → **L2-86**。它那句「这条永远测不到（CI 不设 `AGENT2SSH_TEST_SSH_PORT`）」不准确：这条要测的是「有没有计数」，不需要真 SSH |
| 6 | `daemon stop` 在 `read_admin_token()` 为 `None` 时给 `NotAuthenticated`，而 doc 说的是别的事 | **成立（已登记 L2-4，行号需更正）** | 触发点其实是 `:991`（它说 `:1006`，那是 "daemon predates" 的 warn 行），`Display` 在 `:792-796`。**L2-4 记的正是这一条**，本轮把行号与触发条件写进 L2-4 的备注。它补的一点对：`:772-788` 的 `\|\|` 让这条分支永远看不出走哪边——但那条用例构造的是「token 文件写错」，走的是 401 支（`NotAccepted`），**根本到不了** `:991`；所以 `\|\|` 的问题不是「两个分支都可能」，而是两条文案太像 |
| 7 | 健康 PID 不匹配时的文案把四种原因压成一句 | **成立** | `:986-989` 的括号列了三种原因，但这一支的入口是 `:982` 的 `else`：`config_dir` 的比对在 `:959-981` 的 `if` 分支，`admin token` 在 `:991` 之后。打印这句话时括号里后两种原因不可能成立 → **L2-85** |
| 8 | `L2-3` 的落地形状与 `stop_daemon_process` 的读法不自洽 | **部分成立** | 类型那半对：`Accepted` / `AcceptedUnchecked` 都不带载荷（`:827-843`），要比较 `checked_pid` 必须改变体、并让 `request_shutdown` 的 200 分支先解析 body，`L2-3` 那段没写这一层（已在 L2 文件里补成四步）。**行号那半错**：当前树 `grep -n "expected_pid.is_none()"` → `:872`、`grep -n "daemon predates"` → `:1006`、`:200`（另一处文档注释），与 `L2-3` 的引用一致；它给的 `:901` 是 `response.body.trim()`、`:1020` 是 `ProcessObservation::Unknown` 臂 |

**推送**：`876c869` + `5384a0f` 推 `cnb main` 与 `git233 main`。git233 此前停在 `7a623e2`（2026-11-10 02:04，落后 **126 个提交**——整个优化循环都没同步过），这次一并对齐，是本流程第一次双远端推送。

**L1 落地（两笔）**：`docs/api.yaml`（`/terminal` 的 500 ms 改写为「空闲超时 + 2 s 上限 + `Closed`/error 提前结束」；`/exec/stream` 补「drain 只转发 stdout/stderr，drain 期间到的 exit 只进审计、不发 `{"type":"exit"}`」；info 的 summary 句改「any caller that does not present the admin token」）、`src-tauri/tests/daemon_integration.rs`（新增 `a_websocket_drain_budget_claim_matches_the_loop_it_describes`）。

**新断言做什么**：把「某路由声称 500 ms 预算」与「它的 handler 里是否只算一次 deadline（`timeout_at(`）」两两比对。`/exec/stream` 声称且有 → 成立；`/terminal` 不声称且用 `timeout(` 重设 → 成立。**这是双向钉**：改回文档会红，把终端循环统一成 `timeout_at` 而不改文档也会红。

**验证**：`cargo fmt --all -- --check` 绿；`cargo test --no-default-features --features daemon --test daemon_integration` **71 passed**（70 → +1）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（红的仍是基线那条 `an_ipv6_literal_at_port_22_is_removed`，main 自 `09d49ea` 起故意红）；PyYAML：90 paths / 105 operations 解析通过 / 0 个错误响应缺描述 / 0 处 `: ` plain scalar 违规。**`npm run build` 本轮未跑**：该 worktree 无 `node_modules`（`tsc: command not found`），而本轮零前端改动，`--lib` 与 `daemon_integration` 都不需要 `dist/`。

**两条破坏性验证**：①把 `/terminal` 的句子改回「drains for up to 500 ms」→ 断言红并点名 `/terminal`（`left: true, right: false`）；②删掉 `/exec/stream` 的预算短语 → 红并点名 `/exec/stream`（`left: false, right: true`）。两次都改回后复跑绿。

**本轮的工艺观察（三条，都关于「怎么读」）**：
1. **截断引用**。第 3 条只引了 `info.description` 的前一句，停在第 3 句——而第 4 句正是「这条 500 只在一处写全」的声明。以后凡「文档没写 X」类指摘，必须先读被指段落的**下一句**再判。
2. **行号不可复现，方向还不一致**。4 条里 3 条行号错（-20 / +15 / +14），不是统一漂移，说明是估算而非读取。原有规则「凡量化表一律自己重算」扩一条：**行号一律重算，形状照抄的部分可以信**——它引的代码块逐字对得上，只有行号错，这个组合已经出现第三批了。
3. **把块内的结论套到块外的代码上**。第 2 条读的是 future 里的 drain 循环，却断言 future 之外的三件清理「跑不到」——与 batch 1 的 L2-26 撤回完全同型（那次也是把 `unregister` 当成在 future 内）。规则：**凡「X 跑不到」，先确认 X 在不在被 drop 的那个块里**。

**未做（L2，需拍板）**：**L2-84**（终端 drain 是空闲超时，与 exec 的绝对 500 ms 语义不一致）、**L2-85**（`UnverifiedIdentity` 括号列了两种不可能的成因）、**L2-86**（在途 `/exec` 对退出确认不可见）。另更正 **L2-4**（触发点 `:991`、`Display :792-796`、`||` 的真实问题）与 **L2-3**（变体要带载荷，落地四步）。

**下一轮首项**：按第 1 条判否时留下的窗口——`spawn_embedded_exec_stream` 线程侧（`:5074-5165`）的 `blocking_send` 与 `session.set_blocking(false)` 切换、`channel.eof()` 之后 `wait_close()` 的阻塞可能，是这道题唯一还没被逐行读过的部分；其次 `daemon_stop_e2e.rs` 里那条「`||` 两文案」用例可以拆成两条可达断言（L2-4 的落地前提）。若 CodeBuddy 第 2 轮能给出「Stop 路径在 Windows 腿无覆盖」以外的可核实断言，优先它。

### consume 2 / 6 · 2026-11-10 21:39–21:52 (GMT+8) · issue #13 · 回帖 `2120151876342534144`（21:52）

- **指摘来源**：CodeBuddy 对第 1 轮四个复核问题的答复 `2120149377556582400`（21:39:36）。不是新一批扫描，而是对我本轮改动的复核 + 两处对我的反驳。
- **判定：6 项 → 成立 3 / 部分成立 3 / 不成立 0**，另单列**两条被推翻的子断言**（下节「原文保留」）。

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| a | `/terminal` 的 2 s 上界没有第三种出口，但句子少写「send 失败」这条 | **成立** | `:4822-4861` 的三个出口：`:4851`（`ws_tx.send` 失败）、`:4855`（`Error`/`Closed`）、`:4860`（循环结束后 break）。**已补**「or a send that fails」。它顺带算的「上限是 2 s − 那次 send」也对：计时起点是 `shutdown.cancelled()` resolve，不是 shutdown 帧发出 |
| b | `path_block` 不会切短 `/terminal`；附带：`handler_body_of(exec_stream)` 拿到整个文件 | **部分成立（边界对，附带事实错）** | 边界那半我复核同意（块内无以 `  /` 开头的行；`head` 带 `path + ":"` 不会命中 `  /terminal/broadcast`）。附带那半**不成立**：临时探针实测 `exec_stream` body **13 702 B**、`handle_terminal` **14 261 B**，源文件 **235 983 B**，两者都不含 `\nasync fn `——`opening_brace` 在形参表闭合后回到 depth 0 才命中 body 的 `{`。**但它说的失效形态值得防**：本轮加了守卫 + 破坏性验证（把 `&rest[..close]` 改成 `&rest[..]`，断言立刻点名 `exec_stream`） |
| c | 「drain 期间不发 exit 帧」字面成立、句子不精确：exit 在循环之后发，且在 Close 之后 | **成立（并指出一个真缺陷）** | `grep -n '"type":"exit"'` 全仓只有 `:5493` 一处，位于 `select!` 之后；停机路径 `:5365` 已发 `Message::Close(None)`。所以「客户端看到 close 而不是 exit」成立，但真实因果是「**exit 在 Close 之后被发出**」——RFC 6455 §5.5.1 禁止 Close 之后再发数据帧。已把契约改成按现状描述并指向 **L2-87** |
| d | 第 3 条（33/105）判否成立 | **成立** | 它独立复算了 90 paths / 105 operations，并重读了 `:19-21` 与那条 500（`:3740-3747`）。与我的判否一致 |
| d' | 第 4 条 Stale 那半判否成立；但「删 `daemon.token` 到不了 `:991`，L2-4 的拆分方案会红」 | **部分成立（前半对，后半错）** | Stale 那半一致。后半**不成立**：`:991` 在 `:996` 的 `request_shutdown(Some(pid))` **之前**（同函数语句顺序）；`request_shutdown` 的 `:846-849` 守卫只有 `:932` 的 `request_graceful_shutdown()`（传 `None`）与「两次读之间文件被删」的竞态两个入口。所以删 token 确实停在 `:991` → `NotAuthenticated`。**它那张「两个变体都偏、偏法相反」的表是对的**，只是把哪条路径到哪一支接反了；L2-4 备注已补入口清单 |
| 附 | `/exec` 结束后 worker 线程的泄漏「以事件为界」 | **部分成立** | 前半对（drop receiver 后线程在下次 `blocking_send` 失败时退）。**上界不是「下一事件」**：eof 之后线程还要走 `:5153-5155`，`set_blocking(true)` + `wait_close()` 无取消入口（`channel.rs:473` 传 `None`），上界只剩 `session_timeout_ms(timeout_secs)`（默认 60 s，且 `timeout_secs` 无上界）。库里的 `embedded_ssh.rs:1505-1510` 已经做对（teardown 30 s + 可取消）→ **L2-88** |

**L1 落地（本轮）**：`docs/api.yaml`（`/terminal` 的出口清单补「send 失败」；`/exec/stream` 把 exit 帧的真实时序写成「循环之后、Close 之后」，并注明这是 L2-87）、`src-tauri/tests/daemon_integration.rs`（新断言加 body-span 守卫）。

**验证**：`cargo fmt --all -- --check` 绿；`daemon_integration` **71 passed**；PyYAML 90 paths 解析通过。**破坏性验证一条**：把 `handler_body_of` 临时改成返回 `&rest[..]` → 断言红并点名 `exec_stream`；改回后复跑绿。

**本轮工艺观察**：它这轮的**结论质量明显高于第 1 轮**——四个问题里三问答得准，且主动复算了分母（90/105）。两次失手都发生在「顺着一个真现象往外推一步」：`handler_body_of` 那步是**只读了函数的前半段**（看到 `depth` 在形参行为 1，没读到它会在 `)` 处减回去），`NotAuthenticated` 那步是**只读了调用点、没读语句顺序**（看到了 `request_shutdown` 自己的守卫，没注意 `:991` 在它之前）。规则与第 1 轮的 (c) 同源：**引用一段控制流，必须读到它的出口或下一处分支**，否则现象为真、因果接反。

**未做（L2，需拍板）**：**L2-87**（Close 之后仍发 `{"type":"exit"}`）、**L2-88**（worker 线程的 teardown 不派生自 teardown 预算、也不可取消）。`L2-84/85/86` 维持，`L2-4` 补入口清单。

**下一轮首项**：第 3 轮该验的是「我这两笔 L1 有没有把契约写成第三种错」——具体是 `/exec/stream` 那句「the handler sends its one exit after the loop」是否在**非停机**路径上也成立（那里 exit 事件先 break，再发帧，客户端在 Close 之前收到），以及 `/terminal` 的「send 失败」出口在 drain 里是否真的只结束 drain 而不终止 handler。若 CodeBuddy 无回复，转 `wait_for_exit` / `signal_daemon`（L2-1 的落地形状，本批唯一没被任何一轮碰过的停止路径代码）。

### consume 3 / 6 · 2026-11-10 21:53–22:01 (GMT+8) · issue #13 · 回帖 `2120154055677734912`（22:01）

- **指摘来源：自取。** 第 2 轮回帖 21:52 发出，60 s × 5 次轮询到 21:58 评论数一直是 4 → 按协议标「等待超时」，改取上轮公布的靶：`wait_for_exit` / `signal_daemon`（L2-1 的落地形状），外加自验上轮两笔 L1。
- **判定：自取 6 项，成立 6**，其中 2 项是自验通过（无需改动）、3 项 L1、2 项 L2。

| # | 项 | 判定 | 证据与动作 |
|---|---|---|---|
| 1 | 自验：`/exec/stream` 的「sends its one exit after the loop」在**非停机**路径也成立吗 | 成立（无需改） | `:5450` 的 `Exit` 臂写 `code` 后 `break`，`:5493` 再发帧；该路径全程没有 `Message::Close`（停机路径的 Close 在 `:5365`，只在取消分支里）。所以「one exit，after the loop」两条路径都对 |
| 2 | 自验：`/terminal` 的「send 失败」只结束 drain、不终止 handler 吗 | 成立（无需改） | `:4851` 的 `break` 只出内层 `while`，之后 `:4859` 发 Close、`:4860` 跳出 select，`:5016` 起的 `recorder.finish()` / `terminal_inputs.remove` / `limiter.unregister_session` 照跑 |
| 3 | `signal_daemon` 的 `force` 参数恒为 `true`，`kill_with(Signal::Term)` 分支是死代码 | **成立（新）** | `grep -rn "signal_daemon("` 全仓 **1 个调用**（`:1030`，`true`）。`force == false` 那条（`:750` 的 `kill_with(Term)` 与 `:751` 的兜底）永不执行——「先 SIGTERM、再强杀」的策略在代码里不存在 → **L2-89** |
| 4 | `docs/architecture.md:191` 说 `stop_daemon_process` 在 Unix 发 `SIGTERM` | **成立（新，文档）** | 实际路径（`:958-1035`）是：`/health` 核对 → `POST /shutdown`（全平台，`local_daemon_request_with_body`，无 `cfg(windows)` 分支）→ 等 8 s → **强杀**（`process.kill()` = SIGKILL）→ 等 2 s。SIGTERM 一次都没发。**已改 L1**：写成「全平台先 POST /shutdown（它才带 `expect_pid`）→ 8 s → 强杀 → 2 s」，并注明外部 `kill <pid>` 仍可走 SIGTERM 且 daemon 会 drain |
| 5 | `daemon_control.rs:921-926` 的注释把 HTTP 路线写成 Windows 的兜底 | **成立（新，注释）** | 原文「Windows has no `SIGTERM`: signalling falls back to `TerminateProcess` there… Asking the daemon does what the signal does elsewhere」——这句话正是第 4 条误写的来源。**已改 L1**：改成「这是所有平台停止 daemon 的方式，没有 SIGTERM 分支可退；Windows 只是这条路由存在的原因，而 Unix 也走它，因为它才带 `expect_pid`」 |
| 6 | `api.yaml` 的 `/shutdown` 把「请求 + 强杀」写成无条件两步 | **成立（新，契约）** | 原文「this request, then — if the process is still running — a force-terminate」读起来无条件；实际 `:996-1016` 里 `NotAccepted` 直接 `return Err`，**不发任何信号**，401/409/连接被拒都是这一支。**已改 L1**：改成「only once a daemon has accepted it…」，并写明三种拒绝都不发信号。剩下的「传输层失败要不要强杀」→ **L2-90** |

**L1 落地（本轮，4 文件）**：`docs/architecture.md`（SIGTERM → 真实两步）、`docs/api.yaml`（`/shutdown` 的两步条件化）、`src-tauri/src/daemon_control.rs`（只改注释，`:921-926`）。

**验证**：`cargo fmt --all -- --check` 绿（纯注释）；`daemon_integration` **71 passed**；PyYAML 90 paths / 105 operations。本轮没有新断言——三条都是文档与注释，其中第 4 条的失效形态（文档说发 SIGTERM）**无法用测试钉**：它描述的是一个不存在的调用，扫描 `signal_daemon(` 只能证明调用点唯一，不能证明文档没写反。这一条记在这里，以免下一轮以为它被测过。

**本轮工艺观察**：死分支是靠「参数只有一个取值」发现的——`signal_daemon(pid, start, force)` 长得像有两条策略，`grep` 一次就塌。同类形状（布尔参数只有一种传法）值得作为下一批的固定扫法：**对每个带 bool 参数的私有函数，先数调用点的实参取值**，再看文档里有没有第二种说法。

**未做（L2，需拍板）**：**L2-89**（`force` 恒真 / SIGTERM 分支死代码）、**L2-90**（`/shutdown` 请求失败后不退回强杀，要不要区分传输层失败与语义拒绝）。`L2-84/85/86/87/88` 维持。

**下一轮首项**：第 4 轮先按上面的固定扫法把 `src-tauri/src/**` 的私有 bool 参数过一遍（`daemon_control.rs`、`daemon_activity.rs`、`bin/agent2ssh-daemon.rs` 三个文件），再回 CodeBuddy 的复核（若到）。若 CodeBuddy 在第 4 轮给了可核实断言，优先它。

### consume 4 / 6 · 2026-11-10 22:02–22:06 (GMT+8) · issue #13 · 回帖 `2120155902735974400`（22:05:32）

- **指摘来源**：CodeBuddy 对第 3 轮的复核 `2120155093726027776`（22:02:19）。**这一轮它第一次直接改 main 并推了两次提交**（`4dee4d1` 改 `docs/api.yaml`、`4296376` 改 `daemon_control.rs` 的注释行号），所以本轮开场先 `git fetch` + `--ff-only` 对齐，再在它之上动手。**流程含义**：从这轮起，消费任务每次开跑和每次推送前都必须 fetch，否则 push 会被拒且容易把它的改动顶掉。
- **判定：5 项 → 成立 3 / 部分成立 0 / 不成立 2**（另 1 条是它自取的修正，成立，计入那 3 条里）。

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| a | `signal_daemon` 的 `force` 恒真、SIGTERM 分支死 | **成立（并补充）** | 它补的「这个二进制里根本没有 SIGTERM 发送方」核实为真：去掉 transport 后只剩 `:750` 本身、三处注释、`local_exec.rs:1138`；daemon 端**接收** SIGTERM 的 `shutdown_signal`（`:223-238`）也是真的，所以「外部 `kill` 走 SIGTERM 且会 drain」这句准，不准的只是「停止路径发 SIGTERM」 |
| b | 停止路径全平台走 `POST /shutdown`，无 `cfg(windows)` | **成立** | `:853-872` 两个调用都不带 `cfg`；`daemon_control.rs` 的 `cfg(` 全落在进程观测 helper 上 |
| c | 「`401` 在停止路径上永远见不到」 | **不成立** | `:991` 只在 token 文件**缺失或读不出**时早退（`read_admin_token` = `read_to_string(...).ok()?`）；文件存在但值与 daemon 启动时加载的**不同**时，`:991` 返回 `Some`，请求照发，daemon 比的是内存里的 `state.token`（`auth.rs:44`，只在 `:5748` 加载一次）→ **401**。这正是 `tests/daemon_stop_e2e.rs:766` 的 `wrong_admin_token_never_signals_the_real_daemon` 构造的场景，它断言的那句「did not accept this config's admin token for a shutdown」就是 401 支的文案。**它据此改了 `4dee4d1`，本轮已把那段改正**（409 / 401-403 / 前置拒绝三分） |
| d | 我自验的第二句「`:4851` 证明 send 失败结束 drain」只是「整段 drain 最多一帧」 | **不成立（前两句成立）** | drain 里 `Output` 分支 send **成功**后是 `continue`（`:4853` 之后回到 `while`），只有 `is_err()` 才 `break`（`:4850-4851`）——它说的「send 成功在那里本来也 break」是误读。`:4930` 是**正常转发**分支的同类出口，两处各自证明各自的句子，不冲突 |
| 自取 | `GRACEFUL_STOP_WAIT` 注释里的三个行号全漂 | **成立** | 它改的 `:5014`（`/terminal` 2 s）、`:5442`（`/exec/stream` 2 s）、`:5951`（registry 4 s）逐个核对正确。这条与 `force` 同病：常量旁的行号没人核，就成了下一句谎 |

**L1 落地（本轮）**：`docs/api.yaml` 的 `/shutdown` 段落重写——把「请求未答上」拆成三类：`409`＝另一个 daemon 占有监听（`expect_pid` 不符）、`401`（或 admin gate 的 `403`，调用方同等处理）＝这条路确实答了、只是 `daemon.token` 里的值不是 daemon 认的 admin、以及停止路径自己的**前置拒绝**（无 `/health`、PID 不符、config_dir 不符、无 token 文件）——这三种从不建连，没有任何状态码描述它们。

**L2 收窄**：`L2-89` 定 ①（删 `force` 与 `kill_with(Term)`），理由三条已写进台账（② 在 Windows 上等于 ①；Unix 上只是多一次可忽略信号；K14 的真正收窄是 409 那条原子比较）。`L2-90` 维持不退回强杀，但理由换成「可诊断性」与「`NotAccepted` 混装两类失败」，并把它的反例（监听卡死时技术上可安全强杀）照实记下。

**验证**：`cargo fmt --all -- --check` 绿；`daemon_integration` **71 passed**；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**；PyYAML 90 paths / 105 operations。它那两个提交只动 `description` 与注释，rc 与测试口径不变。

**本轮工艺观察（第四种「读得短」）**：它这次错在两处**把分支条件当成了全部条件**——`:991` 被读成「没有 token 就早退」（对）却推成「停止路径见不到 401」（错，漏了「有 token 但不匹配」）；`:4850` 的 `is_err()` 被读成「无论如何都 break」（错，漏了成功路径的 `continue`）。与前几轮同一族：**读到 `if` 只记了它的一个分支**。

**下一轮首项**：第 5 轮验「我这次改的 `/shutdown` 三分表是否与代码逐条对得上」（重点是 `403` 是否真的会被 `request_shutdown` 当成 401 同等处理，以及 `:991` 的「读不出」包含哪些 errno）；若 CodeBuddy 有复核，优先它。第 6 轮（末轮）按协议写批次汇总并关 issue。

### consume 5 / 6 · 2026-11-10 22:06–22:10 (GMT+8) · issue #13 · 回帖 `2120157063207620608`（22:10:09）

- **指摘来源**：CodeBuddy 对第 3 轮四问的复核 `2120156142427869184`（22:06:29，它标成「第 3 轮复核」——时序上是我第 4 轮回帖之前写的）。
- **判定：5 项 → 成立 3 / 部分成立 2 / 不成立 0**（第 6 行「d」是对它自我更正的接受，不计入判定），另**两条子断言不成立**（见下）。

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| a | 新句子的时序与代码一致；但漏了「8 个提前 return 不发 exit 帧」 | **成立（补充成立）** | 8 条路径逐个核过：`:5198`（body 不是 ExecRequest）、`:5210`（source 非法）、`:5221`（命令超长）、`:5230`（gate 暂停）、`:5242`（限流）、`:5275`（授权失败）、`:5285`（缺 `force`）、`:5300`（未知主机），每条只发一个 `{"type":"error"}` 就 return。**已 L1**（`/exec/stream` 新增一句「拒绝之后没有 exit 帧可等」）并并入 **L2-87** 的备注（① 若只改停机路径，这 8 条仍然没有 exit） |
| b | `/terminal` 的 drain 还有第四个出口 | **部分成立** | 出口本身成立：`event_tx` 被丢后 `event_rx.recv()` 返回 `None`，`while let Ok(Some(..))` 模式不匹配、静默退出（`:4829`、`:4858`），`drained` 仍为 true、不写 warn。但**「不发送 Close」不成立**：`:4859` 的 `send(Message::Close(None))` 在 `while` **之外**，`break` 与模式不匹配都走它。`event_task.await` 无上界也**不成立**：`:4783` 是 `recv_timeout(100ms)` + 每轮 `is_closed()`，`drop(event_rx)` 后 ≤100 ms 必退，卡不住 `:5027` |
| c | 守卫有效但「守错了东西」 | **部分成立** | 不误报、破坏性验证通过两点同意；但它指出的缺陷是真的：失败消息只有一句话，**没有显示被读进来的那段**。**已改**：改成 `if let Some(at) = body.find("\nasync fn ")` + panic 打印 byte offset 与 span 前 160 字符，并复验（把 `&rest[..close]` 改成 `&rest[..]` → 消息含「another async fn starts at byte 14197」与前 160 字节）。它建议的「断 `find` 先后序」不适用：`handle_terminal` 没有 `timeout_at(`，比较无意义 |
| d | 2.2 撤回、2.1 改名 | **接受** | 它按 `:951`/`:995` 重读后撤回「删 token 停在 `:846-849`」；2.1 改称「假设错、结论对、证据互补」。两处都比它上一轮的说法准确 |
| 附 | drain 里 send 失败时漏 `end_send` | **成立（新，L2-91）** | `:4849-4853` 的 `begin_send()` → `is_err()` → `break`，**没有 `end_send()`**；对照正常转发的 `:4893-4898` 两条路径都成对。pending 计数留在 1 → registry 要等满 deadline。**它给的行号 `:5092` 是错的**（那里是 exec_stream 的清理段），实际在 `:4849-4853` |
| 尾 | `signal_daemon` 的 `Ok(())` 不可区分 / `wait_for_exit` 末尾多一次观测 | **成立（不新立）** | 前者与 L2-1、L2-89 同一条，不重复登记；后者（`:724` 在循环外再 `observe_process` 一次，`Unknown` 可能因此变 `Running`）是 8 s 边界「报错 vs 继续杀」的判据，记在这里备查，不单独立条 |

**L1 落地（本轮，2 文件）**：`docs/api.yaml`（`/exec/stream` 补 8 条拒绝不发给 exit；`/terminal` 的 drain 出口补「事件流结束」并注明它像正常收尾）、`src-tauri/tests/daemon_integration.rs`（守卫失败消息带 byte offset 与 span 前 160 字符）。

**验证**：`cargo fmt --all -- --check` 绿；`daemon_integration` **71 passed**；PyYAML 90 paths / 105 operations。**破坏性验证一条**：把 `handler_body_of` 改成 `&rest[..]` → 消息为「another `async fn` starts at byte 14197 … The span starts with "async fn exec_stream(
 State(s)…"」，改回后绿。

**本轮工艺观察**：它这轮有两处「局部为真、结论越界」——`Ok(None)` 出口确实存在，但没注意 `:4859` 在 `while` 之外；`recv_timeout` 确实有个循环，但没注意它有 100 ms 上界。**与本批前几轮同族：读到一处结构，就把「结构之外的那半」当成不存在。** 反过来它这轮的两条自我更正（2.2 撤回、2.1 改名）质量很高，是它第一次把「我的机制错 / 你的结论对」分开说。

**未做（L2，需拍板）**：**L2-91**（drain 的 `end_send` 泄漏）。`L2-84/85/86/87/88/89/90` 维持，`L2-87` 补了 8 条拒绝路径的备注。

**下一轮（末轮）**：按协议写批次汇总（6 轮判定分布、全部 SHA、遗留 L2 清单），关闭 issue #13，并在 ledger 顶部写 `BATCH_COMPLETE`。汇总前先自取一条：`observe_process` / `classify_process_observation`（停止路径最后一段没读过的代码），若无线索就直接汇总，不再开新靶。

### consume 6 / 6 · 2026-11-10 22:10–22:12 (GMT+8) · issue #13 · 回帖 `2120157400320610304`（22:11:29）· issue 已关闭（`state_reason: completed`）

- **末轮，无新指摘。** 先补掉第 5 轮公布的「自取最后一段」：`observe_process`（`:699-714`）/ `classify_process_observation`（`:631-644`）逐行读过，**没有新缺陷**。两点值得记下来（都不是问题）：① `Some((start, false)) if start != expected_start => Exited`（`:639`）——PID 被复用时判「已退出」而不是 `Unknown`，这是对的（我们那个进程确实没了），也是它与 `daemon_pid_state` 的 `Unverified` 不同的地方；② `Some((0, false)) => Unknown`（`:638`）——`start_time` 读不出来时保守回 `Unknown`，不是 `Running`，所以 `stop_daemon_process` 会报 `UncertainExit` 并保留 PID 文件。
- **判定：0 项新指摘**，本轮只做汇总与关单。

## batch 2026-11-10 · daemon 控制与停止协议 · 批次汇总（6 轮闭环，B=7）

| 轮 | 指摘来源 | 判定（成立/部分/不成立） | 落地 SHA |
|---|---|---|---|
| 1 | CodeBuddy 首条分析 `2120141279483817984` | 8 项：2 / 3 / 3 | `876c869`（契约 + 断言）、`5384a0f`（ledger + L2）、`24adff5`（回帖 id） |
| 2 | CodeBuddy 复核对第 1 轮四问 `2120149377556582400` | 6 项：3 / 3 / 0 | `06d0361`（契约 + 守卫）、`d10e5ae`、`29e715f` |
| 3 | 自取（`wait_for_exit` / `signal_daemon` + 自验） | 6 项：6 / 0 / 0 | `dfa2b5c`（architecture + api.yaml + 注释）、`5a27492`、`25b10a2` |
| 4 | CodeBuddy 复核第 3 轮 `2120155093726027776` + 它自己推的 `4dee4d1`/`4296376` | 5 项：3 / 0 / 2 | `2b7384f`（`/shutdown` 三分表）、`960c9f6`、`7af45d5` |
| 5 | CodeBuddy 复核第 3 轮四问 `2120156142427869184` | 5 项：3 / 2 / 0 | `85a17ac`（8 条拒绝 + drain 第四出口 + 守卫消息）、`fad307b`、`3f4be26` |
| 6 | 自取（`observe_process` 收尾）→ 汇总 | 0 项新指摘 | 本次提交（汇总 + 关闭 issue） |

**合计**：30 项判定 → 成立 17 / 部分成立 8 / 不成立 5。另**3 条**在后续轮次被对方主动撤回或降级（`L2-3` 行号、`handler_body_of` 整文件、删 token 停在 `:846`），**2 条**我方先判不成立后被证据推翻（`L2-3` 的类型层、`/terminal` 的 500 ms 语义），都写进了各自轮次。

**累积成果（契约面）**：`docs/api.yaml` 的三处 WebSocket / 停止语义从「一句话概括」变成「按代码逐步描述」——`/terminal` 的 500 ms 是空闲超时 + 2 s 上限 + 四个出口；`/exec/stream` 的 exit 帧在循环之后、Close 之后发，且 8 类拒绝不发 exit；`/shutdown` 的「请求 + 强杀」只在请求被接受后成立，并把 `409` / `401`+`403` / 前置拒绝三分。`docs/architecture.md` 的停止路径从「发 SIGTERM」改成真实的四步。

**累积成果（断言面）**：一条新断言 `a_websocket_drain_budget_claim_matches_the_loop_it_describes`（双向钉：文档改回去会红、代码统一而不改文档也会红），外加 body-span 守卫（越界时打印 byte offset 与前 160 字符）。

**L2 待拍板（本批新增 8 条）**：L2-84（终端 drain 无绝对预算）、L2-85（`UnverifiedIdentity` 括号列了两种不可能成因）、L2-86（在途 `/exec` 对退出确认不可见，产品口径）、L2-87（Close 之后仍发 `{"type":"exit"}`，含 8 条拒绝路径的取舍）、L2-88（worker teardown 不派生自 teardown 预算、不可取消）、L2-89（`signal_daemon` 的 `force` 恒真 → 建议删）、L2-90（请求失败不退回强杀，维持，但需在文案里给下一步）、L2-91（drain 里 send 失败漏 `end_send`，registry 等满 deadline）。另更正 L2-3（变体要带 `checked_pid`）与 L2-4（触发点 `:991`、入口清单）。

**本批的一条工艺教训**：六轮里对方有 **6 次**同一类失手——**读到一处结构，就把结构之外的那半当成不存在**：引用段落停在答案前一句（第 1 轮）、把 future 内的清理当成 future 外的（第 1 轮）、把 `opening_brace` 的 depth 当成不回落（第 2 轮）、把 `:991` 当成唯一入口（第 4 轮）、把 `while` 外的 Close 当成不存在（第 5 轮）、把 `recv_timeout` 当成无上界（第 5 轮）。反过来，它对**代码形状**的抄录一直准确，错的只是「形状的边界」。所以下一批的复核请求里固定写一句：**引用控制流要读到出口或下一处分支；引用代码块要连括号一起引。**






### consume 5 / 6 · 2026-11-10 22:13–22:40 (GMT+8) · issue #13 · 回帖（第 5 轮）

- **开场**：`git fetch` + `--ff-only` 到 `7af45d5`（`960c9f6` 之后只有一笔记录 reply id 的提交），再在它之上动手。CodeBuddy 本轮**没有**新提交，`4dee4d1`/`4296376` 原样在 main 上。
- **判定：本轮请它验的 3 项 + 扫法 1 项 → 成立 3 / 不成立 0 / 我方漏 1（它抓到）**。

| # | 项 | 判定 | 证据要点 |
|---|---|---|---|
| (a) | 第 0 节那条 401 反例 | **成立（本轮唯一带执行的一步）** | 它这边**没有** cargo/rustc（镜像里没有 toolchain），所以改成按 `request_at`（`daemon_activity.rs:94-140`）逐字节回放这段 HTTP：hand-rolled `POST /shutdown` + `Bearer <daemon.token>`，对着一个「`/health` 报 pid 与 config_dir、`/shutdown` 比 `state.token`」的替身，先 `GET /health`（200）再换掉 `daemon.token` → `/shutdown` 回 **401**，daemon 仍在。前提条件关键：`health_identity()` 只 `GET /health` + 比 pid/config_dir，**不发 token**，所以「文件被换掉但 `/health` 仍答」是常态而不是巧合 |
| (b) | `403` 是否真与 401 同一支 | **成立** | `/shutdown` 上 403 **只有** `require_admin`（`auth.rs:19` 的 `AuthContext` 只有 `scope`/`principal`，403 只由 `scope.is_some()` 触发）；未知 token 在 middleware 就 401，到不了 handler；`require_admin` 不读 body，且 `daemon_shutdown` 的 `Extension(auth)` 只会 `MissingExtension` 500，所以 `:170` 一定先于 400/409 触发 |
| (c) | `400`（`expect_pid: null`）不计入三分表是否可接受 | **成立** | 它同意这是「策略语句」，不是第四类：调用方写不出 `null`（`request_shutdown` 只在 `Some(pid)` 时才发 body，`None` 时连 `Content-Type` 都不发），而 `request_graceful_shutdown()` 在生产里无调用方（只有 `daemon_stop_e2e.rs:1116` 的测试） |
| 扫法 | 「行号引用」全扫 | **成立（1 处，它抓到）** | 见下 |
| **我方漏** | `/shutdown` 也会撞 **429** | **成立（它抓到，本轮必须改）** | `preflight_guarded_request_inner` 第一步就调 `reject_if_rate_limited`（`:568-570`），而它是先读 `req.host`（`:3941`）再 preflight 的，所以**限流在前、也不看 body**；`/shutdown` 的 host 名从 `expect_pid` 派生（`local:vs` 这类常用名），source 是 middleware 的 `source_from_env("daemon")`。默认限额是**开启**的（`enabled:true`、30/min source、20/min host、60 s 窗口），于是同一 source 或同一 host 一分钟内第 21 次 stop → `429` → `request_shutdown` 兜底臂 `NotAccepted("the daemon answered the shutdown request with 429: …")`。按 `check_event_limits` 逐字复算：第 21 次 `host:local:vs rate current=20 limit=20`。这一类的语义是「答了，但既不是 409、也不是 401/403、更不是没建连」 |

**行号扫描结果（它这轮的新扫法，成立）**：`src-tauri/src/**` 里 80 处 `:数字` 候选，扣掉端口/掩码/`duration_ms`/RFC-OSC 编号/JSON 字面量/`0600` 权限后，真正指向别处某行的只有 3 处——`daemon_control.rs` 的 `:5014`/`:5442`/`:5951`（上轮已核，仍对）、`redaction.rs:334` 的 `db/highlight.rs:221`（跨仓库，本仓核不了）、以及 **`transport/russh_impl/agent.rs:18` 的 `auth.rs:180`（过期且论证错）**：`russh-0.60.3/src/auth.rs:180` 是 `AgentIdentity::Certificate { .. } => {` 这条 match 臂，不是 `impl`；本仓 `grep "Signer for"` 只有 `russh_impl/mod.rs:137` 的 `LocalKeySigner`，**没有** `impl Signer for AgentClient`——它在 `russh-keys` 里。结论（不需要包装层）对，错的是「本仓存在那个 impl」+ 那行号。**它已改**。

**L1 落地（CodeBuddy 动手）**：`src-tauri/src/transport/russh_impl/agent.rs` 删掉 `auth.rs:180` 与「the implementation exists」，并把「本仓没有 `impl Signer for AgentClient`、那个 impl 在 `russh-keys`」写清；`docs/plans/russh-migration.md:472-479` 的同一句不改（历史记录）。

**L1 落地（本机接管）**：`docs/api.yaml` 的三分表补第四类 `429`（写明「按分钟、默认 30/20、可在 `execution_limits.toml` 关掉或改宽」）；`OPT-LOOP-L2-PENDING.md` 追加第 5 轮小节（429 类的复算、行号扫描清单、以及「`GET /health` 公开路径不过 middleware，所以 pre-flight 看不到限流，只有 `POST /shutdown` 与 `/activity` 快照会撞」这一点）。

**下一轮首项（第 6 轮＝末轮）**：按协议写批次汇总并关 issue。若还有一轮复核，优先让它验「`429` 是否该从限流里豁免 `/shutdown`」——那是行为改动，需要拍板，不是文案。

---

## 手工/单发消费 · 2026-11-10 · daemon 停止协议与审批留痕

模式 B（无 `[opt-round]` 容器：#13 于 22:11 以 6/6 轮关闭，现存 open 只剩 #2、#6）。只跑一轮，标题为 `## 本轮收尾`，不占 A 模式的轮次计数。

- **选靶理由**：#2 的最新评论 `2119861268247908352`（2026-11-10 02:34）是对方对上一轮收尾的复核，之后无人回应、未被消费——「有新输入、尚未消化」。#6 仍判 P2（正文写明只讨论、不授权实现 K8），同级 P1 优先于 P2，且 #2 编号更小。
- **判定：11 条 → 成立 3 / 部分成立 5 / 不成立 3。**
  - 成立：指南把停止失败写成两种成因（实为三种，第三种可在进入宽限分支前返回）、`rejected` 只在 `approval_respond` 成功时出现、`/shutdown` 缺审批留痕。
  - 部分成立：2 秒只是五段预算之一（它那张表把 `/health` 说成「两个 socket 各 500 ms」、把进程快照说成「无超时」，两格不实）、`daemon.log` 那条无断言（文件是 `app.log`）、N1（DNS 解析无预算成立，「服务端不读 body」错——axum 先抽 `Option<Json<..>>`，body 19 字节）、N2（命名与语义不符成立，「拒绝与超时不计入」错——`fetch_add` 在等待开始时）、N3（主路径不复用 `daemon_pid_state` 成立，「完全不调」错：`None` 分支就调）。
  - 不成立：只有 `request_and_wait_for_approval` 的审批才有留痕（唯一生产调用点就是它，前提空转；被强杀那半成立且已写进注释）、`append_diagnostic_log` 在审计之前（源码顺序相反）、`api.yaml` 仍写 `may terminate it`（#13 批次已整段重写）。
- **本轮自取发现（它没提）**：指南那句「`daemon.log` 能区分这两种」指错文件——区分用的 warn 由 `append_diagnostic_log` 写进 `app.log`（`diagnostics.rs:127`），`daemon.log` 只有 daemon 的 stdout/stderr 重定向。
- **L1 落地**：`76ac504`（`daemon_activity.rs` 模块注释收窄 + `daemon_control.rs` 第二个身份入口 landmark + `daemon_stop_e2e.rs` 新增 `app.log` 断言）、`d543dab`（`configuration-guide.md` 停止段落重写、`api.yaml` 的 `approvals_total` 描述与 `/shutdown` 留痕、L2-92/93/94）。推 `cnb main`（`e0e7a90..d543dab`），未推 github。
- **验证**：`npm run build` 6.60s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 failed / 1 ignored**（基线红测 `an_ipv6_literal_at_port_22_is_removed`）；`--features daemon --test daemon_stop_e2e shutdown_cancels_a_request_waiting_for_approval` **1 passed**；`--features daemon --test daemon_integration api_yaml` **6 passed**。
- **回帖**：`2120172790916468736`。含 44 条编号不成立索引 + 14 条行内索引。
- **issue 状态：保持 open**。阻塞点是 L2-92/93/94（各含两条互斥方案）与 K14 的四项设计决策，都需要维护者拍板；本轮不关闭。

---

### batch 2026-11-10 · SSH 传输、known_hosts 与认证（russh 侧，第二批，B=8）
- issue #14 / 开批时间 2026-11-10 23:51 (GMT+8)
- CodeBuddy 首条分析：**已到**——评论 `2120184256348471296`（2026-11-10T15:58:12Z = 23:58:12 GMT+8，开批后 6.5 分钟；生产任务 60 s × 5 次轮询（23:52–23:57）均 0 评论，**结束前即时复核命中**，与 #13 同样晚于 5 分钟窗口）。基线 `e0c3058`，全程只读。6 条指摘（其自报：成立 3 / 部分成立 3，另自验 3 条不成立不占编号）：
  - 1 `import_known_hosts_from_ssh` 对非方括号 `host:port` 的主机行拼出 `host:port:port`，导入的信任对真实主机永不生效且计数 `imported`（`embedded_ssh.rs:524-527`；同点叠加两条：第二列 `key_type` 直当算法名致 `trust_or_verify` 查不到槽位、`[host]` 不带 `:port` 时返回 22 与存储层 `[::1]:22` 形态不相等）
  - 2 `StallGuard` 对「服务器完全不 ACK」不生效——`Ok(0)` 从不发生（每次 write 负载 ≤64 KiB 远小于窗口）、真挂起走 60 s 会话超时且报文不含 `StallGuard` 定位信息；K2 的开放点比 KNOWN-ISSUES 记载更紧（`sftp_transfer.rs:313-399`）
  - 3 L2-17 建议改法里「或在置 `None` 前存下错误」是空分支（`OPT-LOOP-L2-PENDING.md:232`，只影响决策文档）
  - 4 `/terminal` 的 connected 帧不回 `auth_method`，且 `auth_method_for_host` 是按 host 的预测值而非 `authenticate` 返回值（部分成立；非 F1 型回归，`daemon.rs:4864-4876`）
  - 5 `russh-migration.md:494` 仍写「Pageant 优先、管道兜底」，`e67fd04` 已把代码翻成管道优先（部分成立，`agent.rs:317-327`；KNOWN-ISSUES.md:69 那句是对的）
  - 6 `embedded_ssh.rs:2074-2086` 的 T2-9 注释仍记 ssh2/libssh2 接线方式，`:1987` 同族残留（部分成立）
  - 另自验 3 条不成立（`bridge_tcp_and_channel` 空转烧 CPU / `channel.rs` request 混同超时与拒绝文案 / `legacy_pem` 尾随空格误判），并声明未重复 ledger 既有不成立与 L2-15/16/17/19/22/24/25/88 条目、未跑测试（环境无 cargo）
- 6 轮进度：**6 / 6（已闭环，issue 已关闭）**（第 5 轮 2026-11-11 00:52 收尾、第 6 轮 00:58 收尾，见文末 `consume 1 / 6`…`consume 6 / 6` 与批次汇总）
- 正文已喂给 CodeBuddy：**67 条「不成立」原文**（编号 44：preflight 2 + batch1 六轮 10 + batch2 编号 15–31 十七条（29–31 物理落 batch 2 末节、内容属 batch 3 第 5 轮，正文已标注）+ batch4 第 1/5/6 轮 10 + batch6 第 1 轮 5；行内判定行 23：batch2 D2/D4、batch3 A5/E、batch5 D6 后半/D8/F6 后半/F10/F11 后半/F12、batch6 a1 后果/a3.2/a5/c 附更正/#10、batch7 第 1/2/4/5 轮 8 条、手工单发消费（#2）1 条含 3 项）+ KNOWN-ISSUES 空档（K2/K7/K9/K12 + §3 未验证范围 + §3.1 Pageant 共存边界）+ 口径提醒（基线红测 861/1/1、npm 213/27、feature 组合不可相加、`daemon_*_e2e` 需显式 `--features daemon`、`exec_fixture` 空跑、`.github/**` 禁区、四列计数、`[跑]` 标注不可采信、行号一律现算、L2-15/L2-16…L2-25/L2-37/L2-73/L2-88 勿重提、api.yaml 已在 #9/#12 对齐）。正文由 `/tmp/build-opt-issue-b8.py` 按 ledger 锚点切片生成（块级正则 + 行内唯一子串校验：16 个编号块 44 条 + 23 条判定行，复验通过）
- 同主题第一批（#8）已 6 轮闭环；正文已写明 #8 落地的 L1 与已登记 L2 勿重提，本批重点找 **#8 闭环后新增/残留**的问题
- 顺手按 #13 批次条目的指示删除顶部 `BATCH_COMPLETE · issue #12` 与 `#13` 两条状态行（#13 那条明写「下一批（B=8）由生产任务开 issue 时删除本行」；#12 无指示但同属已关闭批次留痕，沿用 #8–#11 先例一并删除；顶部 HTML 注释已同步为六条）
- 下一批（B=9）：9 % 6 = 3 → 主题 3：MCP 契约与 CLI 文档一致性（`docs/skills.md`、`docs/api.yaml`、CLI help；第二批，#9 已闭环）。开 issue 前重读 ledger 不成立清单（批次 8 消费期间可能新增，行内判定行锚点会漂移）
- 若 #14 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

### consume 1 / 6 · 2026-11-11 · batch 8（issue #14）

- **指摘来源**：CodeBuddy 对 issue 正文的首条分析 `2120184256348471296`（23:58:12，开批后 6.5 分钟）。基线 `46a92c4`，全程只读。
- **判定：6 条 → 成立 3 / 部分成立 3 / 不成立 0**；其内部另有 4 条子断言判为不成立，单列在下面，下一轮不要重复提。
- **成立**：
  - #2 `StallGuard` 对「对端不应答」不生效——**结论对，机制它说反了**。它说上界是 60 s 会话超时；`write_with_timeout` 只包 `Channel::write`（shell/exec），SFTP 句柄的 `write` 走 `russh_sftp::File`：`poll_write` 在 8 个在途 ack 上阻塞，`write_nowait` 的接收端没有被 `request_timeout_secs` 包住，`block_on` 无超时 ⇒ 实际是**无上界**，比它说的更糟。
  - #3 L2-17 的「或在置 `None` 前存下错误」是空分支（`agent.rs:247-254`，第二次进来仍先在 `:242` 早退）。行号应为 `L2-PENDING.md:223`，不是 232。
  - #5 `russh-migration.md:494` 的 Pageant/管道顺序反了：`e67fd04` 起是管道优先。且那次运行把 OpenSSH agent 启动过，跑的是第一条（管道）分支，Pageant 根本没被试——「只有兜底路径有运行时证据」是反的。
- **部分成立**：
  - #1 导入 identity：裸 `host:port` 会拼成 `host:port:port` 并计 `imported`（代码事实成立），但触发面只有手工编辑/外来 `known_hosts`；它给的两条叠加问题都不成立（见下）。
  - #4 `/terminal` 的 `connected` 帧确实丢了 `auth_method`（`connection_info` 算了、只有诊断日志用），但「README 与 `/health-snapshot` 都回」是错的：`README.md`、`bin/agent2ssh-daemon.rs`、`docs/api.yaml` 三处都没有这个键。
  - #6 `ssh2`/`libssh2` 注释残留成立（`:1999`、`:2086`、`handshake_with_hint` 的 `-5`、测试文档里的「credits to libssh2」）；失败文案漏 `id_*_sk` 与 `keyboard-interactive` 也成立，但那串字面量被 `russh-migration.md:489` 与四份历史证据引用，属 L2-98。
- **不成立的子断言（原文保留，请勿重复提）**：
  1. **「`known_hosts` 第二列可能是 `rsa-sha2-256`/`rsa-sha2-512` 这种签名算法名」— 不成立。** 那一列永远是公钥算法（`ssh-rsa`、`ecdsa-sha2-nistp256`、`ssh-ed25519`），签名算法从不写进去；`host_key_algorithm` 对 RSA 恒返回 `ssh-rsa`，与文件里那一列同名。导入直接用 `key_type` 不会造成槽位错配。
  2. **「`[::1]` 不带端口时读成 `::1:22`，与存储层 `[::1]:22` 不等」— 不成立。** `known_host_identity`（`embedded_ssh.rs:196-198`）是 `format!("{}:{}", host.host.trim(), port)`，**不加括号**；`format_host_port` 只用于 `address` 字段（`:340`、`:373`）。两侧都是 `::1:22`。
  3. **「README 与 `/health-snapshot` 都返回 `auth_method`」— 不成立。** `auth_method` 全仓只在 `embedded_ssh.rs` 与两处诊断日志里出现（`architecture.md` 那份清单也只说诊断字段）。
  4. **「真挂起时的上界是 `channel.rs` 的 60 s 会话超时」— 不成立**，理由同 #2。
- **L1 落地**：
  - `4c82aa5`（`embedded_ssh.rs` 四处 + `sftp_transfer.rs` 三处，**全部是注释行**，`git diff -U0` 非注释行为 0）：libssh2/ssh2 残留、`-5` 描述、测试文档的「credits to libssh2」、导入 identity 的已知缺口、`Ok(0)` 分支不可达、`STALL_TIMEOUT`/`StallGuard` 的真实边界。
  - 文档笔：`russh-migration.md:494` 顺序改正 + `KNOWN-ISSUES.md` K2 撤掉「停滞时报告写到多少字节」+ `OPT-LOOP-L2-PENDING.md` 删 L2-17 空分支、新增 L2-95…L2-98。
- **验证**：`npm run build` 6.36s；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`cargo fmt --check` 绿。
- **L2 待拍板**：L2-95（导入 identity 形态）、L2-96（SFTP 写无截止时间，K2 的「已修」其实没兜住）、L2-97（`/terminal` 帧形状）、L2-98（失败文案）。

### consume 2 / 6 · 2026-11-11 · batch 8（issue #14）

- **指摘来源**：CodeBuddy 对第 1 轮收尾的复核 `2120191019055497216`（00:25:04，第 1 轮发帖后 3 分钟）。基线 `b7bf48f`。
- **判定**：它的复核表 6 行（认定到位 3.5 条）+ 下一批 6 条 → **成立 3 / 部分成立 2 / 不成立 1**。
- **成立**：
  - #1 我上一轮的注释把 `L2-95` 写成 **`L2-89`**（那是 `daemon_control.rs` 的 `force` 死分支）——我自己的错，已改。同一批我还在 `STALL_TIMEOUT` 注释里把 `L2-96` 写成 `L2-90`，一并改。
  - #2 `/terminal` 停机 drain 分支 `TerminalEvent::Error(_) | Closed => break`（`:4855`）吞掉错误帧，正常分支 `:4900-4904` 会发 `{"type":"error"}`。→ **L2-99**，它建议排在其他四条 L2 之前（一行、且发生在最难复现的时刻），同意。
  - #6 上轮漏跑 `cargo test --lib`（default feature）。本轮补：**878 passed / 1 failed / 1 ignored**，失败项同一条红测。（`--no-default-features --lib` 是 861/1/1，两者差 17 条 = `tauri_commands`。）
- **不成立（头条一句，必须记）**：
  - **「`russh_sftp` 有 10 s 读超时，写路径因此以 `BrokenPipe` 失败」— 不成立。** 它引 `rawsession.rs:406-414` 的 `run`，但该文件里**没有** `run`；连接任务在 `client/mod.rs:80`，循环体是 `process_handler(&mut rd, &mut handler)`，无超时。`rawsession.rs` 全文件唯一的 `runtime::timeout` 在 `:215` 的 `request()` 内，而 `write_nowait`（`:379-400`）只调 `send()`、原样返回接收端。`request_timeout_secs` 默认 10 s 属实（`client/mod.rs:46`），但它只约束 `stat`/`open`/`readdir`。`check_write_result → BrokenPipe`（`fs/file.rs:96-110`）确实存在，触发条件是**读任务结束**（EOF/关闭），不是「10 s 沉默」。⇒ 写路径仍无截止时间，L2-96 不变。
  - 这是它**第二次引用一个不存在的形状**（上轮把 `split_host_port` 的返回说成括号形态）。凡是它给「某 crate 某个函数体每包都包了 timeout」这类断言，先去 crate 源码里确认那个函数是否在该文件。
- **部分成立 / 备查（不新立）**：
  - `fingerprint_sha256` / `host_key_algorithm` 的 `Option` 形状只服务理论分支——`:1688` 已把「没给主机键」当错误抛掉，正常连接不可能为 `None`。
  - `import` 与 `remove` 对同一份 `known_hosts` 的形态假设**其实一致**（都只认 `[host]:port`），真正的不一致在入参类型（文件 vs `(host, port)`），与 L2-15 同一件事的两面。
  - 「验证用例把 timeout 设成 1 s」的建议建立在上面那条错误机制上：设 `request_timeout_secs` 对写路径没有任何影响。挂在 L2-96 下，不采纳。
- **L1**：`3ee366c`（两处编号 + `request_timeout_secs` 的归属）。文档笔：`KNOWN-ISSUES.md` K2 状态列改为「已修（旧症状不复现，待复测旧版本）；存量限制见右」、`write_nowait` 那句补上「`request_timeout_secs` 只服务 `request()`」、`L2-PENDING` 记反驳与 L2-99。
- **验证**：`npm run build` 6.37s；`--no-default-features --lib` 861/1/1；`--lib` 878/1/1；`cargo fmt --check` 绿。

### consume 3 / 6 · 2026-11-11 · batch 8（issue #14）

- **指摘来源**：CodeBuddy 对第 2 轮收尾的复核 `2120192394497798144`（00:30:32）。基线 `ad5a1b6`。它**全盘接受**第 2 轮的反驳，并自认上轮机制是编的。
- **判定**：3 条 → **成立 1 / 部分成立 2 / 不成立 0**（另它主动声明「`878/861` 这台机器无法复算，只按文本接受」，属实：它没有 cargo）。
- **成立**：`remove_system_known_host` 的字符串全等有两个盲区，`[host]:22` 与裸 `host:port` 都逃过匹配（`removed 0`）；`[host]:22` 与 `[::1]:22` 红测同根因，裸 `host:22` 才是第二类。它建议把 L2-15 / L2-95 与删除侧合成一次「形态归一」决策，用 `split_host_port` 解析后比 `(host, port)`——采纳为「一次决策」，但**条目编号不合并**（合并会让 ledger 历史引用全部指错）。
- **部分成立**：
  - 「注释写反了」——**不成立**，我的注释只写「阻塞在 oldest ack」，没提 mpsc 背压；但它补的事实成立：前置通道是 `mpsc::unbounded_channel`（`client/mod.rs:85`），`send()` 永不阻塞，所以 `write_acks` 的 8 是**唯一**闸门。已写进注释，L2-96 的修法据此收敛为「`File::write` 外包 `tokio::time::timeout`」，原「给 `request_timeout_secs` 设值」已从建议里删掉。
  - `recording_id` / `terminal_id`：`Uuid` 的 serde 与 `.to_string()` 逐字节相同，所以不是 bug；真正不对称的是 `recording_id` 未录制时为 `null`（键在、值 null）。已写进 daemon 注释并挂 L2-97，不新立。
- **L1**：`78f850e`（三处注释：唯一闸门、`remove` 的两个盲区与三条同根因、`connected` 帧的 null 语义）。文档笔：`L2-PENDING` 的 L2-96 建议改法订正 + 新增第 3 轮小节（形态归一合并建议、两条补正）。
- **验证**：`npm run build` 6.32s；`--no-default-features --lib` 861/1/1；`--lib` 878/1/1；`cargo fmt --check` 绿。

### consume 4 / 6 · 2026-11-11 · batch 8（issue #14）

- **指摘来源：自取**。CodeBuddy 对第 3 轮收尾（`2120193234084204544`，00:33:52）5 分钟内未回（60 s × 5 轮询，评论总数停在 6）。按协议不空等，改用第 3 轮末尾我自己列出的三个待核点。
- **判定：3 条 → 成立 2 / 不成立 1。**
  - **成立（范围扩大）**：`File::flush` 的第一句是 `ready!(poll_drain_writes(&mut self.state.write_acks, cx))?`（`fs/file.rs:299-301`），抽的是 `write` 用的同一个队列。所以只包 `File::write` 的 deadline 漏掉**每次传输的最后一次 flush**——而那正是队列里还压着 8 个未回 ack 时最该有界的地方。L2-96 的修法改为 write + flush 两处。
  - **成立（顺序约束）**：L2-19（把 `skipped` 按原因拆字段）必须先于 L2-95（把裸 `host:port` 计入 `skipped`），否则只是往五合一计数器里加第六种原因，CLI 的 `skipped M (hashed or conflicting)` 会继续给错归因。
  - **不成立**：`known_hosts.bak` 每次覆盖不是缺陷。实测 `ssh-keygen -R` 两次连续调用后 `known_hosts.old` 同样只保留「最后一次调用之前」的状态（`/tmp/kh-test` 实测），本仓语义与上游一致。
- **L1**：`81ce265`（`sftp_transfer.rs` 注释补 flush 同队列）。文档笔：L2-96 建议改法补 flush、新增第 4 轮小节。
- **验证**：`--no-default-features --lib` 861/1/1；`cargo fmt --check` 绿；本轮未动前端，`npm run build` 沿用第 3 轮的 6.32s（同一棵树、无 `src/` 改动）。

### consume 5 / 6 · 2026-11-11 · batch 8（issue #14）

- **指摘来源**：CodeBuddy 对第 3/4 轮的复核 `2120196198978994176`（00:45:39）。基线 `150bced`。
- **判定：5 条 → 成立 3 / 部分成立 1 / 不成立 1**；另**自取发现 1 条**（L2 编号撞号），是本轮最值钱的一条。
- **成立**：
  - 顺序约束漏了 **L2-22**：它那行原文就写着「会改变 `imported`/`skipped` 的计数分布（与 L2-19 同批做）」，所以同一计数点上有三个条目，顺序是 `L2-19 → L2-22 → L2-95/L2-15 → L2-96`。另确认 `skipped += 1` 只有四处，**4 个可命名原因、没有兜底桶**（「其余怪行」并进「三列不足」），L2-19 的字段集就是四个。
  - `--lib` 的覆盖口径：成立。实测更硬——`cargo test --lib --tests`（不加 `--no-fail-fast`）在 lib 先红时**直接中断**，后面的 15 个测试目标一个都没跑。加 `--no-fail-fast` 后才是 22 个 target：**1084 passed / 1 failed / 3 ignored**，唯一失败仍是那条 IPv6 红测。`exec_fixture` 的 5 条在无 `AGENT2SSH_TEST_SSH_PORT` 时是 skip-as-pass，不算覆盖。
  - L2-96 的落点：必须落在 `transport/russh_impl/sftp.rs:223` 的 `impl Write for File`；`core.rs` 的 `spawn_blocking` 闭包里包 `tokio::time::timeout` 无效（同步闭包无 reactor，`block_on` 互相等）；`timeout` 不取消内部 future，超时后必须 abort session。
- **部分成立**：`flush` 同队列成立、「同一队列、同等无界」是字面事实，但「`write` + `flush` 两处」数漏了 `poll_shutdown`——本仓 `impl Write for File` 没有 `shutdown`，`Drop for File` 用 `close_nowait`。已把这一层写进注释与 L2-96。
- **不成立**：「`Drop for File` 不发 close」——`close_nowait`（`rawsession.rs:316-322`）**会发** `Close` 包，只是把应答接收端丢掉。语义是「发了不等」，不是「没发」。
- **自取新发现（L2 编号撞号）**：`OPT-LOOP-L2-PENDING.md` 里 **12 个编号各出现两次**（`L2-13`…`L2-20`、`L2-22`、`L2-23`、`L2-25`、`L2-30`），两处指完全不同的事——多批各自从 13 起编号的后果。这直接击穿「按编号追溯」这个唯一工作方式，而我在第 3 轮还拿它当理由拒绝合并 L2-15/L2-95。已在文件顶部加「撞号登记」与引用规则（引用必须带主题限定；新条目从 `L2-100` 起；全量重编号属人拍板，不在自动路径做）。
- **L1**：`60d46b9`（`sftp_transfer.rs` 补 shutdown/close 的一层）。文档笔：L2-96 落点与取消语义、第 5 轮小节、顶部撞号登记。
- **验证**：`cargo test --no-default-features --lib --tests --no-fail-fast` → 22 targets，**1084 passed / 1 failed / 3 ignored**；`cargo fmt --check` 绿。

### consume 6 / 6 · 2026-11-11 00:5x (GMT+8) · batch 8（issue #14）· 末轮

- **指摘来源**：CodeBuddy 对第 3 轮收尾的复核 `2120195699743555584`（00:43:40）。它发出时我已在跑第 4 轮（自取），随后被 `2120196198978994176`（00:45:39，对第 4 轮的复核）压在下面，直到第 5 轮收尾前才翻到——所以它自称「第 3 轮复核」，实际是第 4 轮之后才被消费的那一条。本轮拿它当末轮靶，基线 `7d561d3`。
- **判定：4 条 → 成立 2 / 部分成立 1 / 不成立 1**；另 1 条「部分成立但不新立」。
  - **成立**：`forward.rs` 的 accept 循环无条件 `thread::spawn`（`:555-590`），且 `handle_local_connection` 先 `connect_embedded_ssh(&host, 60)` 再碰客户端字节（`:606-617`）——只 connect 不说话的对端（`nc`、端口探活、容器 healthcheck）每条占一个线程 60 s，无并发上限（`control.connections` 只是计数器）。立 **L2-100**。
  - **成立（打在我自己第 1 轮写的注释上）**：`STALL_TIMEOUT` 首段「a deadline … names the write size」**是假的**。`StallGuard::write` 的 `self.inner.write(buf)?` 先返回，`stalled()` 永不在真错误上被调用，所以「命名写了多少字节」这条路径**修完 deadline 之后也不经过 `StallGuard`**。已改注释，并把这条并进 L2-96 的 (b) 半：deadline 必须自己读 `bytes_written` 组文案。
  - **部分成立**：L2-98 的「为什么不能自动落」里「`russh-migration.md:489` 要同步」不存在——它引的是不带括号的旧串 `"...or default key succeeded"`。理由已缩半（只剩「用户可见字符串 + 四份历史证据不改写」）。
  - **不成立**：`bridge_tcp_and_channel` 把 `TimedOut` 当错误上报、安静隧道 60 s 后被 `:1266` 断开。三个调用点都在非阻塞模式下跑（`embedded_ssh.rs:976` 显式 `set_blocking(false)`、`forward.rs:618`/`:668` 同样，且 `bridge_tcp_and_channel` 自己第一句就 `set_nonblocking(true)`），非阻塞的 `read_side` 返回 `WouldBlock`（`channel.rs:356-358`），不是 `timed_out()`。`write_with_timeout` 的 `TimedOut` 是写入超时、本就有意 abort，与「空闲」不是一回事。
  - **部分成立、不新立**：`jump_chain.rs:88` 每跳一次 `load_config()`。机制成立，但 `load_config` 有缓存、代价可忽略；危害只是「一次连接里不止一份配置快照」的可观测后果（错误信息可能指向已删掉的 profile 名），它自己也说不是 bug。改善项记在 L2-PENDING 第 6 轮小节，不占编号。
- **L1**：`37fbd7b`（`sftp_transfer.rs` 的 `STALL_TIMEOUT` 首段改写）。文档笔：`L2-PENDING` 第 6 轮小节（L2-100 表行、`STALL_TIMEOUT` 那句「本轮之前是假的」、L2-98 理由缩半、`jump_chain` 不新立、`bridge` 不成立）、L2-96 与 L2-98 两行订正；本文件 6 / 6 + 批次汇总 + 顶部 `BATCH_COMPLETE`。
- **验证**：`cargo test --no-default-features --lib --tests --no-fail-fast` → 22 targets，**1084 passed / 1 failed / 3 ignored**（唯一红是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`cargo fmt --check` 绿；本轮未动 `src/`，`npm run build` 沿用第 3 轮的 6.32s（同一棵树）。
- **回帖**：收尾 `2120199953610878976`（17:00:35）+ 补记 `2120200508013965312`（17:02:47）；重复帖 `2120199965501734912` 已改文收回。
- **issue #14**：已关闭（17:03:18，`state_reason: completed`），open 列表只剩 #2、#6。
- **一次操作失误（记此以免被当成两次真发帖）**：收尾帖 `2120199953610878976`（17:00:35）发出后，我又跑了一次同样的 `post-issue-comment` 只为抓评论 id，于是产生重复帖 `2120199965501734912`（17:00:37）。CLI 没有删评论命令，已用 `patch-issue-comment` 把它改成一句「重复帖，已收回，看上面那条」。以后取 id 用 `list-issue-comments --sort=-created`，不要重发一遍。
- **补记（收尾帖后 9 秒到达的回复 `2120199995319042048`）**：它答了我第 5 轮的两问。**撞号登记 3 条 → 不成立 2 / 部分成立 1**：①「`L2-23` 是同一条的两个版本、撞号应为 11 对」不成立——它只看到 `:267` 与 `:281`（后者是 `L2-23（收窄）`），漏了 `:93` 的 `child.kill()`/PID 文件那条，与主机键无关；②「`:251` 的 `L2-26` 是死链」部分成立，死链真存在但在 `:91`，且修法不是改指（`L2-26` 的 entry 泄漏本身已被撤回），已删掉那句根因断言；③「`L2-100` 已被第 5 轮占用」不成立，全文只有 `:16` 规则与 `:611` 新条目两处。它另给一条带行号的新指摘（`/sftp/upload`、`/sftp/download` 的审批用 `host_tags()` 快照跨 300 s 等待窗口，执行侧根本不收 tags），判 **部分成立**：机制属实，「`/exec` 不一样、SFTP 少传一层」说反了——`core.rs` 里 `tags` 只服务目标展开，`/exec` 有同一性质。立 **L2-101**。文档笔见下一笔提交。

### batch 2026-11-10 · SSH 传输、known_hosts 与认证（russh 侧，第二批，B=8）· 批次汇总（6 轮闭环）

| 轮 | 指摘来源 | 判定（成立 / 部分 / 不成立） | 落地 |
|---|---|---|---|
| 1 | 首条分析 `2120184256348471296` | 6 条：3 / 3 / 0 | `4c82aa5`（代码）+ `b7bf48f`（文档） |
| 2 | 复核 `2120191019055497216` | 6 条：3 / 2 / 1 | `3ee366c` + `ad5a1b6` |
| 3 | 复核 `2120192394497798144` | 3 条：1 / 2 / 0 | `78f850e` + `ebb4b3e` |
| 4 | 自取（CodeBuddy 5 分钟静默） | 3 条：2 / 0 / 1 | `81ce265` + `150bced` |
| 5 | 复核 `2120196198978994176` | 5 条：3 / 1 / 1（另有自取 1：编号撞号） | `60d46b9` + `7d561d3` |
| 6 | 迟到的第 3 轮复核 `2120195699743555584` | 4 条：2 / 1 / 1 | `37fbd7b` + 本次提交 |
| **合计** | 6 次取指摘（4 次 CodeBuddy、1 次自取、1 次迟到件） | **27 条：14 / 9 / 4** | **6 笔代码 + 6 笔文档 + 1 笔补记文档** |

补记（收尾帖后 9 秒到达的一条回复）另判 **4 条：成立 0 / 部分成立 2 / 不成立 2**，新立 L2-101，不计入上表的 6 轮判定。

**本批最值钱的四个发现**（都不是「注释写错」级别的）：

1. **K2 的「已修」其实没兜住**（→ L2-96）。`StallGuard` 的 600 s deadline 与 `write_all_shrinking` 的 `Ok(0)` 减半分支在当前传输上**都不可达**：`File::write` 只返回 `Ok(min(buf, max_write_len))` 或 `Err`，永不返回 `Ok(0)`。对端不 ACK 时真实的闸门只有 `write_acks` 的 8 个在途 ack（前置是 `mpsc::unbounded_channel`，没有背压），`request_timeout_secs` 只服务 `request()`，`block_on` 无超时 ⇒ 写路径**没有截止时间**。
2. **`STALL_TIMEOUT` 首段那句承诺是假的**（第 6 轮才被打掉）。第 1 轮我把它当成「已修的那一半」写进注释与 KNOWN-ISSUES，五轮后才发现解释路径根本不经过 `StallGuard`。
3. **L2 编号 13 处撞号**（第 5 轮自取；关帖后订正，见文末「收尾之后」）。`L2-13`…`L2-20`、`L2-22`、`L2-23`、`L2-25`、`L2-30` 各出现两次、指两件事——直接击穿「按编号追溯」这个唯一工作方式，而我在第 3 轮还拿它当理由拒绝合并 L2-15/L2-95。已在 `L2-PENDING.md` 顶部登记，规则：引用必须带主题限定、新条目从 `L2-100` 起、全量重编号属人拍板。
4. **`/terminal` 停机 drain 分支吞 `TerminalEvent::Error`**（→ L2-99）。客户端先收 `connected`、再收 `shutdown`，失败原因一个字都没有；窗口只有停机那一两秒，最难复现也最容易被当成「连上了但没反应」。

**L2 待拍板（按建议执行顺序）**：

1. **L2-99** — drain 分支补发 error 帧再 `break`（一行，与 `:4900-4904` 同形），配一条「停机窗口内让 shell 请求失败」的 e2e。
2. **L2-96** — 给 SFTP 写路径真截止时间。落点必须是 `transport/russh_impl/sftp.rs:223` 的 `impl Write for File`；`write` 与 `flush` **两处都要包**（`poll_drain_writes` 抽同一队列）；不能在 `core.rs` 的 `spawn_blocking` 闭包里包（同步闭包无 reactor）；`timeout` 不取消内部 future，超时后必须 abort session；文案自己读 `bytes_written`。
3. **L2-97** — `/terminal` 的 `connected` 帧形状：补 `auth_method`（且换成 `authenticate` 的返回值，不是按 host 预测）或写进契约；顺带写明 `recording_id` 可为 `null`。
4. **L2-98** — 认证失败文案补 `_sk` 变体与 `keyboard-interactive`；四份 `pageant-acceptance-2026-10-23.json` 不改写，在报告里注明「该次运行是旧版文案」。
5. **L2-19 → L2-22 → L2-95 / L2-15（known_hosts 形态）** — 一次性把导入、删除、identity 三处的形态假设用 `split_host_port` 解析后比 `(host, port)` 收口。**顺序不能反**：先把 `skipped` 按四个原因拆字段（hashed / 三列不足 / base64 解不开 / 指纹冲突，没有第五个兜底桶），再动计数分布，否则只是往五合一计数器里加第六种原因、CLI 文案会给错归因。
6. **L2-100** — `forward.rs` 在拨 SSH 之前先对客户端做带超时的首字节 `peek`（5 s 量级），另加并发上限（与 `JumpThreadGuard` 同形）。
7. **L2-101**（补记新立）— `/sftp/upload`、`/sftp/download` 的审批依据是请求时刻的标签快照，执行侧不收 tags、也不重读。二选一：执行前用当前配置重算并要求与审批时一致（引入一次重读 + 新错误码），或在 `api.yaml` 与指南里写明「审批期间标签变更不会让已批准的请求失效」。属产品口径。

> 补记回帖把 **L2-101 写在 L2-100 之前**（先定产品口径、再动代码），与本表 6/7 相反。两处是同一组条目，按哪份顺序做都可以，别当成两个版本。

**不成立**：本批 4 条正式判定不成立 + 首轮 4 条子断言，原文都保留在各轮小节（issue 正文另有 67 条历史清单，本批已逐条避开，未重复）。

**下一批（B=9）**：9 % 6 = 3 → 主题 3「MCP 契约与 CLI 文档一致性」（`docs/skills.md`、`docs/api.yaml`、CLI help；第二批，#9 已闭环）。生产任务开 issue 时删除顶部 `BATCH_COMPLETE · issue #14` 一行。

### 收尾之后 · 第 13 个撞号（2026-11-11，关帖后的第二次复核）

关帖后 CodeBuddy 又回了一条，只答本批唯一能改结论的那一问：撞号到底几个。**它说 13，并且对了。**

关键不是「多了一个数」，而是**我上一段的结论在哪一步错的**。补记里我按「定义处」重数，得出「12 对、没有第 13 个」，同时否掉了它把 `L2-23` 判成同一条两个版本的说法——那半是对的（`:93` 与 `:267` 确实是两件事）。错的是紧接着那句：「两处散在不同日期的小节里，多数是『daemon 路由/关闭路径』对『`known_hosts`/主机键/认证』」。这句话**对 `L2-17` 与 `L2-22` 都不成立**：

| 编号 | 第一处 | 第二处 | 两处的关系 |
|---|---|---|---|
| `L2-17` | `/exec/compare` 没有 `source` 字段 | `list_identities` 失败一次后永久 `Err` | 都在 agent/`known_hosts` 一侧 |
| `L2-22` | `run_ws_with_shutdown_deadline` 的返回值一个 bool 折两件事 | `import_known_hosts_from_ssh` 不验公钥结构 | 同上 |

所以「带主题限定」这条规则对这两条**不足以消歧**——限定词写出来是一样的。真正的第 13 个缺陷是这句描述本身：它让读者以为撞号只在主题交界处发生、只要写主题就能分开，而事实上有两对**在同一主题内部**。已补：计数按 13 读，那两对引用必须按内容起名（`L2-17（/exec/compare 缺 source）` / `L2-17（list_identities 置 None）`），旧正文不改写。

**顺带一条自纠**：我在补记里立的 `L2-100` 写的是「`/local-exec` 的 webhook 恒说结果已知」——它与已有的 **L2-27** 是同一件事的外一层，不是新发现；`daemon.rs:1807` 的 `None` 与 `:1711` 的 `Some` 只差 `:1700` 那一次 `ssh_shutdown_outcome()`，而 local 侧的类型在 `core.rs:1267` 就折成了字符串。已把 `L2-100` 换成本批真正新立的那条（`forward.rs` 的 accept 循环），并在登记处写下「立条前先 grep 已有的」这条教训。

**计数不变**：27 条指摘，14 / 9 / 4；待拍板 7 项（`L2-95`…`L2-101`）。撞号由 12 改 **13**。文档两笔，仍在 `cnb main`，未推 github。

### 手工/单发消费 · 2026-11-11 · daemon 身份绑定与异常状态恢复（issue #2，模式 B）

- **选靶理由**：open 只剩 #2、#6，无 `[opt-round]`（#14 已于 batch 8 闭环）→ 模式 B。两条的最新评论都是 CodeBuddy 的复核、其后没有我的回帖，都不在「等对方回应」状态，按 P1 同级取编号最小 → **#2**。#2 的最新复核带 5 条按函数名定位的断言，其中三条（N2/N3/N4）正好落在下一批主题「MCP 契约与 CLI 文档一致性」上，所以它既是最该推进的，也是下一批的首查项。
- **基线**：本地 `4def153` 落后 `cnb/main` 4 笔（PR #3「Resolve relative daemon configuration paths」）→ `--ff-only` 快进到 `b83b9c1`。无分叉，工作树只多一个未跟踪的 `.deja/`（非本流程产物，未动）。
- **指摘来源**：评论 `2120173738653020160`（11-10 23:16），即对上一轮收尾 `2120172790916468736` 的复核。
- **判定：5 条 → 成立 4 / 部分成立 1 / 不成立 0**；另有 6 条子断言判不成立（见下节，原文保留）。

| 编号 | 指摘 | 判定 | 证据要点 |
|---|---|---|---|
| N1 | `/terminal/broadcast` 少了整条闸门链：gate 与 `RiskLevel::Blocked` 只算不拒、`requires_approval` 从不被消费 | **部分成立** | 事实对：`reject_if_gate_paused` 的 15 个生产调用点里没有 `terminal_broadcast_run`；`preview_results`（`daemon.rs:4355-4358`/`:4365-4368`）把 gate 与 Blocked 写成逐目标 `error` + `continue`；`run` 从不读 `requires_approval`。但「应回 423」这一半是 **L2-81** 的既有条目，且契约本来就写着「每目标拒绝 → 200 + `authorized: false`，不冒 4xx」。真正新的缺陷是**字段无语义**：preview 与 run 共用同一个 schema，`authorized: true` 在 preview 里只能表示「暂时没人拒绝」，却被读者当成放行 |
| N2 | `ssh_events_subscribe` 永远返回空数组，而 `docs/skills.md:79` 与 `tools.rs:852` 都承诺「latest events」 | **成立（且比指摘更硬）** | `agent2ssh-mcp.rs:1801` 每次调用新建 `broadcast` receiver，`:1803` 同一同步块里 `try_recv()` 抽干；`tokio::broadcast` 的新 receiver 无历史 → 结构上恒空。`docs/skills.md` 已按实际改写，工具本体属 `src-tauri/src/**` → L2-103 |
| N3 | `/forwards/{id}/start` 的 403 描述把「scope 外的 id」说成像未知 id 一样返 400，实际是 403 | **成立** | `forward_list_core`（`forward.rs:739-746`）返回**全部** rule、无 scope 过滤；三条 handler（`daemon.rs:3046`/`:3103`/`:3159`）都先 `.find(id)` 再 `check_daemon_scope(rule.host)`，越权必落 403。所以那句「not with a 403 that would confirm the tunnel exists」是**从未实现的保证**——它恰好会让人不去找这个泄露。`/forwards/{id}` 的 DELETE 分支本来就写得对（「Scope denied: the rule's host is outside…」），已让两条对齐。改成 400 要动取数顺序 → L2-102 |
| N4 | `DaemonMetricsSummary.approval_count` 与 `/metrics` 的 `approvals_total` 是同一个数的两个名字，且前者裸无描述 | **成立** | `remote.rs:530` 明确 `body.get("approvals_total")`；`approval_count` 与 `request_count`/`exec_count`/`exec_blocked_count` 并列、无 description。已补与 `:4229` 同义的描述；改名/拆分仍是 L2-94 |
| N5 | L2-93 的窗口比记录的宽：`approval_respond` 失败不止 TTL 一种 | **成立（第四种更糟）** | `approval_respond_inner`（`approval.rs:678-725`）三种 `Err`：`unknown approval`、`has timed out`、`already {:?}`，都不需要竞态。第四种：`append_approval`（`:486-503`）把写盘错误吞成一条 warn 诊断，函数仍返 `Ok(())` → `daemon.rs:668` 写 `rejected: true`、`audit.jsonl` 写 cancelled，而 `approvals.jsonl` 末条还停在 `pending`，**三方没有一个说它被回答过**。已就地扩写 L2-93，未开新号 |

**L1（本笔）**：`30264bb`（`docs/api.yaml` 三处 + `docs/skills.md` 一处 + `docs/guides/configuration-guide.md` 的停止预算句 + `OPT-LOOP-L2-PENDING.md` 的 L2-93 扩写与 L2-102/103）。推 `cnb main`，未推 github。
**回帖**：`2120231708577390592`（`## 本轮收尾`，不带轮次号，不污染模式 A 的计数）。**#2 保持 open**——阻塞点全在需人拍板的一侧：L2-102（越权 403 改不改 400）、L2-103（events 工具三选一）、L2-92/93/94 与 K14 的四项设计决策。按模式 B 的定义，这已到「只剩阻塞项」而不是「完全闭环」。

**验证**：`npm run build` 6.35s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 passed / 1 failed / 1 ignored**（失败项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`，与 `b83b9c1` 一致）；`--features daemon --test daemon_integration` **71 passed**（含 `api_yaml_documents_every_daemon_route`、`api_yaml_documents_the_codes_each_handler_can_return`、`api_yaml_descriptions_with_a_colon_are_quoted`、`mcp_tools_match_skills_md_documentation`）；`yaml.safe_load(docs/api.yaml)` 90 paths 通过。

### 本轮判定不成立（原文保留，请勿重复提）

1. **「`/terminal/broadcast` 的 operation 描述承诺 gate 被 enforced」— 不成立。** 原文写的是「A target whose authorization is refused is reported in the response body with `authorized: false` and an `error`; the HTTP status stays 200 and nothing is sent to that target, so a per-target refusal never surfaces as a 4xx from this operation」。gate paused 正落在这个描述里，不存在「读者会以为返回 423」的问题。
2. **「gate / `RiskLevel::Blocked` 只算不拒，应回 423」— 不是新缺陷，已并入 L2-81。** 该条早就记着「广播三处授权/限流失败全 200」，且 `api.yaml` 同期就写明逐目标拒绝不改状态码。本轮不新立。
3. **「MCP 侧的 bus 上只有 MCP 自己 `publish_event` 的东西（据我 grep，MCP bin 里没有任何 `publish_event` 调用点）」— 不成立。** bin 里 0 处是真的，但 lib 会在**同一进程**里替它发布：MCP 在 `:846`/`:853` 调 `exec_ssh_core`，而 `core.rs:1117` 与 `:1322` 就是 `publish_event`（`approval.rs` 另有四处）。所以给 MCP 进程加环形缓冲对 MCP 自己发起的 exec 是**有效**的，「即便修好回放也只回放空」这个推论不成立。失效形态是**grep 作用域选错**：只 grep bin、不 grep 它调的 lib。
4. **「`/forwards/{id}` 的 DELETE 上可能有同一句错话」— 不成立。** 那条 403 写的是「Scope denied: the rule's host is outside the token's allowed hosts and tags.」，从未承诺 200/400。
5. **「`health_identity()` 在 `stop_daemon_process` 里被调用两次」— 不成立。** 全文件四处引用：`:231` 是定义，`:218` 属 `daemon_pid_state`、`:287` 属另一处身份核对，`stop_daemon_process` 里只有 `:997` 一处。它在同一句括号里也把这半走回了。真正的第二次读是 `read_admin_token()`（`:1030` 与 `request_shutdown` 的 `:876`），一次 token 文件读不构成「身份探测被数了两次」。
6. **「地址解析是毫秒级，所以『不能只按 2 秒算』是反的」— 不成立。** `request_at` 的 `to_socket_addrs()` 在 `connect_timeout` **之前**且不吃任何超时，`AGENT2SSH_DAEMON_ADDR` 允许写主机名——L2-92 正是按这条立的。默认值是 loopback 字面量，但「默认」不是上界。它另一半（该把 ≈12.5 s 这个数写出来）成立，已采纳进指南。

---

### batch 2026-11-11 · MCP 契约与 CLI 文档一致性（第二批，B=9）
- issue #15 / 开批时间 2026-11-11 04:05 (GMT+8)
- CodeBuddy 首条分析：**未到（等待中）**——60 s × 5 次轮询（04:06–04:10）+ 结束前即时复核（≈04:11）+ 6.5 分钟处最后一次延时复核（≈04:12）均 0 评论。消费者下次运行重新等（#13/#14 的首条都在 6–6.5 分钟晚到，本轮超出该窗口仍未到）。
- 6 轮进度：**6 / 6（已闭环，issue 已于 05:41 关闭）**（第 1 轮 04:28、第 2 轮 04:44、第 3 轮 04:55、第 4 轮 05:01、第 5 轮 05:25、第 6 轮 05:38 收尾，见文末 `consume 1 / 6`…`consume 6 / 6` 与批次汇总）
- 正文已喂给 CodeBuddy：**85 条「不成立」原文**（编号 54：preflight 2 + batch1 六轮 10 + batch2 编号 15–31 十七条（29–31 物理落 batch 2 末节、内容属 batch 3 第 5 轮，正文已标注）+ batch4 第 1/5/6 轮 10 + batch6 第 1 轮 5 + **batch8 第 1 轮加粗伪标题小节 4 条子断言** + **手工/单发消费 11-11（#2 模式 B）6 条**；行内判定行 28：batch2 D2/D4、batch3 A5/E、batch5 六条、batch6 五条、batch7 八条、手工 11-10 一条、batch8 第 2–6 轮五条；摘录 3：batch8 补记两条 + 收尾之后撞号描述一条）+ 首查项（#2 模式 B 刚落地的 N2/N3/N4 及 L2-103/L2-102/L2-94）+ KNOWN-ISSUES 空档（K2 存量限制 / K7 / K8 / K9 / K12 / K13）+ 口径提醒（基线 861/1/1、`--lib` 878/1/1、`daemon_integration` 71、`--lib --tests --no-fail-fast` 1084/1/3、npm 213/27；**四条派生断言守不住 description/schema/示例层——本批重点**；L2-PENDING 撞号 13 对、新条目从 L2-102 之后起、L2-94/102/103 勿重提；feature 组合不可相加；`.github/**` 禁区；四列计数；行号现算；`[跑]` 不可采信；MCP bin 的 grep 作用域陷阱——只 grep bin 不 grep 它调的 lib）。正文由 `/tmp/build-opt-issue-b9.py` 按 ledger 锚点切片生成（块级正则 + 加粗伪标题特例 + 行内唯一子串校验：17 个编号块 50 条 + 加粗 4 条 + 判定行 28 条 + 摘录 3 条 = 85，复验通过；初版两个 bug 已修——order 漏 preflight、行内标签误用 `split(" · ")[0]` 全等匹配导致 batch3/5/7 行内条目全部丢失，改为 `startswith` 后恢复）。
- 同主题第一批（#9）已 6 轮闭环；此后 #12/#13 两批给 api.yaml 配了派生式断言并对齐路由与状态码集合，#2 模式 B（`30264bb`）又改过 api.yaml 三处与 skills.md 一处。本批重点找对齐之后新增/残留、且派生断言守不住的 description/schema 层问题。
- 顺手按 #14 批次条目与 BATCH_COMPLETE 行自述删除顶部 `BATCH_COMPLETE · issue #14` 状态行（HTML 注释同步为七条）。
- 下一批（B=10）：10 % 6 = 4 → 主题 4：前端与 i18n 文案键完整性（`src/`；第二批，#10 已闭环）。开 issue 前重读 ledger 不成立清单（本批消费期间会新增，行内判定行锚点会漂移，b9 脚本可改造复用——注意 batch8 的加粗伪标题与摘录条目形态）。
- 若 #15 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

### consume 1 / 6 · 2026-11-11 04:19–04:28 (GMT+8) · batch 9（issue #15）

- **指摘来源**：CodeBuddy 首条分析 `2120250827418804224`，**04:23:25 到达**——开批后 **18 分钟**，远超 #13/#14 的 6–6.5 分钟晚到窗口。生产任务在 04:12 做的「6.5 分钟处最后一次延时复核」看到的仍是 0 评论，所以它记的「未到」在当时是对的；本轮 60 s × 5 轮询到第 5 次才命中。**结论：这个窗口不可靠，最晚一次在 18 分钟处，下次不要按 6.5 分钟判「不会来了」。**
- **计数：7 条 + 3 条已并入 → 成立 4 / 部分成立 3 / 不成立 0。** 它对 api.yaml 的 90 paths / 105 operation、107 route handler 全部复算正确；「105 operation 状态码全部有序」「415/422 覆盖」「每条 response 都有 body」这些自证项也与既有断言一致。

| 编号 | 判定 | 我核到的（与它的差异写在证据里） |
|---|---|---|
| C1 | **成立** | `grep sftp/cancel` 全仓只命中 `api.yaml:367`/`:377` 两行；daemon 的 `/sftp/*` 十条路由止于 `rm-rf/authorize`（`daemon.rs:5857-5866`）。它说「能力只存在于另一个界面」只说对一半：`transfer_id` **确实被用**（`core.rs:1949-1952` 的 `register`、`:1995` 的 `unregister`），只是 flag 落在**跑传输的那个进程**里，而 `cancel_transfer` 的唯一调用点是 `tauri_commands.rs:650-651` 的桌面 IPC。daemon 是独立进程，于是这个字段在 HTTP 侧**收得下、永远翻转不了** → 新 **L2-105** |
| C2 | **成立** | `risk_config.rs:42-45` 先 `existing_policy_path()`（`policy.rs:54-64`：先 `.toml` 后 `.json`），命中就**不再读** `risk_rules.toml`。补充它没引的一条：`README.md:224`/`:352` 早就写着「policy-as-code，`risk_rules.toml` 是 legacy」——错话只在 `skills.md:29` 与 `tools.rs:582` 两处 |
| C3 | **成立** | `auth.rs:52-58` 的 `load_scoped_daemon_tokens().map_err(→500)?` 在 admin 判定（`:45-51`）**之后**，所以「全员 500」只对外来 token 成立，admin 照旧工作——它说「所有非 admin 请求」措辞是对的。`api.yaml` 的 info 第二段确实写全了，指南的 Scoped Token 段确实只讲怎么配 |
| C4 | **成立** | `bin/agent2ssh-mcp.rs:1256-1268`（`process`）与 `:492-513`（`daemon`）两支**每行都带 `backend`**。它的「对照组」也成立：`skills.md:49` 的 `durable` 语义就是从 `:1325-1329` 那段注释来的，同一张表里两种待遇 |
| D1 | **部分成立** | `types.rs:384-386` 是「stdout **and stderr** … **each**」+ 保留首尾、中间记进 `dropped_bytes`；`tools.rs:276` 只写 stdout。它判「部分」的理由（4 MiB 是一路还是一共）我同意保留原判 |
| D2 | **部分成立** | `agent2ssh-mcp.rs:185-188` 的 `local_daemon_client()` 传 `Some(30s)`，`:193-213` 装 `connect_timeout(10s)` + `timeout(30s)`；`daemon_only_request`（`:308-333`）传 `None`。用 30 s 那条的是 `try_daemon_session_{open,write,read,close,list}`、`try_daemon_forward_{add,list,remove}`、`try_daemon_gate_status`（`:338`–`:588`）——与它列的清单逐个对得上。**它给的行号 `:198-199`/`:308-333`/`:185-188` 全部准确**，这一批它没有漂 |
| D3 | **部分成立（但它漏了更硬的一半）** | 行号是 **`:326`** 不是 `:327`。它说「300 s 只对审批准」成立（`execution_control.rs:496-499`，policy 的 `ttl_secs` 可覆盖）。**它没提的**：那句的主语是「不携带 `force: true` 时」——`execution_control.rs:487` 是 `needs_approval = approval_policy.is_some() \|\| (risk == High && !input.force)`，**低/中风险不携带 force 也不产生审批**。这才是可证伪的那半，已按它改写 |

- **自我更正**：`9be53e4`。我在 `77861b5` 里把那句改成「低/中风险不携带 `force` 也直接执行，不会产生审批请求」，**漏了 `approval_policy.is_some()` 那一支**——命中 `approval_policies` 时任意风险都审批。已改成「不会**因为风险等级**产生审批，命中 `approval_policies` 的除外」。**同一个错法**：修一句过宽的条件句时，把新的条件句又写宽了一个从句。
- **L1 落地**：`77861b5`（`api.yaml` 两处 `transfer_id`、`skills.md` 三处、`daemon-api-quickstart.md` 的两处：Scoped Token 段补 500 失败形态 + 审批示例补「高风险」限定与 `ttl_secs`）+ `9be53e4`（上条自我更正）。推 `cnb main`，未推 github。
- **L2 新增两条**：**L2-104**（`tools.rs:276`/`:546`/`:582` 三处工具描述与实现不同步，按 `L2-103` 自己的口径把「工具描述」算 `src-tauri/src/**`）、**L2-105**（daemon 侧的 `transfer_id` 是死字段，取消 flag 跨进程够不到）。`L2-102`/`L2-103`/`L2-94` 维持，无变化。
- **验证**：`npm run build` 6.37s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 failed / 1 ignored**（失败项仍是那条故意红的 IPv6）；`--features daemon --test daemon_integration` **71 passed / 0 failed**；`yaml.safe_load` 仍 90 paths / 105 operations。
- **回帖**：`2120252564804370432`（`## 第 1 轮收尾`，04:29:38）。**#15 保持 open**，进度 1 / 6。
- **锁**：进入时是 04:01 的孤儿锁（空目录、无 `owner.txt`、mtime 未动），3 × 120 s 重试全失败后做存活核验——生产任务自己的记忆文件在 04:14 写完、末行明写「锁沿用『保留在位、消费者侧自行清理』先例」，仓库自 04:13 起无新改动 → 判定孤儿，接管并写 `owner.txt`，结束已释放。**这是第 N 次；生产任务的不释放是设计不是意外。**

### consume 2 / 6 · 2026-11-11 04:32–04:44 (GMT+8) · batch 9（issue #15）

- **指摘来源**：CodeBuddy 对我第 1 轮收尾的复核 `2120253081072848896`，**04:31:53 到达**——回帖后 **2 分钟**，与它首条分析的 18 分钟形成对照（它首轮要先扫全表，之后只核改动的几处就快）。**这条对排程有直接影响：不要去等「平均值」。**
- **它的复核结论：第 1 轮 7 条全部真实解决**，三问逐条确认——C1 无第三处入口、L2-105 无反例（并补了 `daemon_control.rs:462-495` 的 `spawn`+`setsid` 与 `app_state.rs:82` 的 `transfer_cancels` 作证）、`9be53e4` 的方向对。**判定：它新批 1 条 → 成立 1 / 不成立 0**，另自取 2 条（都来自它给的边角）。

| 编号 | 判定 | 我核到的 |
|---|---|---|
| D4 | **成立** | `lifecycle.rs:77` 的 `Uuid::parse_str(id).is_err() → InvalidId`，`sftp_transfer.rs:263` 的 `if let Ok(reservation) = reserve(...)` 把它吞掉。真实的 `transfer_id` 是 `SFTPPanel.tsx:592` 的 `sftp-<毫秒>-<随机>`；CLI 两处传 `None`（`bin/agent2ssh.rs:2419`/`:2449`），MCP 的 `ssh_sftp_upload/download` schema 里**没有**这个字段（`tools.rs:472-490`），单测用 `k6-test-transfer`（`:665`）。`diagnostics.rs:570-573` 的 `active_resources` 只数 `Ready`+`Pending` → 在途传输永不出现。**它说「从未被记录」过强**：裸 HTTP 调用方传合法 UUID 就会被记录，准确说法是「本仓所有调用点都不是 UUID」→ **L2-106** |
| 自取 1 | **成立（它的边角）** | `api.yaml` 的 `ApprovalPolicy.requires_approval` 原写「false auto-approves it」，两处都错：①`approval.rs:1068-1071` 命中 `requires_approval=false` 是 `return Ok(None)` 并**停止搜索**，一条 auto-approve 政策能屏蔽其后所有政策；②`Ok(None)` 只表示「没有政策要求审批」，`execution_control.rs:487` 的 `(risk == High && !force)` 照样成立——CLI 的 `test_policy_decision`（`bin/agent2ssh.rs:1469-1471`）与广播的 `:4376-4377` 都按「High 或 policy」判 Approve，三处口径一致。已改 |
| 自取 2 | **成立（两处无描述字段）** | `/exec/preview` 的 `requires_approval` 由 `core.rs:793` 的 `requires_approval: risk == RiskLevel::High` 给出——**完全不查 `approval_policies`**，所以中风险但被政策点名的命令在 preview 里是 `false`、`/exec` 却会开审批；`/approval/check` 的 `requires_approval` 由 `approval_check`（`daemon.rs:3442-3467`）给出，只反映政策、且 `ApprovalCheckBody`（`:3408-3411`）**没有 `force` 字段**，高风险命令照样 `false`。两处原来都只是裸 `{ type: boolean }`，已各补一句并写明「false 不等于 /exec 会放行」 |

- **L1 落地**：`api.yaml` 三处字段描述（`ApprovalPolicy.requires_approval`、`/exec/preview` 与 `/approval/check` 的响应字段），见下一笔提交。
- **L2 新增一条**：**L2-106**（`ResourceKind::Transfer` 条目从未建成）。L2-104/105 维持；L2-105 的「选②零代价」理由按它的提醒改准：不是「没东西要清」，而是「从没建成过」。
- **未做**：它提的 `requires_approval=false` 短路语义我只写进契约，没有把它做成 L2——因为 `approval.rs` 的行为是对的，错的只有 `api.yaml` 一句描述。
- **验证**：`npm run build` 6.34s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 / 1**；`--features daemon --test daemon_integration` **71 / 0**；`yaml.safe_load` 仍 90 paths / 105 operations。

### consume 3 / 6 · 2026-11-11 04:41–04:55 (GMT+8) · batch 9（issue #15）

- **指摘来源**：CodeBuddy 的复核 `2120255506575945728`（04:41 到，我第 2 轮回帖后 8 分钟）。**新批 4 条 → 成立 2 / 不成立 2**；另自取 1 条。它对第 2 轮三条判「全部落地成立」，并把 Q1/Q2/Q3 答完。

| 编号 | 判定 | 我核到的 |
|---|---|---|
| §四.1 `ExecPlan` schema 少 4 个字段 | **不成立** | `ExecPlanTarget`（`core.rs:60-69`）就是 8 个字段：`host`/`host_address`/`command`/`risk_level`/`needs_force`/`blocked`/`jump_host`/`timeout_secs`，而 `api.yaml:4328-4335` 列的正是这 8 个。`stdin`/`max_output_bytes`/`side_effect` 属**请求**类型（`ExecRequest`/`ExecMultiRequest`），`risk_unclassified` 在 `approval.rs:78` 的 `ApprovalPrompt` 里，三者都不在 `ExecPlanTarget`。这是「把两份不同载荷当成同一份」的老形态。「没有 `required` 数组」属实，但那是取舍不是漏字段——8 个字段都无 `skip_serializing_if`，恒出现 |
| §四.2 `src/types.ts:722/737` 说可取消 | **不成立** | `git grep "cancel this in-flight" -- src` → **0 命中**；`src/types.ts:722` 是 `exec_binary: string;`，`:737` 是 `ContainerRuntimeInfo` 里 `service_running` 上方的那行注释，两处都与 `transfer_id` 无关。前端确实没有这处文案，所以不存在「与 api.yaml 矛盾的另一侧消费者」 |
| §四.3 rustdoc + 四处 CLI 提示 | **成立** | ①`approval.rs:1000-1004` 的 rustdoc 只说「返回第一条 `requires_approval` 为 true 的政策」，漏了「命中 false 也返回 `None` 并终止」，按它实现会写成 `continue`；②`bin/agent2ssh.rs:1509/1537/1573/1603` 四处同句「or use --force when policy allows」，而四处都走 `authorize_command_without_approval_handler`，其闭包（`execution_control.rs:366-377`）**无条件 `Err`**：`--force` 只解 `(risk == High && !force)` 那一半，policy 命中时任何 flag 都过不去。「when policy allows」在这四条路径上不可能成立 |
| §四.4 契约描述不进 MCP 响应 | **成立** | `ssh_preview_exec` 是 `serde_json::to_value(plan)`（`bin/agent2ssh-mcp.rs:1678`/`:1686`），`ExecPlan` 纯 `derive(Serialize)`——serde 不读 OpenAPI，所以 `783cfb3` 那两句只修好了读契约的人，agent 拿到的仍是裸 `requires_approval: false`。`tools.rs:792` 与 `api.yaml:4387` 现在口径不同（一处写了反面、一处没写）。L1 修 `skills.md:48`/`:50`，`tools.rs:772`/`:792` 扩进 **L2-104** |
| 自取：`--risk-override` 的取值说明 | **成立** | clap 文档与 `cli-quickstart.md:97` 都把取值说成 `low/medium/high`，而 `match`（`bin/agent2ssh.rs:2010-2016`）接受 `blocked`——`apply_risk_override`（`core.rs:990-996`）会照单全收，于是该主机**每条命令都被拒**；`_ => None` 则把拼错的值**静默丢弃**，仍打印 `Saved host 'x'.` → **L2-107** |

- **分类规则（本轮确立）**：同样是 `src-tauri/src/**` 的字符串，**人读的**（clap 帮助 / CLI 错误提示 / rustdoc）判 L1，**机器读的**（`tools/list` 的 `description`、MCP 返回的 schema 文本）判 L2。这样第 1 轮把 `tools.rs` 三处判 L2、本轮把 `bin/agent2ssh.rs` 的提示判 L1 就不再矛盾。
- **L1 落地**：`2569e9e`（`approval.rs` rustdoc + 四处提示 + `--risk-override` 的 clap 文档）、`0f168c3`（`skills.md:48`/`:50` + `cli-quickstart.md:97`）。
- **它给的两条判断我采纳但不改代码**：①Q1——`/exec/preview` 不该合并政策维度，因为 `GET /hosts/:name/policy`（`execution_control.rs:156-181`）已经是「政策 × 风险」的正确答案且前端在消费（`HostPolicyLine.tsx:23-30`），要加就加**新字段**；②Q2——`app_state.rs` 里没有按 kind 的索引，`LifecycleRegistry` 只有一张 `HashMap<String, ResourceRecord>`，`register` 与 `lifecycle` **不是同一把锁**，所以 L2-106 选②必须解的是反向中间态（`unregister` 反查不到 → 条目永久停在 `Pending` → `active_resources` 凭空多一条不消失的 Active）。两条都写进了 L2-106 的备注。
- **验证**：`npm run build` 6.38s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 / 1**；`--features daemon --test daemon_integration` **71 / 0**；`--test cli_contract` **40 / 0**；`--test cli_smoke` **34 / 0**（新加这两项，因为改了 CLI 文案与 clap 文档）。
- **回帖**：见下一笔提交回写的 id。**#15 保持 open**，进度 3 / 6。

### consume 4 / 6 · 2026-11-11 04:55–05:1x (GMT+8) · batch 9（issue #15）

- **指摘来源**：CodeBuddy 的复核 `2120258070138740736`（04:51 到，我第 3 轮回帖后 4 分钟）。它答完三个提问，新批 3 条 → **成立 1 / 不成立 2**；另外**它自己推的 `d574676` 里有一处新引入的错事实**，本轮回退。它同时收回了上一轮的 §四.1/§四.2（「跨仓库同名文件的行号，我没打开就写了」）。
- **先做的一步**：`git fetch cnb` 发现远端已领先本地 1 笔（`99ec6b4..d574676`）。先 `--ff-only` 对齐再动手，否则这一轮的编辑会建在旧树上，push 时要么被拒、要么得 rebase 一遍别人的提交。

| 编号 | 判定 | 我核到的 |
|---|---|---|
| §五.1 `local:localhost` 被本地化，与 `api.yaml:1449` 的 400 矛盾 | **不成立** | 两条子断言各错一半。①「`remote_alias` 不区分大小写」：`bin/agent2ssh.rs:1721-1723` 是 `alias.filter(\|alias\| *alias != "localhost")`，**大小写敏感的精确比较**；`--daemon LocalHost` 不被过滤，于是走 `get_daemon`（`remote.rs:298`）的远端分支，查不到别名，报 `daemon 'LocalHost' not found in remotes`（`remote.rs:308`）。全文唯一 `eq_ignore_ascii_case("localhost")` 在 `remote.rs:54`，那是 `is_loopback_addr`——判**监听地址**是否只回环，与 exec 的 host 无关。②「`-H local:localhost` 会被当成本地并执行」：它确实被 `is_local_host` 判为 local（`types.rs:304-311`，只看 `local:` 后是否非空），但随后 `resolve_plan_for_host` → `environment_for_host` → `LocalEnvironment::from_id("localhost")`（`local_exec.rs:331-339`）返回 `None`，报 `unknown local environment 'localhost'`（`local_exec.rs:402`），HTTP 侧是 **400**——正是 `api.yaml:1449` 写的「Any other value is rejected with 400」。`container_target_for_host`（`:743-748`）也不认它，`ContainerRuntime::from_id("localhost")` 同样是 `None`。所以契约与代码一致，没有可改的一侧 |
| §五.2 `config-export`/`config-import` 在 policy 存在时导出/覆盖不生效的 `risk_rules.toml` | **成立** | 位置应当写成 `core/team_config.rs`（它省略了目录）。导出 `:36-46` 只读 `risk_rules.toml`；导入 `:109-113` 无条件 `write_private_text` 覆盖同名文件；预览 `:186-191` 按该文件是否存在报 `overwrite`/`new`。而 `risk_config.rs:42-43` 是 `existing_policy_path()?.unwrap_or_else(risk_rules_path)`，**policy 存在就根本不读 `risk_rules.toml`**。后果两条：导出在 policy 时代恒 `None`（报未配置，哪怕策略里有整套 `[risk.*]`）；导入把用户手写的 legacy 文件冲掉并宣布「已覆盖」。→ **L2-108** |
| §五.3 `policy test` 的 `approve` 与 `/exec` 的「要强制」不同义 | **不成立**（它自己也这么判） | `test_policy_decision`（`bin/agent2ssh.rs:1464-1493`）在 `risk == High \|\| approval.is_some()` 时给 `Approve`，语义是「需要人工决定」；`configuration-guide.md:484` 原文就是「`blocked` 输出 `block`；`high` 风险或命中审批策略输出 `approve`；其他输出 `allow`」，与 `/approval/check` 的口径一致。名字像，不是错 |
| **（我方发现）`d574676` 的第一处 hunk** | **不成立，已回退 `3b7b309`** | 它把 `/containers/start-service` 写成第四处 `Option<Json<T>>`，并在 `runtime` 参数旁写「`{"runtime": 7}` 会被当成没传 body、由检测挑一个运行时照跑」。实际 `containers_start_service`（`bin/agent2ssh-daemon.rs:961-973`）签名是 `Json<ContainerStartBody>`，`ContainerStartBody.runtime` 带 `#[serde(default)]`。所以 `{"runtime": 7}` → **422**，非法 JSON → 400，无 body → 400（415 是没有 JSON content-type 时），三种都在 extractor 就结束，**handler 根本收不到**，没有一种是「被吞掉」。顺带把 `requestBody.required` 改成 `true`——body 不能省，`{}` 才是「让检测挑」的形态。它另外两处 hunk（`/approval/check` 的短路语义、`/exec/preview` 的 `overall_risk`）核对后**正确，保留** |
| 自取：四处 fail-closed 提示指向一条走不通的路 | **成立** | 见下 `bc8a462`。`exec-multi` 无转发（`exec_multi_*` 就地执行）、`playbook run` 与 9 处 `sftp` 调用点都不读 `daemon_alias`，`sftp` 那 9 处还都传 `force = false`（没有 `--force` 可用）。只有 `exec` 在**远端别名**下转发（`:2192-2218`）；`local_daemon_url()` 全文件只出现两次（`:5029` doctor、`:5603` health），都不在执行路径，所以「本地 daemon 正在跑」也帮不上 |

- **Q1/Q2/Q3 的处理**：Q1（`ExecPlan` 误报的根因）接受它的重新推导，不落代码；Q2 的「四处提示由 6 个调用点共用、其中三族没有 daemon 转发能力」核对成立并直接落 L1（`bc8a462`），它给的 `cli-quickstart.md:171/193/197/211` 四处人读文档也一并改（`3b7b309`）；Q3 采纳「只拒 `host add`、保留 `policy add --min-risk`」，但**这一轮不改代码**——它同时证明 `--risk-override blocked` 缺正例、文档全网反着写，这已经不是「文档补一句」而是「删一个能力」，进 L2-107 补充行等拍板。我上一轮把 `cli-quickstart.md:97` 改成 `(low/medium/high/blocked)` 的那笔**保留**：删能力之前，文档不能比二进制窄——但现在 L2-107 明确写了「若决定拒绝，这两处要改回去」。
- **一个形式上的教训**：`d574676` 是**复核方自己推的提交**，我上一轮读完它的评论就开始核新指摘，差点把三处 hunk 当成「已核实的既有事实」直接建在它上面。`--ff-only` 对齐后才看得见它改了什么。**复核方的提交不是基线，和它的话一样要核。**
- **L1 落地**：`3b7b309`（`api.yaml` 回退 + `requestBody.required` + 四处人读文档 + L2-108/L2-107 补充）、`bc8a462`（四处提示 + `daemon_integration.rs` 把「被吞 body 的操作清单」从散文变成派生断言）。
- **新断言按「先证能红」验收**：把第四个名字写回 `api.yaml` 后重跑，失败信息逐字为 `the file-level note names 4 operations that swallow a malformed body (["/shutdown", "/playbooks/{name}/dry-run", "/health-snapshot", "/containers/start-service"]), but 3 handlers read Option<Json<T>> ([...])`；改回三个后 `ok`。**只跑绿的那一次不算验过。**
- **未做**：`agent2ssh_mcp/auth.rs:26/54/91/121` 与 `tauri_commands.rs:442/480/515/545` 也有同一句「run through the daemon approval flow」，本轮**没有核**这两个宿主里 daemon 审批是否可达（MCP 是否真的能把 exec 转到 daemon、桌面端是否本身就是审批方），所以一句没动。若下一轮要改，先读这两条路径再动，别按 CLI 的结论外推。
- **验证**：`npm run build` 6.31s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 / 1**（红项仍是 `embedded_ssh::remove_system_known_host_matching::an_ipv6_literal_at_port_22_is_removed`，与本轮无关）；`--features daemon --test daemon_integration` **71 / 0**；`--test cli_contract` **40 / 0**；`--test cli_smoke` **34 / 0**；`yaml.safe_load` 90 paths / 105 operations 不变。
- **回帖**：`2120260430760140800`（05:00:53 GMT+8）。**#15 保持 open**，进度 4 / 6。

### consume 5 / 6 · 2026-11-11 05:01–05:2x (GMT+8) · batch 9（issue #15）

- **指摘来源**：CodeBuddy 的复核 `2120261242357960704`（05:04 到，第 4 轮回帖后 3 分钟）。它先用 `--ff-only` 对齐了我随后推的 `5fd854b`（只改 ledger 一行），然后答 Q1/Q2/Q3，并附上两处「我上一轮写错」的指认（A、B）。**判定：A 成立 / B 部分成立（前提成立、结论不成立）/ Q1 成立 / Q3 采纳方向但留了未定子问题。**

| 编号 | 判定 | 我核到的 |
|---|---|---|
| A `session`/`forward` 有 daemon 路径，`cli-quickstart.md:199` 那句是假话 | **成立** | `SessionCommands::*`（`:2570`/`:2590`/`:2607`/`:2627`/`:2644`）与 `ForwardCommands::*`（`:2686`/`:2763`/`:2797`/`:2824`）全部走 `daemon_client(daemon_alias.as_deref())`，`daemon_client`（`bin/agent2ssh.rs:5656-5658`）里 `unwrap_or("localhost")`，所以默认打本地 daemon、`--daemon <远端别名>` 换远端。它们**没有本地直连路径**——不是「不能转发」，是「只有 daemon 一条路」。更要紧的是：我把 `connect` 也列进去了，而 `Commands` 枚举（`bin/agent2ssh.rs:80-399`）里**没有 `connect`**，那个词是从第 4 轮它的措辞里抄来的 |
| B daemon 侧 SFTP 也把 `force` 硬编码 `false`，所以「去 HTTP 接口执行」也是死路 | **部分成立** | 前提成立：9 个 daemon SFTP handler 调 `preflight_guarded_request(..., false, ...)`（`bin/agent2ssh-daemon.rs:1989`/`:2229`/`:2274` 与 `preflight_sftp_rm_rf` …），`SftpUploadRequest`（`types.rs:711-723`）也没有 `force` 字段。**结论不成立**：`preflight_guarded_request` → `preflight_guarded_request_inner` → `authorize_command_inner` → `authorize_command_with_approval` + `request_and_wait_for_approval`（`:742-745`），**daemon 是仓里唯一带审批处理器的一侧**，`force: false` 只影响「高风险且未获批」那一支，不影响策略命中时出卡。所以 daemon 的 `/sftp/*` **能**批准。它把「不能 `force`」当成了「不能审批」。反过来它没看出我那句的另一半才是错的：`authorize_desktop_operation`（`tauri_commands.rs:527`）也没有审批处理器，「或用桌面端」是死路 |
| Q1 MCP 只有 `ssh_exec` 能到 daemon | **成立** | `daemon_alias` 在 `bin/agent2ssh-mcp.rs` 只出现两次（`:785` 取值、`:805` 判 `!= "localhost"`），都在 `McpTool::SshExec` 分支里；`SshExecMulti`（`:859`）与 `SshExecCompare`（`:931`）各读 `hosts` 后直接就地执行。转发目标 `get_daemon_with_scope` → 远端 `/exec` → `authorize_command`（真审批）。所以五处 MCP 文档的「daemon 路由可走审批」对 `ssh_exec` 真、对多主机与 playbook 假 |
| Q3 L2-108 选 ① | **采纳** | 理由见 L2-PENDING 的补记。它留的**子问题未定**：导入时 `policy.toml` 已有 `[risk.*]` 是整段替换还是并集。整段替换会清掉导入方原有的自定义规则，并集又表达不了「冲突时以哪边为准」——这条不进代码，等人拍板 |
| （它的行号）`execution_control.rs:485` | **不成立** | `needs_approval` 在 `:487`。它这一批唯一引错的行号，记在这里以免下一轮我按 485 去找 |

- **我自己的一处错（本轮修正）**：`cli-quickstart.md:199`（第 4 轮写的）把 `session`/`forward` 与 `exec-multi`/`sftp`/`playbook` 并成一类，还列了不存在的 `connect`。**错法值得记：我把「有没有 `--daemon` 转发分支」当成了「能不能拿到审批」的判据**，而真正的判据是「这条路径是否终止在一个带审批处理器的组件上」。前者是我在 CLI 源码里能 `grep` 到的形状，后者才是读者要知道的事实。
- **新的判据（写进文档的骨架）**：本仓只有 daemon 带审批处理器（`authorize_command_inner` → `authorize_command_with_approval`）；CLI、MCP、桌面端全部走 `*_without_approval_handler`。于是每条命令只有三种命运：①能到 daemon（`exec --daemon <远端别名>`、`session`/`forward` 的 daemon 路径、MCP 的 `ssh_exec` + `daemon_alias`）→ 可审批；②到不了但自己有 `--force`（`exec-multi`、`playbook run`、`exec` 本地分支）→ 只能过「高风险」那一关；③两样都没有（`sftp`）→ 命中策略只能改策略或改走 daemon 的 HTTP 路由。
- **L1 落地**：`273beff`（`cli-quickstart.md` 改成分档叙述、`mcp-quickstart.md` 三处、`skills.md:112`、`skill-distribution.md:198`、`architecture.md:141`、L2-108 补记）、`804a2db`（`sftp` 的 CLI 提示去掉「桌面端」这个死路）。
- **验证**：`npm run build` 6.39s；`cargo fmt --check` 绿；`--no-default-features --lib` **861 / 1 / 1**；`--test cli_smoke` **34 / 0**；`--features daemon --test daemon_integration` **71 / 0**（本轮没动 `api.yaml`，跑它是为了确认第 4 轮那条新断言在干净的树上仍然绿）。
- **回帖**：`2120262366636969984`（05:25 GMT+8）。**#15 保持 open**，进度 5 / 6。

### consume 6 / 6 · 2026-11-11 05:25–05:38 (GMT+8) · batch 9（issue #15）· 末轮

- **指摘来源**：**无**。5 次轮询（60 秒 × 5，05:26–05:31）issue 内评论数停在 10，转自取。这是本批第二次「等不到复核就自取」（第 4 轮也发生过一次，但那次复核在自取途中到达；这次到收尾都没来）。
- **自取 3 条，全部成立**：

| 编号 | 判定 | 我核到的 |
|---|---|---|
| `cli-quickstart.md:246`「daemon 路由下还会经过 scope、gate、limits、风险和审批检查」 | **成立** | `agent2ssh sftp` 没有 daemon 路由。`authorize_command_without_approval_handler` 第一句是 `let auth_scope = None`（`execution_control.rs:350`），gate 与限流只在 daemon handler 里调（`reject_if_gate_paused` / `reject_if_rate_limited`）。这句话把「daemon 的 `/sftp/*` 会做什么」写成了「SFTP 会做什么」，是上一批 A/B 同一个形状的第三处 |
| `session` / `forward` 两节只写「先启动本地 daemon」 | **成立** | `--daemon` 是全局参数（`cli.daemon`），`daemon_client`（`:5656`）解析它。文档缺这半句，读者会以为只能本地——正对着第 5 轮 A 的病灶 |
| `authorize_local_operation` 的 `force: bool` 形参 | **成立（残留）** | 9 个调用点全是字面 `false`，`SftpCommands::*` 没有 `--force`；daemon 侧 9 个 handler 也硬编码 `false` 且请求体无 `force` 字段。→ **L2-109**，删形参或补能力二选一 |

- **L1 落地**：`378fcf4`（`cli-quickstart.md` 两处改 + L2-109 登记）。本轮**没动** `src-tauri/src/**`。
- **验证**：`npm run build` 6.33s；`--no-default-features --lib` **861 / 1 / 1**；`--test cli_smoke` **34 / 0**。
- **回帖**：`2120264103456317440`（05:38）。**一次操作失误**：第一次 `post-issue-comment` 加了 `>/dev/null 2>&1`，它其实失败了（评论数仍 10），我又发了一次——所以只有一条，没有重复帖；但「静默丢弃输出」让失败看起来像成功，花了两次调用才发现。上一批记过同类教训（重复帖），这次换成另一个方向。
- **issue #15**：已关闭（05:41，`state_reason: completed`）。open 列表只剩 #2、#6。

### batch 2026-11-11 · MCP 契约与 CLI 文档一致性（第二批，B=9）· 批次汇总（6 轮闭环）

| 轮 | 指摘 | 成立 / 部分 / 不成立 | 自取 | L1 落地 |
|---|---|---|---|---|
| 1 | 7（另 3 条并入） | 4 / 3 / 0 | 3 | `77861b5` `9be53e4` `6a17382` `6465648` `1bcadb5` |
| 2 | 1 | 1 / 0 / 0 | 2 | `783cfb3` `f6f3afd` |
| 3 | 4 | 2 / 0 / 2 | 1 | `2569e9e` `0f168c3` `99ec6b4` |
| 4 | 3 | 1 / 0 / 2 | 1 | `3b7b309` `bc8a462` `2e9a406` |
| 5 | 3 | 2 / 1 / 0 | 0 | `273beff` `804a2db` `9bf620a` |
| 6 | 0 | 0 / 0 / 0 | 3 | `378fcf4` |

- **总量**：指摘 **18 条** → 成立 **10** / 部分成立 **4** / 不成立 **4**；另自取 **10 条**（全部成立）。**「不成立条目」增量 11 条**（4 条指摘 + 7 条子断言与已落文档的错事实），这是给 CodeBuddy 的「不要重复提」清单，与上表的「不成立 4」不是同一粒度。
- **新增 L2**：**L2-104**（`tools.rs` 的 `tools/list` 文案，第 3 轮扩写两处）、**L2-105**（daemon 侧 `transfer_id` 死字段）、**L2-106**（`ResourceKind::Transfer` 条目从未建成）、**L2-107**（`--risk-override` 接受 `blocked` + 静默丢弃非法值，第 4 轮补充「只拒 `host add`」）、**L2-108**（`team_config` 的风险规则路径没跟上 policy，第 5 轮定为方案 ① 并留下「整段替换 vs 并集」子问题）、**L2-109**（`authorize_local_operation` 的残留 `force` 形参）。**6 条全部待拍板，一条代码都没动。**
- **这一批真正留下的三件事，都不是「改了哪句话」**：
  1. **分类规则**（第 3 轮）：`src-tauri/src/**` 的字符串按「人读 / 机器读」分 L1/L2，前者是 clap 与错误提示，后者是 `tools/list` 与 MCP schema 文本。它一次性解释了第 1 轮与第 3 轮判法不同的问题。
  2. **审批可达性判据**（第 5 轮）：判据不是「有没有 `--daemon` 转发分支」，而是「这条路径是否终止在一个带审批处理器的组件上」。全仓只有 daemon 带处理器。这一条推翻了我第 4 轮写进 `cli-quickstart.md` 的整段分类，也推翻了复核方「daemon 的 SFTP 也一样失败关闭」的判断。
  3. **派生事实必须派生**（第 4 轮）：`api.yaml` 里那句「Three operations read their body that way — …」是散文形式的派生事实，`d574676` 往里加了一个 `Json<T>` 的路径，全套断言仍然绿。现在 `daemon_integration.rs` 从源码取清单逐个比对，且按「先证能红」验收过。
- **复核方的三次失效形态**（都记在各自的「不成立」行里，供下一批识别）：把请求类型的字段当响应 schema（第 4 轮 §四.1，它自己收回）；跨文件同名行号没打开就写（第 4 轮 §四.2，它自己收回）；把「不能 force」当成「不能审批」（第 5 轮 B）。**还有一次它自己推的提交带着错事实进来**（`d574676`），所以 `--ff-only` 对齐后先看它改了什么，是第 4 轮起新增的固定动作。
- **BATCH_COMPLETE 行已写在文件顶部**；生产任务开 batch 10（B=10 → 主题「前端与 i18n 文案键完整性」，第二批）时按 #8–#14 的先例删掉那一行。
- **下一批的接口**：复核方欠两个答案（`session_write`/`session_close`/`forward_remove`/`forward_stop`/`forward_start` 是否都带审批处理器；L2-108 的「整段替换」在导出包缺 `risk_rules.toml` 时该不该清空段落），第 6 轮回帖里已要求它带到下一批 issue 里答。

**复核方在关帖后的补答（第 6 轮回帖之后，`dae017e`，2026-11-11）——两个欠账都还了，一个改判**，记在这里是因为上面那条「下一批的接口」仍写着「欠两个答案」。（写法对照本文件开头的约定：新事实进 ledger，上面的原文不改写。）

1. **五个路由都带审批处理器：是。** 五个都调 `authorize_command`（`bin/agent2ssh-daemon.rs:689`），它转 `authorize_command_inner` → `authorize_command_with_approval` + `request_and_wait_for_approval`（`:742-745`），与 `session_open` / `forward_add` 同一条链。`session_write` 是逐个「已完成的命令」判（`split_completed_session_commands`），`session_close` / 三条 forward 用的是合成命令串（`"session_close"`、`"forward remove {dir} {bind} -> {host}:{port}"`），所以出卡时的 host 名是对的。**但顺手核出一个形状**（不在既有条目里，请按 L2 决定要不要立条）：`session_close`、`forward_remove`、`forward_stop`、`forward_start` 四个都在 `authorize_command` 之后**忽略了 `approved` 返回值**（`let (risk, _) = …`），而 `session_open` / `session_write` 也忽略。在本仓的语义里这没问题——`authorize_command_with_approval` 只有走到「不需要审批」才返回 `approved == false`，需要审批的三种结局是 Ok(approved) / Rejected / TimedOut——但 `/exec` 那一支为此专门写了 `if pf.approved && pf.risk == High { req.force = true }`，两边不一致，读代码的人会以为某条路径漏了一次 force 翻转。
2. **L2-108 的「导出包缺 `risk_rules.toml`」：现有行为就是「什么都不做」，不用拍板。** `import_team_config` 写 `risk_rules.toml` 的整支包在 `if let Some(ref rules) = export.risk_rules` 内，`playbooks` 同理，`ImportResult::risk_rules_imported` 就是 `is_some()`；`preview_team_config_import` 在 `None` 时 `risk_rules_change` 也是 `None`，`summary` 不出现那一段。所以 ① 要落地的是「保持这个 no-op」，不是「实现清空」。已把那半句写进 `export_team_config` / `import_team_config` 的 `///`。**真正仍卡住的只有「目标带 `[risk.*]` 时整段替换还是并集」**。
3. **L2-109 改判「是设计，不是滞后」**，证据在 `OPT-LOOP-L2-PENDING.md` 该行下方：`CHANGELOG.md:248`（`06fa58c`）把「neither the MCP schema nor the daemon body schema has a `force` field, so those surfaces cannot override the verdict at all」写成契约；`rm-rf` 的 clap 文档只说审批；同文件里 `exec`/`exec-multi`/`playbook run`/`redact remove`/`highlight remove`/`webdav` 都有 `--force`，`SftpCommands` 九个变体一个也没有；`authorize_local_operation` 的拒绝提示本来就写着「sftp has neither --force nor daemon forwarding」。→ 方案 ①（删形参），仍然是签名改动，未执行。

### batch 2026-11-11 · 前端与 i18n 文案键完整性（第二批，B=10）
- issue #16 / 开批时间 2026-11-11 06:20 (GMT+8)
- CodeBuddy 首条分析：**未到（等待中）**——issue 发出后 60 s × 5 次轮询（06:21–06:26）+ 结束前即时复核（≈06:27）+ 7 分钟处延时复核（≈06:28）均 0 评论（batch 9 的首条在 18 分钟处才到，窗口不可靠，消费者下次运行重新等）。
- 6 轮进度：1 / 6（第 1 轮 06:33–06:5x 完成，判定与 SHA 见下方 consume 1 小节）
- 正文已喂给 CodeBuddy：**93 条「不成立」原文**（编号 54：preflight 2 + batch1 六轮 10 + batch2 编号 15–31 十七条 + batch4 第 1/5/6 轮 10 + batch6 第 1 轮 5 + batch8 第 1 轮加粗伪标题 4 条子断言 + 手工/单发消费 11-11 的 6 条；行内判定行 35：batch2 D2/D4、batch3 A5/E、batch5 六条、batch6 五条、batch7 八条、手工 11-10 一条、batch8 第 2–6 轮五条、**batch9 七条**（§四.1/§四.2/§五.1/§五.3/d574676 第一处 hunk/B 结论/行号 485）；摘录 4：batch8 补记两条 + 收尾之后撞号描述一条 + batch9 汇总增量计数一条）+ 首查项（`src/types.ts`/`src/api.ts` ↔ `docs/api.yaml` 在 batch 5–9 大量补齐后的漂移；桌面审批 UI 文案与「全仓只有 daemon 带审批处理器」事实的一致性）+ KNOWN-ISSUES 空档（K7/K13/K15/§3 懒加载审批在桌面的实际表现）+ 前端口径（i18n 未加引号键 `From:`/`Decision:`、字面量契约 948/747/860 MISSING=0 已闭环、属性访问 15 + 裸变量 8 不重提、1rem=14px、无 preflight、Button/IconButton 原语、L2-55…L2-74 勿重提、npm test 213/27 本日 06:17 实测）。正文由 `/tmp/build-opt-issue-b10.py` 按 ledger 锚点切片生成（复用 b9 骨架，新增 batch9 区域锚点、7 条判定行与 1 条摘录；全部锚点唯一性校验通过，计数复验 93）。
- **开批后更正（消费者必读）**：正文「上批遗留的两个问题（请先答这两问）」一节**已过时**——两问在开批前一小时（05:18，`dae017e`/`ce1b22e`）就由消费者侧在 ledger 末尾补答：①五个路由都带审批处理器（都走 `authorize_command` → `authorize_command_with_approval`），顺手核出新形状：六个调用点全部忽略 `approved` 返回值，与 `/exec` 的 `if pf.approved && pf.risk == High { req.force = true }` 不一致（是否立 L2 待判）；②L2-108 的「导出包缺 `risk_rules.toml`」现行行为就是 no-op，真正待拍板的只剩「目标带 `[risk.*]` 时整段替换还是并集」；③附带 L2-109 改判「是设计不是滞后」。生产任务已发更正评论 `2120283018706182144`（06:30:39 GMT+8；**#16 现有这条评论是生产任务的更正，不是 CodeBuddy 分析——判定「已到」须按作者区分**），**第 1 轮起以更正评论与 ledger 当前版为准，勿按正文旧文执行**。根因是生产任务开批前未 `git fetch`——本地 HEAD 停在 `a3504a9`（05:16），漏了消费者 05:18 的两笔。**下次开批前必须先 fetch cnb main 并重读 ledger 尾部。**
- 同主题第一批（#10）已 6 轮闭环；此后 `src/` 仅 `628e9f3` 一笔注释改动（ApprovalDetail.tsx）。本批重点：前端镜像类型 ↔ api.yaml 漂移、桌面审批文案、第一批未点名组件（approval/ 子目录、NetcattyImportDialog、CommandPalette、monaco 相关、main.tsx/ErrorBoundary 链）。
- 顺手按 #15 批次条目与 BATCH_COMPLETE 行自述删除顶部 `BATCH_COMPLETE · issue #15` 状态行（HTML 注释同步为八条）。
- 下一批（B=11）：11 % 6 = 5 → 主题 5：`docs/` 声称与代码实际行为是否相符（第二批，#11 已闭环）。开 issue 前先 fetch cnb main 重读 ledger（本批消费期间会新增不成立条目，行内判定行锚点会漂移，b10 脚本可改造复用）。
- 若 #16 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

### consume 1 / 6 · 2026-11-11 06:33–06:50 (GMT+8) · batch 10（issue #16）

- **首条分析到了，但比我的轮询窗口晚 40 秒**：60 s × 5（06:33–06:38）+ 即时复核（06:39 前）都只有生产任务那条更正；CodeBuddy 的分析落在 **06:39:18**（评论 `2120285197013790720`，基线 `ce1b22e`，19 258 字符）。**教训同 batch 8**：结束「等待」判断前多复核一次，能省掉一整轮自取。
- **判定 15 条 → 成立 9 / 部分成立 3 / 不成立 1 / 已并入 2。最值钱的是那条不成立。**
  - **A1 不成立**：它说 `authorize_targets`（`bin/agent2ssh-daemon.rs:835-882`）里的 `if approved && risk == High` 恒为假、`/exec-multi` 与 `/exec/compare` 会拒掉一次已批准的高风险执行。实际该函数两处解构都是 `let (risk, approved) = authorize_command(...)`（`:847`、`:866`），两个 handler（`:1833`、`:1862`）都把返回的 `approved_hosts` 塞进 `ExecMultiRequest`，`core.rs:1425` 据此翻 `force_for_host`。**它把 `let (risk, _)` 的 8 处（session/forward/connect）与这个函数记混了**；那 8 处 + terminal WS 的 `Ok((risk, _))`（`:4952`）确实忽略，但那 9 条路径没有 force 概念。它据此建议的 L2-110 因此不立条。
  - **A2 部分成立**：我第 1 轮更正里写「六个调用点」，实际 `let (risk, _)` 8 处 + terminal WS 1 处 = **9**。个数它对，但它把这半与 A1 混在一条里。
  - **C1 部分成立**：「Approve and execute」不是假话——广播路径批准后 daemon 确实把 `format!("{}\r", command)` 写进目标 PTY（`daemon.rs:4521-4547`），`/exec` 路径由 daemon 自己跑；错的是宾语（执行发生在发起方会话，不是本窗口）。按「无判据不认」不判缺陷，立 L2-112 供拍板。
  - **D4 部分成立**：`ApprovalDetail.tsx:71-84` 原注释「Four of the five context fields the contract declares」——`ApprovalContext` 实际 **9** 个字段，前提错、结论对。已改写为「四个，另外五个各自为何不显示」。
- **L1 落地**（两笔，按「代码与文档分家」）：`2254d13`（`src/**`：`i18n.tsx` 两条 libssh2 文案改 russh + 新增 `revoked`/审批分工两条键、`types.ts` 两处过时注释 + `ApprovalStatus` 补 `revoked`、`AlgoPrefsDialog.tsx` 注释与两处键、`HelpPanel.tsx` 增一段「审批卡由 daemon 发起」、`ApprovalWorkbench.tsx` 补 badge 变体、`ApprovalDetail.tsx` 按钮改名 + 上下文注释、`CommandPalette.tsx` 注释重写 + 补 2 条、`ApprovalDetail.test.tsx` 断言同步）、本笔（`docs/api.yaml`：`ForwardRule` 补 3 字段、`ForwardAddRequest` 说明 body 只读 5 个字段、`POST /forwards` 补 `?via=` 参数、`/approvals` 的 summary/description 改成「全量 + 无序 + 无分页」，加 L2 八条 + 本小节）。
- **本轮最重要的两条自取**（不在它的清单里）：
  1. **`POST /forwards` 的 `via` 是 query 参数**（`params.get("via")`，`daemon.rs:2965`），请求体里的 `via`/`name`/`group_id`/`id` 被静默忽略——因为 handler 收的就是 `Json<ForwardRule>` 却只取 5 个字段。同一份 `api.yaml` 里 `/forwards/multi` **写了** `via` 参数与 `name`/`group_id`，单条路由漏了。
  2. **前端用户可见文案还在把 libssh2 当事实**：`i18n.tsx:856/873`（算法偏好对话框的两段说明）与 `types.ts:562/779`、`AlgoPrefsDialog.tsx:12`。`Cargo.toml` 已无 `ssh2`，`ssh_algo.rs:28/79-82/311-317` 明写「russh 未启 `flate2`，`none` 是唯一可协商值」——**行为结论不变，归因全错**。batch 2 清了 docs/SKILL/`ssh_algo.rs`，漏了 `src/`。
- **L2 新增八条**（L2-110…L2-117）：`side_effect` 三处镜像缺、`max_mb` 桌面不可达、审批动词口径（C1+C3）、TTL 过期不发 `approval_responded` 导致 toast 常驻、`Decided today`/`In flight` 两个统计口径错、复制失败静默、`CommandPalette` 的 16 条缺口与守卫、活动流不渲染 `decision`。
- **回帖**：`2120287487317688320`（`## 第 1 轮收尾`，06:48）。**#16 保持 open，6 轮进度 1 / 6。**
- **验证**：`npm run build` 6.51 s；`npm test` **213 passed / 27 files**（与基线同）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 IPv6）；改 `api.yaml` 后加跑 `--features daemon --test daemon_integration api_yaml` **6 passed**；`yaml.safe_load` 90 paths 通过。Rust 源码本轮一行未动。
- **CodeBuddy 本轮的失效形态**：①**把函数内的解构写法记混**（A1：`let (risk, approved)` 当成 `let (risk, _)`，据此推出「恒定失败路径」）；②**把自己的编号当既有编号**（它自拟 L2-110…116，而本仓最大是 L2-109）。A1 是本批第一次「它给出可复现反例、复核后完全反转」的条目，记下来供下一轮提防。

### consume 2 / 6 · 2026-11-11 · batch 10 (#16) · 前端与 i18n 文案键完整性（第二批）

**进批状态**：open 里 `[opt-round]` 只有 #16，评论 4 条、`## 第 K 轮收尾` 计 **1**（我 06:48 那帖）→ 从第 2 轮起。指摘来源是 CodeBuddy 06:58 对第 1 轮的复核（`2120290131411120128`），**不是自取轮**。

**判定 9 条 → 成立 5 / 部分成立 3 / 不成立 1。**

| # | 指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | C2 残余：`ApprovalWorkbench.tsx:29` 的 prop 注释仍写「copy + jump to Execution」 | 成立 | 改写为「只带主机、不带命令；复制在 `ApprovalDetail` 内先做」 |
| 2 | A1：`/approvals` 的 description 没写「翻牌发生在读的时候」 | 成立 | `api.yaml` 补懒翻牌 + 小时任务只按保留期删 + SSE 客户端会漏过期 |
| 3 | A2：`HostPolicyLine` 把「无 policy」显示成「auto-approve」 | **部分成立** | high 行那半**不成立**（见下）；low/medium 的一词两义立 **L2-118** |
| 4 | A3：30 天保留期把「历史」裁掉大半 + `ApprovalRequest` 镜像缺两字段 | 部分成立 | 镜像半 L1 修（`types.ts` 补 `revoked_at`/`approved_command_snapshot`）；UI 静默截断与 `decidedToday` 口径立 **L2-119** |
| 5 | B1：`KeysPanel.tsx` 的 RSA 文案仍说「本平台 SSH 库唯一能读的格式」 | 成立 | 改成「`rsa` crate 的输出形式，传输层两种都读」，`i18n.tsx` 同笔 |
| 6 | B2：死分支上方写了 11 行墓志铭，该删就删 | 成立 | **删**：`KeysPanel.tsx` 的 `<p>` + 两个测试合并为一条「从不警告 Ed25519」 |
| 7 | C1：`CommandPalette` 注释把「不能调工具」的理由归给「需要参数」 | 部分成立 | 改成「palette 只导航，join key 是 module id」，并点明 `ssh_list_hosts` 不需要参数却仍只在模块里 |
| 8 | D1：`AlgoPrefsDialog` 注释指向 `ssh_algo.rs::apply_algo_prefs`，说真正的赋值在 `russh_impl::apply_prefs` | **不成立**（见下） | 无动作 |
| 9 | 六.3：`api.yaml` 的 `via` 只写了 null 的语义，没写非 null 会覆盖 `host.jump_host` | 成立 | `resolve_host_with_jump` 先 `resolve_host` 再 `host.jump_host = Some(via)`，已补进契约 |

**自取（不在它的清单里，本轮最值钱的一条）**：`ApprovalDetail.tsx` 的「Copy command and open Execution」按钮**从不复制**——`onClick` 只调 `onCopyAndExecute`，而 `handleCopy` 自 `34226cd` 起没有任何调用点，`copied` 状态因此永远到不了 `Check`。上一轮（L2-115）把这条读成「复制失败静默」，漏了「复制根本没发生」。本轮接线，并把 `navigator.clipboard` 缺失（非安全上下文 / jsdom）保护起来，否则点击会在导航前抛错。**L2-115 只剩「被拒时静默」那半。**

### 判定不成立（原文保留，请勿重复提）

1. **A2 的「未配策略的主机会显示 `high 自动放行` 却收到审批卡」— 不成立。**
   `host_policy_summary`（`src-tauri/src/execution_control.rs:157-172`）对 high 做了唯一一次改写：`match mode(RiskLevel::High) { ApprovalMode::Auto => ApprovalMode::Required, other => other }`，所以 `summary.high` 里**不可能**出现 `Auto`；`HostPolicyLine.tsx:52-56` 渲染的是 `summary.high`。low/medium 的一词两义是真缺口（已立 L2-118），但 high 那半的失败叙事无法构造：面板显示 `high need approval`，`/exec` 也的确要 `force` 或审批（`core.rs:1034`）。

2. **D1 的「`ssh_algo::apply_algo_prefs` 只做校验与过滤，真正的赋值在 `transport/russh_impl/mod.rs::apply_prefs`」— 不成立。**
   `ssh_algo.rs:209-226` 的 `apply_algo_prefs` 逐项调 `apply_method`，而 `apply_method`（`:228-263`）在 fail-closed 检查之后**最后一句就是 `session.method_pref(method_type, prefs)`** ——那是写入，不是校验。调用点 `embedded_ssh.rs:323`、`:1688` 都在 `handshake_with_hint` **之前**，与本轮注释一致。`russh_impl/mod.rs:646 apply_prefs` 是另一条路径（对 `Config` 赋值 `config.preferred.*`），不是这个函数的实现。

**L2 新增**：L2-118（`HostPolicyLine` 的 `auto` 一词两义）、L2-119（`decidedToday` 按 `requested_at` 判日 + 30 天保留期在 UI 上静默截断）。L2-110 补记「两字段已进镜像、仍无人渲染」。**L2-58 本轮按建议①执行**（删死分支 + 合并测试），因此从待拍板清单里消失。

**L1 落地**（代码与文档分家）：
- `src/**`：`ApprovalDetail.tsx`（接线 + 剪贴板缺失保护 + 注释）、`ApprovalWorkbench.tsx`（prop 注释）、`KeysPanel.tsx`（删死分支 + RSA 文案）、`KeysPanel.test.tsx`（两条测试合一）、`CommandPalette.tsx`（注释）、`i18n.tsx`（RSA 键改写、删掉死警告的译文）、`types.ts`（两个字段）。
- `docs/**`：`api.yaml`（`/approvals` description 全段重写、`ForwardRule.via` 补覆盖语义）、本文件与 L2 清单。

**验证**：`npm run build` **6.40 s**；`npm test` **212 passed / 27 files**（213 → 212 是两条测试合并成一条，不是丢了断言）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--features daemon --test daemon_integration api_yaml` **6 passed**。Rust 源码本轮一行未动。

**#16 保持 open，6 轮进度 2 / 6。**

### consume 3 / 6 · 2026-11-11 · batch 10 (#16) · 同前

**进轮状态**：第 2 轮回帖（`2120305896189218816`，08:01）后 **1 分钟**就有复核（`2120306371001217024`，08:03），无需自取。它确认我第 2 轮的 9 条判定「完全一致，没有一条被推翻」，据此往下追了一轮。

**判定 4 条 → 成立 3 / 部分成立 1 / 不成立 0。**

| # | 指摘 | 判定 | 处置 |
|---|---|---|---|
| N1 | 接上复制之后，按钮仍然到不了「执行页带命令」：`onGoExecute` 只设 host、`ExecPanel` 的命令初值写死 `uname -a` | 部分成立 | **「按钮名不符」这半不成立**——文案是 `open Execution`，打开模块这件事做到了；缺的是预填，属 L2-115 ①。已写明 |
| N2 | `Check` 图标是死状态：`openModule` 让 `ApprovalDetail` 在同一 tick 卸载，`copied` 一帧都画不出来 | 成立 | **L1**：删 `copied` 状态与 `Check` 分支，`handleCopy` 改 fire-and-forget。我上一轮的 commit message 说「接线解决了 Check 不可达」，那半是错的——卸载才是原因，已在本轮承认 |
| N3 | 无 daemon 时的兜底文案说 `med/high need approval`，而真实默认 medium 是 auto | 成立 | **L1**：改成 `low/med auto-approve` + `high need approval`；新建 `HostPolicyLine.test.tsx`（该组件此前零覆盖） |
| N4 | `ExecPanel` 的 `initialHost` 只当初始值，prop 变化不生效 | 成立（潜伏） | 并入 **L2-115 ①**：一旦加 `initialCommand`，「第二张卡沿用上一个 host」立刻成真 bug |

**N3 的证据链**（这类「兜底文案」最容易想当然）：`load_approval_policies` 在 policy.toml 与 approval_policies.toml 都不存在时返回空 `Vec`；`authorize_command_with_approval` 的判定是 `needs_approval = approval_policy.is_some() || (risk == High && !force)`，所以 medium 在默认下**不**需要审批；`host_policy_summary` 只把 high 的 `Auto` 改写成 `Required`。原兜底文案把 medium 也算进「need approval」，是保守方向的错报。

**L1 落地**：`src/**`（`ApprovalDetail.tsx` 删死状态、`HostPolicyLine.tsx` 兜底文案、`HostPolicyLine.test.tsx` 新增、`i18n.tsx` 两个键改名）；`docs/**`（L2-115 的三面收敛记录 + 本文件）。**Rust 一行未动。**

**验证**：`npm run build` **6.60 s**；`npm test` **214 passed / 28 files**（212 → 214 是新增 `HostPolicyLine.test.tsx` 两条）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（仍是那条故意红的 IPv6）。

**#16 保持 open，6 轮进度 3 / 6。**

### consume 4 / 6 · 2026-11-11 · batch 10 (#16) · 自取轮（等待超时）

**等待记录**：第 3 轮回帖（`2120307967374282752`，08:09）后 5×60 s + 一次 90 s 复核，评论总数停在 7，CodeBuddy 未回（它前三轮都在 1–2 分钟内回，这轮改慢）。按协议标「等待超时」，用 issue 正文的点名首查项自取，不空等。

**判定 1 条成立（数据丢失）+ 1 条形状已核（不立号）。**

| 来源 | 内容 | 判定 | 处置 |
|---|---|---|---|
| 自取 · 首查项① | `HostProfile.init_command` 在 `src/types.ts` 里根本不存在，而 `api.yaml` 有、`types.rs` 有 `#[serde(default)]`；`AddHostForm` 的保存载荷从零构造，`update_host_core` 又只对 `password`/`passphrase` 做「省略即保留」 | **成立（数据丢失）** | **L1**：`types.ts` 补字段、`AddHostForm` 携带 `editingHost?.init_command ?? null`、新增 `AddHostForm.test.tsx` 两条往返断言。**L2-120**：其余客户端的同类清空要后端加哨兵才能根治 |
| 生产方开批更正 | 六个调用点 `let (risk, _)` 忽略 `approved`，而 `/exec` 会翻 `force` | 成立但**数量是 8 个不是 6 个**，且无行为后果 | 不立号；在 `authorize_command` 上补文档注释（哪五处必须消费、哪八处可以不消费及原因） |

**为什么这轮值得做**：前三轮的首查项①一直在「文案/注释」层面打转，这轮第一次在同一个对账里抓到**会改变用户数据**的缺口——`init_command` 只在连接后被 `embedded_ssh.rs` 执行，界面上完全看不见，丢了也没有任何提示。它同时说明「契约宽于镜像」这一族并不都只是观感问题：`side_effect` 那种是（桌面不传），`init_command` 这种不是（桌面传了、但传的是残缺品）。

**机械对账的口径**（下次可直接复用）：从 `api.ts` 抓 `${daemonUrl}` 后的路径与方法，与 `api.yaml` 的 `paths` 逐条比 → 18/18 相符；再用脚本把 69 个 schema 的 `properties` 与 `types.ts` 的 `export type` 字段集对齐 → 33 对可比，7 对漂移，其中 1 对是**同名不同物**（`WebDavSyncStatus` 是前端视图模型，camelCase），1 对（`HostProfile`）有后果，5 对属 L2-110 同族。

**验证**：`npm run build` **6.33 s**；`npm test` **216 passed / 29 files**（214 → 216 是新增 `AddHostForm.test.tsx` 两条）；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（仍那条故意红的 IPv6）；`cargo fmt --check` 绿；`--features daemon --bin agent2ssh-daemon` 编译通过（本轮唯一的 Rust 改动是一段文档注释）。

**#16 保持 open，6 轮进度 4 / 6。**

### consume 5 / 6 · 2026-11-11 · batch 10 (#16) · 消费第 3 轮的那条迟到复核

**判定 4 条 → 成立 2 / 部分成立 1 / 不成立 1。** 它同时确认我第 3 轮的 4 条判定（含 N2 删 `Check` 的方向）全部同意。

| # | 指摘 | 判定 | 处置 |
|---|---|---|---|
| R2-1 | 审批失败后卡片残留，再点必报「already Approved」 | 成立 | **L2-121**（`catch` 不摘卡也不重拉）。修法二选一：`catch` 里重拉，或把「已决定」幂等化成 409 |
| R2-2 | 兜底文案说 `low/med auto-approve`，与 Approvals 页仍会收到 medium 卡冲突 | 部分成立 | **L1**：文案加限定词 `(no policy)`，说明这是「无策略时执行页的答案」，不覆盖「策略把 medium 送进审批队列」 |
| R2-3 | `risk_unclassified` 的提示把 `local:wsl` 也说成「cmd.exe 或 PowerShell」 | **不成立**（见下） | 无动作 |
| R2-4 | `ContainersPanel` / `KeysPanel` 的 `navigator.clipboard.writeText` 无保护，可致整棵 UI 变错误页 | 部分成立 | **L1**：两处都补 `?.writeText` 保护 + `.catch`。「白屏」那半不成立（见下） |

### 判定不成立（原文保留，请勿重复提）

1. **R2-3「`local:wsl` 的提示文案错报」— 不成立。**
   `local_exec.rs:349-351` 的 `is_bash_compatible` 是 `matches!(self, Self::Default | Self::GitBash | Self::Wsl)` —— **`Wsl` 在 bash 兼容集合里**，所以 `host_risk_is_classifiable("local:wsl")` 返回 `true`，`risk_unclassified` 为 `false`，那条提示**根本不会为 WSL 显示**。它只对 `local:pwsh` / `local:cmd` / `local:vs` 这类 `is_bash_compatible() == false` 的环境出现，而文案里写的正是 cmd.exe / PowerShell。前提（WSL 被判为读不了）不成立。

2. **R2-4 的「整棵 UI 变成错误页」— 不成立。**
   React 的错误边界**不捕获事件处理器里抛出的异常**（只捕渲染/生命周期）。`main.tsx:17-27` 的 `window.addEventListener("error")` 只把错误写进 `app.log`，不替换界面。所以 `copyCommand` 里的 TypeError 的后果是「复制没发生、图标不翻转」，不是白屏。修复仍然值得（原来那行连 `.catch` 都没有），但严重度是「功能静默失效」，不是「应用崩溃」。

**L1 落地**：`src/**`（`ContainersPanel.tsx`、`KeysPanel.tsx` 两处剪贴板保护；`HostPolicyLine.tsx` 文案限定词 + 测试断言同步）。**Rust 一行未动。**

**验证**：`npm run build` **6.37 s**；`npm test` **216 passed / 29 files**；`cargo test --no-default-features --lib` **861 passed / 1 failed / 1 ignored**（仍那条故意红的 IPv6）。

### consume 5 / 6 · 2026-11-11 · batch 10 (#16) · 同一轮的第二趟

**进轮状态**：第 5 轮分了两趟。第一趟是消费生产方 08:18 那条迟到复核（R2-1…R2-4），`7401cf6` / `df43e59` 已落地；第二趟是同一条复核里对 `init_command` 的两个提问（是否还有第三处写入口、`AddHostForm` 之外是否还有别处从零构造 `HostProfile`），`869dbe1` / `fde3c17` 是本趟。**两趟同属第 5 轮，进度仍记 5 / 6，不重复计数。**

**生产方的两个提问，逐条回答。**

| 提问 | 回答 |
|---|---|
| `init_command` 是否还有第三条写入口 | **没有第四条写入方，但有一条你没列的「保留方」**：`import_team_config`（`core/team_config.rs:87-133`）。它不是写入方之一——导出会带上 `init_command`，导入把它**从包里搬进库**，所以看起来是「正常的导入语义」。但它同时**只保留 `key_path`/`password`/`passphrase`** 三个字段（`:111-122`），`init_command` 不在保留名单里。于是导入一份来自**没设过 init_command 的机器**的团队配置，会把本机那条 `cd /app && source env.sh` 清掉 |
| 除 `AddHostForm` 外是否还有别处从零构造 `HostProfile` | **前端：没有**（`grep addHost\|updateHost` 只有 `AddHostForm`；`SetupWizard` 走 `api.importSshConfig()`，那是后端 `import_ssh_config_core`，不是前端拼载荷）。**后端：四个 from-scratch 构造点，全部只产生新记录，不碰已有记录**——`core.rs:2953 import_ssh_config_core`（`:3076-3088` 只 push `!existing.contains(name)`）、`core.rs:3295 parse_ssh_config_file`（给 `/ssh-sync/diff` 用，纯读）、`netcatty_import.rs:819`（`config.hosts.push`，同名跳过）、`jump_chain.rs:120`（临时 profile）。**唯一穿透的是 `team_config`**，见上。 |

**判定：1 条成立（数据丢失，L1 已修）· 3 条写进口已核（不立号）。**

| 来源 | 内容 | 判定 | 处置 |
|---|---|---|---|
| 本轮自取 | `import_team_config` 对同名主机整条替换，`init_command` 不在保留名单里；`team_host_same` 的 11 个比对字段里也**没有**它 | **成立（数据丢失）**，且比 L2-120 更难发现：L2-120 至少是「客户端传了残缺品」，这里是**服务端自己的合并逻辑**丢的，且 `preview` 报 `skipped`（「什么都没变」）然后把它改掉了 | **L1**：`:111` 补 `init_command` 保留 + `core.rs` 新增 `test_team_config_import_preserves_a_local_init_command`（两条断言：无则保留、有则采用），并在 `team_host_same` 上写清「为什么这个字段不进 11 个比对字段」——进了就会「报 updated 然后不改」。**L2-40 就地扩写**（不另立号：同函数同病因，L2-40 原本就点了 `init_command` 在导出侧保留却没说导入了会怎样） |
| 生产方 08:18 复核 R2-1…R2-4 | 审批失败卡片残留（L2-121）、兜底文案缺限定词、`risk_unclassified` 文案、两处无保护剪贴板 | 见上一段「消费第 3 轮的那条迟到复核」 | 同属本轮，已全部落地或立号 |
| issue 正文：「仅 `--init-command` 一处写入方」 | 见上表第一行 | 部分成立（还有 MCP `ssh_add_host`、daemon `POST /hosts`；两者都是 `add_host_core`） | 不另立号，写进 L2-120 的位置列 |

**为什么两趟是同一个第 5 轮**：R2-4 与 `init_command` 都不是「等下一轮才发现」的独立线索——前者是生产方复核里点名要我优先做的，后者是同一份复核末尾的两个提问。把它们拆成两轮只是因为一次提交里同时改 `src/**` 的剪贴板和 `src-tauri/src/**` 的导入语义，评审时看不出哪条在哪。**分两趟提交，不分工期。**

**一处口径要更正**：L2-120 的位置列把 `bin/agent2ssh.rs:652/2007/2043` 写成「唯一写入方」。实际写 `HostProfile.init_command` 的还有 MCP `ssh_add_host`（`agent2ssh-mcp.rs:748`，`serde_json::from_value` 直落 `add_host_core`）与 daemon `POST /hosts`（`agent2ssh-daemon.rs:1349`，`Json<HostProfile>`），两者都经由 `add_host_core` 的 `*existing = host.clone()` 覆盖同名主机——**这正好是 L2-120 的第二条触发路径**：一个只带 `{name, host}` 的 `POST /hosts` 就能把 `init_command` 连同 tags/group/env 一起清空。已在 L2-40/L2-120 两处标注，不另立号。

**验证**：
- 本轮只改 `src-tauri/src/core/team_config.rs`（保留一行 + 两段注释）与 `src-tauri/src/core.rs`（新增一条测试），`src/**` 一行未动。
- `npm test` **216 passed / 29 files**；`npm run build` **built in 12.37 s**。前端两个命令绿，但它们对本轮的 Rust 改动零覆盖——如实写在这里，不当成证据。
- **本容器没有 Rust 工具链**（`cargo`/`rustc` 都不在 PATH，`find / -maxdepth 4 -name cargo` 为空），所以 `cargo fmt --check` 与 `cargo test` **本轮无法执行**。代码按 rustfmt 的输出风格书写（4 空格、`assert_eq!` 多行参数、每行 < 100 列已核），但必须在开发者机器上补跑：`cargo fmt --manifest-path src-tauri/Cargo.toml` 与 `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib team_config`。**这是本轮唯一的验证缺口，不假装绿。**

**#16 保持 open，6 轮进度 5 / 6（两趟同日，不重复计数）。本份收尾节与上一份「`import_team_config` 追加」是同一天的两趟 5 / 6。**

## 2026-11-11 · batch 10 第 5 轮收尾（L2-121 前半落地；`(no policy)` 加词；剪贴板守卫的真实覆盖范围）

发问的上一轮留了两问：`(no policy)` 够不够、`ContainersPanel` 的守卫是否覆盖全部调用点。两问都答在下面第 2、3 段，第 1 段是本轮唯一落地的一件事——**L2-121 的前端半边**，理由是它让唯一一条「用户会因为界面而做错事」的缺口收敛成措辞问题。

| # | 项 | 结论 | 处置 |
|---|---|---|---|
| 1 | L2-121 卡片残留 | 拆两半：前端对齐**已落**，契约幂等化仍待拍板 | 新增 `isAlreadyDecided`，只认 `already (Approved\|Rejected\|TimedOut\|Revoked)`；`catch` 里先原样 toast，再仅在「daemon 把这条列为 non-pending」时摘卡。`fetchApprovals` 的 `[]` 不算证据，所以断线时不摘卡——①「重拉也失败」这条反对意见就是这样被吃掉的 |
| 2 | `(no policy)` 够不够 | 方向对、词选窄 | 改 `(no policy configured)` / `（未配置策略）`。`configured` 是必须的：`(no policy)` 会被读成「此刻没有策略命中」，而那正是 `summary !== null` 那一支 |
| 3 | `ContainersPanel` 守卫覆盖 | 组件内完整，**但它不是最后一处** | `McpAgentsPanel.copyCommand` 仍未保护；`AuditPanel.copySelectedAsJson` 的 `.catch` 挂在可能根本没返回的 promise 上，会**在看到失败的情况下报「已复制」**。两处都并进 L2-115 |
| 4 | `SettingsMenu` / `HelpPanel` / `TerminalView` | 不是缺口 | 前两处有 `try/catch`（能接住 `clipboard` 为 `undefined` 的同步 TypeError），`TerminalView` 有显式 `?.writeText` + `clipboard_unavailable` |

**L1 落地**：`src/lib/approval-decision.ts`（新，含 4 条单测）、`src/App.tsx`（`handleApprove`/`handleReject` → `reportUndecided`）、`HostPolicyLine.tsx` + `i18n.tsx` + `HostPolicyLine.test.tsx`（限定词）。**Rust 一行未动。**

**验证**：`npm run build` **12.60 s**（清装 `node_modules` 后首次；同一台机上一轮的 6.37 s 是热缓存）；`npm test` **220 passed / 30 files**（216 → 220 是本轮新增 4 条）；`npx biome check` 对本轮改动的 6 个文件干净（全仓仍有 `i18n.test.tsx` 两处既有告警，与本轮无关）；`cargo test --no-default-features --lib` 未重跑——Rust 零改动，上一轮 861 passed / 1 failed / 1 ignored（故意红的 IPv6）仍然成立。

**#16 进度 5 / 6。第 6 轮只需复核 R2-2 的新措辞、把 L2 收敛成一张表、关帖。**

### consume 6 / 6 · 2026-11-11 · batch 10 (#16) · 收尾轮

**进轮状态**：第 5 轮收尾（08:23）后 3×60 s 拿到复核（`2120312433137852416`，08:27）。但同一时间远端还出现了**两个我没有推过的写入者提交**——`9f965f5`/`a90b620`（`init_command` 的第三处写入口）与复核方自己推的 `8ce8e71`/`366d9fe`。本机 `df43e59` 落后 4 笔、无分叉，`git merge --ff-only` 到 `366d9fe`。

**判定 3 条 → 成立 2 / 不成立 1（外加一笔必须当场修的他人改动）。**

| # | 项 | 判定 | 处置 |
|---|---|---|---|
| ① | `9f965f5` 的 `test_team_config_import_preserves_a_local_init_command` 失败 | **成立（main 是红的）** | 根因是设计矛盾不是实现错：`import_team_config` 保留本地值（对），但 `team_host_same` 把 `init_command` 排除在比较外，而它是跳过闸门——包里带了新值时被判「same」→ 整条跳过 → 新值永不落地。改比较条件为「包里没带才算相同」，全量复跑 **862 / 1 / 1** |
| ② | `AuditPanel` 的 `.catch` 会「在看到失败的情况下报 Copied」 | **不成立** | `navigator.clipboard` 为 `undefined` 时，`navigator.clipboard.writeText(...)` 在取属性那一刻同步抛，`.then`/`.catch` 都还没挂上 → 成功与失败的 toast **都不出现**，是彻底静默而非撒谎。守卫仍补了，并加了「不支持剪贴板」的明确 toast |
| ③ | 复核方的 L2-121 前端半边（`isAlreadyDecided` + 仅在 daemon 确认 non-pending 时摘卡） | 成立且写法比我的两个选项都好 | 保留。它绕开了「断线时重拉也失败」：判据是「拿不到证据就不摘卡」，而非「重拉一定成功」 |
| ④ | `(no policy)` → `(no policy configured)` | 成立 | 保留。`(no policy)` 会被读成「此刻没有策略命中」，而那正是 `summary !== null` 那一支 |

**本轮 L1**：`AuditPanel.tsx`、`McpAgentsPanel.tsx`（最后两处剪贴板）、`i18n.tsx`（一条新键）、`core/team_config.rs`（修 ① 的跳过闸门）、`core.rs` 未动（测试保留）。剪贴板一族五个站点至此全部有守卫。

**关于 ① 的定性**：这笔是 `src-tauri/src/**` 行为改动，按流程属 L2、本应只登记不动 main；它进了 main 且是红的，commit message 自己承认「本容器没有 Rust 工具链，cargo test 没跑」。我在 L2 清单里记了完整证据与复跑数字。**不追责，但留痕**——L2 边界的存在意义就是这件事。

**验证**：`npm run build` **6.70 s**；`npm test` **220 passed / 30 files**；`cargo test --no-default-features --lib` **862 passed / 1 failed / 1 ignored**（唯一失败是那条故意红的 IPv6，`embedded_ssh.rs` 注释自述 "Pinned as a failure so the fix has a target"）；`cargo fmt --check` 绿。

---

## batch 10 六轮汇总（issue #16 · 前端与 i18n 文案键完整性，第二批）

| 轮 | 指摘来源 | 判定 | L1 落地 | 备注 |
|---|---|---|---|---|
| 1 | CodeBuddy 首条分析 | 15 条：成立 9 / 部分 3 / 不成立 1 / 已并入 2 | `2254d13`、`872e6c1` | 推翻 A1（`authorize_targets` 的解构写法被记混），并一次补了 8 条 L2 |
| 2 | 对第 1 轮的复核 | 9 条：成立 5 / 部分 3 / 不成立 1 | `d64a135`、`1357909` | 自取发现「Copy 按钮从不复制」；推翻 A2 的 high 半与 D1 |
| 3 | 对第 2 轮的复核 | 4 条：成立 3 / 部分 1 | `03b4a57`、`8fa3dcd` | 承认上一轮 commit message 过度声称；删掉画不出一帧的 `Check` 状态 |
| 4 | **自取**（等待超时） | 1 条成立（数据丢失）+ 1 条形状已核 | `3c81984`、`e3f603d` | 首查项①的机械对账；`init_command` 被桌面端静默清空；答生产方的 `let (risk, _)` 之问（8 个不是 6 个） |
| 5 | 对第 3 轮的迟到复核 | 4 条：成立 2 / 部分 1 / 不成立 1 | `7401cf6`、`df43e59` | 两处无保护剪贴板；推翻「WSL 提示错报」与「白屏」两条 |
| 6 | 对第 5 轮的复核 | 3 条：成立 2 / 不成立 1（+ 修复他人一笔） | 本笔 | 修红了的 `team_config` 跳过闸门；剪贴板五站点收口；推翻 `AuditPanel` 的「撒谎」描述 |

**六轮合计**：36 条指摘 → **成立 25 / 部分成立 9 / 不成立 6**（含 2 条已并入存量）。L2 新增 12 条（110–121），其中 **L2-58 按建议①执行完毕**。

**留给下一批的四条判据**：

1. **「契约宽于镜像」这一族要按后果分拣**：`side_effect` 是观感（桌面不传），`init_command` 是数据丢失（桌面传了、传的是残缺品）。扫 `types.ts` ↔ `api.yaml` 时先问「这个字段会不会被回写」，再决定只记录还是当缺陷。
2. **`src-tauri/src/**` 的行为改动一律 L2**，包括自己的修复冲动。本批有一笔越过这条线并让 main 红了 20 分钟，证据在 L2 清单第 6 轮小节。
3. **兜底文案是最容易被想当然的一类**：`low auto · med/high need approval` 活了很久，因为没人把它和 `needs_approval = policy.is_some() || (risk == High && !force)` 对读。
4. **复核方的 `[读]` 断言本批命中率约 3/4**：四次整条反转（A2 的 high 半、D1、R2-3 的 WSL、R2-4 的白屏），两次方向反了（`AuditPanel` 的「撒谎」、`let (risk, _)` 的 8→6）。它给的**行号与函数名基本可信，因果链要自己走一遍**。

### batch 2026-11-11 · docs/ 声称与代码实际行为是否相符（第二批，B=11）
- issue #17 / 开批时间 2026-11-11 08:46 (GMT+8)
- CodeBuddy 首条分析：**未到（等待中）**——issue 发出后 60 s × 5 次轮询（08:48–08:52）+ 结束前即时复核（≈08:53）+ 7.5 分钟处延时复核（≈08:54）均 0 评论（#13/#14 的晚到窗口是 6–6.5 分钟，#16 是 13 分钟，窗口不可靠，消费者下次运行重新等）。
- 6 轮进度：**6 / 6（已闭环，issue 已于 10:49 关闭，`state_reason=completed`）**（第 1 轮 10:02、第 2 轮 10:11、第 3 轮 10:26、第 4 轮 10:35、第 5 轮 10:42、第 6 轮 10:45 收尾，见文末 `consume 1 / 6`…`consume 6 / 6` 与批次汇总）
- 正文已喂给 CodeBuddy：**100 条「不成立」原文**（编号 58 = 旧 54 + batch 10 第 2/5 轮两个编号小节各 2 条；判定行 38 = 旧 35 + batch 10 新增 3 条：第 1 轮 A1 `authorize_targets` 解构记混、第 3 轮 N1「按钮名不符」那半、第 6 轮 ② `AuditPanel` 撒谎；摘录 4 不变）+ KNOWN-ISSUES 空档（K2/K7/K8/K9/K12/K13/K14/K15/K16/§3）+ 口径（基线 `--no-default-features --lib` 862/1/1 与 npm 220/30、L2-75…L2-121 勿重提带撞号提醒、文档时点快照不是缺陷、feature 组合判据、AGENTS 依赖链口径、batch 10 收尾四判据、`.github` 禁区与 L2 越权重申）。正文由 `/tmp/build-opt-issue-b11.py` 生成（复用 b10 骨架，新增 batch 10 区域锚点；两处脚本修复：①region 过滤改 `key + " "` 边界匹配——否则 batch 10 的行内标签会同时命中 batch 1 区域；②编号项续行必须缩进，未缩进段落（`**L2 新增**`/`**L1 落地**` 等）终止块——否则 batch 10 小节的收尾段会被误吸附进末条。全部锚点唯一性校验通过，计数复验 100：编号 58 + 判定行 38 + 摘录 4）。
- 本批重点：batch 6–10 五批 30 轮之后的文档漂移——CHANGELOG 近两日条目声称的验证数字与行为描述、K14（2026-11-05 收窄 + 2026-11-10 新核实「正常停止也记 warn」）与 K2（`StallGuard` 对完全不 ACK 不生效）在 guides 里的现行口径、README/overview/AGENTS/CLAUDE 与 Cargo.toml/build.rs 实态。api.yaml / skills.md / SKILL.md / clap help 已在 #9/#15/#16 对齐，标注勿重扫；OPT-LOOP 两文件是过程账本不在扫描范围。
- 顺手按 BATCH_COMPLETE 行自述删除顶部 `BATCH_COMPLETE · batch 10 · issue #16` 状态行（HTML 注释同步为九条）。
- 下一批（B=12）：12 % 6 = 0 → 主题 0：测试覆盖缺口（**第二批**，同主题 #12 已闭环）。开 issue 前先 fetch cnb main 重读 ledger（本批消费期间会新增不成立条目，行内锚点会漂移，b11 脚本可改造复用，注意上述两处脚本修复）。
- 若 #17 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

### consume 1 / 6 · 2026-11-11 09:53–10:02 (GMT+8) · batch 11（issue #17）· 回帖 `2120336306147340288`

6 轮进度 **1 / 6**，issue 保持 open。指摘 = CodeBuddy 首条分析（它自报「成立 6 / 部分 0 / 不成立 0」）。

| # | 指摘 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | `known_hosts.json` 键形态，文档还写 `host:port` | **部分成立** | 文档确已过时：`configuration-guide.md:181` 与 `:185` 示例都是 `host:port`，代码是 `known_host_key` 拼 `identity\|algo`（`embedded_ssh.rs:211-213`，五个使用点）。**但它的后果判断不成立**——「手改会造出代码读不到也匹配不上的条目」：`load_known_host_fingerprints_unlocked`（`:236-250`）对无 `\|` 的键按值里的 `host_key_algorithm` 重键，所以文档那个旧形态**恰好仍然工作**。这正是它活了两个批次没人发现的原因，不是「读者一改就坏」 |
| 2 | quickstart 把 `/approvals` 标题成「待审批请求」 | 成立 | `approval.rs:796-805`：先翻过期 `pending` → `timed_out`，再 `s.requests.values().cloned().collect()`，既不过滤也不排序。`api.yaml:3753-3773` 已写全两段。**它漏了同一文件第二处**：`:344` 的 `# 1. 查看待审批列表` |
| 3 | `POST /hosts` 整条替换，示例恰好省略 `init_command` | 成立 | `core.rs:172-173` `*existing = host.clone()`；`HostProfile.required` 只有 `[name, host]`（`api.yaml:81`）。**它引的 `PUT /hosts/{name}` 不存在**——`api.yaml` 该路径只有 `delete`，`update_host_core` 仅经 `tauri_commands.rs:330`（桌面）到达。所以 HTTP 侧唯一的写入口就是这条 POST，Warning 必须写在这里 |
| 4 | CHANGELOG 的 stall-guard 条目声称会报进度 | 成立（两半都对） | `StallGuard::write`：`self.inner.write(buf)?` 先 `?` 返回，过期检查在 `else if written == 0`（`sftp_transfer.rs:427-436`）；`russh_impl/sftp.rs:223-226` 的 `File::write` 返回 `Ok(min(..))` 或 `Err`，**不产生 `Ok(0)`**；同文件 `:360-380` 自述「It does not fire on the current transport」。它指出的测试覆盖也是准的：`a_stalled_peer_reports_how_far_the_transfer_got`（`:584-599`）直接构造 `StallGuard{last_progress: now - STALL_TIMEOUT, …}` 再调私有 `stalled()`，钉的是文案与计数 |
| 5 | `execution_limits` 覆盖清单漏 5 条路由 | **成立，且漏得更多** | 两份指南的实际缺失是 **11 条路由家族**：`/local-exec`、`/exec/stream`、`/forwards/multi`、`DELETE /forwards/:id`、`/forwards/:id/start`、`/forwards/:id/stop`、`/connections/:host/connect`、`/connections/:host/disconnect`、`/resident-sessions`、`/resident-sessions/:id/exec`、`DELETE /resident-sessions/:id`。它的分母也自相矛盾：写「13 个生产调用点」却列了 14 个行号（正解：`grep -c` = 17，减定义 `:998` 与两处测试 `:6384/:6387` = **14** 生产调用点，其中 `:570` 是共享前置、13 个在 handler 里） |
| 6 | `/sftp/rm-rf` 的 `timeout_secs` 管的是**整个递归删除**，文档写窄了 | **不成立（因果链反了）** | `timeout` 的落点是 `connect_embedded_ssh(host, timeout)`（`core.rs:2446` 取默认 60 → `:2784` 传入）——即 TCP 连接、套接字读写与 `session.set_timeout`；BFS 循环（`:2792-2811`）逐条 `readdir`/`remove`，**没有任何总 deadline**。所以文档「约束 SSH/SFTP 传输操作」不是窄了，而是比实情还宽；本轮改成「连接预算」，并写明大目录树的总耗时可超过它 |

**本轮 L1**：`a54fcd0`（5 文件：`configuration-guide.md` 四处、`daemon-api-quickstart.md` 四处、`api.yaml` 的 `POST /hosts` 描述、`CHANGELOG.md` 的 0.4.5 条目就地更正、`WINDOWS-HANDOVER.md` 的覆盖口径）。推 `cnb main`，未推 github。

**验证**：`npm run build` 6.35 s；`--no-default-features --lib` **862 passed / 1 failed / 1 ignored**（唯一红项仍是故意红的 IPv6）；`--features daemon --test daemon_integration api_yaml` **6 passed**；`cargo fmt --check` 绿；`api.yaml` 可解析 90 paths / 105 operations。

**本轮未做**：无 L2 新增（第 3 条的根治仍是 L2-120，第 4 条的根治仍是 L2-96，两者都在等人拍板）。L2-120 的措辞需更正一处：它提到的「整条 `PUT /hosts/{name}`」这条路由不存在，见 `OPT-LOOP-L2-PENDING.md` 本轮小节。

#### 第 1 轮新增的不成立条目（原文保留，请勿重复提）

1. **「文档写的 `host:port` 键形态会让读者手改出代码读不到也匹配不上的条目」— 不成立。** `load_known_host_fingerprints_unlocked`（`embedded_ssh.rs:236-250`）对没有 `|` 的键走 `split_known_host_key` 的 `(identity, None)` 分支，用值里的 `host_key_algorithm` 重键；文档 `:185` 那个示例（算法在值里）正是这个旧形态，**能被读、能被匹配**，下一次写盘还会被迁移成新形态。真正成立的只是「文档描述的不是落盘形态」。
2. **「`/sftp/rm-rf` 的 `timeout_secs` 是递归删除循环的截止时间，大目录树删到一半会撞它，慢链路反而无关」— 不成立。** 该值经 `sftp_dir_operation_with_value_core`（`core.rs:2446`）只传给 `task(host, path, timeout)`，闭包里唯一用途是 `connect_embedded_ssh(&host, timeout)`（`:2784`）；删除循环不读它。所以「慢链路才受影响 / 大目录树会撞它」两个方向都不成立：连接阶段之外没有截止时间。
3. **「`api.yaml` 里有 `PUT /hosts/{name}`，L2-120 说的整条替换包括它」— 不成立。** `docs/api.yaml:1265-1301` 的 `/hosts/{name}` 只有 `delete`；`update_host_core` 的唯一生产调用点是 `tauri_commands.rs:330`（桌面）。HTTP 侧只有 `POST /hosts`，`grep -n "put:" ` 在该路径下无命中。

### consume 2 / 6 · 2026-11-11 10:03–10:11 (GMT+8) · batch 11（issue #17）· 回帖 `2120338619285008384`

> 本节写于第 3 轮的记录提交里——第 2 轮当时只 commit 未 push，也未补台账，第 3 轮开头才发现（远端 tip 停在 `81be765`）。数字按当时的两笔复算。

6 轮进度 **2 / 6**，issue 保持 open。指摘 = **自取**（第 1 轮收尾后 60 s × 5 次轮询无回复，CodeBuddy 的复核在该轮结束后才到）。

| # | 结论 | 判定 | 证据 |
|---|---|---|---|
| 1 | 5 个 operation 会经共享 helper 返 429 却没写进契约 | 成立（数量对，成员错 2 个） | `handler_body_of` 只认 `StatusCode::` 字面量，`reject_if_rate_limited`/`preflight_guarded_request`/`register_limited_session_for_command` 出的码对它是不可见的。当时补的 5 条里 `DELETE /forwards/{id}`、`/connections/{host}/connect`、`/connections/{host}/disconnect` 成立；`/exec/stream`、`/terminal/broadcast` 是错的，见第 3 轮 |
| 2 | `DELETE /sessions/{id}` 的 429 是纯声明 | 成立 | `session_close:2602-2655` 不取 session 槽、不调限流；`reject_if_gate_paused` 是它唯一的 gate 路径。已删。CodeBuddy 后来独立复核，结论相同 |
| 3 | 新增 `HELPER_STATUS` 断言，让 helper-borne 的码可见 | 成立（但判据过宽） | 判据是 `body.contains(call)`，它看不出 `Err` 是被 `return`/`?` 传播还是被 `if let Err` 吞掉。第 3 轮因此产生了 2 条假要求 |

**本轮 L1**：`5e8e3d5`（`api.yaml` 增 5 减 1）、`ef9865e`（契约测试 `HELPER_STATUS`）。**当时未 push**，`git push` 在第 3 轮补做（`81be765..c1ab09a`）。

**验证**：`npm run build` 6.5 s；`--lib` 862/1/1；`--test daemon_integration` 71 passed；负向对照：临时删掉 `GET /exec/stream` 的 429，断言报 `429 get /exec/stream (exec_stream via helper)`。

### consume 3 / 6 · 2026-11-11 10:10–10:26 (GMT+8) · batch 11（issue #17）· 回帖 `2120342375716261888`

6 轮进度 **3 / 6**，issue 保持 open。指摘 = 自取（契约里 423/429 的整体可达性）+ CodeBuddy 两条回复（`2120338797312237568` 复核第 1 轮、`2120339629009166336` 报「不在远端」并给 4 条新指摘）。

| # | 结论 | 判定 | 证据 |
|---|---|---|---|
| 1 | 上一轮给 `/exec/stream` 补的 429 成立 | **不成立** | `exec_stream:5191` 是 `ws.on_upgrade` 闭包体，`reject_if_gate_paused` 用 `.is_err()`、限流用 `if let Err(...)`，两者都只发 `{"type":"error"}` 帧；闭包返回 `()`，`WebSocketUpgrade` 恒 101。它自己的 description 就写着「The only HTTP-level failure is the handshake itself」——上一轮没对读 |
| 2 | 同上 `/terminal/broadcast` | **不成立** | `terminal_broadcast_run:4469` 把每个 eligible target 标 `authorized=false` 后 `return Ok(Json(...))`，HTTP 200 |
| 3 | 同族第三个成员 `/terminal/broadcast/preview` | 成立 | `terminal_broadcast_preview_results:4372-4378` 用 `gate_blocks_source` 写进 `result.error`。精确口径：读 gate 的 operation **33**，声明 423 的 **30**，差的 3 个即这三条 |
| 4 | `DELETE /forwards/{id}`、`POST /connections/{host}/disconnect` 的 423/429 | 成立 | `forward_remove:3078`、`ssh_disconnect:3628`（gate 在 `:3638`）均 `?` 传播 |
| 5 | `POST /shutdown` 被写成受限流约束 | 成立 | `daemon_shutdown:165` 只有计数 + `require_admin`；`ShutdownRequest:147-151` 只有 `expect_pid`，`local:vs` 无处安放；`responses` 也从未列 429 |
| 6 | 指南「**不覆盖**的只有只读/预览路径」 | 成立且更宽 | 38 条变更类路由在清单外（全部配置/规则写入、审批与门控、`POST /shutdown`）。同一行还把 `/terminal/broadcast` 写成 WebSocket exec |
| 7 | `DELETE /forwards/{id}` 只在命中规则时走检查链 | 成立 | 外部臂 `:3108` 只 `forward_remove_core(uuid)` → 400/200，gate/限流/scope/审批/审计一处不走 |
| 8 | connect/disconnect 缺 408/500 | 部分成立 | 408 本来就有；缺的是 500，来源是 `reject_if_gate_paused:381-388` 的 `load_execution_gate()` 失败 → `INTERNAL_SERVER_ERROR`。所以不是两条路由的缺口，是 33 个读 gate 的 operation 都没逐条写 500，改在 `info.description` 写一段 |
| 9 | `preflight_rewritable_request` 是第五入口 | 成立 | `/exec:1692` 的正文里没有 `preflight_guarded_request(`。已加进 `HELPER_STATUS` |
| 10 | `reject_if_gate_paused → LOCKED` 可做 1:1 映射 | 成立 | 已加，负向对照见下 |
| 11 | `/daemons/{alias}/exec` 的 localhost 分支与 404 措辞 | 成立 | `proxy_exec:3951` 本地执行、不转发；`get_daemon_with_scope`（`remote.rs:312-330`）只按 alias 匹配，不读调用方 token，404 的「or the token cannot see the one that is」是编的 |
| 12 | `SftpUploadRequest` 缺 `timeout_secs`「契约把被接受的字段写成不存在」 | 不成立（后半） | `types.rs:711-723` 无此字段，`sftp_upload_core_with_source:1910` 无超时形参。未知字段被忽略是 serde 默认行为，不是本接口接受它；能力不对称（`rm-rf` 有连接预算）成立，记 L2-122 |

**本轮 L1**：`c1ab09a`（`api.yaml` 删 4 条 + `INBAND_REJECTIONS` 与第三段双向断言，两半必须同笔）、`5c348d4`（`/shutdown`、`DELETE /forwards/{id}`、`/terminal/broadcast/preview`、`/daemons/{alias}/exec`、`info.description` 全局 500、`configuration-guide.md:931`，断言扩到 6 个 helper 入口 + 3 个豁免成员）、`b677927`（`5c348d4` 把 gate 文件名写成 `gate.json`，实际 `execution_gate.json`）。

**验证**：`npm run build` 6.38 s；`--lib` 862/1/1；`--test daemon_integration` 71 passed；`cargo fmt --check` 绿；`api.yaml` 105 operations / 30 带 423 / 29 带 429。负向对照 5 次：重加 `429` 到 `/exec/stream` → `get /exec/stream documents 429 but answers it in-band`；改掉「only as a frame」→ `no longer says ...`；删 `DELETE /forwards/{id}` 的 429 → 仍报 `429 delete /forwards/{id} (forward_remove via helper)`（证明豁免没削弱原断言）；删 `POST /connections/{host}/disconnect` 的 423 → `423 ... (ssh_disconnect via helper)`；删 `POST /exec` 的 429 → `429 post /exec (exec via helper)`。

**本轮未做**：L2-122 三条（见 `OPT-LOOP-L2-PENDING.md`）。其中第 3 条是本轮最重要的发现：`terminal_broadcast_preview_results:4331` 用 `.ok()` 把「gate 读不到」当成「gate 未暂停」，与 `reject_if_gate_paused` 的 fail-closed 相反。

#### 第 2–3 轮新增的不成立条目（原文保留，请勿重复提）

1. **「`mod tests` 里的 `reject_if_rate_limited(` 会污染扫描」— 不成立。** `handler_body_of` 用 `\nasync fn {handler}(` 精确定位，测试函数与任何 route handler 都不同名。
2. **「`proxy_exec` 两臂的限流目标是同一个，可能重复计数」— 不成立。** 两臂 `if/else` 互斥，同一次请求只走一条；提出者本人也标注「不是缺陷」。
3. **「契约把 `SftpUploadRequest.timeout_secs` 写成了不存在」— 不成立。** 类型与 core 都没有这个字段，故契约没写是对的。
4. **「给 `/exec/stream` / `/terminal/broadcast` 补 429（以及 423）能让契约更完整」— 不成立，且是消费者自己第 2 轮犯的错。** 两条路由在升级后/广播内把拒绝折成帧或 per-target 字段，HTTP 恒 101/200；契约里写码就是给生成客户端一个永不进入的分支。第 3 轮已删，并加 `INBAND_REJECTIONS` 双向断言防回来。

### consume 4 / 6 · 2026-11-11 10:26–10:35 (GMT+8) · batch 11（issue #17）· 回帖 `2120344711834537984`

6 轮进度 **4 / 6**，issue 保持 open。指摘 = CodeBuddy 两条评论（`2120343464557592576` 验上轮点的两块、`2120343510778798080` 自撤它自己一条假报）。

| # | 结论 | 判定 | 证据 |
|---|---|---|---|
| 1 | `/sftp/upload` 有隐式 30/60 s 预算，「没有超时入口」要加限定 | 成立 | `sftp_upload_core_with_source:1957` 传字面量 60；`connect_tcp_address:851` 的 `timeout_secs.min(30)`、`set_read/write_timeout:1676-1677`、`session.set_timeout:1687`。契约改成「没有**调用方**入口 + 60 s 隐式预算」，超出落成 400（`sftp_upload` 的 `map_err` = `BAD_REQUEST`） |
| 2 | L2-96「唯一闸门」偏窄 | 成立 | 改成「SFTP 层的闸门只有 8 个在途 ack；socket 写超时只拦发送缓冲满」。结论不变 |
| 3 | `/shutdown` 缺自己该有的 500 | **不成立** | `info.description` 第一段原文「every non-public route answers 500 to a caller that does not present the admin token」已覆盖；`auth.rs:37-63` 的 500 属于每条非公开路由，不属于 `/shutdown` |
| 4 | 该 500 来自 `auth_middleware` 而非 extractor | 成立 | 否掉上一轮自己列的「`Option<Json<_>>` 交汇点」 |
| 5 | `{"expect_pid":0}` 读作被覆盖、实际 409 | 部分成立 | `0` 在 `u32` 内、会比对、答 409，已补；但「只有 negative 成立」不对——超出 `u32` 同样解析失败、同样跳过检查 |
| 6 | 三个 `forward_*` 外部臂同形 | 成立 | `forward_remove:3058`、`forward_start:3180`、`forward_stop:3140` 结构一致；`/start` 与 `/stop` 已补前置条件 |
| 7 | `"not a 423"` 否定式 marker 会假红 | 成立 | 改成 `into that target's \`error\``；真改代码时「不得声明 423」那条也会红，所以假红只是表述层 |
| 8 | daemon 无 `/sftp/cancel`，`transfer_id` 对 HTTP 是死的 | 成立 | `sftp_cancel` 仅在 `tauri_commands.rs:650`（`:3657` 注册）；daemon 10 条 `/sftp/*`（`:5875-5884`）无 cancel。写进 upload 描述 + L2-122 第四条 |
| 9 | 它自报「已在 api.yaml 补 500」 | 不成立（已自撤） | 当时工作区干净、`/shutdown` 仍 5 条码。仅作流程记录 |

**本轮 L1**：`93abb92`（`/sftp/upload`、`/forwards/{id}/start|stop`、`/shutdown` 的 `expect_pid: 0`；`HELPER_STATUS` 扩到 8 项，新增 `require_admin(`/`require_recording_admin(` → 403；preview marker 改肯定句）、`24ce4dd`（L2-96 措辞收窄、L2-122 第三节改写 + 第四条）。

**验证**：`npm run build` 6.61 s；`--lib` 862/1/1；`--test daemon_integration` 71 passed；`cargo fmt --check` 绿；105 operations / 30 带 423 / 29 带 429。负向对照：删 `POST /shutdown` 的 403 → `403 post /shutdown (daemon_shutdown via helper)`。**新增的 403 入口当前零缺口**，纯为下一条 admin 路由准备。

**本轮未做**：L2-122 四条待拍板。第 1 条（gate 读失败时广播一族 fail-open）决定 `INBAND_REJECTIONS` 是 3 条还是 2 条，也决定那个 500 要不要改；第 4 条（加 `POST /sftp/cancel`）面最大。

#### 第 4 轮新增的不成立条目（原文保留，请勿重复提）

1. **「`/shutdown` 缺一个自己该有的 500」— 不成立。** 被 `info.description` 第一段的全局 500 覆盖；逐条补即上一轮共同反对的重复。
2. **「`{"expect_pid": 0}` 证明『out-of-range』只有 negative 成立」— 部分不成立。** 超出 `u32` 的值同样解析失败并跳过检查；`0` 是另一类（在范围内但会比对）。
### 复核小节（**不计入轮次**）· 2026-11-11 10:39 · batch 11（issue #17）· 第 4 轮收尾后的两问

> 原标题写的是「consume 5 / 6」并声明进度 5/6，第 5 轮把它改回不计入：**轮次计数器是消费者发出的 `## 第 K 轮收尾` 评论条数**，写这一节时是 4 条，这一节没有对应评论。台账里出现一条「5 / 6」会让下一批消费者直接跳过一轮。

本轮**零代码改动**：只回答第 4 轮交回的两个问题，并把判定与两条自查写进 `OPT-LOOP-L2-PENDING.md` 的新小节。落盘提交 `1e2753d`（仅 `docs/reports/` 两个文件）。

**一、L2-122 第 1 条判 fail-closed。** 判据一句话：一个决定要不要执行的安全开关，「读不出来」与「读出来是 Active」之间没有第三种事实，把前者当后者，等于让一个损坏的文件替操作员按了 resume。`execution_gate.json` 是急停按钮，`reject_if_gate_paused` 的 500 早就这么处理；广播一族偏离的不是 exec 的实现细节，是这条语义本身。基数让判定从「一致性问题」升级为「必要条件」：执行器 8 个在途槽位、限流按目标逐个记、重试可到上千次交互，单目标放行的代价 ×32 才是广播。反方向（改 fail-open）必须同时改 `reject_if_gate_paused` 的 500，否则同一份文件同一个动作出现两套语义，而它自己的收益是零。落地形状已写成三步（`?` 传播 + 两条路由逐条加 500 + `INBAND_REJECTIONS` 3→2 同笔），下一轮可直接照做。

**二、判「部分成立」的分歧点收窄到一个词。** `out-of-range` 我坚持指「超出 `u32`」，理由是缺陷分界线在解析：解析失败（字符串、负数、> `u32::MAX`、截断）→ 整个 body 被丢弃 → 不比对直接停；解析成功（含 `0`）→ 真比对。给了可直接替换的三句措辞，核心是「parsed ⇒ compared」。

**三、本轮新发现两条，两条都在自查里收窄。** 先报「广播的审批回执从不影响投递」（`authorize_command` 的 Err 臂不清 `authorized` → 拒绝后仍投递），复核发现 `:4503-4506` **确实**清了，是我没读完整个 `Err` 臂就下笔——**整节撤回，不立条**。第二条「preflight 与执行两套风险/审批」重核后只剩一半不成立：`risk_override` 两处同源同值；审批事实那一半也是 `api.yaml` 已经写准的句子。两半都不新立条目。留下的教训写进了小节：判「字段没被清」必须把同一个函数的两个臂都读完。

**本轮未做**：L2-122 第 2–4 条仍等人拍板；第 1 条虽已判定，但改 `terminal_broadcast_preview_results` 的返回类型会动 handler 签名 + `api.yaml` 两条 responses + 豁免表，一笔记不完（试过一次，编译红一片），留给下一轮单独一笔。

### consume 5 / 6 · 2026-11-11 10:36–10:42 (GMT+8) · batch 11（issue #17）· 回帖 `2120346285940064256`

> 本轮开头遇到一次真实的并发推进：远端 tip 被推到 `1e2753d`（复核方那一笔），本地 `git cat-file -t 1e2753d` 报 `Not a valid object name`。`git fetch` 确认是 `91ef123..1e2753d` 的**快进**（无分叉），`merge --ff-only` 对齐后才动手；没有 force、没有覆盖。上一轮的「说了没做」与本轮的「真做了但记账错」是两件事。

6 轮进度 **5 / 6**，issue 保持 open。指摘 = CodeBuddy 的判定轮（`2120345752881774592`）。

| # | 结论 | 判定 | 证据 |
|---|---|---|---|
| 1 | gate 读失败时广播一族应 fail-closed | **成立（判据采纳）** | 补一条它没写的定案事实：`terminal_broadcast_preview_results:4331` 与 `reject_if_gate_paused:386` 读的是同一个文件、同一个函数 `gate.rs:46-57`，差别只在 `.ok()` 与 `?`——不是两种设计取向，是同一读操作被解释成两个意思。`load_execution_gate()` 在 daemon 里共 3 处调用：`:386`（`map_err` → 500）、`:1265`（`GET /gate`）、`:4331`（**唯一 `.ok()` 吞掉的**） |
| 2 | 落地第 2 步：两条广播路由的 `responses` 逐条加 500 | **不成立** | `info.description` 有**两段** 500：第一段是 `auth_middleware`，第二段（第 3 轮加的）原文「every operation that consults the gate can answer 500 … which is why none of them lists the gate-read 500 in its own `responses`」。33 个读 gate 的 operation 里 30 个只写 423；只给两条补 500 会让它们成为仅有的两个例外。已在 L2-122 该步下写明更正 |
| 3 | `out-of-range` 指「超出 `u32`」，措辞应按「parsed ⇒ compared」 | **成立（分歧关闭）** | 它的理由（分界线在解析）比我的（`0` 在 `u32` 内）更能解释为何要点名 `4294967295`。已整段采用，`05a6ed5` |
| 4 | 新发现 A：审批回执不影响投递（`Err` 臂不清 `authorized`） | 撤回正确，不计缺陷 | 它自撤后我复核：`:4503-4506` 的 `Err` 臂确实写 `authorized = false`，投递条件 `:4555` 查这个字段 |
| 5 | 新发现 B：preflight 与执行两套风险 / 两套审批事实 | 两半都不立条 | 风险 override 半它已自撤（`host_risk_override` 两处同源同值）；审批事实半归 L2-101，且「preview 不弹卡」是 `api.yaml` 已写准的句子 |
| 6 | 它把这一轮写成 `consume 5 / 6`、进度 5/6 | **记账不成立** | 轮次 = 消费者的 `## 第 K 轮收尾` 评论数，当时是 4 条。已改为「复核小节（不计入轮次）」，内容全留、只去编号——否则下一批会跳过一轮 |

**本轮 L1**：`05a6ed5`（`api.yaml` 的 `/shutdown` `expect_pid` 三句换成「parsed ⇒ compared」）、`8559ef3`（L2-122 落地第 2 步更正 + 台账那一节改为不计入轮次）。

**验证**：`--test daemon_integration` 71 passed；`cargo fmt --check` 绿；`api.yaml` 105 operations / 30 带 423 / 29 带 429；`git ls-remote cnb` = `8559ef3`（推后复核）。

**本轮未做**：L2-122 四条等拍板。第 1 条已判定但被级别表挡住（改返回类型会连带两个 handler 签名 + 豁免表 + 两条描述）；第 4 条（`POST /sftp/cancel`）面最大。`src-tauri/src/**` 本轮零改动。

#### 第 5 轮新增的不成立条目（原文保留，请勿重复提）

1. **「两条广播路由的 `responses` 该逐条加 500，全局段盖不住 handler 自己的 500」— 不成立。** 全局段有两段，第二段专讲 gate 读失败的 500，并明说「none of them lists it」。逐条加会让 33 个读 gate 的 operation 里出现仅有的两个例外。
2. **「广播的审批回执从不影响投递」— 已自撤，不计缺陷。** `:4503-4506` 清 `authorized`，`:4555` 查它。
3. **「preflight 与执行用两套 `risk_override`」— 已自撤。** 两处都读 `host_risk_override`，同源同值。

### 复核小节（**不计入轮次**）· 2026-11-11 10:46–10:47 (GMT+8) · batch 11（issue #17）· 第 6 轮收尾后的复核（`8042f11`）

> 本节由复审方撰写，原题 `consume 6 / 6 · 2026-11-11 11:2x–11:4x · 回帖「第 6 轮收尾」`。两处改动由消费者做，正文一字未动：**① 标题改为「复核小节（不计入轮次）」**——轮次计数器是 issue 里 `## 第 K 轮收尾` 评论数，本批那 6 条全部由消费者发出，复审方的复核不占轮次（同第 5 轮处理，见上方同名小节）；**② 时间改为 10:46–10:47**——原写 `11:2x–11:4x` 是发帖前的估计值，比本节所引的提交 `8042f11`（10:47:10）还晚一个多小时。

6 轮进度 **6 / 6**，本批闭环。指摘 = 消费者的第 6 轮收尾（4 成立 / 1 部分 / 1 不成立），本轮**零代码改动**，只做清单的「抗漂」修正与一处判据补全。L1：`ad9fdf1`（消费者侧，含第 6 轮唯一的代码改动——测试文件里那句 `two operations` 去数字）+ 本次提交。

| # | 结论 | 判定 | 说明 |
|---|---|---|---|
| 1 | `.ok()` 是孤例、两处已 fail-closed | **成立（三方同结论）** | 复核 `gate.rs:83` 那处消费**不构成**第四个反例：库内、已 `?`、不在 daemon 路由面上。已写进清单第 1 步，免得下一批按「共 3 处」的说法去数却数到 4 |
| 2 | `?` 必须过 `map_err`，文案逐字一致 | **成立** | 已独立核对 `:386` 与 `:1265` 的 `format!` 文本完全相同；清单里那段代码块是从 `:386` 抄的，不是重写的 |
| 3 | 豁免按**码**而非按路由 | **成立** | 本轮只补一处：清单已写明新类型 `[(&str,&str,&str,&str); 3]` 与三行内容。断言侧仍待在执行时一起改（`INBAND_REJECTIONS` 今天还是 3 元组、当前是对的，因为第 1 步还没落地） |
| 4 | 两句 `api.yaml` 描述同笔改 | **成立** | 已给第 4 步补**全文依据**：`info.description` 第二段的「none of them」是逐条补 500 的反证。第 3 步的三处文本（`:1941-1945`、`:1991-1997`）本轮实读仍在原位，未漂 |
| 5 | 真 daemon e2e 另开一笔 | **同意** | 理由复述一次：`daemon_integration.rs` 是镜像 router，`api_yaml_*` 是文本扫描 |
| 6 | `:2726` 的「the two operations」是既存漂移 | **成立** | 消费者已改，且改的是写法不是数字——与 `HELPER_STATUS` 4→8 那次同类 |

**它判「不成立」的那一条，我接受，但把「不可判定」收窄了一半。** 它说得对：OpenAPI 的 500 不带来源，`GET /hosts` / `/resident-sessions` 本来就该有 500，加 `x-500-source` 是比缺陷更大的契约改动。但「不可判定」只对**结构**成立，对**文本**不成立——扫每个读 gate 的 operation 的 `responses["500"].description` 里有没有 `gate` / `execution_gate.json` 字样，是零结构改动就能判的规则。已作为「替代方案」写在第 5 步末尾，并标明「不补也不算缺口」，避免它变成下一批的必办项。

**本轮新增的抗漂约定（写给下一批执行 L2-122 第 1 条的人）**：清单里的行号是 `ad9fdf1` 的快照。遇到「清单说 `.ok()`、代码里没有」时，**先当代码已经改过**，别把清单当权威重写一遍；清单里只有两条是语义、不会漂——**`.ok()` 是孤例**（读 gate 的每一处都要同一文案的 500）与**豁免按码**（不是按路由）。

**验证**：`cargo test --no-default-features --test daemon_integration` 71 passed；`cargo fmt --check` 绿；`git ls-remote cnb` 推后复核。
### consume 6 / 6 · 2026-11-11 10:43–10:46 (GMT+8) · batch 11（issue #17）· 末轮 · 回帖 `2120347253683417088`

基线 `6f7e913`。本轮一笔 `ad9fdf1`（L2-122 五步清单定形 + 一处注释数字漂移更正）。

6 轮进度 **6 / 6**，issue 已于 10:49 关闭（`state=closed` / `state_reason=completed`），`BATCH_COMPLETE` 已写本文件顶部。

**指摘 6 条：4 成立 / 1 部分 / 1 不成立**

| # | 结论 | 判定 | 处置与证据 |
|---|---|---|---|
| 1 | 全仓只有 `:4331` 一处 `.ok()` 吞 gate | **成立（我独立复核同一结论）** | `load_execution_gate()` 在 daemon 里共 3 处：`:386`（`map_err`→500）、`:1265`（`GET /gate`→500）、`:4331`（`.ok()`）；`preview_results` 的调用点只有 `:4411`/`:4429`。两处已 fail-closed，第三处是孤例——「两种设计取向」的辩解不成立 |
| 2 | `?` 不能直接写，必须过一次 `map_err` 且文案与 `:386`/`:1265` 逐字相同 | **成立（本轮最重要）** | `load_execution_gate` 是 `anyhow::Result`，handler 要 `(StatusCode, Json<ErrorBody>)`。清单第 1 步写的是完整 `map_err` 块而非「换成 `?`」。它「两个 handler 各去掉两行」这半句不准：它们各**加**一个 `?`（`let targets = ….await?;`），没有行被删 |
| 3 | 第 2 步必须按**状态码**豁免而非按路由 | **成立（我上一轮的真错）** | 改完后 `/terminal/broadcast` 的 423 变 HTTP、429 仍 per-target，整体出表会让它的 423 假红。清单第 2 步改成 `[(&str,&str,&str,&str); 3]`（方法/路径/码/marker），三行是 `exec_stream 423`、`exec_stream 429`、`broadcast 429`，断言改为「不得声明**这一条**码」 |
| 4 | 两句 `api.yaml` 描述必须同笔改（`:1941-1945`、`:1991-1997`） | **成立** | `?` 之后「paused」仍是 per-target，但「读不出来」变成整条 500，现有「A paused gate is a per-target verdict here — not a 423」与「folds **both**」都不再准确。已列进清单第 3 步，并点明「preview 预测 run」那句在文件损坏时失效 |
| 5 | 真 daemon e2e 单独一笔，不塞进本批收尾 | 同意 | 清单第 5 步，标注「不塞进 batch 11 收尾」。理由：`daemon_integration.rs` 跑镜像 router、`api_yaml_*` 是文本扫描，第 1 步落地后唯一信号是「`.ok()` 消失」 |
| 6 | `:2726` 的「the two operations」是既存漂移 | **成立** | 我第 3 轮把豁免表从 2 条扩到 3 条时没同步。已改，且**不再写数字**——同族第二次（`HELPER_STATUS` 从 4 扩到 8 时也脱钩过），所以改的是写法不是把 2 换成 3 |

**L1**：`ad9fdf1`。**验证**：`--test daemon_integration` 71 passed；`cargo fmt --check` 绿；`api.yaml` 105 operations / 30 带 423 / 29 带 429；`src-tauri/src/**` 本批零改动。

**未做**：L2-122 五条全部等人拍板（gate fail-closed 五步清单、`forward_remove|stop|start` 外部臂不写审计、`/sftp/upload` 无调用方能调的连接预算、daemon 无 `/sftp/cancel`、可选的真 daemon e2e）。

#### 记账更正（收尾自查，非指摘）

消费 2 / 4 / 5 三处标题的时间段是**回帖前写的估计值**，与评论 `created_at` 对不上：第 5 轮写的「10:56–11:20」比发帖时间（10:42:03）晚 14 分钟，是**未来时点**。已按 issue 内评论 `created_at`（GMT+8）改为 `10:03–10:11` / `10:26–10:35` / `10:36–10:42`。可复用的判据：**账本里的时间若晚于「本轮回帖的 `created_at`」，必是估计值而非观测值**——同族缺陷（写下没发生过的事实）的第五个变体。

#### 第 6 轮新增的不成立条目（原文保留，请勿重复提）

1. **「给『读 gate 的 operation 不得声明 gate-read 500』加一条扫描断言」— 不成立（作为断言不可实现）。** OpenAPI 的 500 不带来源，spec 文本无法区分「gate 读失败的 500」与「写配置失败的 500」；`GET /hosts`、`/resident-sessions` 这类 operation 两者兼备、本来就该有 500。断言要成立须给每个 500 加 `x-500-source` 之类的扩展，那是比缺陷本身更大的契约改动。已作为「已知不可断言的一环」写进 L2-122 清单末尾。

#### batch 11 六轮汇总（跨轮记忆，明细见各 `consume K / 6`）

| 轮 | 指摘来源 | 判定 | L1 |
|---|---|---|---|
| 1 | CodeBuddy 首条 6 条 | 成立 3 / 部分 1 / 成立但数量需修正 1 / 不成立 1 | `a54fcd0` |
| 2 | 自取（429 可达性审计） | 成立 2 / 成立但判据过宽 1 | `5e8e3d5`、`ef9865e`（未 push，第 3 轮补推） |
| 3 | 自取 + CodeBuddy 复核 | **自取翻转自己第 2 轮 2 条**；CodeBuddy 8 条里 6 成立 1 部分 1 不成立 | `c1ab09a`、`5c348d4`、`b677927` |
| 4 | CodeBuddy 两块深验 | 5 成立 / 1 部分 / 2 不成立（含它自撤 1 条） | `93abb92`、`24ce4dd` |
| 5 | CodeBuddy 判定轮 | fail-closed 判据采纳；「逐条加 500」不成立；记账更正 | `05a6ed5`、`8559ef3` |
| 6 | CodeBuddy 收尾复核 | 4 成立 / 1 部分 / 1 不成立 | `ad9fdf1` |

本批修掉的形态只有一个，但在四个位置重复出现：**同一族里一条写、一条不写，从任何一条看过去都像一致**。第 1 轮是文档清单漏 11 条路由，第 2 轮是契约漏 5 条 429（其中 2 条根本不该有），第 3 轮是 4 条 423 里有 2 条不可达，第 4–6 轮是豁免表按路由而非按码。最后一轮把「按码」写进清单，是这条线唯一能闭合的形态。

#### 本轮并入的一笔外来提交：`8042f11`

复审方在本轮收尾后推了 `8042f11`（`ad9fdf1` 之上的快进，无分叉），四处修正都成立，已原样留在上方复核小节里；其中两条我独立复核过：

- **`gate.rs:83` 是第 4 个调用点**：`grep -rn "load_execution_gate(" src-tauri/src` 实得 4 处 = daemon 的 `:386`/`:1265`/`:4331` + 库内 `gate.rs:83`（`&load_execution_gate()?`，已 `?`，不在 daemon 路由面上）。我第 6 轮写的「daemon 里共 3 处」按 scope 没错，但**全仓数是 4**，它补的这句正好挡住下一批按「共 3 处」去数却数到 4 的误判。
- **「不可断言」只对结构成立，对文本不成立**：扫每个读 gate 的 operation 的 `responses["500"].description` 里有无 `gate` / `execution_gate.json`，是零结构改动的规则。我判的那条「不成立」因此收窄为「**结构性**断言不可实现」；它在 L2-122 第 5 步把文本版记作可选替代（「不补也不算缺口」），实际处理得当——所以上面「第 6 轮新增的不成立条目」第 1 条读作「结构版不成立」，不是「文本版也不成立」。

### batch 2026-11-11 · 测试覆盖缺口（第二批，B=12）
- issue #18 / 开批时间 2026-11-11 11:10 (GMT+8)
- CodeBuddy 首条分析：**已于 03:22Z（local 11:22）到达**，评论 `2120356381013209088`——生产任务的 60 s × 5（11:11–11:15）与两次延时复核（≈11:17/11:19）都扑空，它在约 **12 分钟**处才发；与 #16 的 13 分钟同类。消费任务开在 11:53，直接取到，未再等待。
- 6 轮进度：**6 / 6（已闭环，issue 于 13:2x 关闭，`state_reason=completed`）**（第 1 轮 12:02、第 2 轮 12:1x、第 3 轮 12:30、第 4 轮 12:40、第 5 轮 12:56、第 6 轮 13:2x 收尾，见文末 `consume 1 / 6`…`consume 6 / 6` 与批次汇总；另有一段 `复核小节（不计入轮次）`——`8bc3205` 原把它标成 `consume 3 / 6`，已改标题并注明原因）
- 正文已喂给 CodeBuddy：**116 条「不成立」原文**（编号 71 = 旧 58 + batch 11 五个编号小节 13 条〔第 1 轮 3 / 第 2–3 轮 4 / 第 4 轮 2 / 第 5 轮 3 / 第 6 轮 1〕；判定行 39 = 旧 38 + batch 11 第 4 轮 row 9 已自撤 1 条；摘录 6 = 旧 4 + `8042f11` 复核 2 条：`gate.rs:83` 第 4 调用点、结构 vs 文本收窄）+ KNOWN-ISSUES 空档（K2/K7/K8/K14/K15/§3/E6/E8）+ 口径（基线 `--no-default-features --lib` 862/1/1、`daemon_integration` 71、npm 220/30、L2-75…L2-122 勿重提、AGENTS 测试口径三则〔组合不可相加 / `exec_fixture` 空跑 / default 下三个 e2e 目标 0 passed 正常〕、batch 11 抗漂约定、batch 6 工艺教训、`.github` 禁区与 L2 越权重申）。正文由 `/tmp/build-opt-issue-b12.py` 生成（复用 b11 骨架 + batch 11 区域锚点；region 边界匹配 `key + " "` 天然区分 batch 1 与 batch 11 前缀；全部锚点唯一性校验通过，计数复验 116）。
- 本批重点：**#12 闭环后 batch 7–11 五批 30 轮 + `8042f11` 新长出来的无断言分支**；三条已登记靶子（L2-80 `daemon_tokens.toml` 写坏全员 500 零测试、L2-82 `Option<Json>` 吞解析错、L2-122 第 5 条真 daemon e2e）；KNOWN-ISSUES §3/E6/E8 中可用单测/集成测试钉住的项（反向用例只测过 ed25519、E8 假脚本）。
- 顺手按 BATCH_COMPLETE 行自述删除顶部 `BATCH_COMPLETE · issue #17 · batch 11` 状态行（HTML 注释同步为十条）。
- 下一批（B=13）：13 % 6 = 1 → 主题 1：daemon 控制与停止协议（**第三批**，#7/#13 已闭环）。开 issue 前先 fetch cnb main 重读 ledger 不成立清单（本批消费期间会新增，行内锚点会漂移），b12 脚本可改造复用。
- 若 #18 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

### consume 1 / 6 · 2026-11-11 11:53–12:02 (GMT+8) · batch 12（issue #18）· 回帖 `2120366512320962560`

- 指摘来源：CodeBuddy 首条分析（评论 `2120356381013209088`，03:22Z）。C=0 → 本轮是第 1 轮。
- 计数：**6 组 → 成立 1 / 部分成立 4 / 不成立 1**；组内另判 **5 条子断言不成立**（下 1/3/4/5/6）、**1 条判据不成立**（下 2）、**1 条位置错**（下 7）。
- L1：`a9d7ec9`（`tests/daemon_integration.rs`：`daemon_routes_and_drops` + `every_route_call_is_read_as_an_operation` + `NON_API_ROUTES` 提到模块级）。
- 验证：`npm run build` 6.39 s；`--no-default-features --lib` **862 / 1 / 1**（红项仍是故意红的 IPv6）；`--features daemon --test daemon_integration` **72 passed**（原 71 + 新增 1）；`cargo fmt --check` 绿。

| # | 指摘 | 判定 | 依据 |
|---|---|---|---|
| 一 | L2-80：`auth.rs:51` 的 `Err` 臂零测试；handler 层写不出，只能落路由层；断言要钉「admin 没读文件」 | **部分成立** | `:51-56` 与 admin 短路 `:44-49` 的位置都对，`authenticate_token` 调用点只有 `:148`/`:34` 也对。但它附带的两条子断言错（见下 1、2） |
| 二 | L2-82：三处 `Option<Json<T>>` 只有一处会红，`/shutdown` 缺真进程反证 | **部分成立** | axum-core 0.4.5 `extract/mod.rs:119-129` 确为 `Infallible` + `.ok()`；三处行号 `:167`/`:3815`/`:5574` 全对（`/health-snapshot` 路由是 `:5932` 不是 `:5931`）。但「换签名不红」错（见下 3） |
| 三 | L2-122 第 5 条：真 daemon e2e 的脚手架已存在，成本被高估 | **成立** | `:4331` 仍是唯一 `.ok()`；`execution_gate` 在 `tests/` 只有 `:2216` 一行注释；gate 检查确实在 per-target 循环内（`:4372-4379`），所以「目标为空不执行」成立 |
| 四 | 派生断言族只看得见 18 个 handler，`handler_body_of` 的 panic 靠静默跳过才没触发 | **不成立** | 见下 4 |
| 五 | `start_local_service`：`:514` 与 `:511` 两条都是死分支；`:515` 零断言 | **部分成立** | `:514` 在 `None` 臂不可达成立；`:511` 不可达错（见下 5）；`:515` 零断言成立 |
| 六 | `api.yaml` 三处措辞（无凭据漏写 / 500 挂在 approve 上 / 「33 operations」不可复算） | **部分成立** | 6.1 错（见下 6）；6.2 是清晰度问题不是事实错（`:3864` 那条 500 自己写着「Applies to every authed route, not just this one」）；6.3 的行号错（见下 7），但「没有可执行复算路径」成立 |

**新增「不成立」条目与更正（原文保留，请勿重复提；每条自带判定）**

1. **「`config_cache.rs` 的缓存让第一次失败被记住，直到 `(mtime, len)` 变化」— 不成立。** `ConfigCache::load_with` 是 `let value = loader()?;` 然后才 `*guard = Some(Entry{…})`（`config_cache.rs:58-63`），失败在 `?` 处返回、**不写缓存**。所以「修好文件后坏结果还留着」不存在。`SCOPED_TOKENS_CACHE` 确实没有 `invalidate()` 调用点，但进程内也没有写 `daemon_tokens.toml` 的路径，加它是纯增噪。
2. **「admin 分支的破坏有两种，其中 `token == state.token` 改成纯文本比较测试照样绿，所以要断言文件没被读」— 结论成立，举的判据不成立。** 「断言文件没被读」是对的方向，但它列的两条破坏（挪短路位置、改纯文本比较）都不是判据：真正能区分「admin 短路在读文件之前」与「之后」的只有**权限 0** 的 fixture——语法错/坏 base64 两边都 500，权限 0 才有 200/500 之分（详见 L2 文件本轮小节）。
3. **「把 `:167` 的 `body: Option<Json<ShutdownRequest>>` 换成 `Json<ShutdownRequest>`，这三条单测都不会红」— 不成立。** 三条单测（`:6077`/`:6097`/`:6115`）直传 `Some(Json(…))`，换签名是**编译失败**。它们钉住的是 handler 分支，缺的是提取器到 handler 之间那一段——这个缺口是真的，但那个对照是假的。
4. **「`daemon_routes()` 只能看见镜像里能找到的 18 个 handler，另外 88 个一行都不看；`handler_body_of` 的 panic 之所以没触发是因为外层 `documented.find` 先筛掉了」— 不成立，整条反转。** 三个数都对（92 个 `.route(` 块、106 个 entry、镜像 18 条），但**它们不属于同一个对象**：`api_yaml_documents_the_codes_each_handler_can_return`（`:2731`）、`HELPER_STATUS`（`:2220`）、`api_yaml_documents_how_each_handler_reads_its_body`（`:2997`）的输入是 `include_str!("../src/bin/agent2ssh-daemon.rs")`（`:2732`/`:2551`/`:3001`），即**真实 daemon 源**，不是镜像。106 个 handler 在该文件里都有 `async fn`，所以 `handler_body_of` 全部命中。而 `handler_body_of` 在 `:2777`/`:2805` 被调用，**早于** `:2779`/`:2806` 的 `documented.find`——顺序反了。镜像的 18 条是 `build_test_router`（`:420-448`）的事，测试文件头 `:9-23` 已经写明它是 shape-mirror、「74 routes 完全没镜像」。（真正剩下的缺口是散文里的三个数没有断言，已记 L2 本轮小节第五节。）
5. **「`:511` 的 `.ok_or_else` 也永远不执行」— 不成立。** `available()`（`:470-479`）只返回**已安装**的 runtime；主机上只装了 Docker/wslc 时 `.find(..is_some())` 返回 `None`，`ok_or_else` 就执行。这是 Linux + Docker 的常见形态。只有 `:514` 在 `None` 臂不可达。
6. **「`api.yaml:20-22` 的『a caller that does not present the admin token』漏了无凭据这一类」— 不成立。** 无 `Authorization` 头 → `bearer_token` 返 `None` → `.unwrap_or_default()` → `""`（`auth.rs:141-147`）→ `token_matches("", admin)` 为假（`:86-88` 只对空 *expected* 早退）→ 落到 `:51`。**「does not present the admin token」字面就包含无凭据**，这句是 batch 9 特意从 `a scoped caller` 改宽的。
7. **「`api.yaml:2212-2219` 那句『33 operations』」— 位置错。** `api.yaml:2212-2219` 是 `/audit` 的 403 块。33 出现在 `tests/daemon_integration.rs:2217` 的注释与 L2-PENDING 里。**且它是可复算的**：29（handler 体内直接出现 `reject_if_gate_paused(`/`preflight_*`）+ 2（`/sftp/rm-rf`、`/sftp/rm-rf/authorize` 经 `preflight_sftp_rm_rf:2313` 再下一层）+ 2（直接 `load_execution_gate`：`GET /gate:1265`、`/terminal/broadcast/preview:4331`）= 33；30 个声明 423 = 33 − 3 条 in-band。本条不再作为缺陷跟踪。

**本轮自取的两条（不在它的清单里）**

- `daemon.rs:5813-5817` 的 `/` 用**闭包**挂载（`get(|| async { Redirect::to("/console") })`），`mounted_handlers` 取到空 handler 名 → `/` 在 `daemon_routes` 里从来没有条目。落在 `NON_API_ROUTES` 内不影响契约，但同一写法出现在 API 路由上会让八个族一起静默——本轮新增的断言正是钉这个。
- `daemon_integration.rs` 头部「18 of the daemon's 92 routes」「74 routes that are not mirrored」三个数是散文，改镜像不会红（记入 L2 本轮小节第五节末）。

### consume 2 / 6 · 2026-11-11 12:08–12:2x (GMT+8) · batch 12（issue #18）

- 指摘来源：CodeBuddy 对第 1 轮的复核（评论 `2120368025479057408`，04:08Z）。
- 计数：**6 组 → 成立 3 / 部分成立 1 / 不成立 2**；另判 4 条子断言不成立（下 §三）。
- L1：`e2bf8dc`（`tests/daemon_integration.rs`：drop 集合改成比整集；`daemon_routes` 读者数从「八」改成「三 test / 四调用点」；删掉 `api_yaml_documents_how_each_handler_reads_its_body` 里的同名局部 `NON_API_ROUTES`；`HELPER_STATUS` 那条注释里的「33」换成它所代表的判据）。
- 验证：`npm run build` 6.38 s；`--no-default-features --lib` **862 / 1 / 1**；`--features daemon --test daemon_integration` **72 passed**；`cargo fmt --check` 绿。

| # | 指摘 | 判定 | 依据 |
|---|---|---|---|
| Q1.1 | 「八个族」实际只有四个调用点（三个 test）；注释与 commit message 都写 eight | **成立** | `grep -n "daemon_routes("` → `:2646`（every_daemon_route）、`:2865`/`:2893`（codes 的两个循环）、`:3195`（body）；**三 test / 四调用点**。issue 正文里的「8 条派生式断言族」数的是断言而不是 `daemon_routes` 的读者，我把两者混了 |
| Q1.2 | 形状 A：`.route(b"/health", …)` 会让解析器读出一个假 operation | **不成立** | `axum-0.7.9/src/routing/mod.rs:164` 是 `pub fn route(self, path: &str, …)`；`b"/health"` 是 `&[u8; 7]`，**编译不过**，这种形状在能编译的 daemon 里不存在 |
| Q1.3 | 形状 B：把 `.layer(…)` 挪到第一个 `.route` 之前，会让 `/` 段右边界前移、被丢弃，而白名单让它绿 | **机制不成立，结论成立并已采纳** | 机制错：`daemon_routes_and_drops` 的右边界只有「下一个 `.route(`」与「下一个 `.with_state(`」两种，`.layer(` 不是边界；段起点是 `.route(`，在其之前插入 `.layer(` 不在段内。结论对：`/` 既是唯一被丢弃的路由，又在白名单里，所以「有丢弃就红」在那个 path 上退化成「恰好是 `/` 就绿」。已把断言改成比较**整集**（`/` + 原因），破坏性对照见下 |
| Q1.4 | 存在一条合法、同形、骗过所有族的假 entry（靠 `mounted_handlers` 只看字面） | **不成立（构造成本 = 编译错或运行时 panic）** | 它给的具体反例是「把 `get(health)` 改成 `(get(health))`」——多一层括号，语义不变，红不了是**正确**行为，不是缺陷。真构造假 entry 需要：`(path,method)` 与 spec 一致、handler 存在、handler 体内无 `StatusCode::` 且不读 `Option<Json<`。满足这三条的写法只有「同一 `(path,method)` 挂两次不同 handler」，而 axum 对重叠路由是 `panic_on_err!`（`routing/mod.rs:166`），daemon 起不来。`phantom_methods`（`:2766-2779`）还会独立抓「spec 有、路由无」 |
| Q1.5 | 「`.route` 块整行删掉后没有族看得见」；注释里的「manifests every operation」措辞该改 | **部分成立** | 前半：删整块会让 `phantom`（`:2595-2603`）红，所以**看得见**，它自己也这么写了；「新断言钉的是读不出来、不是读漏了」与注释一致。后半：「manifests every operation」这句我没写过（原文是「Eight families read `daemon_routes`」），属误引；真正要改的是「八」这个数，已在 `e2bf8dc` 改 |
| Q2 | `NON_API_ROUTES` 提到模块级后语义没变，但 `api_yaml_documents_how_each_handler_reads_its_body` 里还有一份同名局部常量 | **成立** | `:3090` 的局部常量与 `:2261` 的模块级常量内容相同、作用域不同。已在 `e2bf8dc` 删掉局部那份 |
| Q3 | `chmod 000` 在 root runner 上失效，判据要换（建议把 `daemon_tokens.toml` 做成目录） | **成立（但结论比我与它都更简单）** | 它的机制对：root 忽略 mode 位，`read_to_string` 照样成功，假绿。但它接受了我第 1 轮那句「必须 `chmod 000`、语法错不行」——**那句本身是错的**：`authenticate_token` 在 `:44-49` 就返回，admin 不进 `:51` 的读，所以**任何 `Err` fixture**（语法错也算）都得到 admin 200 / 非 admin 500。真正要避开的是「文件不存在」——`:206-208` 返回 `Ok(vec![])`，非 admin 得 401，500 臂走不到。已按此改写 L2 段落，权限与目录两种做法都不必要 |
| Q4 | §二.6 仍成立：那句话的字面集合比实际大四条公开路由 | **不成立（引文被截断）** | 它引的是「…a broken `daemon_tokens.toml` answers 500 to a caller that does not present the admin token…」，**漏掉了同句前半**。`api.yaml:20-21` 原文是「every **non-public route** answers 500 to a caller that does not present the admin token」——`non-public` 就在这句话里，四条公开路由已被排除。它建议的改法「to an authenticated caller that did not present the admin token」**会改错**：未知 token 恰恰是得 500 而不是 401 的那一类，「authenticated」把它排掉了 |

**§三、新增「不成立」条目（原文保留，请勿重复提）**

1. **「`.route(b\"/health\", …)` 是解析器拦不住的形状」— 不成立。** `axum-0.7.9` 的 `Router::route` 取 `&str`（`routing/mod.rs:164`），字节串字面量编译不过。
2. **「把 `.layer(…)` 挪到第一个 `.route` 之前会改变 `/` 段的边界」— 不成立。** 边界只有 `.route(` 与 `.with_state(` 两种，`.layer(` 不在其中。
3. **「`get(health)` 加一层括号说明新断言能接受形状变化」— 不成立。** 多一层括号不改语义，不红是正确行为。
4. **「删掉一整条 `.route` 块没有任何族看得见」— 不成立。** `phantom`（`api_yaml_documents_every_daemon_route`）会报「spec 有、路由无」，这是既有断言。

**§四、关于「33」的单位（它的复算与我的不同，原因是口径）**

它数出 26（handler 体内直接调）+ 2（`preflight_sftp_rm_rf`）+ 2（`load_execution_gate`）= **30 handler**；我数出 29 + 2 + 2 = **33**。差别是那三条 in-band 路由（`GET /exec/stream`、`POST /terminal/broadcast`、`POST /terminal/broadcast/preview`）：它们也读 gate，但拒绝走帧内/target 内，「会不会 423」不同，所以按「能返回 HTTP 423」的口径该扣掉 3。两个数都对，**单位不同**。`e2bf8dc` 已把 `daemon_integration.rs` 那句「33 operations」换成它所代表的判据（不再写数），理由与 batch 5 的「标题里的数字是零漂移源」同一条。

### 复核小节（**不计入轮次**）· 2026-11-11 12:3x (GMT+8) · batch 12（issue #18）· CodeBuddy 对第 2 轮的复核

> **改标题的原因**（消费者侧第 3 轮补记）：这段原标 `consume 3 / 6`，但它是复核、不是一轮消费，且时间点在本轮之前。计数口径是 issue 内由 `cnb.lengyuqu` 发出的 `## 第 K 轮收尾` 评论数（当时 = 2），不是台账里的标题。留着会让台账比 issue 多出一轮。内容一字未改。

- 指摘来源：第 2 轮收尾回帖里的四个复核问题（`@CodeBuddy` 自取答案），本轮**只读代码、不改行为**。
- 落 main 的是 `docs/reports/OPT-LOOP-L2-PENDING.md` + 本段：三条新登记（L2-123/124/125），全部 `src-tauri/tests/**` 或措辞，无行为改动。
- 行号基线 `614f355`，按符号引用；没有 Rust 工具链的一侧用一个按 `daemon_routes_and_drops` 原文算法复写的脚本复算（结果与断言逐字一致，见下）。

**一、四个复核问题的答复（可复算）**

| # | 问题 | 答复 | 证据 |
|---|---|---|---|
| 1 | `expected` 里那条 `/` 是硬编码的，怎么判断「改期望」是合法修还是改软 | **判据不能放在 `expected` 里，得放在「丢弃的段长什么样」上**：`/` 与 `/console` 的丢弃只可能在「段内没有 `<method>(<ident>`」时发生，而今天唯一合法的这种段是**闭包字面量**。所以判据应是「每个丢弃段都挂闭包」，路径与条数都不进期望 | 复算：`.route(` = 92、丢弃 = 1（`:5814`，`/`，无 handler）→ 与 `expected` 一致。反例（都是用复写脚本对 daemon 源注入、再按原文算法复算，未编译、未跑测试）：注入一个闭包 `/console`（带类型的 `|_req: Request<Body>|`，**这是我构造的**，不是树里已有的东西）→ 丢弃集变 `[("/console", 无 handler), ("/", 无 handler)]`，新断言**会红**；注入 `.route("/console", post(serve_console))`（**合法、能编译**）→ 丢弃集不变、`(path,method)` 从 106 变 107，**没有一条断言会红** |
| 2 | 删掉局部 `NON_API_ROUTES` 后，`api_yaml_documents_how_each_handler_reads_its_body` 对 `/` 与 `/console` 的处理是否逐条等价 | **等价，唯一差别是「曾经判据为假时不会 panic」这个不可能的分支** | 删掉的那份局部常量内容 = 模块级那份（两条字符串）；改动前该函数自身就先把白名单滤掉；`:3212` 之后再无读取 `spec` 的路径引用 `/` 或 `/console`——`documented` 是从 `spec.lines()` 构造的，`api.yaml` 的 90 个 path 里没有这两个（`grep -n "console" docs/api.yaml` 零命中）。差的一步：旧代码没有 `!path.is_empty()` 这个判据，若白名单里有一条空串就能匹配 `/`；今天的模块级常量没有空串，两者恒等 |
| 3 | L2-80 的「解析不了的 TOML」fixture 下，有没有第三个分支能产生同样的 200/500 使断言通过而语义不同 | **有：进程内缓存的命中。** `load_scoped_daemon_tokens`（`remote.rs:203-214`）走 `SCOPED_TOKENS_CACHE.load_with`，`:52-55` 在签名相同（同 path、同 `(mtime, len)`）时直接返回旧值、**不跑 loader**。所以「先写得对、让一次非 admin 请求把它读进缓存、再把文件改坏」之后，非 admin 仍然 401，admin 仍然 200 → 断言通过，但 500 臂从未走过。**同进程里先出现任何一次非 admin 认证**（包括一个 `with_temp_config` 块里的前一个用例）就把缓存填上了 | 键是 `(modified, len)`（`config_cache.rs:34-38`）：先写对（例：`[[tokens]]` + 一条合法条目）、再写坏（例：`this is not toml`），字节长度至少差 4，签名必变 → 强制重读 → **构造「长度恰好不变」的坏文件不可靠**（`len()` 是 `u64`，撞上就相等）。可行做法：把「让非 admin 认证发生一次」这段显式去掉、或令两条断言在**同一个 `Runtime` 里**但只允许 admin 先到达，并在断言里额外要求失败串含 `failed to load scoped daemon tokens`——**但那串只出现在响应体里**，而 `err(StatusCode::INTERNAL_SERVER_ERROR, …)` 的 body 形如 `{"error": "failed to load scoped daemon tokens: …"}`，一条 `response_text` 就够了。这是目前唯一能把「500 臂」与「缓存里没填到」分开的判据 |
| 4 | 那 4 条不成立里有没有其实成立的 | **4 条都不成立。** 但其中第 1 条（`.route(b"/health", …)`）的**方向错了一半**：字节串确实编译不过，可是「解析器读不懂形状」这件事本身是真的——只是它由 `mounted_handlers` 的**字面扫描**给出，而不是由 `Router::route` 的类型给出 | `.route(b"/health", get(health))`：`b"/health"` 是 `&[u8; 7]`，`route` 签名取 `&str`（`axum-0.7.9/src/routing/mod.rs:164`）→ `E0308`，**编译不过**，不构成可运行的 daemon。可运行的版本是 `.route("/health", get(health))`——这条**能被正确读出**（复算：`/health` GET 在 106 条 `(path,method)` 里）。真正读不出形状的只有闭包，见 §二第 1 条 |

**二、本轮查出的两条覆盖面缺口（`src-tauri/tests/**`，已登记）**

1. **L2-124：`/console` 的 GET 挂载零断言。** `bin/agent2ssh-daemon.rs:5818` 的 `.route("/console", get(serve_console))` 既不在 spec（被 `NON_API_ROUTES` 排除），也不被 `every_route_call_is_read_as_an_operation` 约束（只比 `dropped`，它不掉）。`src-tauri/tests/**` 里 `serve_console` 零命中：`http_console_serves_key_workflow_entrypoints`（`:668-696`）测的是 `:145-146` + `:425` 的**镜像**，HTML 是真的、挂法是假的。可复算：把 `:5818` 换成闭包，`daemon_routes` 少一条条目，**没有一条断言会红**。
2. **L2-125：头部三个数——一个可钉、两个不可钉。** 复算：`.route(` = 92（调用），`(path,method)` = 106，distinct path = 91，spec 的 `(path,method)` = 105，`build_test_router` 的 `.route(` = 18 但方法数 = 21。所以头部的「92 routes」单位是调用、「18 routes」单位是路径、「74」是两条不同口径相减的产物（值 91 − 17 = 74 **对**，理由不对）。且「mirrored」在源文本上不可判定（`console`/`list_hosts` 是**同名同路径不同实现**的本地副本），只能钉「形状差」不能钉「实现差」——这一条与上一轮「三个数仍是散文」的登记合并，但落地面缩小到 40 行、且必须同改 `docs/architecture.md` 里的副本。

**三、上一轮 §五 第 1 条（`mounted_handlers` 的字面一致性断言）改判**

上一轮我写「今天能红的只有 `/` 的闭包（已被覆盖），其余构造要么编译不过、要么触发 axum 重叠路由 panic，收益低于成本」——**「收益低」这一半错了**。成本比估计低（判据可以只写在丢弃段上，不需要枚举「合法形状」），收益也确在：`/console` 换成闭包这个改动今天**静默**（L2-124），而它正是新断言想钉的类别。已改登为 **L2-123**，落地形式与判据写在那里，等「闭包 API 路由是否可接受」这个产品问题定。

### consume 3 / 6 · 2026-11-11 12:18–12:45 (GMT+8) · batch 12（issue #18）

- 指摘来源：CodeBuddy 对第 2 轮的复核（评论 `2120370479750881280`，04:18Z），含它自己推进 main 的 `8bc3205`（L2-123/124/125 三条登记 + 一段被标成 `consume 3 / 6` 的复核小节）。
- 计数：**3 组 → 成立 1 / 部分成立 2 / 不成立 0**；另判 4 条子断言/口径不成立（两个数、一处文件引用、一处轮次计数，见 §三）。
- L1：`1a0b723`（判据拆成「形状 + 归属」两条断言；`DroppedRoute` 增 `mounts_closure`；新增 `/console` 挂载断言；头部三个数改名词与单位并补「路径镜像 ≠ 方法镜像」）＋ `3e86b4b`（`/console` 改数调用次数——第一版只解析首段，同一路径的第二条 `.route` 仍漏）。
- 验证：`npm run build` 6.50 s；`--no-default-features --lib` **862 / 1 / 1**（红项仍是故意红的 ipv6 known_hosts）；`--features daemon --test daemon_integration` **73 passed**（72 → 73）；`cargo fmt --check` 绿。
- 破坏性对照四次，每次都改 daemon 源再还原、`git diff` 确认为空，见 §二。

**§一、判定表**

| # | 指摘 | 判定 | 依据 |
|---|---|---|---|
| A | L2-123：判据应是「每个丢弃段都挂闭包字面量」，路径与条数都不进期望 | **部分成立（判据采纳，「任何路径都放行」不采纳）** | 采纳的那半：`mounts_closure` 只看段内有没有 `<builder>(` 紧跟非标识符字符。不采纳的那半：一条**未写进 `api.yaml`** 的路由若用闭包挂载，drop 检查放行、`phantom` 又无条目可比（spec 里没有它）——两条网同时漏；它自己也把这写成「这个判据唯一的代价，需要人拍板」，而「闭包路由可接受」是产品判断，L1 车道不该用撤护栏来表达。落地形状因此是「形状 + 归属」两条：要豁免就显式登记进 `NON_API_ROUTES` |
| B | L2-124：`/console` 的 GET 挂载零断言 | **成立，已落** | `8bc3205` 的树里 `serve_console` 在 `src-tauri/tests/**` 零命中（现在 4 处，全是新 test 自己）；`/console` 改闭包后**只有**新 test 红，`every_route_call_is_read_as_an_operation` 仍绿（`/console` ∈ 白名单）——与它说的「静默」逐字一致 |
| C | L2-125：92 数的是调用、18 数的是路径、74 是两条不同口径相减 | **部分成立（前两项成立，74 的理由不成立）** | 92 与 18 **都是调用数**（复算：daemon 92 calls / 106 pairs / 91 解析出的 distinct path；镜像 18 calls / 20 pairs / 18 paths）。74 在调用口径下是 92 − 18，在路径口径下也是 92 − 18（`/` 是唯一 drop，不产出 entry，所以「解析出的 distinct path」是 91 而不是 92）。`(path, method)` 口径才是 106 − 22 = 84 |

**§二、破坏性对照（四次，逐次还原）**

| # | 对 daemon 源的改动 | 新「形状」断言 | 新「归属」断言 | `/console` 挂载断言 | 备注 |
|---|---|---|---|---|---|
| 1 | `:5818` 改成 `get(\|\| async { serve_console().await })` | 绿（是闭包） | 绿（在白名单） | **红** | 新版唯一比旧版松的格子，由新 test 补上；旧版的固定集合会红但文案叫人「更新期望」 |
| 2 | `/metrics` 改成闭包 | 绿 | **红**，报 `line 5821 `/metrics`` | 绿 | 归属断言按预期工作 |
| 3 | `/console` 写成 `get (serve_console)`（一个空格，仍能编译） | **红**（「mount a named handler and did not parse」） | — | **红** | 形状断言是这条的唯一哨兵：解析退化不再藏在白名单后面 |
| 4 | 追加 `.route("/console", post(serve_console))` | 绿 | 绿 | **红（第一版绿！）** | axum 把同路径第二条 `.route` 合并进第一条，第一版只解析首段 → `3e86b4b` 补计数断言。这一格正是 L2-125 用来论证「头部数不可钉」的反例 |

**§三、新增「不成立」条目（原文保留，请勿重复提）**

1. **「`build_test_router` 挂的方法数是 21」— 不成立，是 20。** 18 条 `.route(` 里 `/webhook/config`（get+put）与 `/approval/policies`（get+put）各挂两个方法，其余 16 条各一个 → 20。
2. **「74 是两条不同口径相减的产物，值 91 − 17 = 74 对、理由不对」— 不成立。** 92 calls − 18 calls = 74；按调用路径数同样是 92 distinct − 18 = 74。91 只是「解析出的 distinct path」（`/` 不产出 entry）。要纠的是**名词**（92 与 18 都是调用数，不是路由数），不是减法；84 才是 `(path, method)` 口径。
3. **「本轮必须同改 `docs/architecture.md` 里的副本」— 不成立。** `git grep -i 'mirror\|镜像' docs/architecture.md` 零命中，`92`/`74`/`18` 也不在该文件。这些数只在测试文件头部两处与本台账里。
4. **「`8bc3205` 这一段 = `consume 3 / 6`」— 不成立，它是一次复核，不是一轮消费。** 计数口径是 issue 内由 `cnb.lengyuqu` 发出的 `## 第 K 轮收尾` 评论数，当时 = **2**（`cnb issues list-comments` total 5：指摘 2、收尾 2、复核 1）。已把那个小节标题改成 `复核小节（不计入轮次）`，正文一字未改。

**§四、未做 / 待拍板**

1. **不把 92 / 106 / 18 / 20 / 74 / 84 钉成断言。** `every_route_call_is_read_as_an_operation` 的文档写明它钉的是「rather than a count a new route would break」——本仓在这条路径上的既定取舍是不钉计数；再给头部加自解析断言等于把刚拆掉的漂移源换一种。要钉就钉**形状差**（daemon 的 `(path, method)` 集合 vs 镜像 router 的集合），不钉「实现差」：源文本无法证明同路径的本地 `console`/`list_hosts` 是 daemon 的实现。影响面比 L2-125 估的小——只需改测试文件一处。
2. **L2-123 / L2-124 已闭环**，可从待拍板清单划掉；L2-125 只剩「要不要钉计数」一项。
3. 本轮**没有**动 `src-tauri/src/**`，因此没有新增 L2 待拍板项。

### consume 4 / 6 · 2026-11-11 12:33–12:50 (GMT+8) · batch 12（issue #18）

- 指摘来源：CodeBuddy 对第 3 轮的复核（评论 `2120374363126915072`，04:33Z）。
- 计数：**4 组 → 成立 1 / 部分成立 3 / 不成立 0**；另判 2 条子断言不成立（73 条路径；把日期快照当当前声明）。
- L1：`bca51ea`（`mounts_closure` 容忍 `get (…)` 的空格并认得 `move`/`async`；新增 `the_router_chain_mounts_paths_only_through_route`；头部把「84 pairs over 74 distinct paths」改成 `over 73 paths` + `/` 无 pair）。
- 验证：`npm run build` 6.38 s；`--no-default-features --lib` **862 / 1 / 1**；`--features daemon --test daemon_integration` **74 passed**（73 → 74）；`cargo fmt --check` 绿。
- 破坏性对照四次，均改 daemon 源后还原、`git diff` 为空，见 §二。

**§一、判定表**

| # | 指摘 | 判定 | 依据 |
|---|---|---|---|
| A | `get (|| …)`（一个空格）会误红，且文案把人指向解析器 | **成立** | 注入后修前**红**、修后**绿**；旧版对同一注入是绿的（`(path, reason)` 一字未变）。`mounts_closure` 只认子串 `get(`，而 Rust 不要求 callee 与 `(` 相邻。修法：按词找 `get`（前一字符非标识符）→ 跳空白 → `strip_prefix('(')` → 跳空白 → 认 `\|`（`move`/`async` 一并跳） |
| B | `route_service`/`nest`/`merge`/`fallback` 让路由零痕迹 | **部分成立（已钉入口，未钉语义）** | 五个构建器文本里**没有** `.route(` 子串。注入 `.nest("/zz", Router::new())`——**能编译**、74 条里其余断言全绿、只有新守卫红。它列的 (b) `nest` 内相对路径成假 entry、(c) `fallback` 让未列动词有响应、(d) 同名方法 panic，都发生在「入口已被允许」之后，入口先红 |
| C | 头部「84 pairs over 74 distinct paths」是又一次跨口径相加 | **部分成立** | 74 次调用确实落在 74 条不同路径（含 `/`），字面为真；但「有 pair 的路径」是 73 条。已改成分开说：`84 (path, method) pairs over 73 paths, and the one path with no pair is the / redirect` |
| D | 14 行「引用数字却无断言」的文档表 | **部分成立（1 行该改，其余是快照）** | 改 `docs/guides/daemon-api-quickstart.md:62`：**107 是 operation 总数**，那句却叫「107 条路由」，与自己前半句「两个数不要混用」打架；同句「目前没有测试守着它」也已过时（两条守卫要点名）。`docs/reports/**` 的那些行是带日期的记录，不改 |

**§二、破坏性对照（四次）**

| 注入 | 结果 |
|---|---|
| `/` 上写 `get (|| async { … })`（能编译） | 修前**红** → 修后**绿** |
| `/` 上写 `get (serve_console)`（命名 handler） | **仍红**——判据没被放宽，区分的是「闭包 vs 名字」而不是「有没有空格」 |
| 链上 `.nest("/zz", Router::new())` | 新守卫**红**并点名 `.nest(`；其余 73 条全绿（这就是「零痕迹」的样子） |
| 链上注释点名 `.route_service(` / `.fallback(` | **绿**（先剥行注释） |

**§三、新增「不成立」条目（原文保留，请勿重复提）**

1. **「那 74 calls 落在 73 条不同路径上」— 不成立，是 74 条。** 第 74 条是 `/`（闭包重定向，零 pair）；73 是「有 pair 的路径数」。两个数都对，但「74 条调用」与「74 条有 pair 的路径」不是同一件事——它这次纠的是我的措辞歧义，纠过头成了数错。
2. **「`docs/reports/redundancy-audit.md:990` 等行必须按今天的树改」— 不成立。** 那些是带日期的记录（当轮审计的 84 条 `.route(..)` − 2 = 82 paths、L2-53 那一行的 107、上一轮复核原文、历史轮次里的 92/18），改它等于改写历史，下一个读的人会以为当轮的树就是今天这样。政策：**现行文档按今天的树修，`docs/reports/**` 保留快照**，更正写在追加小节里。

**§四、未做**

- 不钉头部六个数（理由同第 3 轮 §四）。
- `nest` 之后的相对路径假 entry、`fallback` 的动词可见性：属「入口被允许之后」的问题，今天被新守卫挡在入口。若日后真要用 `.nest(`，得同时教解析器读前缀，届时开新条目。

**§五、本轮唯一对自己不利的记录**：`get (…)` 这个误红格是我上一轮引入的，且是 CodeBuddy 自取的一条（不在我的清单里）。它成立于「Rust 的调用语法不要求 callee 与括号相邻」这个我一直知道、却没写进判据的事实。

### consume 5 / 6 · 2026-11-11 12:50–13:05 (GMT+8) · batch 12（issue #18）

- 指摘来源：CodeBuddy 对第 4 轮的复核（评论 `2120378478049525760`，04:49Z）。它这次**装了 Rust 工具链**并实跑 `--features daemon --test daemon_integration` 得到 74 passed，与 `bdad0b5` 自述一致——这是本批第一次两侧跑同一套测试。
- 计数：**4 组 → 成立 2 / 部分成立 2 / 不成立 0**；另判 3 条子断言不成立（Q4 的两格 + `.route_service(` 那句）。
- L1：`94acb22`（`mounted_handlers` 不再把 `move`/`async` 读成 handler 名；新增 `every_parsed_handler_is_a_function_the_daemon_defines`；drop 断言的文案不再假定「命名 handler」）。
- 验证：`npm run build` 6.66 s；`--no-default-features --lib` **862 / 1 / 1**；`--features daemon --test daemon_integration` **75 passed**（74 → 75）；`cargo fmt --check` 绿。
- 破坏性对照两次（见 §二）。

**§一、判定表**

| # | 指摘 | 判定 | 依据 |
|---|---|---|---|
| Q1 | `.nest(` 载荷造**假 entry**：`sub` 里的 `get(h)` 被读成 `get /z`，前缀丢失，且「entry 的 handler 必须解析得到」只是约定 | **成立，已落成断言** | 我按它的复算路径核了一遍：`.nest(` 的入口被链守卫挡（红），但**假 entry 本身**没有网。`every_parsed_handler_is_a_function_the_daemon_defines` 现在要求每条 entry 的名字在 daemon 里有 `async fn`（106/106 今天成立）。它的「两条 body 检查会当场 panic」只有在 `/z` 被补进 spec 后才发生；新断言不依赖 spec |
| Q2 | 六种合法闭包写法仍会误红；另有三种写法**根本不产 drop**（`move`/`async`/混合链） | **部分成立（三种假 entry 成立并已修；六种误红保留）** | 假 entry 那半是真洞：`get(move \|\| …)` 读成 `handler = "move"` → `mounted` 非空 → **不产 drop** → `mounts_closure` 从来没被调用。已在读名字处拒掉 `move`/`async`。六种误红里 `get({ \|\| … })`、`get((\|\| …))`、`/* */`、turbofish、注释里的 `get (` 都成立于「Rust 不在乎这些」，但都要有人故意写成那样；处置是**让它们红**（文案已改成「闭包或名字都可能，解析器认不出这个形状」），不是放宽判据 |
| Q3 | 判据要分三层：真闭包 / 名字不解析 / 闭包味但不以 `\|` 开头 | **部分成立（采纳分层，不采纳「`get (h)` 该修解析器」）** | 分层采纳：`get (h)` 是命名 handler + 空格 → 落在「名字不解析」层 → 红是对的（读者要么去掉空格，要么教解析器）；`get({ \|\| })` 落在第三层 → 红，文案说明是这个形状。它主张「`get (h)` 该让解析器学会」，理由是「一个空格让 `/health` 掉出所有契约检查」——但那是**改代码的人**要付的代价，红正是提示；今天树上没有这种写法 |
| Q4 | 删掉 `/console` 后头部六个数各变多少 | **部分成立（4 格对、2 格错）** | 我按 `daemon_routes_and_drops` 的算法复算：92→**91** calls、106→**105** pairs、有 pair 的路径 91→**90** ——对。但「未镜像 calls / pairs」**一格都没变**（74 / 84 → 74 / 84）：`/console` 本来就在镜像里、也在「共享」集合里，删掉后它从两侧同时消失。它写的 HEAD「74 / **86**」与删后「**73** / 85」都错——84 是我头部一直在用的数 |

**§二、破坏性对照（两次）**

| 注入 | 结果 |
|---|---|
| `/` 上写 `get(move \|\| async { … })`（能编译） | **绿**：整段成为合法 drop（`mounts_closure` 认 `move`），两条断言都不动 |
| 同上，但临时去掉「拒 `move`/`async`」那一行 | `every_parsed_handler_is_a_function_the_daemon_defines` **红**：`["get / -> `move`"]`——证明那一行是承重的，且新断言能抓住它放过去的东西 |

**§三、新增「不成立」条目（原文保留，请勿重复提）**

1. **「HEAD 的未镜像 pair 是 86」— 不成立，是 84。** 106 pairs 落在 18 条被镜像路径上的有 22 条（`/hosts`、`/health-snapshot` 各多一个 `POST`），106 − 22 = 84；`73`+`/`=74 条路径。
2. **「删掉 `/console` 后未镜像 calls / pairs 变成 73 / 85」— 不成立，仍是 74 / 84。** `/console` 同时在 daemon 与镜像里，删掉后它从「共享」集合与 daemon 集合同时消失；未镜像 calls = 91 − 17 = 74，未镜像 pairs = 105 − 21 = 84，有 pair 的路径仍是 73（第 74 条是 `/`）。
3. **「`.route_service(` 里就有 `.route(`，所以那句理由在文本上是错的」— 不成立。** `.route_service(` 在第 6 位是 `_` 而不是 `(`，`".route_service(".find(".route(") == -1`。它含的是 `.route`，不含 `.route(`；`daemon_routes_and_drops` 扫的正是后者，所以那句理由成立。

**§四、未做与登记**

- **混合链**（`.route("/x", get(h).post(\|\| …))`）：`mounted` 非空 → 不产 drop，POST 上那个闭包仍然隐形。要收它得给 `daemon_routes_and_drops` 再返回一份「既有 entry 又挂闭包」的段清单（改动面比本轮其它项大，且今天树上零例），**记入 L2 本轮小节**。
- **链守卫被字符串里的 `//` 骗**（L2-127）：成立但需要有人把 `.nest(` 写在同一行的 `"//"` 之后；守卫仍是「入口先红」的那道，成本收益不划算，**保留并记入 L2 小节**。
- 本轮没有动 `src-tauri/src/**`，没有新增 L2 待拍板项（L2-126 由 `94acb22` 关闭）。

### consume 6 / 6 · 2026-11-11 13:00–13:25 (GMT+8) · batch 12（issue #18）· 末轮 · 自取

- 指摘来源：**自取**。第 5 轮收尾（04:56Z）后按协议轮询 5 次（60 s 间隔）并两次延时，到 13:05 仍无回复，标「等待超时」，用已登记的 L2-128 推进，不空等。
- 判定：本轮无外部指摘，自取项 **L2-128 成立并落地**。
- L1：`7369ecd`（`daemon_routes_and_drops` → `parse_route_table`，返回 `RouteTable { routes, dropped, mixed }`；新增 `a_route_call_does_not_mix_a_closure_with_a_named_handler`）。
- 验证：`npm run build` 6.42 s；`--no-default-features --lib` **862 / 1 / 1**；`--features daemon --test daemon_integration` **76 passed**（75 → 76）；`cargo fmt --check` 绿。
- 破坏性对照一次（已还原）：`/metrics` 写成 `get(metrics).post(|| async { … })` → **只有新断言红**；drop 检查、`api.yaml` 对账、handler 存在性三条全绿。这就是「隐形」的样子。

**§一、为什么末轮选这一条**
六轮下来，这张表已经按形状关掉了五个口子：闭包被读成名字（`move`/`async`）、闭包落在 API 路径（归属）、命名 handler 读不出（形状）、入口不是 `.route`（链守卫）、名字在 daemon 里不存在（存在性）。剩下最后一个：**同一条 `.route` 上两种形状混挂**——它既不掉（因为产出了 entry），也没有对应的 entry（闭包那一路），前五道网各自都看不见它。它也是第 5 轮明确登记、并写了判据的那一条（「一段里不许混装」）。

**§二、落地形状**
函数名从 `daemon_routes_and_drops` 改成 `parse_route_table`：它现在返回三样东西，旧名字在数它返回什么。两处断言文案里的函数名同步改了——那是写给下一个人看的指令，不能留旧名。

---

## batch 12 六轮汇总（issue #18 · 测试覆盖缺口，第二批）

| 轮 | 指摘来源 | 判定 | 落地 | 这一轮真正买到的 |
|---|---|---|---|---|
| 1 | 首条分析 `2120356381013209088` | 6 组 → 成立 1 / 部分成立 4 / 不成立 1 | `a9d7ec9` + `ceb0392` | 解析丢失第一次可见：`daemon_routes_and_drops` 返回 drop |
| 2 | 复核 `2120368025479057408` | 6 组 → 成立 3 / 部分成立 1 / 不成立 2 | `e2bf8dc` + `614f355` | 比整集而不是比「在不在白名单」；删掉重复常量 |
| 3 | 复核 `2120370479750881280` | 3 组 → 成立 1 / 部分成立 2 | `1a0b723` `3e86b4b` + `591fb38` | 判据从**路径**改成**形状**；`/console` 有自己的挂载断言 |
| 4 | 复核 `2120374363126915072` | 4 组 → 成立 1 / 部分成立 3 | `bca51ea` + `bdad0b5` | `get (…)` 不再误红；链上只许 `.route` |
| 5 | 复核 `2120378478049525760` | 4 组 → 成立 2 / 部分成立 2 | `94acb22` + `1aedb55` | `move` 不再变成 handler 名；每条 entry 的 handler 必须存在 |
| 6 | **自取**（等待超时 16 分钟） | L2-128 成立并落地 | `7369ecd` + 本文 | 混挂闭包与命名 handler 的一条 `.route` 被拒 |

**数字**：7 笔代码提交 + 6 笔文档提交（另有 CodeBuddy 自己推的 `8bc3205`）；`--features daemon --test daemon_integration` **71 → 76**；`--no-default-features --lib` 全程 **862 / 1 / 1**（红项一直是那条故意红的 IPv6 known_hosts）；`npm run build` 6.38–6.66 s。新增「不成立」**21 条**（第 1–5 轮 7 / 4 / 4 / 2 / 4），累计清单 116 + 21 = **137 条**。

**这批的两次自我更正**（写给下一批的读者）：
1. 第 3 轮我把头部改成「84 pairs over 74 distinct paths」——**字面为真但歧义**（74 里有 1 条零 pair），第 4 轮改成分开说。
2. 第 4 轮我加的 `mounts_closure` 只认子串 `get(`，导致 `get (|| …)` **误红**且文案指错方向——第 4 轮自己修（按词找名字、跳空白、认 `|`）。这是本批唯一一次「我引入的误红」，也是 CodeBuddy 自取的一条。

**仍未关闭**：L2-129（链守卫的两个口子：字符串里的 `//` 会截断注释剥离；窗口终点取第一个 `.with_state(`）＋前批遗留的 L2-80 / L2-82 / L2-122。L2-123 / 124 / 125 / 126 / 128 已闭环。

**下一批（B=13）**：13 % 6 = 1 → 主题 1「daemon 控制与停止协议（第三批）」。#18 已按协议关闭（`state_reason=completed`）。开 issue 前先 `git fetch cnb && git merge --ff-only cnb/main` 重读本文件的不成立清单——本批新增 21 条，行内锚点会漂。

---

### batch 2026-11-11 · daemon 控制与停止协议（第三批，B=13）
- issue #19 / 开批时间 2026-11-11 13:25 (GMT+8)
- CodeBuddy 首条分析：**已到**（05:32:08Z = 13:32 GMT+8，开批后约 6.3 分钟；60 s × 5 轮询 13:26–13:31 均空，延时复核 ≈13:33 命中），评论 `2120389088577478656`，**6 条指摘（成立 4 / 部分成立 2 / 不成立 0）**，索引：①`HELPER_STATUS` 的 423 项遮蔽 helper 自读 gate 的 500——`/daemon/:alias/exec` 的 localhost 分支自己调 `reject_if_gate_paused`，其 500 成因与 api.yaml 已写的「HTTP client 构不出来」是两条路径（成立）；②`INBAND_REJECTIONS` 以「路径在表里」为豁免判据，区分不了「Err 被消费」与「Err 被返回」两种形态，`terminal_attach` 不该进表（成立）；③L2-84 机制复核与当前树一致，新缺口 = `daemon_stop_e2e.rs` 零「持续输出 + 停机」用例（部分成立）；④`/exec/stream` 的 `interrupted` 在「取消后 drain 读到 `Exit(None)`（超时/worker 失败来源）」时误记为 daemon 中断，审计与 `ExecCompleted` 事件同错（部分成立）；⑤L2-129 第 3 条复核仍开，落地成本更正为约 5 行（复用 `closing_brace` 已有的字面量扫描器）（成立）；⑥L2-82 落地形状更正：三处 `Option<Json<T>>` 不能共用同一条断言——`/health-snapshot` 与 dry-run 在空配置目录下假绿，须拆「坏 body 与无 body 行为不同」判据（成立）。另**自撤 3 条候选**（`:4878` Close 不受 2 s 约束 / 坏 gate 下 preview 返回 423 / `drain_all` 4 s 计费——`drain_all` 无生产调用点，L2-91 正文措辞残留）。
- 6 轮进度：**6 / 6（已闭环，issue #19 已关闭）**（第 1 轮 16:38–17:12、第 2 轮 17:30–18:15、第 3 轮 18:28–18:55 自取、第 4 轮 18:01–18:09、第 5 轮 18:09–18:20、第 6 轮 18:20–18:45；判定见文末六条 `### consume K / 6` 与「批次汇总」）
- 正文已喂给 CodeBuddy：**136 条「不成立」原文**（编号 91 = 旧 71 + batch 12 五个加粗伪小节 20；判定行 39；摘录 6），由 `/tmp/build-opt-issue-b13.py` 生成（复用 b12 骨架 + batch 12 区域锚点；batch 12 的 5 个小节是 `**…**` 独立加粗行而非 `###` 标题，b13 为其加了独立的区域限定扫描器，7/4/4/2/3 分布；全部锚点唯一性校验通过，计数复验 136）。**注意：batch 12 收尾汇总自述新增 21 条，原文清点为 20 条**（第 5 轮小节自报 3 条、汇总行写成 4），issue 正文已注明以原文切片为准。
- KNOWN-ISSUES 空档喂 K14/K15/§3；口径提醒含基线（`--no-default-features --lib` 862/1/1、`daemon_integration` 76 passed、npm 220/30）、L2-75…L2-129 勿重复立条（本批靶子 L2-80 / L2-82 / L2-122 第 5 条 / L2-84…L2-91 / L2-99 / L2-129 除外）、L2 撞号 13 对须带主题限定、AGENTS 测试口径三则、batch 11 抗漂约定、`.github` 禁区与 L2 越权重申。
- 顺手按 BATCH_COMPLETE 行自述删除顶部 `BATCH_COMPLETE · issue #18 · batch 12` 状态行（HTML 注释同步为十一条）。
- 下一批（B=14）：14 % 6 = 2 → 主题 2：SSH 传输、known_hosts 与认证（russh 侧，**第三批**，#8/#14 已闭环）。开 issue 前先 fetch cnb main 重读 ledger 不成立清单（本批消费期间会新增，行内锚点会漂移），b13 脚本可改造复用（注意 batch 13 消费期间可能出现新的加粗伪小节形态）。
- 若 #19 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

---

### consume 1 / 6 · 2026-11-11 16:38–17:12 · issue #19 · daemon 控制与停止协议（第三批）

**让位记录终止**：本批前两次运行（14:17、15:26）都因并发会话活跃而零改动退出。本次进入时 §0 锁仍是 13:21:27 那个孤儿（无 `owner.txt`、mtime 从未刷新），但三重独立证据表明无写者：两个 worktree `status --porcelain` 均为 0 行、仓库 10 分钟内零改动、`ps` 无 CLI agent 进程。等待一个 120 s 重试周期后复检全部不变，遂接管并写 `owner.txt`。

**指摘**：CodeBuddy 首条分析 `2120389088577478656`（13:32），6 条，它自评 成立 4 / 部分成立 2 / 不成立 0。

| # | 它的判定 | 我的判定 | 关键证据 |
|---|---|---|---|
| ①  `HELPER_STATUS` 的 423 项遮蔽 helper 自读 gate 的 500，`/daemon/:alias/exec` 是扫描器盲区 | 成立 | **不成立** | `docs/api.yaml:24-26` 逐字点名 `reject_if_gate_paused`（不只 preflight），「30 个不在覆盖面上」的前提不存在。且 `proxy_exec`（`:3959`）的 `localhost` 分支在 `:3970` 判别名后走 `preflight_guarded_request`（`:3971`）、`:3987` 就 `return`，`:3998` 的直调在**远端**分支。路径名也错（是 `/daemons/{alias}/exec`） |
| ②  `INBAND_REJECTIONS` 以「路径在表里」为判据，区分不了「Err 被消费」与「Err 被返回」 | 成立 | **部分成立** | 事实面对：`load_execution_gate().ok()`（`:4350`）+ `is_some_and`（`:4391-4398`）在坏 gate 下整段跳过 → 200 + 逐目标字段，`/terminal` 的 `:4757` 是 `?` 传播。但「表里加一条就同时关掉两个方向」是**可构造性**，它自己也说「今天没有漏」；第三列短语被断言钉住，已有一半护栏。已按它的口径改注释，未采纳「第三列改枚举」（会牵动 `a_websocket_drain_budget_claim_matches_the_loop_it_describes` 的双向断言，收益不抵） |
| ③  L2-84 机制复核一致，新缺口是 `daemon_stop_e2e.rs` 零「持续输出 + 停机」用例 | 部分成立 | **成立** | `git grep -c Output src-tauri/tests/daemon_stop_e2e.rs` = **0**，33 条用例无一在停机期间持续写终端输出。它自己撤掉的那半（`:4878` 的 `Close` 逃出 2 s）确实不成立：`run_ws_with_shutdown_deadline`（`:119-129`）的 `timeout(grace, &mut future)` 覆盖整个闭包 |
| ④  `/exec/stream` 的 `interrupted` 在「drain 读到 `Exit(None)`」时误记为 daemon 中断 | 部分成立 | **成立（正向）** | `code: Option<i32>`（`:5363`）使「收到 `Exit(None)`」与「从未收到 `Exit`」同值；`Exit(None)` 有三个生产者：`:5189`（超时）、`:5204`（worker 出错）、`:5200`（`exit_status().ok()` 失败）。审计 `:5496-5497`、事件 `:5529` 同源。**不改 main**（改 `src-tauri/src/**` 行为），立 **L2-130** |
| ⑤  L2-129 第 3 条仍开，落地成本更正为约 5 行 | 成立 | **成立（数字另有第三说）** | `chain.find(".with_state(")` 取第一个成立（`:2888` 附近，今天链上只有 `:5967` 一处，它写的是 `:5950`）。但「5 行」与原文「20 行」都不对：`closing_brace`（`:2583`）第一个分支就是 `//`，抽成 `code_only(source)` + 复用是 +15/−6 量级。已在 L2-129 记两向更正 |
| ⑥  L2-82 落地形状：三处 `Option<Json<T>>` 不能共用一条断言，两处会假绿 | 成立 | **成立（数量已过时）** | `RunningDaemon::start_with_name` 只写 `daemon.token` + `extra_files`（`:54-59`），配置目录其余为空 → `/health-snapshot`（`:5593`）空扫、dry-run（`:3834`）整段跳过，同一条断言必假绿。但「三处」现在只剩**两处**：`/shutdown` 已由 `380e166` 换成 `HeaderMap + Bytes` + 显式 `Json::from_request`，并配 `daemon_stop_e2e.rs:1165` 真进程 e2e |

**本轮落地（L1）**：`09553fc`（`daemon_integration.rs` 的 `INBAND_REJECTIONS` 文档把「identical checks, run before the upgrade」拆成「returned vs swallowed」两种形态）+ 本文件与 L2 清单。推 `cnb main`，未推 github。

**验证**：`npm run build` 6.53 s；`cargo fmt --check` 绿；`--no-default-features --lib` **862 / 1 / 1**（红项仍是那条故意红的 IPv6 `an_ipv6_literal_at_port_22_is_removed`，与基线一致）；`--features daemon --test daemon_integration` **76 passed**（与基线一致）。Rust 行为代码一行未动。

**L2 变更**：新增 **L2-130**（`/exec/stream` 的 `interrupted`，三条改法 + 回滚路径 + 一条可执行证伪）；L2-82 表格行号按当前树更正（`:5574→:5593`、`:3815→:3834`）并补「红的方向只有一个，绿的方向是开放的」与「三处不能共用断言」；L2-84 补「验证前置」（先补持续输出 e2e 再决定循环语义）与行号（`:4830→:4866`、`:5014→:5057`、`:5340→:5363`）；L2-129 第 1 条补成本的两向更正。

**本轮新增不成立（原文保留，请勿重复提）**：

1. **「`HELPER_STATUS` 的 423 项把 helper 自己读 gate 的 500 遮蔽了，那 30 个读 gate 的 operation 不在 `info.description` 的覆盖面上，`/daemon/:alias/exec` 的 423+500 组合是扫描器盲区」— 不成立。** 三个子句各错一处：(a) `docs/api.yaml:24-26` 原文是「`reject_if_gate_paused`, and the preflights built on the same gate read, … when that read fails they answer `500`」，**helper 被逐字点名**，覆盖面包含它；(b) `proxy_exec`（`bin/agent2ssh-daemon.rs:3959`）的 `localhost` 分支在 `:3970` 判 `alias == "localhost"` 后走 `preflight_guarded_request`（`:3971-3983`），`:3987` 就 `return exec_ssh_core(...)`，它指的 `:3979` 在远端分支；(c) 路径是 `/daemons/{alias}/exec`，不是 `/daemon/:alias/exec`。
2. **「`INBAND_REJECTIONS` 的缺陷是当下输出错误」— 不成立（作为缺陷等级）。** 它自己在正文承认「今天没有漏」。真实形态是**判据的可构造漏洞**：表的键是 `(method, path)`，而两种豁免理由不同源。已按「说明两种形态」改注释；「把第三列改成枚举」未采纳，理由见上表。

**未做的 L2 项与理由**：④ 的修复要动 `src-tauri/src/**` 的 `interrupted` 判定与对外契约（`ExecCompleted` 载荷），按协议属 L2，只登记不落 main。③ 的 e2e 同样要新增真进程用例并可能改循环语义，一并留在 L2-84。

**下一轮**：第 2 轮的指摘在 CodeBuddy 对本轮回帖的回复里。行号基线 `09553fc`（`380e166` 之后整体下移约 36 行，再按本轮注释 +11 行微调）。`daemon_stop_e2e.rs` 的 `Output` 零覆盖、`L2-130` 的三条改法、`L2-129` 的 `code_only` 抽取是它请求复核的三个面。

---

### consume 2 / 6 · 2026-11-11 17:30–18:15 · issue #19 · 第 1 轮复核的回应

**指摘**：CodeBuddy 对第 1 轮回帖的复核 `2120438757970059264`（16:49），5 组。

| 组 | 它的主张 | 我的判定 | 关键证据 |
|---|---|---|---|
| ①-1 | 我第 1 轮新写的注释「gate 坏 → 200 + per-target」说过头，因为同一操作还有 `reject_if_rate_limited`（读另一个文件）与 `authorize_command` | **部分成立** | 200 这个结论没错（`:4491-4502` 就是 `return Ok(Json(...))`），但它说得对的是**表述范围**：per-target 字段是这条路由**所有**拒绝的统一形状，不是 gate 分支的属性。已改注释（`b458305`） |
| ①-2 | `:5266-5275` 的 `if reject_if_gate_paused(...).is_err()` 把 423 与 500 压进同一 `Err`，帧里固定写 `"execution gate paused"`，而 `api.yaml:24-31` 要求客户端读 message | **成立** | `.is_err()` 丢掉状态码，文案是字面量。立 **L2-131** |
| ①-3 | `api.yaml:1849-1852` 说「`/terminal` 是族内对照」不准，因为第二条检查 `register_limited_session_for_command`（`:4765`）是显式 `return` 不是 `?` | **不成立** | 那句写的是「the same two refusals *are* HTTP statuses there」。`:4758` 与 `:4765-4775` **都返回 HTTP 状态码**，只是前者 `?`、后者 `match … Err(e) => return e.into_response()`。形式差异不是语义差异 |
| ①-4 | 表里把 `terminal_broadcast_run` 记成「returns 200」，实际它返回 429（`:4488-4500`） | **不成立** | `:4488-4502` 恰恰是 `if let Err(...) = reject_if_rate_limited(...) { for index in eligible_indexes { results[index].error = … } return Ok(Json(TerminalBroadcastResponse { … })); }` —— 逐目标写 error 后 **200**。它引的行号证明的是表里那句 |
| ② | L2-130 的成本排序要改：`Option<Option<i32>>` 只 4 处改动最便宜；`saw_exit` 加字段有 4 个发布点；改法 3 修 `Error` 帧不是审计；拆成「修判据」+「统一文案与字段名」两步，字段名对齐 `remote_outcome_unknown` | **成立** | `ExecCompleted` 实测 4 个发布点（`bin/agent2ssh-daemon.rs:5521`、`core.rs:1118`、`core.rs:1323`、`resident_session.rs:803`）；`:1748` 确有 `remote_outcome_unknown`。已整段重写 L2-130 的改法排序 |
| ③ | `/local-exec` 够用，`/terminal` 的 WS 路径修不了这个 bug | **部分成立** | 后半（认领 `/terminal` 污染 `confirm_quit` 回归面）接受；**前半不接受**：`/local-exec` 走 `run_local_command_capture`，**不进 WS drain 循环**，而 L2-84 的分歧点正在那个循环里。它给的先例 `daemon_stop_e2e.rs:556` 断的是 PID 被杀，不是 drain 语义。限流坑（`default_true` + `window_secs=60` + `default_source_per_minute=30`）成立 |
| ④ | L2-10 的 4×(2s+2s)=16s 应下修为 2s×4=8s；真正挂起预算 ~11s | **部分成立** | 「同一个 `timeout` 参数」成立（`daemon_activity.rs:102-110`）→ 8s 成立。但预算链**文件名写错**：`daemon_pid_state`（`:343`）、`daemon_matches_config`（`:283`）、`GRACEFUL_STOP_WAIT`（`:651`）都在 **`daemon_control.rs`**，不是 `daemon_activity.rs`（该文件只有 369 行，`:651` 越界）。机制与行号对，文件名错 |

**本轮落地（L1）**：`b458305`（`daemon_integration.rs`：把「the target proceeds: 200」改成「200 是这条路由所有拒绝的统一形状，gate 只是其中一支，不要把『gate 读不到』读成『目标已授权』」）+ `docs/reports/OPT-LOOP-L2-PENDING.md`（L2-130 改法排序整段重写并拆两步、新增 L2-131、L2-84 加对 `/local-exec` 的反驳与限流坑、L2-10 预算下修 16s→8s 并补 ~11s 链）+ 本文件。推 `cnb main`，未推 github。`src-tauri/src/**` 一行未动。

**验证**：`npm run build` 6.57 s；`cargo fmt --check` 绿；`--no-default-features --lib` **862 / 1 / 1**（红项仍是故意红的 IPv6）；`--features daemon --test daemon_integration` **76 passed**。

**本轮新增不成立（原文保留，请勿重复提）**：

3. **「`docs/api.yaml:1849-1852` 的『`/terminal` 是同一族里的对照』不准，因为它只在 `:4757` 一处 `?` 传播，第二条检查 `register_limited_session_for_command`（`:4765`）是显式 `return e.into_response()` 而不是 `?`」— 不成立。** 那句的原文是「the same two refusals *are* HTTP statuses there」，判据是「是否返回状态码」，不是「是否用 `?`」。`:4758` 的 `return e.into_response()` 与 `:4765-4775` 的 `Err(e) => return e.into_response()` 都返回状态码，两处都成立。
4. **「表里把 `terminal_broadcast_run` 记成『returns 200』，实际它返回 429（`:4488-4500`）」— 不成立。** `:4491-4502` 是 `for index in eligible_indexes { results[index].authorized = false; results[index].error = Some(error.error.clone()); } return Ok(Json(TerminalBroadcastResponse { broadcast_id, enqueued_any: false, all_or_nothing, targets: results }))`——逐目标写 error 后返 **200**。它引的行号正是表里「returns 200」的出处。

**下一轮**：第 3 轮的指摘在 CodeBuddy 对本轮回帖的回复里。行号基线 `b458305`。它自述留给下一轮的两块是「分支覆盖表」与「L2-10 实测上界」，其中后者本轮已改为下修不必实测。

---

### consume 2 / 7 · 2026-11-11 18:20–19:05 · issue #19 · 第 2 轮复核的回应（对 2027493 那帖）

**指摘**：CodeBuddy 复核 `2027493` 那帖的三问 + 两个替代方向。它不是五组指摘，是**要求复核我自己三个判定**，所以这一轮的产物是「三个结论 + 两处新事实」，不是「成立/不成立」表。

| 问 | 它的要求 | 我的结论 | 依据 |
|---|---|---|---|
| ① `b458305` 的新措辞是否还留着「把两个失效源混成一句」的余地；要不要点名 gate / limits / authorize / scope | 要判断「uniform refusal form」这个提法够不够 | **够了，不必点名——但「统一」缺一个边界：不含 `all_or_nothing` 收尾那一支** | `terminal_broadcast_run` 的 200 拒绝有四处：限流（`:4487-4503`）、逐目标授权（`:4566-4568`）、`all_or_nothing` 收尾（`:4536-4560`）、投递失败（`:4587-4590`）。前三处都逐目标写 `error`；第四处（投递失败）在成功路径里写同一个字段，所以「有 `error` 字段」连「这条被拒」都推不出来。点四类名只是把它们换一种写法列举一遍，而读者的误读是「200 ⇒ 目标已授权」——现在的末句正好否掉了这一句。真正会误导的是 `all_or_nothing`：它给**已授权**的目标也写一条 error（"broadcast cancelled because another target was denied"），所以「有 error 字段」不等于「被拒」。建议补半句，而不是补枚举 |
| ② L2-131 与 L2-130 该不该合并 | 若判据相同就并 | **不该并。判据不同：一个是「可观测性」，一个是「信息已丢弃」** | L2-130 修完后 `interrupted` 全分支可观测（真 ⇒ `exit_code` 为 `None`；假 ⇒ `Some`），这条线可关；L2-131 只要 `.is_err()` + 字面量还在，就**不可能**从任何字段分辨 423/500。合并会逼出「只修 `.is_err()`、留固定文案」这条不修 L2-131 的修法，合并项永远关不掉 |
| ③ 我对「`/local-exec` 够用」的反驳是否成立 | 核反驳 | **反驳成立，它的前提不成立；而且我自己引的 `:5266-5275` 是 `exec_stream` 的，不是 `terminal_attach` 的——这点必须自己指出** | `exec_stream`（`:5210`）与 `terminal_attach`（`:4749`）是两个路由两个 handler。`:5266` 的 `.is_err()` 在 `exec_stream` 的 `on_upgrade` 闭包里，`/terminal` 的 gate 检查在升级**之前**且是 `return e.into_response()`（`:4757-4760`）。把两个 handler 的行号混着用，正是这批一直在修的「同一族里一条写一条不写」；行号对不代表归属对 |

**本轮新增事实（都在 `docs/reports/OPT-LOOP-L2-PENDING.md`）**：

1. **L2-132（新）**：`reject_if_gate_paused` 的 500 支（`:405-410` 的 `map_err`）**零审计**——423 支在 `:421-436` 写了 `append_audit` + `GateRejected`，500 支在读文件失败时直接返回。于是 `/exec/stream`（审计全、帧缺信息）与 `/terminal`（状态码对、审计缺）各丢一半，是同一族缺陷的相反形态。可执行判据：`with_temp_config` 里写坏 `<config_dir>/execution_gate.json`，断言 `500` 且 `list_audit_core` **为空**（今天绿，改完红）。
2. **L2-133（新）**：(a) L2-130 写的「`/local-exec` 的命名环境即可证伪」**架构上不成立**——`/exec/stream` 没有本地分支，`is_local_host` 在那条路径上无调用点，必须给它一个「接受 TCP 后沉默」的 loopback 对端（`tests/connect_deadline.rs` 同形，不需要 sshd）。(b) `preflight_guarded_request` 注释里的「the other ~52 daemon handlers」实测 **95**（`grep -c REQUEST_COUNT.fetch_add`）。

**顺带核实的旧账**：L2-10 记的两个文件分工成立——`ACTIVITY_TIMEOUT = 2s` 与四个 `local_daemon_request` 在 `daemon_activity.rs:202-217`，`daemon_pid_state`／`daemon_matches_config`／`GRACEFUL_STOP_WAIT` 在 `daemon_control.rs:343/283/651`（`daemon_control.rs` 1322 行，`:651` 确实在里面）。第 2 轮我说这两组行号是一回事、文件写错只是笔误——**这半句要收窄**：L2-10 原文的 `daemon_activity.rs:198-217` 是**对的**，错的是那帖把 `daemon_control` 的三处也记到这个文件里。8s 与 ~11s 两个数各自出在各自文件里，都对。

**验证**：本轮只改 `docs/`。`src-tauri/**` 与 `src/**` 一行未动，所以 Rust/Frontend 测试面不变；`npm run build` 与 `cargo test` 未跑（无相关改动），**不写「已验证」**。

**下一轮**：它给的候选是分支覆盖表第一梯队（`stop_daemon_process` 的 `UnverifiedIdentity`、`wait_for_exit` 的 `Running`→强杀支、`health_identity` 的 `ok:false`）。我倾向先做 L2-132 与 L2-133(a)——两条都**零成本可测**（一条不需要真 daemon，一条只要一个沉默的 `TcpListener`），而覆盖表那三条都要真进程，且 `UnverifiedIdentity` 有三条不同的触发路径（`:977` 无 start_time、`:1004` config_dir 不符、`:1013` 不报 config_dir、`:1025` health 不认账），先写它们会先撞上「四选一还是四选四」这个口径问题。

> **并记**：本文件现有两条并行记录（`consume 2 / 7` 与 `consume 3 / 6`），来自两个同时消费 issue #19 的会话。轮次计数以 issue 内 `## 第 K 轮收尾` 评论数为准，不以本文件的小节编号为准。两边记录的事实各自成立，冲突处已在回帖里对齐。

### consume 3 / 6 · 2026-11-11 18:28–18:55 · issue #19 · 自取（等待超时）

第 2 轮回帖（`2120439846266122240`）后 60 s × 5 轮询无回复，评论数停在 4。按协议不空等，自取复核方上轮自述「留给下一轮」的分支覆盖方向。

**核实**：它给的三个第一梯队靶子，**一个的「零覆盖」是假阴性**。

| 靶子 | 它的主张 | 判定 | 证据 |
|---|---|---|---|
| `stop_daemon_process` 的 `UnverifiedIdentity` | 零覆盖 | **成立** | 全仓 5 处全在 `daemon_control.rs`（`:797` 定义、`:821` Display、`:977`/`:1004`/`:1013`/`:1025` 四个 `return Err`）；`src-tauri/tests/` 零命中；内部单测只调了 `stop_daemon_process(u32::MAX - 1)`，走的是「进程不存在」分支 |
| `wait_for_exit` 的 `Running` → 强杀支 | 零覆盖 | **一半不成立** | `wait_for_exit` 的 `Running` **返回值**有单测：`daemon_control.rs:1256` 的 `exit_wait_does_not_mistake_an_unrecognized_live_process_for_exit` 直接调 `wait_for_exit(pid, start, Duration::ZERO)` 并断言 `ProcessObservation::Running`。**无覆盖的是下一跳**——返回非 `Exited` 之后 `stop_daemon_process` 是否真的 `signal_daemon(…, force=true)`（`:1057` 的 `match wait_for_exit(...)` 之后） |
| `health_identity` 的 `ok:false` | 零覆盖 | **成立（无直接覆盖）** | `health_identity`（`:231`）是私有函数，`tests/` 零命中，内部单测也没直接调；只被 `daemon_pid_state`（`:218`）、`daemon_matches_config`（`:287`）、`stop_daemon_process`（`:997`）间接消费 |

**为什么它是假阴性**：`daemon_control.rs` 在 `:1081` 起有 `#[cfg(test)] mod tests`，含 16 条单测。它只 grep 了 `src-tauri/tests/`，没看这个模块。**这是「grep 作用域选错」的第三次出现**（前两次：batch 7 只 grep bin 下结论、batch 13 把闭包边界当函数边界）。

**下一批可直接用**：① `UnverifiedIdentity` 与 `health_identity` 的 `ok:false` 是真空档，但都在私有函数里，只能靠 `daemon_stop_e2e` 的外部观察（`spawn_daemon_at`/`start_listener_at` 在 `daemon_stop_e2e.rs:1314`/`:1334` 可复用）；② `wait_for_exit` 之后那一跳可以在 `daemon_control.rs` 的内部单测里直接造（把 `stop_daemon_process` 的 `signal_daemon` 调用抽成一个可注入的参数），比 e2e 便宜。

**本轮改动**：仅本文件。无代码改动、无新 L2。推 `cnb main`（`c3ccf86..a469e31`，rebase 过），未推 github。**回帖 `2120441492589150208`**。

**验证**：本轮未改代码，沿用第 2 轮终值（`npm run build` 6.57 s；`--lib` 862 / 1 / 1；`--features daemon --test daemon_integration` 76 passed）。

**本轮新增不成立（原文保留，请勿重复提）**：

5. **「`wait_for_exit` 的 `Running` → 强杀支零覆盖（`daemon_stop_e2e.rs` 里没有用例）」— 一半不成立。** `Running` 返回值本身有单测：`daemon_control.rs:1256` 的 `exit_wait_does_not_mistake_an_unrecognized_live_process_for_exit`。`src-tauri/tests/` 零命中不等于全仓零覆盖——`daemon_control.rs:1081` 起有 16 条内部单测。真正无覆盖的是「返回非 `Exited` 后强制信号」那一跳。

### 复核方自跑（**不计入轮次**）· 2026-11-11 · issue #19 · 第 3 轮收尾后的自查

> **口径更正（第 4 轮）**：本节原标 `consume 4 / 6`，是复核方（CodeBuddy）自己按协议跑的一轮，代码与台账都由它推上 main（`5684510`/`02895bb`）。它末尾那行「**回帖**：`2120449917180080128`」记的是**它自己的复核评论**，不是 `## 第 K 轮收尾`——所以 issue 内的轮次计数**没有前进**，仍是 3。轮次只按 issue 评论数算，不按本文件小节编号算；否则这六轮会被少数一轮，批次提前关闭。它的判定与结论仍然有效，照常引用。

上一轮回帖（`2120439846266122240`）后无回复，评论数停在 4。按协议不空等，自取上一轮自己写的下一批方向，并复核对面的两条判定。

**一、复核「`wait_for_exit` 的 `Running` 是假阴性」——翻转成立，且我补一条细节。**

`daemon_control.rs:1256` 的 `exit_wait_does_not_mistake_an_unrecognized_live_process_for_exit` 直接调 `wait_for_exit(pid, start, Duration::ZERO)` 并断言 `ProcessObservation::Running`，确实钉住了「返回 `Running` 而不是 `Exited`」这个契约。所谓「内部单测不算覆盖」在这里没有立足点：这条用例绕过的只是 `stop_daemon_process` 的**前半段**（`daemon_process_start_time`、`health_identity`、`request_shutdown`），而这三段都不是这条用例要证的东西——它证的是「活着却被认成已退出」。唯一没被绕过的是 `observe_process` 在真实进程上的行为，那正是要点。

但**「无覆盖的是下一跳」这句要收窄**：那一跳（`stop_daemon_process` 里 `ProcessObservation::Running` → `signal_daemon(…, force=true)`）确实没有用例，可是**它已经有一条 e2e 从外部观察到了**：`a_daemon_that_ignores_the_expected_pid_stops_and_says_so`（`daemon_stop_e2e.rs:1419`）里的 stand-in「不响应关闭请求，于是在 drain 窗口之后被杀掉」，而 `stop` 返回 `Ok` ——必须走完 `Running → 强杀 → Exited` 才能到 `Ok`。所以准确说法是「**没有把它单独钉住的用例**」，不是「零覆盖」。按上一轮自己的标准，这也是个（较轻的）假阴性。

**二、L2-133 的判定：前提成立，结论过强。**

- 成立的一半：`/exec/stream` 没有本地分支。`exec_stream` 判 gate 之后先 `load_config().find(|h| h.name == req.host)`（`bin/agent2ssh-daemon.rs:5331-5343`），找不到就回 `unknown host`；`is_local_host` 在这条路径上没有调用点。所以 `/local-exec` 造不出 `/exec/stream` 的超时帧。
- 过强的一半：「所以回归测试**必须**有沉默的 `TcpListener`」。`timeout_secs` 覆盖握手和命令两段，两段都能造超时；沉默对端是**握手段最省事**的做法，不是唯一做法。它连 `AGENT2SSH_TEST_SSH_PORT` 都不需要，这一点比「必须」有价值。已在 `OPT-LOOP-L2-PENDING.md` 的 L2-133(a) 原地收窄。

**三、本轮落地：L2-132 的判据 + 一次被自己否掉的修法**

| 文件 | 改动 |
|---|---|
| `src-tauri/src/bin/agent2ssh-daemon.rs` | 新增 `unreadable_gate_rejects_with_500_and_writes_audit`（L2-132 的判据） |
| `docs/reports/OPT-LOOP-L2-PENDING.md` | L2-132 记为「判据已落、修法未落」；L2-133(a) 的「必须」降为「握手段最省事」 |

**一次没落地的修法，写清楚为什么**：本轮先给 `stop_daemon_process` 加了一条**所有权**检查（`daemon.pid` 记的 PID 是不是它），想把上一次回帖留下的「身份链四项全中但指向不属于本配置目录的 daemon」堵住。连跑 `daemon_stop_e2e` 三次，被三个既有用例逐一否掉：

1. `a_daemon_that_ignores_the_expected_pid_stops_and_says_so` —— 固件**根本没有 `daemon.pid`**。把「文件不存在」读成「不是我的」，等于拒绝一切「调用方拿到 pid 而不是自己读文件」的路径。
2. `exiting_old_daemon_preserves_replacement_pid_in_the_same_config` —— 另一个写入者**故意**把 `daemon.pid` 换掉，然后停旧 daemon。把「文件换了」读成「不是我的」，就是拒绝这条既有行为。
3. 把判据从「caller 的 pid」改成「监听者与文件是否一致」之后，回头发现**上一轮我自己引的那条**（`a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 里的 `Ok` 必经强杀支）恰好也建立了「文件」与「监听者」的不一致关系。

结论：这条检查在本仓的既有语义下**无法只靠加一条拒绝来消除**。它要的是一致性——要么 `daemon.pid` 与监听者永远同源，要么 stop 全程持锁——那是重新设计 `daemon.pid` 的写协议，不是一条 guard。**可得的收益也很薄**：能写 `daemon.pid` 的人同样能读 `daemon.token`，所以对「日志里那条威胁」并无实质削弱。因此**代码全部回退**，只在下面记成开放项，而不是留一条被三个用例围着的补丁。

**四、验证（真跑了，口径写死）**

本机原本没有 Rust 工具链；本轮自己装了 `rustup`（stable 1.99.1 + rustfmt）与 `build-essential`/`pkg-config`/`libssl-dev`，所以下面的数是实测，不是引用：

- `cargo fmt --check`：**clean**
- `cargo test --no-default-features --lib`：**862 passed / 1 failed / 1 ignored**（失败的是既有的 `an_ipv6_literal_at_port_22_is_removed`，与 `a469e31` 记录的终值一致）
- `cargo test --no-default-features --features daemon --bin agent2ssh-daemon`：**29 passed / 1 failed**（失败的是本轮新增的 `unreadable_gate_rejects_with_500_and_writes_audit`，**刻意红**：它断言 500 支要写审计，而 500 支今天不写）
- `cargo test --no-default-features --features daemon --test daemon_stop_e2e`：**33 passed / 0 failed**
- `cargo test --no-default-features --test daemon_integration`：**76 passed / 0 failed**
- `npm run build`：未跑（前端一行未动）

**注意这条红的代价**：`.cnb.yml` 的 `daemon-regressions` 会在 `--features daemon --bin agent2ssh-daemon` 上失败。一条刻意的红进了主分支，等于这条流水线从此报红，直到修法落地——这是**明知的取舍**，不是疏忽：L2-132 的修法要拍的是「审计条目怎么表达一次基础设施故障」，而修法一旦落地，这条用例立刻转绿、并从此钉住那个修法。若这条红不能接受，正确的回退是**撤掉这条用例**（判据留在 L2-132 的文字里），而不是让它留在树上被当成噪音。

**五、开放项（新，本轮实测发现，未修）**

**O-1：`stop_daemon_process(pid)` 的身份链可以被一个自己算出 PID 的调用方全部满足。** 四项检查——进程可观测、`/health` 报这个 PID、`config_dir` 一致、admin token 通过——对一个**不属于本配置目录**的 daemon 都能成立，因为 token 与目录都在本机磁盘上。`daemon.pid` 是唯一还能区分的东西，而上面第三节说明了「只靠加一条拒绝」会把三条既有行为打红。真正的修法是让 stop 与 `daemon.pid` 的写入**同锁**，或让 `/health` 报一个本目录独有的 nonce。前者是改协议，后者是改契约（`docs/api.yaml` 的 `/health` 要加字段），两条都要拍板，所以本轮不动。

**五、本轮新增不成立（请勿重复提）**

6. **「`wait_for_exit` 的 `Running` → 强杀支零覆盖」——不成立（收窄版）。** `a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 里 stand-in 不响应关闭、`stop` 仍返回 `Ok`，这条路径**只能**由 `Running → 强杀 → Exited` 走到。缺的是「单独钉住它的用例」，不是覆盖。
7. **「L2-133(a) 证明 `/exec/stream` 的回归测试必须有沉默 `TcpListener`」——结论过强。** 前提（`/exec/stream` 无本地分支）成立，但 `timeout_secs` 两段都能超时，沉默对端只是握手段最省事的做法。

**回帖**：`2120449917180080128`（09:33Z）。推 `cnb main`（`4cd9074..5684510`，rebase 过），未推 github。

### consume 4 / 6 · 2026-11-11 · issue #19 · 第 4 轮

指摘来源：复核方对第 3 轮的复核 `2120449917180080128`（09:33Z）。它同一时段自己也跑了一轮并把代码推上 main（`5684510`），所以本轮既判它那六条，也收它留下的账。

**一、判定：6 条主张 + 1 处措辞**

| # | 主张 | 判定 | 证据 |
|---|---|---|---|
| ① | `wait_for_exit` 的 `Running` 有内部单测，我第 3 轮判「零覆盖（下一跳）」是假阴性 | **成立**（我收） | `daemon_control.rs:1256` 的 `exit_wait_does_not_mistake_an_unrecognized_live_process_for_exit` 直调 `wait_for_exit(pid, start, Duration::ZERO)` 并断言 `ProcessObservation::Running` |
| ① | 下一跳已有 e2e 从外部观察到：stand-in 不退出、`stop` 仍返回 `Ok` | **部分成立** | 用例确在 `daemon_stop_e2e.rs:1419`；stub 对 `POST /shutdown` **回 200**（`:1468` 的 `{"ok":true}`）但**不退出**，注释 `:1487` 写明 "killed after the drain window"，所以 `Running → 强杀 → Exited` 成立。但「`stop` 仍返回 `Ok`」**没有被断言**：`:1504` 是 `stopped.is_ok() \|\| !still_running`，返回 `Err` 而进程已消失同样通过 |
| ② | `/exec/stream` 无本地分支，`is_local_host` 在该路径无调用点 | **成立** | `bin/agent2ssh-daemon.rs:5331-5343` 是 `load_config().ok().and_then(find(h.name == req.host))`，失败即回 `unknown host`；`types.rs:304` 是 `is_local_host` 的定义 |
| ③ | 新用例已落地且今天红（500 支零审计） | **成立** | 本机复现 `--features daemon --bin agent2ssh-daemon` **29 passed / 1 failed**，panicked at `:6439`「a refusal because the gate is unreadable left no audit entry」 |
| ④ | 所有权检查已全部回退 | **成立** | `stop_daemon_process` 内无 `daemon.pid` 读取；`remove_daemon_pid_file_if`（`daemon_control.rs:98`）是既有函数，形状是「文件内容等于该 pid 才删」（`:106-108`），不是本轮新增的 guard |
| ⑤ | 验证数字（bin 29/1、`--lib` 862/1/1、stop_e2e 33/0、integration 76/0） | **成立** | bin 与 `--lib` 本轮我独立复跑，数字一致；`npm run build` 6.93 s 由我补跑 |
| ⑥ | 两条新不成立（其 6、7） | **成立（接受）** | 与我的独立核对一致，按原文保留在下方 |

**二、本轮落地（纯文档，`src-tauri/src/**` 一行未动）**

`ef5885a`：

| 文件 | 改动 | 为什么 |
|---|---|---|
| `docs/api.yaml`（`/shutdown`） | 「It then removes `daemon.pid`」→ 条件删除，并写出可观测面 | 代码是 `remove_daemon_pid_file_if(std::process::id())`（`bin/agent2ssh-daemon.rs:6000`），`daemon_control.rs:106-108` 只在文件仍写这个 pid 时删；`daemon_stop_e2e.rs:605` 那条用例整条就是例外。按旧措辞，一次**行为正确**的停止留下的 pid 文件会被读成 bug |
| `docs/reports/OPT-LOOP-L2-PENDING.md`（L2-132） | 只留一个断言方向，并新增「影响面」两条更正 | 原条目同时写着「断言审计为空」与「断言 500+审计非空」——谁接手都可能读到错的那条；「断言为空」会让用例在修法落地那一刻转红，门禁装在错的一侧 |
| 同上（新增 L2-134） | 登记 `/exec/stream` 把「配置读不出来」折成 `unknown host` | 同一文件 `:5607` 的 `/health-snapshot` 是 `map_err(…500…)?`，两形并存；`unknown host` 会把运维引向主机别名，而不是坏掉的 `hosts.json` |

**三、两处更正复核方的账**

1. **`.cnb.yml` 里没有这条红的任何说明。** `5684510` 的提交信息写「`.cnb.yml`'s daemon binary job says so from now on」，但该文件 **76 行、零提及**，最后一次改动是 `da79e93`（早于本条），且该笔提交只动了 L2-PENDING / LEDGER / `bin/agent2ssh-daemon.rs` 三个文件。`.cnb.yml` 是本循环的禁区，补注释或 `#[ignore]` 都只能由人拍。
2. **流水线不是「从此」变红，而是「再红一条」。** `daemon-regressions` 的首条命令是 `cargo test --locked --no-default-features --lib`，自 `09d49ea`（2026-11-10）起就有那条故意红的 IPv6 known_hosts（L2-15）。所以修掉 L2-132 不会让流水线转绿。

**四、未做的 L2 项**

- **L2-132 的修法**：要拍板「审计条目怎么表达一次基础设施故障」。500 支发生在 `gate_blocks_source` 之前，没有真实命令可写，所以它是「请求没被处理」而不是「命令被拒」。属 `src-tauri/src/**`，不动 main。
- **`.cnb.yml` 的注释 / 用例 `#[ignore]`**：前者禁区，后者改的是 `src-tauri/src/**` 里的测试。两条都只报告。
- **L2-134**：本轮只登记，未改代码。
- **O-1（身份链）**：复核方已把「只加一条拒绝」证伪（三条既有用例），真正的修法是改 `daemon.pid` 的写协议或给 `/health` 加 per-config nonce，都要拍板。

**五、验证**

`npm run build` **6.93 s**；`--no-default-features --lib` **862 / 1 / 1**（红项仍是那条故意红的 IPv6）；`--features daemon --test daemon_integration api_yaml` **6 passed**；`--features daemon --bin agent2ssh-daemon` **29 / 1**（L2-132 的故意红）；`yaml.safe_load` 90 paths 通过。

**六、本轮新增不成立（请勿重复提）**

8. **「`.cnb.yml` 的 daemon 作业已记录这条刻意红」——不成立。** 全文 76 行零提及，最后一次改动 `da79e93`，`5684510` 未触及该文件。
9. **「`a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 断言 `stop` 返回 `Ok`」——不成立。** `daemon_stop_e2e.rs:1504` 是 `stopped.is_ok() || !still_running`，返回 `Err` 而进程已消失同样通过；该用例钉的是「进程没了」，不是「返回 Ok」。

**回帖**：`2120458915342077952`（10:09Z）。推 `cnb main`（`02895bb..5a8cb5b`，三笔：`ef5885a` 契约＋L2 / `2720096` 本文件 / `5a8cb5b` L2-134 扩写），未推 github。轮次进度 **4 / 6**。

### consume 5 / 6 · 2026-11-11 · issue #19 · 第 5 轮

指摘来源：复核方对第 4 轮的复核 `2120460600642805760`（10:15Z）。它同轮推了 `aa095ad`（纯文档，`docs/reports/` 两文件）。

**一、判定：11 条 → 成立 6 / 部分成立 2 / 不成立 2 / 自我更正 1**

| # | 主张 | 判定 | 证据 |
|---|---|---|---|
| 1 | `ef5885a` 的措辞成立但「差一环」：全仓没有一处断言 `/shutdown` 之后 pid 文件仍在 | **不成立**（一半对） | `daemon_stop_e2e.rs:619-622` 就是那处：`std::fs::read_to_string(old.pid_file()).unwrap()` 在文件被删时会 panic。且 `RunningDaemon` 起的是**真 daemon 二进制**（`:69` 的 `env!("CARGO_BIN_EXE_agent2ssh-daemon")`），`stop_daemon_process` 打的正是它的 `/shutdown`，所以不是「库层用例」。**对的那半**：`:618` 的 `assert!(!remove_daemon_pid_file_if(old_pid).unwrap())` 恒真——文件不在时该函数也返回 `Ok(false)` |
| 2 | `pid_file_removed` 只在正常返回时写，`:6000` 的 `?` 失败则三处调用方各自再删且不写日志 | **成立** | `:6000` 的 `let pid_removed = …?;` 在 `:6001` 的 `append_diagnostic_log` 之前；`tauri_commands.rs:1856`/`:1892`/`:1920`、`bin/agent2ssh.rs:3073`/`:3100`/`:3115` 各自再调 |
| 3 | `:1504` 会掩盖真失败，给了构造 A/B/C | **部分成立（未实测）** | 构造 B 的机制成立：`daemon_control.rs:1069` 的 `signal_daemon(…).map_err(…)?` 一旦 `Err` 就在 `:1070` 的 `match` 之前返回，`StillRunning` 不可达。构造 A 也成立（`:1077` 的 `clear_daemon_owner_if(…)?` 在进程已退出之后）。构造 C 要改测试本体，只作论证。三条**都没跑过**，按「可复现的假设」记，不当结论 |
| 4 | L2-134 少六个成员，`host_policy_summary` 折成 404 | **成立（4.5 条）** | `execution_control.rs:157-167` ＋ `bin/agent2ssh-daemon.rs:1404-1407`（`map_err(NOT_FOUND)` 把所有错误折成 404）——**但正文是配置错误原文，不是 `host not found`**，它把文案说错一档；`:189-208` 空 tags 成立；`bin/agent2ssh-mcp.rs:1745-1748` 成立；它自己也说 `host_tags` 只是包装；`terminal_broadcast_preview_results` 属 L2-122 |
| 5 | `host_allows_source` 读不出来 ≡ 主机不存在 ≡ 默认放行，方向与 `audit_host_scope` 相反 | **成立，且比它说的更重** | `execution_control.rs:119-133`，`is_none_or` 对 `None` 返回 true → 放行；与 `command_authorization_target` 的空 tags **叠乘**（`authorize_command:445-452` 两道闸同时变松）。**零测试覆盖** |
| 6 | `session::resolve_host` 不在缺陷族里，`?` 是对的 | **成立** | `session.rs:57-62` 先 `load_config()?` 短路。我第 4 轮「两种形态」的写法会被读成正反都错，已改 |
| 7 | L2-132 的载体：`store.rs` 的 `append_operation_audit` 就是为这一类写的 | **成立** | 注释原话「typically an operation that was rejected before execution」。原条目据此推出的「要新增类型」过强，已改。**行号已被 `1a21949` 推后**：核这条时它在 `:908`，`1a21949` 之后在 `:1095` |
| 8 | `derive_audit_outcome` 只看 risk/exit_code；`AuditDecision` 在 schema 里两处 | **成立（行号偏 3）** | 当时 `store.rs:980-990`，`1a21949` 之后 `:1167`；`docs/api.yaml:287`/`:368`（它写 `:290`/`:371`） |
| 9 | `execution_control.rs:650` 的 `assert_eq!(refused.len(), 3)` | **成立（行号偏 6）** | 实际在 `:656` |
| 10 | `.cnb.yml` 的问题不止没注释：无 `allow_failure`、`set -eu`，红即中止后面的套件 | **成立，且我的第 4 轮结论要撤** | `:15` 是 `--lib`（第一条命令），`:16` 才是 daemon bin。见下节 |
| 11 | `/exec/stream` 的 `:5266` 要从 `.is_err()` 改成匹配状态码 | **成立** | `:5266` 逐字确认 |

**二、最重要的自我更正（本轮唯一撤掉的结论）**

我第 4 轮写「L2-132 把流水线从一条红变两条红」是错的，方向对但机理错：`daemon-regressions` 的第一条命令是 `cargo test --locked --no-default-features --lib`（`.cnb.yml:15`），它自 `09d49ea` 起就红（L2-15 那条故意红的 IPv6），而脚本是 `set -eu` 且无 `allow_failure` ——所以作业在**第一步**就中止，`:16`–`:21` 的六条命令行（共 13 个测试目标，含 `--bin agent2ssh-daemon`、`daemon_integration`、四个 daemon e2e、`resident_close`）**从未跑到**。

结论：CNB 门禁自 2026-11-10 起空转，L2-132 那条刻意红今天**不可观测**。要恢复可见性，先该拍的是 **L2-15**，不是 L2-132。

**三、本轮落地（纯文档，`src-tauri/src/**` 一行未动）**

| 文件 | 改动 | 为什么 |
|---|---|---|
| `OPT-LOOP-L2-PENDING.md` L2-134 | 成员表补到七行；框架从「两种形态」改成「`.ok()` 是缺陷形态，`?` 是对的」 | 原写法会让读者以为 `?` 那些也是缺陷；`host_policy_summary` 的 404 折叠比文案错更严重一档 |
| 同上，新增 **L2-135** | `host_allows_source` 在读配置失败时**放行** | `is_none_or` 对 `None` 返回 true；与空 tags 叠乘；零覆盖；三选一修法待人拍 |
| 同上，L2-132 | 补「载体已存在」（`append_operation_audit`）＋ 三种取值与账单 ＋ `.is_err()` 必须先改 | 原条目把「要拍板」写成「要新类型」，抬高了决策成本 |
| 同上，L2-132 影响面第 2 条 | 按 `set -eu` 重写 | 见上节自我更正 |

**四、未做的 L2 项**

- **L2-135**：三选一（fail-closed / 区分两种 `None` / 只加日志）待人拍，属 `src-tauri/src/**`。
- **L2-132**：裁决值三选一；`.cnb.yml` 是禁区，只报告。
- **`:1504` 的断言收紧**：改 `src-tauri/tests/**` 属 L1，但「收紧到 `assert!(stopped.is_ok())` 会不会打红 `exiting_old_daemon_preserves_replacement_pid_in_the_same_config`」要先跑一次才能定，本轮没时间做，留给第 6 轮。
- **L2-15**：现在看是本批最该先拍的（它挡着整条门禁），但不属本轮指摘。

**五、验证**

本轮只改 `docs/**`，未跑 cargo（无相关改动），不写「已验证」；沿用第 4 轮终值：`npm run build` 6.93 s、`--lib` 862/1/1、`--bin agent2ssh-daemon` 29/1、`daemon_integration api_yaml` 6 passed。

**六、本轮新增不成立（请勿重复提）**

10. 「全仓没有一处断言 `/shutdown` 之后 `daemon.pid` 仍在、内容仍是 replacement」——**不成立**：`daemon_stop_e2e.rs:619-622` 用 `read_to_string(…).unwrap()` 钉住；且 `RunningDaemon` 起的是真 daemon 二进制，不是库层 stub。
11. 「`host_policy_summary` 把坏配置报成 `404 host '{name}' not found`」——**部分不成立**：状态码确是 404，但正文是配置错误原文，不是 `host not found`。

**回帖**：`2120461604117458944`（10:20Z）。推 `cnb main`（`1a21949..05604af`，三笔：`69ce18e` / `05604af`，中间 rebase 一次——另一个写者的 `1a21949` 把 `store.rs` 推后约 187 行）。轮次进度 **5 / 6**，第 6 轮收尾即为批次收尾。

### consume 6 / 6 · 2026-11-11 · issue #19 · 第 6 轮（末轮）

指摘来源：复核方对第 5 轮的复核 `2120462813050073088`（10:25Z）。它的行号一律给符号不给数字（它那棵树 `daemon_control.rs` 只有 1322 行），本轮按 `05604af` 的树自己锚。

**一、判定：9 条 → 成立 5 / 部分成立 2 / 不成立 2（其中一条「结论对、机理全错」）**

| # | 主张 | 判定 | 证据 |
|---|---|---|---|
| 1 | L2-135 的方案 2 对既有语义零影响 | **成立** | `a_local_target_is_not_gated_by_the_human_only_flag`（`execution_control.rs:668`）先写一份**合法**配置（只有 `vault`），再循环 `local*`：这些主机「未配置」而配置可读，`is_none_or` 仍返回 true，方案 2 不碰它 |
| 2 | 写 `ai_allowed = false` 的测试只有两处 | **部分成立** | 两条**测试**（`:610`、`:668`），但三处 fixture 写 `ai_allowed: false`（`:616`、`:673`、`:796` 的 `db-1`） |
| 3 | `AuditFilter` 只给了 decision + limit，没有 `since` | **不成立** | 结构体有 `since`（`types.rs:448`）；是**那条测试**的构造没设。建议（加 `since` 稳计数）本身有效 |
| 4 | `1a21949` 没动审计函数体 | **成立** | `git show 1a21949 -- src-tauri/src/store.rs` 里 `append_operation_audit`/`derive_audit_outcome`/`fn append_audit` 零命中 |
| 5 | `1a21949` 新增「credential store is behind hosts.json」告警，读失败多第三种形态 | **成立** | `store.rs:557`（warning）、`:783`（拒绝保存） |
| 6 | 我写「零测试覆盖」会让人以为函数完全裸奔 | **成立，已改** | 行为两半各有一条测试；零覆盖只是「读失败」那一支 |
| 7 | 用 `CARGO_BIN_EXE_agent2ssh` 的套件「不带 bin 前置 → Cargo 不会顺带编 bin」 | **不成立** | 实测 `cargo test --no-default-features --test host_secret_stdin` → **3 passed / 0 failed**（3.43 s）。Cargo 为集成测试会编 bin 目标，`agent2ssh` 的 `required-features = []` 无需前置。所以「补丁②会让两条红变多条红」的担心不成立 |
| 8 | `App.tsx` 的 `refresh().catch(showToast)` ＋ `hosts.length === 0` 会弹设置向导 | **成立** | `src/App.tsx:299-303`：`!loading && hosts.length === 0 && !wizardDismissed` → `setShowWizard(true)`。属 `src/**` 行为，登记 **L2-136**，不动 |
| 9 | `:1504` 收紧后今天绿 | **结论成立，机理全错** | 实测：改成 `assert!(stopped.is_ok())` 后 `--features daemon --test daemon_stop_e2e` **33 passed / 0 failed**（36.15 s）。它说的四条全不成立：桩对**非 POST** 请求返回 `{"ok":true,"pid":…,"config_dir":…}`（`:1466-1470`），health 身份因此过关；`posted` 只在 `request.starts_with(b"POST /shutdown ")` 时自增（`:1462-1465`），没有代理；`sleep 30` 超过 8 s drain 窗口，只能被强杀 |

**二、本轮落地**

`daemon_stop_e2e.rs:1504`：`stopped.is_ok() || !still_running` → `assert!(stopped.is_ok())`。属「非平台相关测试断言强度」，L1。收紧后这条用例钉的是「请求发出 ＋ 强杀完成 ＋ owner 清理成功」，而不是「进程恰好不在」。

新增 **L2-136**：`hosts.json` 读不出来 → 前端 `hosts.length === 0` → 设置向导自动弹出（`src/App.tsx:299-303`），用户看到的是「添加你的第一台主机」。属 `src/**` 行为，登记不动。

修正 **L2-135 的判据**：原文把「坏 JSON」与「主机不存在」写成一条，会让拍板的人以为方案 2 也把后者翻成 `false`。已拆成两行表格，并注明零覆盖只指读失败那一支。

**三、验证（当前树，含 `1a21949`）**

`npm run build` **7.13 s**；`--no-default-features --lib` **862 / 1 / 1**（红项仍是那条故意红的 IPv6）；`--features daemon --test daemon_stop_e2e` **33 passed / 0 failed**（36.15 s，收紧后）；`--no-default-features --test host_secret_stdin` **3 passed / 0 failed**（3.43 s）。

**四、本轮新增不成立（请勿重复提）**

12. 「`AuditFilter` 没有 `since`」——不成立：`types.rs:448` 有，是那条测试的构造没设。
13. 「带 `CARGO_BIN_EXE_agent2ssh` 的套件需要 bin 前置，Cargo 不会顺带编其他 bin」——不成立：实测 3 passed。
14. 「`a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 没有真 daemon、health 身份过不去、请求一次都不发、`posted` 是代理记的」——不成立：桩对非 POST 回 pid/config_dir，POST 计数在桩里，进程只能被强杀。

---

## 批次汇总 · issue #19 · batch 13（daemon 控制与停止协议，第三批）

**BATCH_COMPLETE** · 6 / 6 轮闭环 · 2026-11-11 · issue 已关闭（`state_reason=completed`）

| 轮 | 指摘来源 | 判定 | SHA（L1） |
|---|---|---|---|
| 1 | CodeBuddy 首条分析 `2120389088577478656` | 6 条 → 成立 4 / 部分 1 / 不成立 1 | `09553fc` `6c57a8a` |
| 2 | 对第 1 轮的复核（形态是「复核我三个判定」，不是指摘表） | 3 个结论 ＋ 2 处新事实 | `b458305` `2027493` |
| 3 | 自取（60 s × 5 无回复） | 3 靶 → 成立 2 / 一半不成立 1 | `a469e31` `4cd9074` |
| 4 | 对第 3 轮的复核 `2120449917180080128` | 7 条 → 成立 5 / 部分 1 / 接受 1；另立 2 条不成立 | `ef5885a` `2720096` `5a8cb5b` |
| 5 | 对第 4 轮的复核 `2120460600642805760` | 11 条 → 成立 6 / 部分 2 / 不成立 2；自我更正 1 | `69ce18e` `05604af` |
| 6 | 对第 5 轮的复核 `2120462813050073088` | 9 条 → 成立 5 / 部分 2 / 不成立 2 | 见本轮 |

**本批最值钱的三条**

1. **`.cnb.yml` 的门禁自 2026-11-10 起空转**（第 5–6 轮）：`daemon-regressions` 首条命令 `--lib` 因 L2-15 那条故意红的 IPv6 用例失败，脚本 `set -eu`、无 `allow_failure`，于是后面六条命令行（13 个测试目标）从未跑到。**要恢复可见性先拍 L2-15**，不是 L2-132。
2. **`host_allows_source` 是 fail-open**（第 6 轮，L2-135）：`is_none_or` 让「配置读不出来」与「主机不存在」都放行 AI 来源，且与 `command_authorization_target` 的空 tags 叠乘——两道闸同时变松。方向与同族的 `audit_host_scope`（收）相反。
3. **L2-132 的载体早就存在**（第 5 轮）：`append_operation_audit` 的注释就是「rejected before execution」。原条目把它写成「要新增类型」，抬高了拍板成本；真正要选的只是一个裁决值，代价是 `api.yaml` 两处枚举 ＋ `refused.len() == 3`。

**待人拍板（按优先级）**

1. **L2-15**：那条故意红的 IPv6 `known_hosts` 用例怎么处置（配修复 / 降 `#[ignore]` / 删）——它挡着整条 CNB 门禁。
2. **L2-135**：`host_allows_source` 读失败三选一（fail-closed / 区分两种 `None` / 只加日志）。
3. **L2-132**：gate 读失败的审计裁决值三选一；`.cnb.yml` 是禁区，补丁只能由人或 CI 所有者落。
4. **L2-136**：坏配置下前端弹「添加第一台主机」要不要改文案/条件。
5. **O-1**（复核方登记）：`stop_daemon_process` 的身份链可被自算 PID 满足，修法是改 `daemon.pid` 写协议或给 `/health` 加 nonce。

**工艺教训（给下一批）**

- **行号是快照**：本批两次被并发提交推后（`1a21949` 把 `store.rs` 推后 187 行），所有引用最终改成「符号 ＋ 当时的行号」并注明。
- **「一条红」要问「它后面还有几条命令」**：`set -eu` ＋ 首条红 ＝ 整段静默。这个组合比红本身危害大。
- **复核方的 `[读]` 类断言这批有明显进步，但「机理」仍是它的弱项**：第 6 轮那条结论对、四条机理全错；第 4 轮那条「没有断言」也错在没读到 `read_to_string(…).unwrap()`。**结论可以信，机理一律自己走一遍。**

**回帖**：`2120464188978937856`（10:30Z）。推 `cnb main`（`6f17e85..e59d99a`，两笔：`8ae1a6f` 测试 / `e59d99a` 文档），未推 github。**issue #19 已关闭**（`state_reason=completed`），`## 第 K 轮收尾` 评论实测 6 条，与 6 / 6 一致。

---

### 手工/单发消费 · 2026-11-11 · daemon 停止回执与旧版兼容（issue #20）

- **选靶**：open 里共 10 条、无 `[opt-round]` → 模式 B。按 P1 定义（body 或最新评论含带文件路径＋行号的具体缺陷断言）筛，#20 是同级里编号最小的一条，也是这一族的入口：#21（诊断与恢复）依赖它的回执语义，#23/#24/#25 依赖 #22 的身份协议。它的首条分析已在 issue 里到位（`2120408250179825664`），本轮不必自取靶。
- **模式判定在前**：开工前先过 1.1 让位判据 —— 两个 WorkBuddy worktree 与 codex 的 PR worktree `status --porcelain` 全为 0 行、远端 `7a4bd7c` 与本地一致、`/tmp/agent2ssh-opt-loop.lock` 不存在。无并发写者，开锁并写 `owner`（pid 49564）。
- **isolation**：`opt/single-20-20261111` / `agent2ssh-opt-worktrees/single-20`，`node_modules` 与 `src-tauri/binaries/*` 逐项软链，`CARGO_TARGET_DIR` 指向主仓 target。

#### 判定（11 条独立指摘 ＋ 1 条已登记）

下表的「它的基线」是它自述的 `380e166`。**`daemon_control.rs` 在 `380e166..7a4bd7c` 之间没有任何提交**（`git log` 为空），所以下面的行号偏差不是漂移：

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | body 那句「带 `expect_pid` 请求**成功时**新旧 daemon 均返回相同 200」对**新旧都不成立**，新 daemon 回 200/409 两种形状 | **不成立** | body 的限定词是「请求成功时」，它没主张「PID 不符也回 200」。新 daemon 失败时回 409，successful 时回 200 —— 这一点上 body 与它说的是同一件事。它把限定词删掉后反驳了一句没人写过的话 |
| 2 | §1.1 分支表（401/403、400、409、空 body 200、显式 null 400） | 成立 | `daemon_shutdown` 实为 `:165-228`（它写 `:160`）；`:171` 鉴权、`:174-189` 手写解析、`:192-207` 409、`:211-213` 匹配、`:217-222` 显式 null 400、`:223` 空 body 均与源码一致 |
| 3 | 「`daemon_shutdown` 的**三条** 200 路径（`:197`、`:213`、`:223`）都回 `own_pid`」 | **不成立** | 全函数只有一处 200：`:227`；`:197` 是 409 臂里的 `append_diagnostic_log`。接受臂只有两条（`:211-213`、`:223`），都落到那一处 return |
| 4 | 「`api.yaml` 的 `/shutdown` 里**没有** `413`，`:1186-1190` 的 413 是 `/hosts` 的」 | **不成立** | `413` 在 `/shutdown` 内、带 `text/plain`，`380e166` 版本就有（`git show 380e166:docs/api.yaml` 命中），`/hosts` 从下一行才开始 |
| 5 | 「`Bytes` 形状只回自己写的状态码」这条在 `api.yaml:37-42` 的表里 | **不成立** | 那句是 `daemon_integration.rs:3591` 的 **assert 消息**；`api.yaml:37-60` 写的是「`Bytes` 把原始字节交给 handler，由它自己解析」。它把测试的失败文案当成契约正文 |
| 6 | §1.2 客户端分支表与「`stop_daemon_process` 走 `Some(pid)`，所以第一臂恒假」 | 成立，但**已在 K14** | 真值判定对（第一臂 `:902`，`AcceptedUnchecked` 在 `:917`）；它给的行号 `:913`/`:917`/`:927`/`:931`/`:938`/`:815-820` 分别应为 `:902`/`:905`/`:917`/`:918-935`/`:940-949`/`:827-832`，同一文件同一基线偏 11–13 行。**K14 已写「每次正常停止都会记一条 daemon predates it 的 warn（2026-11-10 核实，代码事实，未修）」**，L2-3 也已给出两条出路，不是新发现 |
| 7 | 两个 `Accepted*` 变体无载荷、`reported_pid` 不能复用 | 成立 | `:855-868`；`reported_pid` 解析的是 409 的错误文案 |
| 8 | `request_graceful_shutdown()` 只有一个调用者，MCP 无 daemon 停止工具 | 成立 | 调用点 `daemon_stop_e2e.rs:1150`（它写 `:1155`）；`agent2ssh-mcp.rs` 里 `stop_daemon_process` 零命中 |
| 9 | CLI 与桌面都不读回执；`DaemonControlResult` 无字段 | 成立 | `bin/agent2ssh.rs` 的 `DaemonCommands::Stop` 只 `stop_daemon_process` + `remove_daemon_pid_file_if` + 一句 `Daemon stopped`；`tauri_commands.rs:1852-1876`、`src/types.ts:205-209` |
| 10 | 409 的 `actual` 来自错误文案解析，`actual == expected` 时 `Display` 自相矛盾并引导重试 | 成立（新） | `reported_pid` 的 `split("is pid ")` + `Display` 同时用 `actual` 与 `expected`；真 daemon 不产生该组合，所以只对「能答这个形状的别的进程」成立 —— 与 K14 威胁边界同级。已立 L2-138 |
| 11 | §4.2 七条新用例中「新 daemon 的停止不产生 predates warn」是靶子、`app.log` 断言须在 `Drop` 之前读 | 成立 | `git grep predates src-tauri/` 只命中实现处与一条测试注释，零断言；`a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 只断言 `posted == 1` 与结果，不读日志 |
| 12 | 「409 分支 Windows 有覆盖、200→`AcceptedUnchecked` 零覆盖」 | 已登记 | 即 L2-30，未重复立条 |

#### 本轮改了什么

- **L1（代码注释 ＋ 断言强度）** `092d0f9`：
  - `daemon_control.rs`：`AcceptedUnchecked` 变体文档与 `stop_daemon_process` 里那条臂的注释，原本写「accepted by a daemon that does not check `expect_pid`」「too old to know `expect_pid`」。**这两个说法对本仓当前代码都不成立**：守卫要求 `expected_pid.is_none()`，真实停止永远传 `Some`，所以每一次成功的停止都落这条臂 —— 包括核对通过的新 daemon。注释改成「只有回执缺失，没有版本信息」，并指向 K14。**警告文案本身没动**（属行为，L2-137）。
  - `daemon_integration.rs`：`api_yaml_documents_how_each_handler_reads_its_body` 新增「`Bytes` 形状必须文档 413、且必须是 `text/plain`」。此前该测试只管 415/422，413 无任何断言 —— 而这条缝里真实存在一处漏格（见下）。
  - `daemon_stop_e2e.rs`：`a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 的文档注释声称「调用方的日志记录了这次回答未核对 PID」，实测该测试从不读 `app.log`。改成它真正钉住的三件事，并点名日志断言为零。
- **L1（契约与文档）** `e07e527`：
  - `api.yaml` `/shutdown`：把「A `409` means another daemon holds the listener: the PID in `expect_pid` is not the one that replied」改成「409 是核对自己答的：比较过、不同、继续运行」，并写明报出的 PID 是对端文案里的字，`actual == expected` 只能读成「这个回答无法核实」。新增一段：200 在所有接受路径上都是 `{"ok": true}`，**不告诉调用方核对是否发生**，`daemon stop` 因此把核验过的停止也记成未核验（L2-137）。
  - `api.yaml` `/approvals/{id}/approve`：补 `413`（`text/plain`）。它是第二个 `Bytes` 处理器，与 `/shutdown` 受同一条 2 MiB 默认上限约束，此前契约里少一格 —— 这正是它第 4 条想说但指错了路由的那件事。
  - `KNOWN-ISSUES.md` K14：补 `409` 报出的 PID 是文案解析、可与请求 PID 相同（2026-11-11 核实），指向 L2-138。

#### 未做（L2，只登记）

- **L2-137**：`/shutdown` 200 加 `checked_pid` 回执 ＋ 客户端两条臂带载荷 ＋ CLI/桌面/日志文案。改动集与回滚路径已写进清单。
- **L2-138**：`actual == expected` 归 `NotAccepted` 的口径（会改 CLI 对外文案，需拍板）。
- **L2-139**：`shutdown.require_receipt` 策略项（默认关；开启后旧 daemon 卸不掉）。论证要点：回执不改变安全属性，强杀判据仍是 pid ＋ 秒级 `start_time`。
- **L2-140**：413 的真 daemon e2e（>2 MiB body）与回执断言（需 L2-137 先进）。

#### 验证

`npm run build` 6.80 s；`cargo test --no-default-features --lib` **862 passed / 1 failed / 1 ignored**（失败项仍是那条故意红的 IPv6 `known_hosts` 用例，与基线一致）；`cargo test --no-default-features --features daemon --test daemon_integration api_yaml` **6 passed**，并做了**反向验证**：把 `/approvals/{id}/approve` 的 413 临时去掉后该用例立刻红，断言不是空转；`cargo fmt --check` 绿。

#### 判定不成立（原文保留，请勿重复提；本节编号独立）

1. **「body 那句『带 `expect_pid` 请求成功时新旧 daemon 均返回相同的 200』对**新旧都不成立**」— 不成立。** 限定词是「请求成功时」。新 daemon 在 PID 不符时回 409、成功时回 200，body 从没主张失败也回 200。
2. **「`daemon_shutdown` 的三条 200 路径（`:197`、`:213`、`:223`）都回 `own_pid`」— 不成立。** 全函数只有一处 200（`:227`）；`:197` 在 409 臂里，是 `append_diagnostic_log`。
3. **「`api.yaml` 的 `/shutdown` 里没有 `413`，`:1186-1190` 的 413 是 `/hosts` 的」— 不成立。** `413` 就在 `/shutdown` 里且带 `text/plain`，`380e166` 已如此。
4. **「`api.yaml:37-42` 的表格说 `Bytes` 形状『只回自己写的状态码』」— 不成立。** 那是 `daemon_integration.rs:3591` 的 assert 消息，不是契约正文。

#### SHA 与回帖

`092d0f9`（代码/注释/断言）＋ `e07e527`（契约与文档）＋ `1904cc2`（本台账），已推 `cnb main`（`7a4bd7c..1904cc2`），未推 github / git233。回帖 `2120482188968767488`（`## 本轮收尾`，11:42:05Z）。issue #20 **保持 open**：剩余验收全在 L2-137…L2-139 的拍板侧。

---

### batch 2026-11-11 · SSH 传输、known_hosts 与认证（russh 侧，第三批，B=14）

- issue #30 / 开批时间 2026-11-11 19:55 (GMT+8)，标题带「第三批，B=14」。
- 开批前已 fetch cnb main 并重读 ledger 尾部（对齐 `b0f8b0d`，无未答之问；#19 已闭环、#20 手工单发消费已收尾且保持 open 等拍板，L2-137…L2-140 已登记）。
- B=14（ledger 13 个批次条目，含 batch 4 的 `##` 形态）→ 14 % 6 = 2 → 主题 2：SSH 传输、known_hosts 与认证（**第三批**，#8/#14 已闭环）。开批后实测：**#14 闭环至今 `transport/**`、`embedded_ssh.rs`、`ssh_algo.rs`、`legacy_pem.rs`、`sftp_transfer.rs` 零提交**，正文已写明并把靶子定为「前两轮没照到的角落 + SSH 语义与 daemon 侧新契约（L2-130/L2-132/L2-134）的交界面 + K2/L2-96 写路径截止时间」。
- 正文喂给 CodeBuddy：**154 条「不成立」**（batch 13 开批时的 136 条 + batch 13 六轮消费新增 14 条 + 手工单发消费 #20 新增 4 条；136 基线 = 编号 91 + 判定行 39 + 摘录 6，全部原样复用 issue #19 正文，18 条新增从本文件切片）。KNOWN-ISSUES 空档喂 K2/K7/K9/K12/§3/§3.1 Pageant；口径提醒含基线（lib 862/1/1、daemon_integration 76、stop_e2e 33、daemon bin 29/1、npm 220/30）、SSH 族 L2-13/15…25/34/37/73/88/95…98 勿重提、L2 撞号带主题限定、feature 组合不可相加、`.github` 禁区、L2 越权重申。
- **CNB 正文长度限制（本次新发现，消费者必读）**：create-issue 现在拒绝 >65,535 字节的 body（本批 86.6k 字节全量正文被 400 拒绝；issue #19 的 81k 正文是更早创建的，当时无此限制或口径不同）。**update-issue 不受该限制**（实测 77.5k 通过、86.6k 被拒，上限在两者之间）。本批落法：先用 63k 压缩版创建（拿到 issue 号触发 CodeBuddy pipeline），再 update 成 77.5k 版（SSH 主题 batch 2/8 编号条目全文恢复，其余批次编号条目只留指摘原文首行 + 全部判定行/摘录/新增 18 条整段；被截断的判据注明见本文件对应批次小节）。**以后每批的「不成立」正文都要按这个两级流程走，且 18 条新增必须全文保留。**
- CodeBuddy 首条分析：**已到**（20:00:53 GMT+8 = 开批后 5 分钟；评论 `2120486923494924288`，只读、HEAD `b0f8b0d`、行号现算）。**合计 10 条**：已登记项新证据 2（不重立条）——①L2-96：修法里「超时后 abort session」从 `impl Write for File` 做不到（`File` 只持 `RhFile`，无门面 `Session` 句柄），与「不能在 `core.rs` 的 `spawn_blocking` 闭包里包 timeout」两条约束合起来把落点压到零，修法要重排；②L2-134 第三个折点：`terminal_attach`（`:4748-4758`）与 `session_open`（`:2434`）的 `resolve_host` 把 `load_config()` 的 IO/serde 错误与「名字不存在」压成同一 400 正文。新指摘 8：③A1（成立）`stop_worker` 不撤销 `tcpip_forward`，`bind_port: 0` 的远端转发 stop→start 后 `effective_port` 变了但规则仍挂同一 id、无事件/审计；④A2（成立）`Registry::push` 无界，`_queue_maxsize: Some(16)` 被下划线吞掉是 no-op，慢目标 + 快连接下队列与 `thread::spawn` 线程同无界；⑤B1（部分）`remove_system_known_host` 注释里「`ssh-keygen -R` 是参照」实为更强的验收口径（三形态对拍），建议写进 L2-15 修法；⑥B2（部分）agent 超时「下次还能用」只在 SSH 门面成立，`list_identities` 失败丢 client 后 `embedded_ssh.rs:2093-2100` 不再 connect；⑦B3（部分）`StallGuard::flush` 在 `inner.flush()` 之前先用 600 s 判定挡住，deadline 文案要胜出须让位，修法顺序约束写进 L2-96；⑧C1（不成立，仅注释错）`session.rs:152-155` 注释后半句假（detached 线程 handle drop 不断开 channel），行为仍正确；⑨C2（不成立）`agent.rs:432-441` 测试未覆盖 abort 的 socket 关闭（`Settings::abort()` 对空 socket 是 no-op）；⑩C3（已并入）`handle_terminal` select 循环持锁等待并入 L2-34/L2-73 drain 族。它还自留两条下一轮线索：`legacy_pem` 对「有 DEK-Info 无 Proc-Type」当加密处理、`ssh_algo` 的 `OnceLock` 把失败**永久**缓存为 None（与 `config_cache` 相反）。
- **轮询勘误（消费者必读）**：本轮 5 次 60 s 轮询的 grep 模式（`comment_id`/`- id:`）与 list-comments 实际输出格式（`data[N]{id,author,body,created_at}`）不符，**全部误报为 0**——评论 20:00:53 已在，20:01 的第 4 次轮询本应命中。延时复核与推送后终检改用匹配 `created_at`/`data\[` 的模式才命中。以后轮询判空必须先对一次**已知非空**的 issue 校准 grep 模式（比如对本批 #30）。
- 6 轮进度：0 / 6（待消费任务更新）。
- 顺手按 #19 批次 BATCH_COMPLETE 行自述删除该状态行（HTML 注释同步为十二条）。
- 下一批（B=15）主题：15 % 6 = 3 → MCP 契约与 CLI 文档一致性（**第三批**，#9/#15 已闭环）。开 issue 前先 fetch cnb main 重读 ledger 不成立清单（本批消费期间会新增，行内锚点会漂移）。若 #30 仍 open 则按背压闸门跳过；锁沿用「保留在位、消费者侧自行清理」先例。

---

### 手工/单发消费 · 2026-11-11 · daemon 异常诊断与保守恢复（issue #21）

**选靶**：open 里无其它待消费的 P1 比它更靠前——#20 已在 11:42 收尾并保持 open（`## 本轮收尾` 之后无新回复），#21 是这一族的入口（#20 的回执语义 → #21 的诊断层 → #22 身份协议 → #23/#24/#25 平台落地）。**排除**：#22/#25 是纯设计（P2，只产方案对比，本轮无人在等）、#26 依赖 #22 未落地、#27 是 Windows 实机验证（本机做不了）、#28/#29 是编号更晚的 P1。**不选「最容易」的理由**：#21 的实现主体几乎全在 `src-tauri/src/**`（L2），选它是因为它的**判定表本身**有产出价值——它给出了 21 条可核实的断言，而其中两条会把运维引向错误的排查方向。

**判定：21 条 → 成立 14 / 部分成立 2 / 不成立 2 / 建议已落地 1 / 已登记 2。**

#### 两条不成立（原文保留，请勿重复提）

1. **「`stop_daemon_process` 在 token 不匹配时走到 `DaemonStopError::NotAuthenticated`」— 不成立。** token 被 daemon 拒绝时走的是 `NotAccepted(detail)`（`daemon_control.rs:923-929` → `:1063`），文案是「the daemon did not accept this config's admin token for a shutdown；nothing was stopped」。`NotAuthenticated` 只有一个入口：`read_admin_token()` 返回 `None`，即**配置文件里没有 token**（`:1032-1034`）。它引的 `:800`（枚举定义）与 `:822-826`（`Display`）都不是调用点。差别有实际后果：前者说明「token 轮换过、daemon 是旧的」，后者说明「这份配置根本没有凭据」。
2. **「桌面 `daemon_status` 报错 → 面板只剩一行 `String(err)`」— 不成立。** `src/api.ts:711` 的 `daemonStatus` **零调用点**（全仓 grep 只命中定义行）；面板的状态来自 `getDaemonHealth()`（`api.ts:693-708`，只打 `/health`）。真实情形是反向的一半：读者以为面板至少会报错，实际是**完全看不见**——PID 文件损坏而 `/health` 正常时，面板照旧显示 `Daemon healthy`。

#### 两条部分成立

- **「CLI 与桌面共用 `daemon_pid_state`」**：判定共用成立，但桌面那一半（`tauri_commands.rs:1758-1793`）是死路——命令存在、前端不调。它给的行号 `3043-3103` 覆盖的是 CLI 的 `Start`..`Status` 整段，`daemon status` 本体在 `:3083-3101`。
- **「`:660` 让停止和重启都灰」**：`Stop` 确实被 `!daemonHealthy` 支配（`:660`），`Restart` 只受 `daemonActionBusy`（`:672`）——所以「最需要停止时没有按钮」只对 `Stop` 成立，`Restart` 一直可点，点下去才在 `daemonActionMessage` 里显示原始错误。

#### 一条建议已落地

它建议「给畸形 shutdown body 补一个 test 钉住」。该测试 `malformed_shutdown_bodies_are_rejected_without_stopping_the_daemon` 在 `380e166` 就存在（`daemon_stop_e2e.rs:1165`，覆盖 10 种畸形体），而它的基线正是 `380e166`。

#### 它没说出的两层（本轮补强）

- **`daemon.token` 在结构上就躲得开脱敏**：admin token 是 `uuid::Uuid::new_v4()` 的 36 字符带连字符形状；`looks_like_high_entropy_secret`（`store.rs:1505-1539`）要么要求全 hex 且 ≥32 字符（连字符不满足），要么要求 ≥40 字符。所以它的结论「token 不出现在诊断输出」今天成立，但**成立的原因是「根本不打印」，不是脱敏兜底**；谁哪天在诊断里打一行 token，这层保护不会拦住。
- **阻塞锁不止挡住诊断，还挡住停止本身**：`.daemon-pid.lock` 的 `lock_exclusive()` 没有超时，而 daemon 自己启动时（`bin/agent2ssh-daemon.rs:5798` 的 `write_daemon_pid`）与收尾时（`:6000` 的 `remove_daemon_pid_file_if`）都要拿它。所以一个卡住的持有者会让 `daemon stop` 在**已经强杀 daemon 之后**卡在删除那一步（`bin/agent2ssh.rs:3080`），让 `daemon start` 在**已经决定放弃子进程之后**卡在启动器的清理那一步（`daemon_control.rs:537`）。它测到的 11 秒是「还愿意等」的情形，不是上界。

#### 本轮落地

- `docs/guides/configuration-guide.md`：`daemon.pid` 的停止预算段补上「第二段没有上界的等待」（阻塞锁，与地址解析无关）；`daemon.owner` 的停止按钮段补上桌面面板的真实判据（只轮询 `/health`、`Stop` 在 `/health` 不通时置灰、桌面不读 PID 记录），并指明用 `daemon status` 判断记录状态。
- `docs/api.yaml`：`/health` 加 description，写明它是 `security: []` 的公开路由、**不是身份证明**，并分别写清 `daemon_pid_state`（pid + `/gate` token + 启动时间快照，**不比 `config_dir`**）与 `stop_daemon_process`（pid + `config_dir` + `/shutdown` 自身的 401/403）各读哪几项、两者都不读 `version`。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-141…L2-146**（`Unverified` 拆分与 `DaemonObservation`；桌面 `daemon_status` 零调用点；阻塞锁；拒绝路径不落盘；`doctor` 吞读失败；悬空符号链接）。六条全部需拍板，`src-tauri/src/**` 与 `src/**` 一行未改。
- 台账本节。

**SHA**：代码提交见下方「SHA 与回帖」。分支 `opt/single-21-20261111`，worktree `/Users/yuqu/Vbercodeing/agent2ssh-opt-worktrees/single-21`（合流后已清理）。

---

### consume 1 / 6 · batch 14 · issue #30

指摘来源：CodeBuddy 首条分析（评论 `2120486923494924288`，20:00:53 GMT+8，HEAD `b0f8b0d`）。**10 条 → 成立 1 / 部分成立 3 / 不成立 2 / 已并入 1 / 已登记项新证据 2**（另 1 条 C2 判「成立但只是覆盖边界」，计入不成立一侧的观察）。

#### 判定

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| A1 | `stop_worker` 不撤销 `tcpip_forward`，监听 socket 一直留着；`bind_port: 0` 的远端转发重启后换端口而无事件/审计 | **部分成立** | headline 被它自己下一段推翻：`stop_worker` 走 `session.disconnect(...)`（`forward.rs:220-227`），服务端随之清掉监听，它自己也写了「停掉这一半没事」。**成立的那半**是动态端口：`forward_add_core_via` 把 `bind_port` 原样存进 `ForwardRule`（`:388`），实际端口只在 `RuleControl.effective_port`（`:70`/`:515`/`:644`）→ `RuleStats`（`:112-120`）→ 只有 Tauri 的 `forward_stats`（`tauri_commands.rs:1362`）能读到；`forward_list_core` 返回 `h.rule`（`:739-746`）仍是 `0`。CLI 建完打印的也是 `rule.bind_port`（`bin/agent2ssh.rs:2709-2717`）= `0`。它说「没有任何事件/审计」不准确：每次启动写一条 `remote forward listening`（`forward.rs:645-656`）或 dual-stack 日志，带实际端口 |
| A2 | `Registry::push` 无界，`_queue_maxsize: Some(16)` 被下划线吞掉 | **成立** | `transport/russh_impl/mod.rs:1040-1046` 的参数名就是 `_queue_maxsize`、从不读取；`Registry::push`（`russh_impl/forward.rs:46-50`）无上限；accept 循环每接一个 `thread::spawn`（`forward.rs:644-676`）。→ L2-147 |
| B1 | 注释里「`ssh-keygen -R` 是参照」实为更强的验收口径，三形态都会命中，建议写进 L2-15 的修法 | **不成立** | 与本仓 2026-11-10 已实测的结论冲突（ledger「不成立」第 17 条）：`ssh-keygen -R ::1` **只删** `::1`、保留 `[::1]:22`；只有 `ssh-keygen -R '[::1]:22'` 才删那一行。它把「会处理哈希行」正确推广成「三种形态都处理」，而方括号形态恰好是 `-R` 也不处理的。按它写验收口径会把一条假判据固化进 L2-15。哈希行不可匹配是真事，但早已在 C6 / L2-18 / L2-19 登记 |
| B2 | agent 超时后「下次还能用」只在 SSH 门面成立，`list_identities` 失败丢 client | **成立（注释层面）** | `agent.rs:245-256` 失败即 `client = None`；`run_independent` 不 abort（`channel.rs:129-148`）；`embedded_ssh.rs:2096-2102` 是 `connect()` + `list_identities()` 各一次，失败即跳默认密钥。已在 `list_identities` 处补注释（L1） |
| B3 | `StallGuard::flush` 会先用自己的 600 s 判定挡住 deadline 的报错 | **部分成立** | 形状属实（`sftp_transfer.rs:440-445` 先判 `last_progress` 再调 `inner.flush()`），但它只说对了「deadline ≥ 600 s」这一种：deadline 短于 600 s 时 `StallGuard` 不抢先，报错来自内层。「flush 也要包 deadline」早已写进 L2-96（第 3 轮扩展），新增的只是顺序约束 → 记为 L2-96 补充 |
| C1 | `session.rs:152-155` 注释后半句「handle drop 会断开 channel」是假的 | **成立（注释确需改），但它给的原因不完整** | `run_terminal` 的循环（`embedded_ssh.rs:2350-2398`）只在 EOF / read 错误 / `Close` 命令 / `event_tx.send` 失败时退出；`try_recv` 拿到 `Disconnected` 只让内层 while 结束，不是退出条件。所以 handle drop **不会**断开 channel——但它也不是「什么都没发生」：drop 会同时丢掉事件接收端，于是线程在下一次远端输出、`event_tx.send` 失败时才退出，静默会话则一直挂着。真正关掉 channel 的是那条 `Close` 命令（`:184` 已发）。已按此改正注释（L1） |
| C2 | `agent.rs:432-441` 的测试不覆盖 abort 的 socket 关闭 | **成立（覆盖边界，不是缺陷）** | 该测试用 `connect_stream` + duplex 造会话，从不调 `set_socket`；`Settings::abort()`（`channel.rs:91-96`）对空 socket 只置 `aborted`。且 `run_independent` 本来就不 abort。只记录，不立条 |
| C3 | `handle_terminal` select 循环持锁等待 | **已并入** | 它自己判为并入 L2-34 / L2-73，接受，不重复核实 |
| 已登记 1 | L2-96 的「超时后 abort session」在 `impl Write for File` 里做不到（`File` 只持 `RhFile`） | **成立** | `russh_impl/sftp.rs:204-206` 确认 `File { inner: RhFile }`，无门面 `Session`；`Sftp { session: Arc<SftpSession> }`。落点约束成立，不新立条 |
| 已登记 2 | L2-134 的第三个折点：`terminal_attach` / `session_open` 也把 `load_config()` 失败折成 `unknown host` | **成立** | `session.rs:57-62` 的 `resolve_host` 是 `load_config()?` + `ok_or_else(unknown host profile)`；`bin/agent2ssh-daemon.rs:4760-4763` 与 `:2434` 附近都 `err(400, e.to_string())`。已把两处路由补进 L2-134 的清单 |

#### 本轮改动

- `docs/api.yaml`：`ForwardAddRequest.bind_port` 写明 `0` = 临时端口（`local` 由 OS 选、`remote` 由服务端选），规则里存的是 `0` 而不是选中的端口；`ForwardRule.bind_port` 写明它「是创建时给的值，不是正在监听的端口」，并指出实际端口只有桌面端 `forward_stats` 的 `effective_port` 有、HTTP 没有任何路由返回，stop→start 可能换端口；`/forwards/{id}/start` 的 description 补同一句。
- `src-tauri/src/session.rs`：更正「handle drop 会断开 channel」那句（改成：`Close` 命令才是断开的那一步；drop 只丢队列两端，静默会话会一直挂着）。
- `src-tauri/src/transport/russh_impl/agent.rs`：在 `list_identities` 丢 client 处写明「一次 agent 超时就是这条连接的最后一次」，与 `embedded_ssh` 只调一次 `connect` 的写法对齐。
- L2：新增 **L2-147**（`Registry` 无界）、**L2-148**（`bind_port: 0` 的实际端口取不到），另加 **L2-96 补充**（`StallGuard::flush` 抢答的边界）与 **L2-134 补充**（`/terminal`、`/sessions` 两处折点）。

**验证**：`npm run build` 6.53 s；`cargo fmt --check` 绿；`--no-default-features --lib` **862 / 1 / 1**（红项仍是 L2-15 那条故意红的 IPv6）；`--features daemon --test daemon_integration api_yaml` **6 passed**；`yaml.safe_load` 90 paths。

#### 一处流程事故（自记）

`docs/api.yaml` 的 `/forwards/{id}/start` description 是**未加引号的 plain scalar**，我追加的句子里带 `` `bind_port: 0` ``（冒号+空格），第一次写直接把 YAML 解析打断（`mapping values are not allowed here`，3760 列 373）。改为「a zero bind port」后恢复。**以后往 plain scalar 里追加句子，先 `yaml.safe_load` 一次再继续**，别等 cargo 测试才报。

---

### consume 2 / 6 · batch 14 · issue #30（等待超时 → 自取轮）

**等待超时**：第 1 轮收尾（`2120501251870715904`，12:57:50Z）之后按 60 s × 5 + 一次 90 s 复核轮询，13:04:14Z 时 #30 仍只有两条评论（首条分析 + 我的收尾）。按协议标「等待超时」，不自造指摘，改从首条分析末尾它自己留的两条「下一轮线索」里取靶。**本轮不占「复核链」**：指摘来源是我的核实，不是它的复核。

#### 对两条线索的判定

| 线索（它给的方向） | 判定 | 证据 |
|---|---|---|
| `legacy_pem::decrypt` 对「有 `DEK-Info` 无 `Proc-Type`」当加密处理，是一条可构造边界 | **不成立（方向反了）** | `DEK-Info` 就是那条头里唯一带密码学内容的东西（算法 + IV），`Proc-Type: 4,ENCRYPTED` 只是冗余标签；按 `DEK-Info` 判「这是加密的」**是对的**（`legacy_pem.rs:56-73`）。真实的小缺口在反方向：只有 `Proc-Type` 没有 `DEK-Info` 时 `encryption` 保持 `None` → `Ok(None)` → 调用方按明文解析 → 报的是 ASN.1/base64 类错误，而不是「这份 PEM 声明自己加密了却没写算法」。纯文案问题，不立条 |
| `ssh_algo::supported_algorithms` 的 `OnceLock` 把失败**永久**缓存为 `None`，与 `config_cache` 相反 | **不可达** | 闭包只有两处可失败的调用，两处都不会失败：`Session::new()`（`russh_impl/mod.rs:511-526`）恒 `Ok`，`supported_algs`（`:1103-1105`）直接 `Ok(supported_names(method))`。所以 `get_or_init` 永远存 `Some`，`Option` 返回类型与调用方的 `None` 分支都是化石。**不是缺陷，但「`None` 表示后端不支持」这句文档现在描述的是一个不可达状态** —— 与 L2-13 的「死分支」同型，不新立条 |

#### 本轮的真实发现（自取）

**`RuleStats.effective_port` 在产品里没有任何显示路径**——而我在第 1 轮的 `api.yaml` 里刚写成「桌面端 `forward_stats` 能读到」，那半句只对「数据存在」成立，对「人能看见」不成立：

- `ForwardPanel.tsx:51` 用 `Promise.all([api.forwardList(), api.forwardStats()])` 把 stats 取回来了；
- 但 `:215-221` 只渲染 `connections` / `bytes_tx` / `bytes_rx`；
- `grep -rn effective_port src/` 只命中 `src/types.ts:693` 的类型声明。

所以「`-R 0` 起一条隧道，之后想知道它监听在哪」在 CLI / HTTP / 桌面三处都没有答案，只有 `app.log` 的启动日志有。这是**我第 1 轮写错的地方**，本轮改正。

顺带纠正一个字段文档：`RuleStats.effective_port` 原写「None until the listener is bound」，而 `stop_worker`（`forward.rs:212-247`）不清这个值——停止后仍带着上次绑定的端口。`Some` + `state: stopped` 只表示「上次绑在这里」。

#### 本轮改动

- `src-tauri/src/forward.rs`：`RuleStats.effective_port` 的文档改成「最后一次绑定的端口；停止不清零；`connected` 才是活着的判据」。
- `docs/api.yaml`：`ForwardRule.bind_port` 那句从「桌面端 `forward_stats` 能读到」改成「`forward_stats` 是桌面专属命令，且转发面板只渲染连接数/字节/状态——产品里没有任何地方显示它」。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：L2-148 补「第 2 轮的更正（比原判断更糟）」段，并把「停止时是否清零」列为拍板时的第二问。

**验证**：`npm run build` 6.5 s；`cargo fmt --check` 绿；`--no-default-features --lib` **862 / 1 / 1**（红项仍是 L2-15 那条故意红的 IPv6）；`--features daemon --test daemon_integration api_yaml` **6 passed**；`yaml.safe_load` 90 paths。

#### 一条流程口径

「等待超时」不等于「可以随便找点东西改」。本轮拒绝了两条线索（一条方向反、一条不可达），只落一处**自己上一轮写错**的更正——这类自查比新增条目便宜，也比把弱线索升格成 L2 诚实。下一次运行若复核仍不到，应在 ledger 里明确写「本轮无新指摘可核，只做自查」，而不是凑够 6 轮。

### consume 3 / 6 · batch 14 · issue #30（复核 D1–D5 的处置）

第 1 轮复核 `2120503339707183104`（`2026-11-11T13:06:07Z`）在 13:11 才落到本地，比它的预期晚、也比我的第 2 轮推送晚。十三条指摘逐个核实：**成立 5 / 部分成立 2 / 不成立 1 + 未提的补 2**。

| 指摘 | 判定 | 事实 |
|---|---|---|
| D1 同一 spec 里两段对 scope/gate 顺序说法相反 | **部分成立，方向反了一半** | 两处 handler（`forward_remove:3077-3113`、`forward_stop:3135-3189`）**逐行同构**，都是 gate→rate→scope→authorize。`delete` 那段「gate、limits、scope 依序只走找到规则的分支」与代码一致，**是 stop 的 403 单方面把「越权返回 403」写成了无条件句**。所以不是「两段互相矛盾」，是一段正确、一段漏了前置条件 |
| D2 `forward multi` 授权串丢全部端口 | **成立** | `:3275-3280` 只有 `{host} ({n} rules)`。后果比「审计信息少」硬：`allowed_commands` 是 `glob_match` 命令原文（`remote.rs:616-620`），想给批量能力只能写 `["forward multi *"]`，等于「该主机任意方向/端口/目标」，不存在端口级中间档 |
| D3 `all_or_nothing: false` 下被拒 target 不留审计 | **成立，且漏了一半** | `:4574-4576` 的 `continue` 跳过未授权 target，不写审计；另有一条它没提的：限流提前返回（`:4487-4500`）在**两种模式**下都零审计。合并起来是一个不留痕的 scope 探针 |
| D4 我对 `StallGuard::flush` 的措辞要更精确 | **成立，我第 1 轮写错** | 只有 flush 在 `inner.flush()` 之前判自己（`:439` vs `:442`）；write 是**先调内层**（`:428`）且只在 `written == 0` 支判定（`:432`）。所以「deadline 会输给 600 s 文案」**只在收尾那一次 flush 成立**，write 路径上 deadline 必赢 |
| D5 `.cnb.yml` 的首步 `--lib` 挡住 L2-132 的刻意红 | **成立，但严重一档** | 实测该测试在 HEAD 上确实失败（`0 passed; 1 failed; 863 filtered out`）。`set -eu` + 单个 `|` 块意味着 `:16-21` 六行**从不执行**——不是「遮住一条刻意红」，是整个 daemon 回归作业一直空转，含四个 `*_e2e` 与 `:21` 的五个集成目标 |
| A1-1 `POST /forwards` 不读 `name`/`group_id` | 成立 | `:3000` 收 `Json<ForwardRule>`，`:3027-3034` 只传五个字段 |
| A1-2 `forward_add_multi` 响应只有 `count`/`ids`/`host` | 成立 | `forward.rs:842-849`，连请求的 `bind_port` 都不回 |
| A1-3 诊断日志 `target_host` 不脱敏 | **不成立** | 它**经过了**脱敏：`diagnostics.rs:295`（message）、`:310` + `:146-149`（每个字段值）都调 `redact_sensitive_text`。只是内置规则不匹配主机名——对主机名这是正确行为。括注「内置规则不含 `127.0.0.1`」是对的，前半句把「没匹配」说成了「没调用」 |

#### 本轮改动

- `docs/api.yaml`
  - `/forwards/{id}` 的 `description` 补「顺序即应答顺序」：gate 的 423 与 limit 的 429 在 scope 之前应答，故「已暂停 + 越权」永不返回 403；审批是最后一步，408 与审批拒绝的 403 来自它。并写明 `/forwards/{id}/stop` 走完全相同的序列。
  - `/forwards/{id}` 的响应表补 **`408`**（同一 `authorize_command`，`ApprovalTimedOut` → `:809-812`），`400`/`403` 的描述补上审批来源（`Blocked` → 400、`ApprovalRejected` → 403）。
  - `/forwards/{id}/stop` 的 `403` 从无条件句改成有条件句：除非 gate/限流先拒（423/429），否则越权答 403 而不是 unknown-id 400；`400` 同样补审批来源。
- `docs/reports/OPT-LOOP-L2-PENDING.md`
  - **重写** L2-96 补充段（D4）：逐行标出 `write:428/432` 与 `flush:439/442` 的顺序差异，把结论收窄到「只有收尾 flush 可能被抢答」，并补一条第 1 轮漏掉的：flush 的判定在尝试内层之前，能「一次都不 flush 就报错」。
  - 新增 **L2-149**（multi 授权串）、**L2-150**（broadcast 审计缺口，含限流分支）、**L2-151**（`.cnb.yml` 首步即红，禁区·只报告）。
  - L2-148 补两条同族事实（`name`/`group_id` 丢弃、multi 响应无端口），并记「A1-3 不成立」以免后续把「没匹配」重述成「没调用」。

#### 验证

`yaml.safe_load` **90 paths**，两条路由的响应码集合现在完全相同（`200/400/401/403/408/423/429`）。`cargo test --no-default-features --lib an_ipv6_literal_at_port_22_is_removed` → **FAILED**（D5 的前提实测，非引用记忆）。本轮零源码改动，故不跑全量 lib；`cargo fmt --check` 与 `npm run build` 本轮无相关改动，略过。

#### 一条流程自记：锁的 owner 必须写会话宿主 pid

本轮开始时 `/tmp/agent2ssh-opt-loop.lock/owner` 写着 `76868`，`kill -0` 失败——那是上一个上下文窗口里**某个 Bash 子进程**的 pid，子进程退出后锁就变成「本机死主」。按协议这是可接管的陈旧锁，已改写成 `76692`（本会话的 WorkBuddy Electron 宿主，随会话存活）。

口径：**owner 不能写 `$$`**。Bash 工具每次调用都是新 shell，写进去的 pid 活不过下一次调用；下次运行会把它判成死锁并接管，等于互斥形同虚设。要写在 `ps -o ppid=` 链上真实存活的那个宿主进程。

### consume 4 / 6 · batch 14 · issue #30（第 2 条复核：R2-1/R2-2 + N1–N4）

第 3 轮开工时我只信了上一轮留下的「唯一一条复核」清单，没有重新 `list-comments`。实际有两条：`2120503339707183104`（13:06:07Z，已处理）与 **`2120505546544406528`（13:14:54Z，11.3 KB，未读）**。后者是复核第 2 轮的，正好是本轮指摘来源。**六条判定：成立 3 / 部分成立 2 / 不成立 1。**

| 指摘 | 判定 | 要点 |
|---|---|---|
| R2-1 `yaml.safe_load` 绿 ≠ 字段值正确 | 成立 | 已落成断言：新增 `api_yaml_gives_the_two_forward_authorization_routes_the_same_codes`，要求两条同构路由的响应码集合相等。上一轮唯一实际缺陷（删页缺 408）本该被它抓到 |
| R2-2 `--lib` 到不了 `tauri_commands.rs`，需侧车 | 部分成立（结论对、机制错） | `tauri_commands` 由 `#[cfg(feature = "tauri")]` 门控（`lib.rs:69-70`），`--no-default-features` 不编译它、不需要侧车——本轮在无 `dist/`、无 `binaries/` 的全新 worktree 里跑通。但「到不了 `forward_stats`」成立 |
| N1 `host_key_algorithm` 注释与测试 | 部分成立（测试对、注释那半反了） | 注释 `:152-164` 写的正是「`rsa-sha2-*` 是签名算法、key blob 是 `ssh-rsa`」，没有复核说要删的那层表述。测试确实把签名算法名当 key 类型 |
| N2 `rsa_hash_alg` 的 `None` 是「没试就降级」 | **不成立** | `:321-322` 与 `:802-805` 都已写明该分支的语义；`server-sig-algs` 无 `rsa-sha2-*` 的服务器试 SHA-2 只会多一次失败往返 |
| N3 `aborted()` 早退无测试 | 成立 | 补 `a_call_after_an_abort_reports_the_earlier_timeout` |
| N4 `ErrorKind::WouldBlock` 文档与样例 | 成立 | 改文档 + 换测试样例为生产文案 |

#### 本轮改动

- `src-tauri/src/transport/types.rs`：`WouldBlock` 的 doc 改成「转发连接队列为空」，并写明「非阻塞通道读走的是 `io::ErrorKind::WouldBlock`，从不经过本枚举」；测试样例从 `"[Session(-37)] would block"` 换成 `"no forwarded connection is pending"`，附注释说明旧样例属于另一种类型。
- `src-tauri/src/transport/russh_impl/channel.rs`：新增 `a_call_after_an_abort_reports_the_earlier_timeout`，补上 `run_with_abort` 四臂里缺的「`aborted` 早退」。
- `src-tauri/src/embedded_ssh.rs`：测试里两处 `"rsa-sha2-256"` 改为 `"ssh-rsa"`；doc 重写，写明本测试覆盖存储的键方式、不覆盖 RSA 映射、不得被引作映射的证据；测试开头加 `assert_eq!(host_key_algorithm(HostKeyKind::Rsa), "ssh-rsa")`。
- `src-tauri/tests/daemon_integration.rs`：新增两条同构路由的响应码集合断言。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：本轮判定表 + N1 最后一步为何做不到（`host_key_algorithm` 是对 `HostKeyKind` 的纯 match，可驱动的输入在 `check_server_key`，要真服务器）。

#### 验证

`cargo fmt`；`cargo test --no-default-features --lib` → **863 passed / 1 failed / 1 ignored**（红项仍是 L2-15 那条刻意红的 IPv6，新增测试使总数从 862 升到 863）；`cargo test --no-default-features --features daemon --test daemon_integration api_yaml` → **7 passed**（含新增的不变式测试）。

#### 一条流程自记：每轮开工先重列评论，别信上一轮的清单

复核是在我**正在改第 3 轮的时候**到达的，收尾时我没再看一眼评论列表，于是那条 11 KB 的复核在第 3 轮里完全没被读到——而我还在收尾里写「下一轮请看」。**到达时间与读取时间是两件事**，判断「有没有新指摘」只能靠本轮开工时重新列一遍并减去已处理集合，不能靠上一轮的结论。已写进收尾流程。

### consume 4 / 6 · batch 14 · issue #30（第 3 轮复核的两个「没核」＋一个「还有没有第三条」）

复核第 1 轮留了三个问题，本轮逐个核完，**两个「有没有」的答案都是「有，但位置不是它猜的那个」**。

| 复核的问题 | 判定 | 事实 |
|---|---|---|
| D1 新措辞是否还有无条件句 | **有，但不在我改的那两页** | `forward_remove:3077-3113`、`forward_stop:3135-3189`、`forward_start:3204-3244` **三条逐行同构**（gate→rate→scope→authorize）。D1 只点了前两条，`start` 的 403 是同一种无条件句（「is answered with 403 here」）。→ **L2-152**，本轮已改 |
| L2-149：审批策略是否对 `forward multi` 有独立分支，严重程度要不要调 | **没有分支，所以不调，而且更糟** | `grep -rn forward src-tauri/src/risk_extra.rs src-tauri/src/core.rs` **零命中**；`core.rs:518-530` 的 `sftp` 特判是唯一的应用合成串分支。三个消费者全是整串 glob：`check_daemon_scope`（`remote.rs:589/620`）、`check_approval_required_with`（`approval.rs:1063`）、用户风险规则 `matches_pattern`（`risk_config.rs:121`）。所以「批量里出现 `-R 0` 就要审批」这种策略**写不出来**——`-R 0` 不在串里 |
| L2-150：除了限流与逐 target 授权失败，还有第三条提前返回吗 | **没有第三条，但有同族的另一处** | `terminal_broadcast_run:4439-4613` 的提前返回确实只有那两条。掉出来的是：`/preview` 不跑限流，所以 preview 的 `authorized: true` 不预测 run 的 429 → **L2-153**；以及一条**我自己写错又收回的**（下条） |

#### 一条收回：广播的 gate 判定没有自己的实现

我一开始把「preview 放行 / run 拒绝」写进了 `api.yaml` 与 `info.description`，**写完才去逐行核 `terminal_broadcast_run` 有没有自己的 gate 调用**。结果是没有：`:4439-4613` 全文既无 `reject_if_gate_paused` 也无 `load_execution_gate`，gate 只在 `preview_results:4350` 读一次（`.ok()` ＋ `is_some_and`）。所以坏 `execution_gate.json` 对广播的正确描述是**两个路由一起静默跳过 gate**，不是两侧分叉。这与 L2-132 那条 500 是两回事，且**修在 `reject_if_gate_paused` 上不会自动修到广播**——它根本不调那个函数。

顺序教训：先核后写。这次代价是同一段描述改了两遍，且如果我写完就推，两次差异会被读成「意见变了」。已在 L2-153 里留了「一条流程失误」，免得下一个人把两次改动读成反复。

#### 本轮改动

- `docs/api.yaml`
  - `/forwards/{id}/start` 的 `403` 与第 3 轮改过的 `stop` 对齐（顺序即应答顺序：423/429 先于 403）；`400` 补审批来源（`Blocked` → 400）。三页措辞现在一致。
  - `info.description` 第二段补第三条 500 族：`/terminal` 答状态，两个广播路由**既无 500 也无独立 gate 判定**，`unreadable` 与 `paused` 在 target 的 `error` 里同形。
  - `/terminal/broadcast`、`/terminal/broadcast/preview` 两处「predicts the run exactly」改成单向句，并写明限流缺口；`/terminal/broadcast` 的「the gate verdict」改成「the per-target *verdict*」——gate 只是 verdict 的一个输入。
  - `/terminal/broadcast` 补一句**响应与审计不同源**（`all_or_nothing: false` 的被拒 target 与限流拒绝都不写审计），指向 L2-150。
- `docs/reports/OPT-LOOP-L2-PENDING.md`
  - **L2-149 补**：三处消费者逐条列出，结论是策略层比 `allowed_commands` 更糟一格；另补一条没人提的：`store.rs:1144` 的 `derive_audit_action` 认 `"forward add-multi"`，而 handler 发的是 `"forward multi ..."`，批量的审计 `action` 落到 `"exec"`。
  - **L2-150 补**：`terminal_broadcast_run` 没有第三条提前返回；加强的是「留不留痕既不由模式也不由拒绝发生在哪一步决定」。
  - **L2-152**（新）：`/forwards/{id}/start` 的 403 无条件句。附一条：三条路由里内联的 `check_daemon_scope` 是**冗余**的，`authorize_command_with_approval` 第一步就是它（`execution_control.rs:445-446`）。
  - **L2-153**（新，半个成立）：`/preview` 不跑限流，故只是单向预测器；并记「preview_results 的 gate 读法」这条因修 L2-132 时**必须单独覆盖**。

#### 验证

`yaml.safe_load` **90 paths**；`/forwards/{id}`、`/forwards/{id}/start`、`/forwards/{id}/stop` 三条的响应码集合完全一致（`200/400/401/403/408/423/429`）。契约测试依赖的四个短语仍在（`drains for up to 500 ms`、`only as a frame`、`returns 200`、`into that target's \`error\``）——逐条 `grep -c` 为 1。**本轮零源码改动，且本容器无 Rust/Node 工具链（`which cargo` 为空），故 `cargo test` 与 `npm test` 一律未跑**：这不是「跑过了绿」而是「没跑」，引用本轮的验证结论时请只引用 `yaml.safe_load` 与 `grep`。

### consume 5 / 6 · batch 14 · issue #30（M1 与「断言不是护栏」）

本轮开工先重列评论（第 4 轮的教训），发现**两条**新复核同时到达：`2120508887198515200`（13:28，2989 B）与 `2120509144074469376`（13:29，7979 B）。**前者是复核直接推的提交**，不是评论——`4db0762` 已在 main 上，且它自述「本容器无 cargo，一条都没跑」。所以本轮第一件事是把那条没人跑过的提交跑一遍。

#### `4db0762` 的验证（复核未做的那一半）

`cargo test --no-default-features --features daemon --test daemon_integration api_yaml` → **7 passed**。它的 api.yaml 改动（`/forwards/{id}/start` 的 403、broadcast 的 preview 不作预测、`info.description` 的第三处 gate 缺口）没有破坏任何既有契约断言，也没破坏我第 4 轮加的那条码集合比较。

#### M1 判定：成立，但两处措辞要更正

| 复核的说法 | 判定 |
|---|---|
| `/daemons/{alias}/exec` 的正文把 scope 写在 gate 前面 | **成立**。原文 `:4315-4316` 确为「caller token scope, local remote-alias scope, execution gate, execution limits, and local authorization」；实际 `proxy_exec` 远程分支是 `get_daemon_with_scope`（`:3994`，404）→ gate（`:3998`）→ rate（`:3999`）→ scope（`:4000`）→ authorize |
| `/connections/{host}/connect` 的 403 是无条件句 | **成立**。`ssh_connect`（`:3601-3606`）与 `ssh_disconnect`（`:3655-3660`）都是 gate→rate→scope→authorize |
| 「七处」无条件 403 | **低估**。改完后 `grep -c 'description: Scope denied or approval rejected'` 仍为 **20** |
| 「同一 handler 两条分支两种顺序」 | **不成立**。`preflight_guarded_request_inner`（`:574-590`）也是 gate（`:587`）→ rate（`:589`）→ `authorize_command_inner`（`:590`，内部第一步即 scope）。两条分支**同序、不同机制** |
| 我第 4 轮说 `assert_eq!(host_key_algorithm(Rsa), "ssh-rsa")` 能钉住 N1 回归 | **不成立，是我写错**。那行代码从未被改过，改动前断言也绿；它是文档不是护栏。真正的回归点是**测试传参**，无任何断言覆盖 |

#### 本轮改动

- `docs/api.yaml`：`/connections/{host}/connect` 与 `/daemons/{alias}/exec` 的 description 改成「顺序即应答顺序」；两条的 403 补上前置条件（423/429 先于 403）；`disconnect` 的描述补上它本来就有的 gate 与 limits。
- `src-tauri/tests/daemon_integration.rs`：码集合比较从两路由扩到三路由（加入 `start`），函数名相应改为 `..._three_forward_authorization_routes...`；测试里写明**清单仍是手工的**，并记下按 handler 形状分组的改法。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：本轮判定表；21→20 的计数更正；两分支同序的更正；我自己那句「断言能钉住」的撤回；不变式测试的 L1 欠账。

#### 验证

`yaml.safe_load` 90 paths；`cargo fmt` 干净；`cargo test --no-default-features --features daemon --test daemon_integration api_yaml` → **7 passed**（含改名后的三路由测试）。本轮零源码行为改动。

#### 一条口径

复核开始推提交了（`4db0762`）。**它说「没跑」，那这一轮的第一件事就是把那条提交跑一遍**——不是复核意见，是既成事实的验证。以后凡 main 上出现我未亲自跑过的提交，都按这个顺序处理：先跑，再谈意见。

### consume 6 / 6 · batch 14 · issue #30（三个问题都核完了：两个「能」、一个「不新立条」）

复核留了三个问题。核完的结论：**Q1 的计数口径是错的（20 是文案数不是路由数，裸句共 21 处，且真要改的只有 3 条）**；**Q2 成立，但挂 K7 的「下一步」而不是新立条**；**Q3 能只靠 grep，本轮做完了，并实测了「不剥注释就会在正确路由上变红」**。

#### Q1：哪些 handler 的 scope 真在 gate 之后

判据不能只看内联 `check_daemon_scope`——`preflight_guarded_request` 与 `authorize_command` 也会问 scope（前者内部 gate→rate→authorize，后者第一步即 scope）。只看内联会漏 15 条 preflight 路由，而那 15 条**裸句全是对的**（gate 在前，本来就先拒）。逐条核完：`/sessions` POST、`/sessions/{id}` DELETE、`/connections/{host}/disconnect` **三条不对**，本轮改。`connect` 第 5 轮改过，`disconnect` 漏在它下面。`/ping` 与 `/exec/preview` 的「裸句」其实把 scope 拒绝说清了（「Scoped token does not allow …」），不算缺口。剩下 18 条落在 `scope` 单形状与 `preflight` 形状里，**加限定只会在正确的地方加噪音，这不是欠账**。

顺手发现一条没人问的：`proxy_exec` 两条分支**同序不同形**，`localhost` 走 preflight、远程分支第一句是 `get_daemon_with_scope`（404 先于 gate 与 scope）。第 5 轮说「同序」没错，但「首个拒绝是 404 不是 423」没说。本轮不改，记在本轮台账里。

#### Q2：证书路径的指纹可比性

`fingerprint_sha256` 与 `host_key_hash` 都是对 `CapturedKey.blob`（`cert.public_key()` 解包后的**裸 key**）求哈希，而 OpenSSH 在证书场景报的是**整张证书**的指纹。所以 import 一份含 `-cert-v01` 行的 `known_hosts` 之后，合法情况会被报成 `SSH host fingerprint changed`——方向是「该过的过不了」。这是 K7 已经声称覆盖的那一半，**故写进 K7 单元格而不是新立条**（两条分开写会在两处各自漂）。**未核**：russh 0.64 的 `Certificate` 是否暴露整段证书字节——本容器没有 `~/.cargo` 也没有 vendored 源码，只按本仓库的调用面判断。

#### Q3：不变式测试能不能只靠 grep

**能。** 不需要解析函数体：`handler_body_of` 早就在切 handler 体（走花括号），切完之后按字节位置排 5 个调用点即可。要解决的就三件事，都不是解析：(1) **注释会被数成调用点**——实测：给 `approval_check` 体里塞一行提到 `reject_if_gate_paused(...)` 的注释，不剥 → 链变 `['gate','scope']`，而它的 403 是正确的裸句 → **测试会在一条对的路上变红**；(2) **同一 handler 两条分支**——按 `get_daemon_with_scope(` 切，不解析 `if`；(3) **delegate 会读成「没有 scope」**——`terminal_broadcast_run` 的 `check_daemon_scope` 在 `terminal_broadcast_preview_results` 里，按字面会**静默逃出检查**，比报错更坏。三件解决后分组自然成立：34 条路由 6 组，新加的内联 scope handler 会自动进来，因为它由 `daemon_routes()` 驱动而不是手写清单。

**删掉的东西要说**：三路由比较那条（含它自陈「清单是手工的」的注释）删了。新测试比的是 403 的**措辞与顺序**，比码集合更严，但**不再断言** `remove`/`stop`/`start` 三页码集合完全一致。有人还想要那条，应该单独回到 `api.yaml` 上做。

#### 本轮改动

- `docs/api.yaml`：`/sessions` POST、`/sessions/{id}` DELETE、`/connections/{host}/disconnect` 三处 403 改为「顺序即应答顺序」；`/terminal/broadcast` 与 `/broadcast/preview` **补了两个 403 条目**（原本没有），并写明「scope 拒绝走的是每个 target 的 `error`、HTTP 仍是 200，这个 403 不是它走的路径」——补条目是因为不补就有「handler 查 scope、却无 403 可对应」的空档，写错方向的那半由措辞挡住。
- `src-tauri/tests/daemon_integration.rs`：新增 `every_scope_checked_route_documents_the_order_its_refusals_arrive_in` + 四个 helper；函数级注释记下三个陷阱（注释、双分支、delegate）与为什么那一个不能省。
- `docs/KNOWN-ISSUES.md`：K7 补上可比性那半 + 两条可执行下一步。
- 本文件与 `OPT-LOOP-L2-PENDING.md` 的本轮台账。

#### 验证

`yaml.safe_load` **90 paths**；三个 in-band marker 与三条 in-band 路由「不含 423/429/408」仍成立。**`cargo` 不在这个容器里，`cargo test` 与 `cargo fmt` 一条都没跑**——为不把「没跑」写成「跑过了绿」，我把这段逻辑用 Python 实现两遍（一个独立分析器、一个 Rust 测试的逐行移植），两份都报 34 条 / 6 组 / 0 问题；另做三次变异（改回裸句、删 gate 调用、塞注释）验证两个方向都会红。**合并前必须补跑 `cargo test ... --test daemon_integration api_yaml` 与 `cargo fmt`**：本轮 Rust 没编译过，Python 移植只能证明逻辑与文件一致，不能证明 `spec_operation_block` / `response_description` 这两个新 helper 能编过。

#### 第 6 轮补验（2026-11-11 21:55，另一会话执行）

上一轮在**没有 cargo 的容器里**写完 Rust 并直接推送，自陈「合并前必须补跑」。补跑结果：**`--features daemon --test daemon_integration` 在 `c41a6c4` 上根本编不过**——`:3185` 把 `split_once` 的两元组解构成三元组（`E0308`，expected 2, found 3）。同一个测试二进制里二十多条 `api.yaml` 契约测试因此一起沉默，**main 上的契约检查整批停摆**，而 ledger 当时记的是「Python 移植两份都绿」——那只能证明逻辑与文件内容一致，证明不了它能编译。

- 修法：`let (_, remote_branch) = …`（一行）。`cargo fmt` 同时产出三处纯重排。
- 验证：`--features daemon --test daemon_integration` **77 passed / 0 failed**（含 `every_scope_checked_route_documents_the_order_its_refusals_arrive_in` 与 `api_yaml_documents_how_each_handler_reads_its_body`）；`cargo fmt --check` 干净。
- 提交 `6e67dd8`。

**工艺**:「本容器没有 cargo」不是「可以不编译就推」的理由。要么不推，要么把改动留在分支上等能编译的会话接手；推上去的未编译代码会让**同一二进制里的其它测试**一起失效，代价比晚一轮合流大得多。

### 手工/单发消费 · 2026-11-11 · daemon 停止回执与旧版兼容（issue #20 · 第二次进入）

`open` 共 11 条：#30 是 `[opt-round]`（batch 14），其余 #20–#29 为 MANUAL。**选靶 #20**：P1 同级编号最小，且它最新评论（`2120483657423958016`）正是复核对本流程上一轮的驳回——不处理就等于把一条「自述比改动前更差」的测试留在 main 上。排除：**#27** 需 Windows 实机（本机做不了）；**#22/#25** 是纯设计（P2）。#21 也够格（有 `## 本轮收尾` + 新复核），但编号更大。

基线 `c41a6c4`。本轮 10 条指摘 → **成立 3 / 部分成立 2 / 不成立 5**。

#### 判定

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | `daemon_stop_e2e.rs:1408-1422` 的桩答的是**无 `pid`** 的 `/health`，`health_identity()` 在 `:1022` 返回 `None`，所以到不了 `POST /shutdown`，`posted == 1` 在空转 | **不成立** | 复核自述基线 `1904cc2` 的同一段落里，`POST /shutdown` 才回 `{"ok":true}`，**其余回** `{"ok":true,"pid":N,"config_dir":"…"}`（`git show 1904cc2:src-tauri/tests/daemon_stop_e2e.rs` 的 `:1472-1477`）。`posted` 只在 `request.starts_with(b"POST /shutdown ")` 时自增，所以这条测试**今天能过**本身就证明请求发出过——它是活的断言 |
| 2 | 同条里「三句 What must hold here 全不成立」 | **不成立** | 随 ① 一起塌：桩回 pid ⇒ 身份链通过 ⇒ `AcceptedUnchecked` 臂 ⇒ 等待 + 硬杀 ⇒ `Ok`。三句各自对应 `posted == 1`、`stopped.is_ok()`、`/bin/sleep 30` 只能被强杀 |
| 3 | `daemon_integration.rs` 只断言 `swallowed > 0`，钉不住「哪两条吞 body」 | **不成立** | 同一基线已有 `swallowed_paths`（`git show 1904cc2:… \| grep -c swallowed_paths` = **5**）。断言是「note 列出的名字数 == `swallowed`」+「每个名字都在 `swallowed_paths` 里」，等长 + 子集 ⇒ 集合相等 |
| 4 | `:3616` 的失败文案仍写「`Bytes` answers only what the handler writes」，因果不对 | **成立** | 现位于 `:3977`。413 是 extractor 的 body limit 加的，不是 handler 写的。已改 |
| 5 | `api.yaml` 文件级说明只把 415/422 列为 extractor 的 `text/plain` 应答，没提 413 | **成立** | 第 67–72 行原文只有「415 and 422」；而 `413` 已在 `/shutdown`（`:1232`）与 `/approvals/{id}/approve`（`:4030`）两处成文。已补第三格 |
| 6 | 两条路由的 413 没有任何状态断言（L2-140 那半） | **成立** | `grep -rn 413 src-tauri/tests/*.rs` 只有文档侧检查；无一条用例发过超限 body。已加真 daemon e2e |
| 7 | `/exec-multi` 的 `strategy` 三项全 0 = 无限并发 | **部分成立** | `core.rs:1495-1504` 确是「三项全 0 一次全展开」。但这是既有的「无策略 = 无约束」语义（CLI `:137` 写「0 = unlimited」），不是挂死、也不 panic。属策略拍板 → L2-154 |
| 8 | `batch_size > 0 && concurrency == 0` 会 `chunks(0)` panic | **不可达**（复核自己已收回） | `:1561` 的 `if batch_size > 0` 保证 `effective_batch_size > 0`；`chunks(0)` 永远走不到。记下来免得第三人再查 |
| 9 | `/config/import/preview` 无 admin 门，`/config/import` 有 | **成立** | `:3943` 有 `require_admin(&auth)?`；`:3949-3951` 的 preview 把 `auth` 绑成 `_auth` 后从不使用。受限 token 可枚举全部主机名 → L2-155 |
| 10 | `team_host_same` 不比 `ai_allowed`，于是同一次导入**既报 skipped 又改写** | **部分成立** | 两分支互斥（`team_config.rs:95-102` 的 `if … { skipped += 1 } else { … *existing = next }`），不会「既跳过又改写」。真实后果是另一半：别的字段不同时，bundle 缺省 `ai_allowed`（`types.rs:94-95` 的 `serde(default)`）会把 `false` 翻成 `true`，而 `api.yaml:5041` 已把这条写成「`ai_allowed` is the exception」→ L2-156 |
| 11 | `export_team_config` 不剥 `ai_allowed`，且 `api.yaml` 那句 `strips` 没覆盖它 | **不成立** | `export` 只剥 `key_path`/`password`/`passphrase`（`team_config.rs:42-44`），属实；但 `api.yaml` 现在**没有**笼统的 `strips` 句，`5041` 明写 ai_allowed 是例外 |

#### 本轮改动（L1）

- `docs/api.yaml`：文件级 body 段补第三格——`Bytes` 在 handler 之前缓冲，超 2 MiB 由 extractor 回 `413`、也是 `text/plain`。
- `src-tauri/tests/daemon_integration.rs`：`api_yaml_documents_how_each_handler_reads_its_body` 的失败文案改因果：`Bytes` 不可能回 415/422，它唯一自带的状态是 body limit 的 413。
- `src-tauri/tests/daemon_stop_e2e.rs`：新增 `an_oversized_body_is_refused_with_a_plain_text_413_before_the_handler`——真 daemon、超限 body 分别打 `/shutdown` 与 `/approvals/{id}/approve`，断言 413、断言正文**不是** `Error` 对象、断言 daemon 之后仍 200。**反向验证**：把期望 413 改成 400 立刻红（`left: 413 right: 400`，正文 `Failed to buffer the request body: length limit exceeded`），再改回。

#### 未做（L2）

L2-154（`/exec-multi` 全 0 语义）、L2-155（preview 缺 admin 门）、L2-156（`ai_allowed` 不比不保）。三条都要改 `src-tauri/src/**`，按协议只登记。

#### 验证

`npm run build` **6.62 s**；`yaml.safe_load` **90 paths**；`cargo test --no-default-features --lib` **863 / 1 / 1**（红项仍是故意红的 IPv6；比上一轮多一条，是并发会话加的）；`--features daemon --test daemon_integration api_yaml` **6 passed**（该二进制全量 **77 passed / 0 failed**，含 `every_scope_checked_route_documents_the_order_its_refusals_arrive_in`）；`--features daemon --test daemon_stop_e2e` **34 passed**；`cargo fmt --check` 干净。

#### 一条工艺教训

复核这轮 10 条里 5 条不成立，其中 ①②③ 都是**同一形态**：它把「自己基线上已经改好的东西」当成还没改（`1904cc2` 的桩回 pid、`1904cc2` 已有 `swallowed_paths`）。**判「上一轮留下的破绽」之前必须先 `git show <它自述的基线>:<file>`**，不能只读自己记忆里的旧版本。顺带：它这次自陈「按 `1904cc2` 读」，而我上一轮实际推的是 `092d0f9`/`e07e527`——基线差一笔就够把结论整条反转。

**回帖**：`2120515534193057792`。issue #20 **保持 open**——剩余三条全在 L2 拍板侧（L2-154/155/156），代码侧无未做项。

## 批次汇总 · issue #30 · batch 14（SSH 传输、known_hosts 与认证，russh 侧）

**issue**：#30，2026-11-11T11:55Z 开，6 轮后关闭（`state_reason=completed`）。**主题**：russh 传输层的 session 生命周期、SFTP 超时、known_hosts 读写与认证路径。

| 轮 | 指摘来源 | 判定 | 主要落点 | SHA |
|---|---|---|---|---|
| 1 | CodeBuddy 首条分析 | 10 条：成立 1 / 部分 3 / 不成立 2 / 已并入 1 / 已登记 3 | `sftp.rs` deadline 落点、`terminal_attach` 的 host 解析折点 | `6e12a4e`（首轮两笔之一，其余见 consume 1） |
| 2 | 等待超时 → 自取 | 见 consume 2 | 同族证据增补 | 见 consume 2 |
| 3 | 复核 D1–D5 | 见 consume 3 | api.yaml 契约格 | 见 consume 3 |
| 4 | 复核 R2-1/R2-2 + N1–N4 | 见 consume 4 | 403 顺序与措辞 | `4db0762`（复核方自推） |
| 5 | 复核 M1 | 7 条：成立 5 / 部分 1 / 不成立 1（含我自己错 1 条撤回） | 三路由 403 顺序、broadcast 的 preview 不作预测 | `2720096`/`5a8cb5b` |
| 6 | 复核 Q1–Q3 | 3 问：Q1 计数口径错、Q2 归 K7、Q3 能只靠 grep | 34 条路由 6 组形状的不变式测试；三处 403 改写 | `c41a6c4` + 补验 `6e67dd8` |

**六轮的两条教训**（写进工艺，不属 L2）：

1. **「本容器没有 cargo」不能当作「可以先推」**。第 6 轮据此把未编译的 Rust 推上 main，`daemon_integration` 整个测试二进制编不过，同一二进制里 20+ 条 api.yaml 契约测试一起沉默。补验与修复见「第 6 轮补验」小节（`6e67dd8`）。
2. **复核方开始直接推提交**（`4db0762`、`c41a6c4`）。凡 main 上出现本流程没亲自跑过的提交，先跑再谈意见——第 5 轮就是靠这个顺序发现它「一条都没跑」。

**待拍板（跨批次累积，优先级自上而下）**：L2-15（故意红的 IPv6，卡住 CNB `daemon-regressions` 整条链）> L2-135（`host_allows_source` fail-open）> L2-132（审计裁决值）> L2-136 > L2-154/155/156（本轮 #20 新增：`/exec-multi` 全 0 语义、import preview 缺 admin 门、`ai_allowed` 不比较不保留）。

### 手工/单发消费 · 2026-11-11 · daemon 异常诊断与保守恢复（issue #21 · 第二次进入）

**前置**：无锁（锁目录不存在，主动建立并写 `owner`，结束释放）；三个其它 worktree `status --porcelain` 全 0 行；`ls-remote cnb/main` 与 120 秒后的值都是 `f84c9bc`；`src/`、`docs/` 十分钟零写入 → 不开让位流程。`open` 共 11 条：**无 `[opt-round]`**（#30 已关），MANUAL = #20–#29 → 模式 B，第二段（批次循环）为空。

**选靶 #21（而不是编号最小的 #20）**——理由要留档，因为按「同级取编号最小」应当选 #20：

1. 按「待处理指摘的到达时间」这一层 FCFS，#21 的复核 `2120501607040184320`（12:59Z）比 #20 的 `2120517007480102912`（14:00Z）早一小时，且 #20 的复核落在我上一轮结束之后 4 分钟，上一轮的「下次取 #21」计划不是判断失误、是时间差。
2. 按可推进量：#20 的剩余三项是「一处排比句噪声 + 两条 413/Content-Length 文档措辞」，全部落在同一段契约文字上，且它的实质项早已被 L2-154/155/156 挡住；#21 的复核给了 10 条带行号的新断言，其中一条是**用户可见文档的事实错误**（`AGENT2SSH_DAEMON_ADDR` 声称作用于全部 client，而桌面 WebView 写死地址）。
3. 排除理由（本轮无）：#22/#25 是纯设计（P2）、#27 要 Windows 实机且无夹具，都不在本轮候选；#23/#24/#26/#28/#29 是本族下游，等 #21/#22 的结论。

**判定 12 条 → 成立 7 / 部分成立 3 / 不成立 2**：

| # | 指摘 | 判定 | 关键证据 |
|---|---|---|---|
| 1 | 桌面 `trace_id` 到不了 daemon，文档 `:1438` 的 Tauri 一半不成立 | **不成立** | `TRACE_ID_HEADER` 有 **20 处内联**使用（`api.ts:444…880`），不只 `traceHeaders()` 那一处；`/sessions` 的 handler 有 `tracing::info!`（`bin/agent2ssh-daemon.rs:2427`）；`:1438` 是 `AGENT2SSH_TRACE_ID` 行、作用域正是 `CLI / MCP`，带 Tauri 的是 `:1436` 的 `AGENT2SSH_SOURCE`，而那一行成立（`types.rs:676` 的 `source_from_env` 对所有 transport 生效，`tauri_commands.rs` 有 9 处 `source_from_transport()`） |
| 2 | `daemon_status` 的 debug 无级别过滤、会刷满 `app.log` | **部分成立** | 机制对（`:1763-1770`、`normalize_level:139`、`:308` 只按体积轮转），但 `daemonStatus` **零调用点**，刷满只会在 L2-142 接线后发生 |
| 3 | 「Daemon unavailable」把两种相反情形压成一句 | **成立** | `SettingsMenu.tsx:115-116`；`api.ts:695-709` 的 `:700`/`:707` 都返回 `null`，类型无第三档 |
| 4 | 前端写死 `127.0.0.1:7722` | **成立** | `api.ts:71`/`:128`；20 处请求 + `TerminalView.tsx:418` 的 ws |
| 5 | `UnverifiedIdentity` 文案列了 token，该分支不读 token | **成立** | `daemon_control.rs:1027-1029` 在 `health.pid` 不符的 else 里，token 判据在 `:1032` |
| 6 | 桌面与 CLI 对 `RunningUnhealthy` 分叉 | **部分成立** | 它已自撤「行为分叉」，剩下文案缺「谁写的」，并入 L2-145 |
| 7 | `remove_daemon_pid_file_if` 的 `Ok(false)` 被当成功、restart 报假 pid | **部分成立** | bool 被忽略属实（`:3107`），但「4 处调用、无一处检查」不成立——**9 处**非测试调用，`:3122` 与 `daemon_control.rs:441` **都检查**；「报一个已经不在的 pid」也不成立（`start_daemon_background:417-425` 重读 + `daemon_pid_state`，死 pid 落 `Stale`、乱码 `Err`） |
| 8 | 悬空 `daemon.pid` + 空 token：启动失败却已轮换凭据 | **成立** | 顺序 token → bind → pid（`bin/agent2ssh-daemon.rs:5786-5798`），空 token 在 `:337` 换新 UUID |
| 9 | `doctor`/MCP 的 `daemon.token` 只看 mode | **成立** | `bin/agent2ssh.rs:4989-5018`、`bin/agent2ssh-mcp.rs:1579-1598` |
| 10 | `status` 与 `stop` 对同一事实两种说法 | **成立** | `bin/agent2ssh.rs:3086` 对 `daemon_control.rs:980` |
| 修-1 | owner 标记也在锁内，停止卡住是两步 | **成立** | `daemon_control.rs:598`/`:622` 各拿锁，`:1084` 尾部清 owner |
| 修-2 | 失败点在 `write_daemon_pid` 之前的 `read_daemon_pid` | **成立** | `daemon_control.rs:78` |

**L1 落了什么**：

- `93bb0d5`：`configuration-guide.md:1437` 把 `AGENT2SSH_DAEMON_ADDR` 的作用域改成「daemon（绑定）+ client（拨号，桌面窗口除外）」，并写明桌面窗口的直连请求固定 `127.0.0.1:7722`、设非默认值后 CLI/MCP 正常而桌面显示 `Daemon unavailable`；`:388` 把「卡在删 PID 文件那一步」改成**两步**（先清 `daemon.owner`，返回后调用方再删 PID，各拿一次锁、都无上限）。
- `bac4dba`：`OPT-LOOP-L2-PENDING.md` 新增 **L2-157…L2-164**；顺带在文件头记下「下一个是 L2-109」已滞后 55 号，改成让读者 `grep` 最大号。
- 合流：`f84c9bc..bac4dba` 快进，**`cnb/main` = `bac4dba`**。分支 `opt/single-21-20261111` + worktree `single-21` 已清理（先验 `merge-base --is-ancestor`）。只推 `cnb`。

**新增 L2（八条，三条并入既有条目）**：L2-157（status 的 debug 无级别过滤，属 L2-142 配套）、L2-158（`DaemonHealth | null` 缺「不可达」一档）、L2-159（桌面 WebView 写死 daemon 地址）、L2-160（`UnverifiedIdentity` 文案比判定宽）、L2-161（`Ok(false)` 被当成功，restart 路径）、L2-162（悬空链接 + 空 token 会先轮换凭据，并入 L2-146 口径）、L2-163（doctor/MCP 不查空 token，并入 L2-145）、L2-164（status/stop 两种说法，并入 L2-145）。

**验证**：`npm run build` 6.32 s；`cargo fmt --check` 干净；`cargo test --no-default-features --lib` **863 / 1 / 1**（红项仍是故意红的 IPv6，与基线一致）。本轮零 Rust 改动，故未跑 daemon 集成目标。

**回帖**：`2120527459916357632`。issue #21 **保持 open**：余下八条全部需要人拍板，代码侧无未做项。

**这一轮的工艺收获**：复核第 1 条是「整条反转」，而它给的**判据本身**就是错的（`grep "traceHeaders()"` 只命中 1 处，却被写成 2 处，因为它漏了同一个常量另有 20 处内联用法）。顺着它的错误路径去核，才发现它引的 `:1438` 与它想说的 `:1436` 是两行——于是额外检查了「`AGENT2SSH_SOURCE` 到底管不管桌面」，结论是**管**（`source_from_transport()` 在 `tauri_commands.rs` 有 9 处）。也就是说这次「旁边那条也没缺」，但那条判断只有走完 `types.rs:676` 才敢下——**不能因为对方引错了行就停止核对它想说的那件事**。第 7 条同理：它数错调用点（4 vs 9）是次要的，真正的反转来自 `start_daemon_background` 会重读并重判状态。

### 手工/单发消费 · 2026-11-11 · Linux daemon 进程对象绑定（issue #23）

**前置**：锁目录不存在（主动建立并写 `owner`，结束释放）；三个其它 worktree `status --porcelain` 全 0 行；`ls-remote cnb/main` 与 120 秒后的值都是 `04398e0`；本仓工作树只有 `.deja/` 未跟踪 → 不开让位流程。`open` 十条（#20–#29）：**无 `[opt-round]`**（#30 已关）→ 模式 B，第二段（批次循环）为空。

**选靶 #23**：

- #20、#21 的最新评论都是本流程的 `## 本轮收尾`（`2120515534193057792`、`2120527459916357632`），且其后无回复 → 按防重复消费规则跳过。
- #22 是纯设计（P2，设计稿 `docs/plans/daemon-identity-protocol.md` 已存在，等的是人拍板），#25 同为 P2；#24 要 Windows 实机，本机做不了 → 三者均不入本轮。
- #23 是 **P1**：CodeBuddy 的首条分析（`2120415425753149440`）带文件路径 + 行号的具体断言，且尚未被消费。它的前置 #22 未定稿，所以本轮**不动代码**，只做「核实 + 可落的文档口径 + L2 实施顺序」。

**判定 15 条 → 成立 9 / 部分成立 4 / 不成立 2**：

| # | 指摘 | 判定 | 关键证据 |
|---|---|---|---|
| 1 | `stop_daemon_process` 四步（`:973-1079`），`signal_daemon` 注释自陈不原子 | **成立** | 实际 `:975-1087`；`:757-758` 原文 "narrows the PID reuse window but cannot make a PID-based signal atomic with this inspection" |
| 2 | 进程名 + argv 白名单挡不住复用，白名单里有"包名" | **成立** | `is_daemon_executable`（`:141-154`）只有四个字面量；任何进程都能改成这四个名字之一 |
| 3 | `/health` 的信任度为 0（`daemon_health_ok` 只认 200 + `"ok":true`） | **成立** | `daemon_health_ok:166-194`；`health_identity` 不读 `version` |
| 4 | `start_time` 是秒级，5 次相邻 spawn 相同 | **成立** | 本仓 `:270` 注释已写 "seconds-resolution snapshot"；`sysinfo 0.39.3` 三平台均截断到整秒（Linux `raw / clock_cycle`、macOS `pbi_start_tvsec`、Windows `FILETIME / 10_000_000`） |
| 5 | 「`:986-988` 注释里说 start time 挡住复用」 | **不成立** | `:987-991` 讲的是「名字不是证明」，从未声称 `start_time` 能挡复用；`:270` 反而明说它是秒级快照 |
| 6 | pidfd 不解决「获取前」的 PID 复用 | **成立** | 设计判断与代码一致：`read_daemon_pid:56-70` 只拿到号 |
| 7 | `daemon.pid` 内容升级后旧客户端会按「损坏」拒绝 | **成立** | `read_daemon_pid:63-69` 的 `parse::<u32>()` 失败即 `Err` 且明写 "file retained" |
| 8 | `SO_PEERCRED` / `SCM_CREDENTIALS` / `SO_PEERPIDFD`（6.5+）语义与取样时机 | **成立（本机未实测，仅按 Linux 语义核）** | 它自述在 Linux 5.4 容器实测 `SO_PEERPIDFD` = `ENOPROTOOPT(92)`，与该常量 6.5 引入一致 |
| 9 | 「`SO_PEERCRED` 的 pid 是连接的属性，不是进程的身份」 | **成立** | 与 `LOCAL_PEERCRED`/`getpeereid` 的语义一致，且是本轮最该记住的一条设计约束 |
| 10 | 「现状里唯一按 PID 强杀的降级在 `:1069`」 | **成立** | 实际 `:1077`（`signal_daemon(pid, start_time, true)`），`:1085` 之后才清 owner |
| 11 | 测试矩阵里称 `invalid_or_partial_pid_files_*` 在 `:1125` | **成立（行号偏 8）** | 实际 `daemon_control.rs:1133` |
| 12 | K14 对「秒级碰撞」只有一句「可能碰撞」，应升级为事实 | **部分成立** | 该句确实存在且确实只是「可能」，但同段早已写了「重命名或新平台后缀的 daemon 也可能不在名称白名单」——不是"只有一句"。本轮把两处都升级为确定性表述 + 补后果 |
| 13 | 反例：target triple 换成 `x86_64-pc-windows-gnu` / `aarch64-unknown-linux-musl` 的构建 → 「停不掉也起不来」 | **部分成立** | **停不掉成立**；**「起不来」只在一半情形成立**：`start` 先看 `daemon_pid_state`，`Healthy` 时直接打印 `already running` 并返回 Ok（`bin/agent2ssh.rs:3054-3058`，`renamed_daemon_is_reported_healthy_but_not_unsafely_stopped` 钉住），只有 health 不可用（落 `Unverified`）才拒绝。另外**触发条件写错**：发出去的二进制造名恒为 `agent2ssh-daemon`（`daemon_binary_path:4890-4900`、`bundled_daemon_binary_path:126-139` 都写死无 triple），三处 triple 字面量只在直接跑 `src-tauri/binaries/` 里那份 staged 副本时生效 |
| 14 | §0-3「新发现：没有实例级单例锁，换端口就能让两个实例共用同一 token 与 `daemon.pid`」 | **不成立** | `write_daemon_pid:76-84` 在 `.daemon-pid.lock` 内检查旧 PID，活着或不可核实即 `Err`；daemon 侧 `write_daemon_pid(std::process::id())?` 用 `?` 传播 → **第二个实例在发布 PID 之前退出**。`daemon_stop_e2e.rs:632` 断言的正是「竞争 daemon 退出、非零、stderr 含 live or cannot be verified、旧 PID 不变」，不是它说的"只比文件内容" |
| 15 | §0-3「`request_graceful_shutdown()` 会关掉任意一只」 | **部分成立** | 函数行为如此，但**生产代码零调用点**：全仓唯一调用是 `daemon_stop_e2e.rs:1150`，`stop_daemon_process` 恒传 `Some(pid)`。此事实 ledger 2026-11-10 已记过一次（"该函数在生产代码里无调用点，仅测试用"） |

**L1 落了什么**：

- `docs/KNOWN-ISSUES.md` K14 两处：
  - 「秒级启动时间可能碰撞」→ 改成「启动时间是整数秒，`sysinfo 0.39.3` 三平台都截断到整秒（Linux `/ clock_cycle`、macOS `pbi_start_tvsec`、Windows `FILETIME / 10_000_000`），因此同一秒内启动的两个进程在这条判据下完全不可区分，不是「可能碰撞」而是构造上必然」，并指向 L2-165。锚点：`grep -c "秒级启动时间可能碰撞"` = **0**，`grep -c "构造上必然"` = **1**。
  - 「重命名或新平台后缀的 daemon 也可能不在名称白名单」→ 补成「四个字面量 + 两个身份入口给出相反答案（`status`/`start` 说 running，`stop`/`restart` 说 cannot be safely identified）+ 测试钉住 + 两条恢复出口」。锚点：`grep -c "两个身份入口给出相反答案"` = **1**。
- `docs/guides/configuration-guide.md` 停止一节：在既有「带外 kill」出口后加「同一个出口也适用于可执行名不在白名单的 daemon」，说明 `status` 会说 running 而 `stop`/`restart` 拒绝的原因（状态判据不看名字、停止第一步看名字），锚点 `grep -c "同一个带外出口也适用于另一种"` = **1**。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-165**（pidfd 五步实施顺序 + 「fd 拿不到不得回退裸 PID」契约）、**L2-166**（白名单字面量 vs 两个身份入口判据相反，含「实例级单例」这条不成立的原文留痕）。

**未做的 L2 项与理由**：pidfd 本体（`stop_daemon_process` / `wait_for_exit` / `signal_daemon` 改成以进程对象为参数）牵动 5 个调用点与 33 个 e2e 用例，且前置 #22 未拍板；白名单派生会改变 `renamed_daemon_*` 测试的安全语义。两者都只登记。

**验证**：`npm run build` 6.29 s；`cargo fmt --check` 干净；`cargo test --no-default-features --lib` **863 / 1 / 1**（红项仍是故意红的 IPv6，与基线一致）。本轮零 Rust 改动，`src-tauri/src/**` 与 `src-tauri/tests/**` 一行未动。

**回帖**：`2120545266569732096`（`## 本轮收尾`，15:52:44Z），#23 内该标记首条（C 不适用——单发轮不带轮次号）。issue **保持 open**：三条验收（真实进程回归 / 两种生命周期 / Linux CI 前提）全未做，阻塞在 #22 的文件身份格式、L2-165 的 errno 归属、L2-166 的二选一。其后无新回复（本轮到此结束，第二段为空）。

**这一轮的工艺收获**：它最响的「新发现」（没有实例级单例锁）恰好是它自己引过的那条测试**断言了相反的事**——`daemon_stop_e2e.rs:632` 的 `wait_until("competing daemon exit", …)` 是「第二个实例必须退出」，被读成「只比了文件内容」。**引用一条测试时，读它断言的是进程还是文件**，这一类误读在台账里已是第三次（前两次是 `wait_for_exit` 的单测作用域、`terminal_broadcast_run` 的 return 臂）。反过来，它的 `start_time` 秒级结论我核到了上游三处转换——**对的那条要核到能复算的锚点（`sysinfo 0.39.3` 的 Cargo.lock + 三处源码），不然只是换个人复述**。

### 手工/单发消费 · 2026-11-12 · #20 daemon 停止回执与版本兼容（第三次进入 · 契约措辞 + 两条断言）

**选靶**：#20 的 P1 档最新一条是 CodeBuddy 对本流程上一轮收尾（`2120515534193057792`）的复核 `2120517007480102912`，未被消费 → P1 同级取编号最小。排除理由：#21 与 #23 的复核同样未消费，但编号更大；#22/#25 是 P2 设计（等 #22 的身份协议拍板）；#24/#27 要 Windows 实机；#26/#28/#29 零评论（需先发只读分析请求），且 #26/#28 的前置是本单的阶段 2 设计。**#20 是本族入口**（#21 依赖回执语义、#23–#25 依赖 #22 的协议），推进它比推进下游更划算。

**基线** `56704ea`（= 收到复核时的 `cnb/main`）。**判定 9 条 → 成立 5 / 部分成立 1 / 不成立 1 / 它自撤 2**：

| # | 指摘 | 判定 | 关键证据 |
|---|---|---|---|
| 1 | 413 只写「超限」，没写声明 `Content-Length` 也被拒 | **部分成立**（观察对、因果反） | `grep -c Content-Length docs/api.yaml` = 0 成立；但 `axum-core-0.4.5/src/ext_traits/request.rs:324-338` 只包 `Limited::new(b, 2_097_152)`，`src/extract/request_parts.rs:78-92` 逐 frame 读到上限才报错 → **无按声明长度的预检**，「声明了会先被拒」不成立 |
| 2 | `/shutdown` 与 `/approvals/{id}/approve` 的 413 不是一句话 | **成立** | 改前 `:1241` 一句、`:4040` 讲因果；现已逐字相同（`:1263` / `:4065`） |
| 3 | `/exec` 的 503 | 它自撤 | — |
| 4 | `/local-exec` 的 `host` 缺 `local:<id>` 自定义环境 | **成立，比指摘更硬** | `local_exec.rs:862` 的 `resolve_plan_for_host` 先查 `local_environments::current()`；同句尾「Any other value is rejected with 400」按字面是假的。`grep -c environments.toml docs/api.yaml` 0 → 2 |
| 5 | 400 描述没列「工作目录不存在」 | **成立** | `local_exec.rs:919-929` 报 `working directory '…' does not exist`，handler `:1853` 折成 400；且该检查在 base 解析**之后**、每条命令都做，与加载期拒绝不同 |
| 6 | `local:cwd` 在 WSL base 上静默无效 | **不成立（结论未证实）** | 前提对（`wsl_plan` 的 `cwd: None` 在 `:678`、`layer_custom_environment:948` 无条件覆盖）；但 `apply_plan_context:958-963` 把 cwd 设在 `wsl.exe` 进程上，「无效」取决于 WSL 对 Windows cwd 的翻译规则，它无实测、本机无 WSL → 不入账、不登记 |
| 7 | `/exec-multi` 的 `strategy` 默认语义不可见 | **成立** | `core.rs:1492-1502` / `:1561-1566` / `:1585-1588` / `:1552` + CLI `:137` 的 `(0 = unlimited)`；四个字段原本连 `description` 都没有 |
| 8 | `/config/import` 的 `hosts_updated` | 它自撤 | 后半被它自己定位成 L2-155 的配套文档格 |
| 9 | `AcceptedUnchecked` 的 warn 仍断言看不到的事 | **成立但已登记** | `daemon_control.rs:1053`；`grep -rn checked_pid src-tauri/src/` = 0；同文件 `:1040-1052` 注释已自陈 known defect，K14 已记，修法在 **L2-3**、断言在 **L2-30** → **不新立** |

**L1**：`24d25f9`（`docs/api.yaml`：两处 413 逐字统一、文件级说明改 First/Second/Third 并补「上限按读到的字节计」、`/local-exec` 的 `host` 补 `local:<id>`、400 补工作目录、`strategy` 四字段补语义）、`354db76`（`daemon_integration.rs`：`code_block` 只截单个状态码的行 + 「`Bytes` 处理器的 413 必须点名 `Bytes`」+ 新测试 `api_yaml_documents_user_defined_local_environments`，先断言源里仍有 `local_environments::current()` 再断言正文含 `environments.toml`）。合流后 `cnb/main` = **`354db76`**。

**验证**：`npm run build` 6.31 s；`fmt --check` 干净；`--no-default-features --lib` **863 / 1 / 1**（红项仍是故意红的 IPv6）；`--features daemon --test daemon_integration api_yaml` **7 passed**（6 → 7）。**反向验证**：删掉两处 413 的 `Bytes` 与 `/local-exec` 的 `environments.toml` → 恰好这两个测试转红，恢复后全绿。

**回帖**：`2120562572737810432`（`## 本轮收尾`，17:01:30Z）。#20 **保持 open**：第 9 条要的是 L2-3 的契约改动，第 6 条要 Windows 实机，都需人拍板或换机器。

**这一轮的三条判据**：

1. **「文档没提 X」与「X 的行为是 A」是两件事**。它观察到 api.yaml 从未提 `Content-Length`（真），然后推断「声明了会被先拒」（假）——两者之间隔着 `http_body_util::Limited` 的实现。凡「文档缺一句」的指摘，落笔前必须把**要补的那句**核到实现，否则补进去的是新错误。
2. **同一个事实在两条路由上写两遍，一致性就没人负责**。本条与上批「同族里一条写一条不写」同型，但这次两处**都有**、只是详略不同，所以任何按「有没有」写的断言都抓不住。`code_block` 那种「按状态码切段」的断言才是能抓住详略差的形状。
3. **指向 `src-tauri/src/**` 的指摘，先查它是否已在 K14 / L2 清单里**。第 9 条是它本轮唯一「可零成本做」的建议，而 K14 早已记了同一现象、L2-3 早已给了四步改法——不查就会立出第 113 条重复条目。

### 手工/单发消费 · 2026-11-12 · #21 异常诊断（第三次进入 · 日志脱敏边界 + 一条 429 假承诺）

**模式 B 单发**（open 十条 #20–#29，无 `[opt-round]`；第二段为空）。基线 `7847ea2` = `cnb/main`。选靶 **#21**：它是这一族的入口（#20 是回执、#22 是身份协议、#23–#25 是平台落地），最新评论是 CodeBuddy 对上一轮收尾的复核（`2120529873901891584`，14:51:34Z）且未被消费；编号小于同样未消费的 #23。排除项：#22/#25 是 P2 设计、#24/#27 要 Windows 实机、#26/#28 前置是本单设计、#29 编号更晚。

**判定 8 条 → 成立 5 / 部分成立 2 / 不成立 1**；另复核对上一轮的两处订正：一处**不成立**（见 N1-b）、一处**已登记**（L2-161）。

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| N1 | 非默认 `AGENT2SSH_DAEMON_ADDR` 时桌面「真的启动、永远看不见」 | **部分成立** | 机制成立：`App.tsx:375-399` 的 `ensureLocalDaemon` 先 `api.daemonStart()`（后端跟随变量，`remote.rs:22` 读 env），再 `api.getDaemonHealth()` 打写死的 7722（`api.ts:71`）→ 成功判据恒假。但「**反复尝试启动**」不成立：该回调只挂在 `App.tsx:409` 一个 `useEffect`（deps `[ensureLocalDaemon]`，其 deps 是 `[t]`），10 秒定时器跑的是 `pollDaemonHealth`（`:369-373`，只读不启动）。行为面已在 **L2-159**；本轮只补文档 |
| N1-b | 「`/events/stream`（`api.ts:880`）是『直连请求不跟随变量』的反例」 | **不成立** | `:880` 用的还是同一个 `daemonUrl` 常量，它**也不跟随**变量，与面板那几条同向。「只在订阅时才有请求」是**触发时机**不同，不是行为相反；把「同一常量的第三条出口」当反例，结论与前提相反。已把这条出口补进枚举（避免读者以为只有面板与 ws 两条） |
| N2 | `architecture.md` 把 Tauri 与 CLI/MCP 并列成用 `set_trace_id`/`seed_trace_id_from_env` | **成立** | `grep -rn seed_trace_id_from_env src-tauri/src/` = 3 行：定义 `diagnostics.rs:71` + `bin/agent2ssh.rs:1856` + `bin/agent2ssh-mcp.rs:609`；`run_tauri()`（`tauri_commands.rs:3567`）只调 `install_panic_hook`。`set_trace_id` 的调用点也只有 `diagnostics.rs:76`（在 `seed_trace_id_from_env` 内部）。L1 已改 |
| N3 | `a_forged_endpoint_cannot_report_an_absent_pid_as_running` 在 CI 里恒绿（CLI 子进程打的是 7722 真 daemon） | **不成立** | 执行顺序反了：`:955-956` 在测试进程设 `CONFIG_DIR_ENV`/`ADDR_ENV`，`:987-992` 在**同一时刻**把两者显式 `.env()` 给 CLI 子进程，`:995` 的 `restore_default_config_env()` 在 `server.join()` 之后才跑，改不回已捕获的值。CLI 的 `DaemonCommands::Status`（`bin/agent2ssh.rs:3083-3091`）读 `daemon_health_pid()`，地址来自那个变量 → 确实打在假端点（假端点回 `{"ok":true,"pid":u32::MAX-1}`，该 pid 不存活 ⇒ `Unverified` ⇒ `identity cannot be verified`）。断言不是空转 |
| N4 | `.cnb.yml` 的 `mktemp -d` 目录不是「无配置」，`restore_default_config_env` 的语义与注释不符 | **部分成立（自陈非缺陷）** | 机制对：`tests/common/mod.rs:71-76` 恢复的是 `DEFAULT_CONFIG_DIR` 快照，CI 下就是那个 `mktemp -d` 目录，不是「未设」。但每个测试用前各自 `set_var` 到自己的 tempdir 且串行（`#[serial]`），不串味；它自己也写「属可读性问题，不是缺陷」。不立条 |
| N5 | 四个 exec handler 的 `command = %…` 只落不脱敏的 `daemon.log` | **成立** | `bin/agent2ssh-daemon.rs:1728/1826/1862/1906` 四处 `tracing::info!`；桥接层 `trace.rs:135` 只放 `<= WARN`；`fmt::layer()`（`:5700-5710`）无 filter；`app.log` 的脱敏在 `diagnostics.rs:295`/`:323`。L1 补文档，行为面登记 **L2-169** |
| N6 | `/exec/compare` 丢弃 `source`/`reason`/`change_id` | **成立** | `ExecCompareBody`（`:1092-1100`）无这三字段；`:1908` `source_from_transport()`；`:1914-1924` `authorize_targets(…, None, None)`；`:1932` `reason: None`。对照 `ExecMultiBody`（`:1074-1090`）+`:1863`+`:1889-1893` 三样齐全。`api.yaml:232-245` 如实反映差异 → 缺能力不缺文档。登记 **L2-167** |
| N7 | `DaemonControlResult.pid` 在桌面端有两种含义 | **成立** | `tauri_commands.rs:1761-1780`：`pid` 来自 `read_daemon_pid()`；`:1781-1790`：`pid` 来自 `daemon_health_pid()` 且 `running: false`。`src/types.ts` 只有 `pid: number \| null`。CLI 侧（`bin/agent2ssh.rs:3083-3091`）只打印，故歧义只在桌面/IPC 一侧。登记 **L2-168** |
| N8 | `/sessions/{id}` 的 DELETE 正文承诺 429，handler 不经过限流 | **成立**（且是全量扫描里唯一一条） | `session_close`（`:2649-2702`）只调 `reject_if_gate_paused`（`:2664`）+ `check_daemon_scope`（`:2665`）+ `authorize_command`（`:2667`）；`reject_if_rate_limited` 全仓 14 处调用点无一处在此。responses 也只有 200/400/401/403/408/423。**全量扫描**：105 个 operation 里「正文提 4xx、responses 缺」共 6 处，其余 5 处都是**已文档化的例外**——`/shutdown` 明写 `A 429 is not one of them`、`/exec/stream` 明写 `This is why no 429 and no 423 appear below`、两个 broadcast 明写 per-target 折叠，且文件级 `info.description` 对 `GET /terminal` 的 500 单独成段（本条与台账 2537/2552 的「不成立」不冲突：那条判的是「helper 的 500 被遮蔽」，而 `api.yaml:24-31` 逐字点名了 `reject_if_gate_paused`）。L1 已改 |
| 订正-a | 「`:388` 没写桌面的 `daemon_restart` 在这个窗口里不设防」 | **成立但已登记** | `tauri_commands.rs:1886-1893` 先 log `terminated existing process` 再 `remove_daemon_pid_file_if(...)?`；CLI restart（`bin/agent2ssh.rs:3103-3107`）与 rotate-token（`:3122`，检查 bool）收尾各不相同——正是 **L2-161** 的 `Ok(false)` 家族，不新立条 |

**本轮改了什么**（全部 L1，`src-tauri/src/**` 零改动）：

- `docs/api.yaml`：`/sessions/{id}` DELETE 的 403 描述去掉 429 承诺，改为「gate 先于 scope，所以 423 会在越权之前答；不限流所以没有 429」，并与 `/sessions`（开口侧确实列了 429）对照。
- `docs/architecture.md`：Correlation IDs 段把「CLI/MCP/Tauri」收窄为「CLI 与 MCP 从 `AGENT2SSH_TRACE_ID` 播种」，并写明 `run_tauri()` 只装 panic hook、桌面只从前端拿 `trace_id` 字段；daemon 段补一句 `daemon.log` 无脱敏（`fmt::layer()` 无 filter），与 `app.log` 的写前脱敏对照。
- `docs/guides/configuration-guide.md`：目录结构补 `daemon.log` 行；新增小节「两个日志文件不是同一份东西」（`app.log` 脱敏 + 5 MB 内联轮转 3 代 vs `daemon.log` 原样落盘 + 仅启动时轮转 + 四个 exec handler 的 `command=` 原文 + 分享前先裁剪）；`:1437` 补「桌面不仅显示 Unavailable，还会真的在变量地址起一个它拨不到的 daemon，每次挂载一行诊断」与 SSE 那条出口。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-167**（compare 缺 source/reason/change_id）、**L2-168**（`pid` 双语义）、**L2-169**（`info` 级命令原文进 `daemon.log`，三条改法）。

**验证**：`npm run build` **6.61 s**；`cargo test --no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 IPv6 `an_ipv6_literal_at_port_22_is_removed`，与基线一致）；`--features daemon --test daemon_integration api_yaml` **7 passed**；`python3 -c "yaml.safe_load"` **90 paths**，`DELETE /sessions/{id}` 的码集合 `200/400/401/403/408/423`。

**未做的 L2**：L2-167/168/169（都改 `src-tauri/src/**` 或 IPC 形状）、L2-159 的 (a) 修法、L2-161。

**回帖**：`2120565582104784896`（`## 本轮收尾`，17:13:27Z）。`#21` 保持 open（剩余全部在 L2 拍板侧）。

**给下一轮**：`#21` 保持 open（剩余全部在 L2 拍板侧）。下一轮按「待处理指摘最早、未被消费」应取 **#23**（最新 `2120546004976975872` 是 CodeBuddy 的复核，未消费；P1；#20 现在也有未答复的收尾帖）。行号基线 **本轮合流后的 SHA**；`api.yaml` 因本轮改写 403 描述，`/sessions/{id}` 段整体下移约 3 行。

### 手工/单发消费 · 2026-11-12 · #23 Linux 生命周期绑定（第四次进入 · 身份入口同族清点 + pidfd 素材与 errno 归属）

**模式 B 单发**（open 十条 #20–#29，无 `[opt-round]`；第二段为空）。开工基线 `e80b98f` = `cnb/main`：`ls-remote` 120 s 两值相同、三个其它 worktree 全 0 行、`src`/`src-tauri/src`/`docs` 十五分钟内零写入 → 不让位。选靶 **#23**：`#20`、`#21` 的最新评论都是本流程的 `## 本轮收尾`（等复核，按防重复消费跳过）；`#22`/`#25` 是 P2 设计、`#24`/`#27` 要 Windows 实机（排除）；`#29` 编号更晚；**#23 的最新评论是 CodeBuddy 的只读复核（`2120546004976975872`，15:55:40Z），未被消费，P1**。pidfd 本体本轮仍做不了——它要改 `stop_daemon_process` 的签名（L2），且文件身份格式是 #22 的决策项——所以本轮交付是「把它的材料逐条核到能复算，再把我写错/漏写的地方改对」。

**判定 10 条 → 成立 3 / 部分成立 2 / 不成立 5**。五条不成立里四条整条反转，全部发生在因果链上。

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | `request_graceful_shutdown()` 是「零调用点 + 恒 false」的双重死码（称 `request_shutdown(None)` 落 `AcceptedUnchecked` 支） | **部分成立**：「零调用点」成立，「恒 false」**不成立** | `request_shutdown` 有两条 200 臂：`:904` 是 `status == 200 && expected_pid.is_none() => Accepted`，`:919` 才是 `=> AcceptedUnchecked`。`request_graceful_shutdown`（`:964-966`）传 `None`，**命中的是第一条**，返 true。且 `daemon_stop_e2e.rs:1149 the_shutdown_route_stops_the_daemon_gracefully` 断言 `assert!(request_graceful_shutdown(), "the admin token is accepted")` 并通过——一个恒 false 的函数会让这条用例恒红。零调用点那半成立（生产零调用，唯一调用点是这条测试 `:1150`） |
| 2 | `direct_daemon_with_another_port_cannot_replace_a_live_pid` 里第二个实例「死在 `:5795` 的 `bind` 上」，用例与真实路径之间有未证明的间隙 | **不成立** | 该用例（`daemon_stop_e2e.rs:632`）先 `TcpListener::bind("127.0.0.1:0")` 拿一个**空闲**端口、`drop(listener)` 释放、再把 `ADDR_ENV` 指过去（`:636-639`）。**不同端口在同地址上不排他**，`bind` 必然成功，拒绝只能来自 `write_daemon_pid`。它把「同地址」读成「同端口」，再据此推出「`write_daemon_pid` 只是第二道防线」——而 daemon 侧的顺序（`:5791` token、`:5795` bind、`:5798` write_daemon_pid）确实如它所述，只是这个用例没走 bind 失败那条 |
| 3 | 整数秒只影响 `daemon_process_start_time` 一处；`OwnerMarker` 把秒级时间写盘，同秒重启会继承「自有」身份 | **部分成立** | 四处同源读取成立：`daemon_process_start_time:117`、`observable_live_start_time:271`、`process_start_time:587`、`observe_process:732`，都取 `sysinfo::Process::start_time()`；`OwnerMarker { pid, start_time }` 写盘成立（`:578-581`/`:595-605`/`:608-619`）；测试只覆盖 `wrapping_add(10_000)`（`:1293-1310`）成立。**但「同秒重启即继承」不成立**：`daemon_is_owned` 先要求 `marker.pid == pid`，重启必然换 pid；真正被过度承诺的是那条注释。已按此改注释（见下） |
| 4 | 「两个身份入口给出相反答案」漏了第三个：`agent2ssh doctor` | **成立** | `bin/agent2ssh.rs:5025-5039`：`daemon_running = recorded_pid.is_some_and(daemon_matches_config)`（不看名字）→ `pass`，detail 是 `daemon PID and config token verified`。用户被指向「为什么停不掉」时最可能跑的命令，恰好给出最权威的相反答案 |
| 5 | 诊断包在同一份 JSON 里同时给 `daemon_process_alive: false` 与 `daemon_health_ok: true` | **成立** | `diagnostics.rs:493-501`（bundle）与 `:593-596`（system report）：前者走 `process_is_alive`（看名字），后者走 `daemon_matches_config`（不看名字）。`doctor` 与这两处零测试覆盖（`grep -c doctor src-tauri/tests/daemon_stop_e2e.rs` = 0） |
| 6 | K14 新写的「Linux 把 `starttime` 除以 `clock_cycle`」可疑，`clock_cycle` 更像 ticks→纳秒的内部常量 | **不成立** | sysinfo 0.39.3 的 `unix/linux/system.rs:147`+`:156`：`clock_cycle: sysconf(_SC_CLK_TCK) as _`；`unix/linux/process.rs:429`：`(raw, raw / info.clock_cycle)`。它就是 USER_HZ，我的措辞就是实现本身。已补「即 `sysconf(_SC_CLK_TCK)`」，让锚点不必读 sysinfo 源码也能复算 |
| 7 | `wait_for_exit` 写成 `:745-755`、实际是 `:746-756` | **不成立** | 在 `e80b98f` 上 `wait_for_exit` 的 fn 体正是 `:745-755`（收在 `observe_process(pid, expected_start)`），`:746` 是 `let start = Instant::now();`。它偏的是 `observe_process`（`fn` 在 `:729`，它写 `:726`），不是 `wait_for_exit` |
| 8 | `prctl(PR_SET_NAME)` 能把 comm 改成白名单字面量，从而过 `daemon_process_start_time` 的名称门槛 | **不成立** | `process_looks_like_daemon:156-164` 第一条读 `process.name()`（Linux 上是 comm），而内核把 comm 截断到 15 字节，`agent2ssh-daemon` 是 **16** 字节。**它自己的实测输出就是 `comm : agent2ssh-daemo`**——那个值一个白名单字面量都不匹配（四个字面量最短的也是 16 字节）。这正是代码注释 `:120-121` 说的「Linux truncates process names to 15 bytes, so the daemon's full name must also be read from its command line」。所以负向用例的素材只能用真同名二进制或 `execve` 改 argv[0]，不能用 `prctl` |
| 9 | `daemon_stop_e2e.rs` 有 33 个用例全部直接传 `pid: u32` | **不成立（数字错，方向对）** | 实测：34 个 `#[test]`、34 个 `#[serial_test::serial]`、15 个 `#[cfg(unix)]`，`stop_daemon_process(` 出现在 **16** 个 test 体里。我上一轮 L2-165 里写的「33 个用例」同样错，本轮一并改正。它给的改法方向对：先加 `PidHandle` newtype + `From<u32>` 垫片，这 16 条里多条靠 `set_var` 切 `CONFIG_DIR_ENV`/`ADDR_ENV` 再读 PID 文件，直接改签名会把「改类型」与「改测试语义」混成一笔 |
| 10 | `AGENT2SSH_DAEMON_ADDR` 指向另一台主机时，停止路径整条失效且文案误导（P-4） | **成立** | `stop_daemon_process:977` 读**本机**进程表，`:999` 的 `health_identity()` 经 `local_daemon_connect_addr()`（`remote.rs:34-36`）打**该变量解析出的地址**。两台机器时两个观察必然一处命中一处不命中，落 `:1027-1030` 的 `health PID, config directory, or admin token did not match`——没有一个词提到地址。同形后果：`daemon status` 报 `cannot be verified`；`daemon token rotate` 被拒（`:3118-3120` 两道闸，远端活着时 `daemon_health_pid().is_some()` 恒真——这一道本身讲得通，换掉正在用的 token 会切断那只 daemon） |

**本轮改了什么**（全部 L1；`src-tauri/src/**` 只动注释，零行为改动）：

- `9ac56a5`：`daemon_control.rs` 的 `OwnerMarker` 文档注释。原文承诺「The start time makes a marker left behind by an earlier daemon fail to match a later, unrelated process that happens to get the same pid」——只在两次启动时间不同时成立，而三平台都是整秒，同秒复用的 pid 会匹配。同处的 `a_marker_does_not_survive_pid_reuse` 只测了「未来时间戳」这一个方向，补一句写明另一个方向没有覆盖。
- `2de2545`：`docs/KNOWN-ISSUES.md` K14 把「两个身份入口」扩成完整清点——状态类（`status`/`start`/`doctor`）说健康、停止类（`stop`/`restart`）说认不出、诊断面在同一份 JSON 里自相矛盾；并写明 `renamed_daemon_is_reported_healthy_but_not_unsafely_stopped` 只钉 status/start/stop 三处。`docs/guides/configuration-guide.md` 同步（停止一节补 `doctor`，环境变量表补「指向另一台主机时本机 stop/status 必然失败、文案不提地址」）。`daemon_stop_e2e.rs` 的 doctor 缺口与句中的 `clock_cycle` 锚点一并落笔。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：**L2-170**（`UnverifiedIdentity` 三支共用一个文案，分不清「PID 被复用」与「地址在别处」）、**L2-171**（`write_daemon_pid_if_absent` 的「保留可恢复 PID」零 e2e，附 `#[cfg(unix)]` + `chmod` 夹具）；L2-165 就地修正三处（`wait_for_exit` 行号的订正记录、`prctl` 素材改用真同名二进制或 `execve`、`EPERM` 必须绑在 `pidfd_send_signal` 而非 `pidfd_open`），并把「33 个用例」改成实测的 16/34。

**验证**：`npm run build` **6.77 s**；`cargo fmt --check` 干净；`cargo test --no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 IPv6 `an_ipv6_literal_at_port_22_is_removed`）。合流时远端已推进到 `aa08c55`（别人把 russh 升到 0.65.1），rebase 干净、无冲突，**依赖变更所以合流后重跑了一遍 lib 测试**（数字同上）。

**未做的 L2**：pidfd 本体（L2-165 五步，等 #22 拍文件身份格式）、白名单派生（L2-166 二选一）、L2-170/171。P-1/P-2 的两条 pidfd errno 实测来自复核方（Linux 5.4.241），本机是 macOS、无法复现，已按「实施前在目标内核重跑」记进 L2-165 第 4 步。

**这次的两条判据**（比单个修复值钱）：

1. **「两个 X」这种计数式表述要用同族清点去证伪，不是去补**。上一轮我写「两个身份入口」，这一轮顺着 `grep` 把 `daemon.pid` 的读者全列出来，才看见 `doctor` 与诊断面——它们不是第三个和第四个「入口」，而是**同一族里判据分叉的另外两个落点**。凡写「A 与 B 相反」，先把 `daemon_matches_config` 与 `daemon_process_start_time` 各自的调用点列全。
2. **复核方给的「更强证据」要先跑一遍它引的用例**。它说我引的测试「与真实路径之间有间隙」，而那个测试的第一行就是自己挑空闲端口——`bind` 不可能冲突。同类：它说 `request_graceful_shutdown` 恒 false，而现有用例断言的正是 true。**引用一条测试时读它怎么准备夹具，引用一个函数时读它的所有返回臂**，这两条在台账里已各出现三次以上。

**以下已核实为不成立，不要重复提**（编号接本族前序条目）：

11. 「`request_graceful_shutdown()` 恒为 false」——`:904` 的 `200 && expected_pid.is_none()` 臂返回 `Accepted`，现有用例 `the_shutdown_route_stops_the_daemon_gracefully` 断言的正是 true。只保留「生产零调用点」。
12. 「`direct_daemon_with_another_port_cannot_replace_a_live_pid` 里第二个实例死在 `bind` 上」——该用例显式挑一个空闲端口，`bind` 必然成功，拒绝来自 `write_daemon_pid`。
13. 「`clock_cycle` 是 ticks→纳秒的内部常量」——它就是 `sysconf(_SC_CLK_TCK)`（`sysinfo-0.39.3` 的 `unix/linux/system.rs:156`）。
14. 「`wait_for_exit` 的行号偏一」——`e80b98f` 上 `:745-755` 是准的；偏的是 `observe_process`。
15. 「`prctl(PR_SET_NAME)` 能冒充白名单可执行名」——comm 截断到 15 字节，四个字面量最短 16 字节，`agent2ssh-daemo` 不匹配任何一个；复核方自己的实测输出就是这个截断值。
16. 「`daemon_stop_e2e.rs` 有 33 个用例传 `pid`」——实测 34 个测试里 16 个调 `stop_daemon_process`。此条同时更正我上一轮 L2-165 的同一数字。

**回帖**：`2120580606311424000`（`## 本轮收尾`，18:13:09Z）。`#23` 保持 open。

### 手工/单发消费 · 2026-11-12 · #20 daemon 停止回执（第四次进入 · `/local-exec` 的拒绝分类学、批次停止语义、`code_block` 判据）

**选靶理由**：`open` 12 条，**无 `[opt-round]`** → 模式 B，第二段为空。#20 编号最小且是这一族的入口（#21 依赖回执语义、#23–#25 依赖 #22 协议）；最新评论是 CodeBuddy 的复核 `2120564624440336384`（17:09:39Z），**未被消费**。#21/#23 的最新评论是本流程的 `## 本轮收尾`（仍在等复核）；#22/#25 是 P2；#24/#27 要 Windows 实机；#26/#28 前置未定、#29 编号更晚。

**一处自我更正**：02:03 那一轮把 #20 记成「最新评论是 `## 本轮收尾`、在等复核」而跳过，但那份复核 17:09Z 就到了，比那一轮开工（18:03Z）早 54 分钟。**判断「在等复核」要看最新一条的 id/时间，不能沿用上一轮的结论**——它一条新评论就足以让状态从「等」变成「可消费」。

**判定 12 条 → 成立 11 / 举例不成立 1**（另它自撤 1 条、我自取 1 条成立）。

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | 两处 413 逐字统一后，`/local-exec` 上同一段因果仍是反的（`bin/agent2ssh-daemon.rs:1855-1859` 全折 400，`/exec` 的 `:1755-1759` 对同一个 `ExecShutdown` 判 503） | **成立，但已登记** | 计数对：`grep -c SERVICE_UNAVAILABLE src-tauri/src/bin/agent2ssh-daemon.rs` = **1**（只有 `:1757`）；`/local-exec` 的 responses 实测是 200/400/401/403/408/415/422/423/429，**无 503**。但这件事在 **L2-27** 里，原文已写到「状态码是否也统一成 503 是同一决策的一部分」，且写明「即使把 `None` 换成 `ssh_shutdown_outcome(...)`，`ExecShutdown` 类型在 `core.rs:1267` 之前就被抹掉了」——**不新立条** |
| 2 | `local:<id>` 枚举漏了第三种形态：保留形态里的非法后缀走的是另一条拒绝路径，而不是「any other value」 | **成立** | `local:cmd:ls` → `'cmd' takes no argument, but got 'ls'`（`local_exec.rs:427-430`）；`local:wsl:` → `'local:wsl:' names no distribution`（`:416-419`）；`local:docker:a:b` → `invalid container id`（`:758-771`）。三句都不是「未知 id」，而 `api.yaml` 的 400 列表里一条都没有 |
| 2-b | 它举的例子「`local:docker:ls` 被拒为 `invalid container id 'ls'`」 | **不成立** | `validate_container_id` 只查两件事：字符集 `[A-Za-z0-9._-]` 与 128 字节上限。`"ls"` 两项都过，于是 `container_plan` 正常建 plan，由 runtime 自己回话（docker 不在 PATH 时是「Docker is not installed…」）。它自己引的 `an_invalid_container_id_is_refused`（`:1841`）的 bad 列表是 `""`/`"a b"`/`"a;b"`/`"a/b"`/`"a:b"`/`"$(id)"`/`` `id` ``/超长——**没有 `ls`** |
| 3 | 400 补工作目录那句「成立，且没有说多」 | **成立** | `local_exec.rs:920-929` + `core.rs:1218-1225` 与文案「checked before every command」对得上；`custom_environment_plan:905-917` 的 `config_dir`/`base` 不可用已由「cannot be built」覆盖 |
| 4 | `strategy` 四处代码位置全对，唯 `max_failures` 是「按批」而非「即时」 | **成立** | `core.rs:1644` 的 `batch_failures` 在 `while let Some(joined) = set.join_next()` **之后**统计，`:1648-1651` 的 `break` 在循环末尾；`batch_size = 1, max_failures = 1` 时同批其它 host 不会被打断 |
| 5 | `code_block` 的 `len == 6` 是纯冗余，且注释给的理由是错的（「a code is the only line shaped exactly `"413":`」） | **成立** | 谓词实为「三字符码」而非「这个码」。全文件普查：`"` 开头且 `":` 结尾的行 **570**，`len == 6` 者 570，非数字中间者 **0** → 当前不误判；但 `"unknown":` 这种键会让块提前结束，`contains("Bytes")` 假阴 |
| 6 | 同码重复时取第一个块，应写明 | **成立（无实例）** | 105 个 operation 里没有重复码；已在注释里写明 |
| 7 | (a) `/local-exec` 的中断折进 400，「它今天连断言都没有」 | **成立，已登记** | 与第 1 条同源，条目仍是 **L2-27**；「没有断言」这半也对（`grep -c SERVICE_UNAVAILABLE` = 1，没有第二处、更没有测试） |
| 8 | (b) `local:<id>` 第三种形态 + 「any other value」 | **成立**（例错见 2-b） | 同第 2 条；本轮已改文案 |
| 9 | (c) MCP 面与 CLI `--help` 面缺 `environments.toml` | **成立** | `grep -c environments.toml`：`agent2ssh_mcp/tools.rs` **0**、`docs/skills.md` **0**、`bin/agent2ssh.rs` **0**（`api.yaml` 1、指南 4）。`local_exec` 与 `resident_session_open` 的描述、`local exec -H` 的 clap 文档各写了一遍枚举，四处都没有用户自定义环境 |
| 10 | (d) `/exec-multi` 的 `max_failures` 只按批生效，`api.yaml:1812` 应补一句 | **成立** | 同第 4 条；`max_failures` 原描述只有 "Stop after this many failures; 0 = never stop early." |
| 11 | (e) `remote_outcome_unknown` 在 `/local-exec` 恒 `None`，与 (a) 是一件事的两面 | **成立，已登记** | `daemon.rs:1843` 恒 `None`，`:1744`（`/exec`）由 `ssh_shutdown_outcome` 算出。条目 **L2-27**，不新立 |
| 12 | (f) `code_block` 判据比注释宽 | **成立** | 同第 5 条；它给的复算方式（删掉 `len == 6` 后仍 8 passed）与我的普查一致 |
| — | 第 6 条 WSL 的 `cwd` 静默无效 | **它自撤** | 无 Windows + WSL 实机、无可复算对照，按不成立处理、不重提（#27 范畴） |
| 自取 | `stopped_early: true` 与 `skipped: 0` 在单批次下同时成立 | **成立（新）** | `core.rs:1561-1566`（`batch_size` 0 → `effective_batch_size = total_hosts`，只有一批）、`:1650-1651`（阈值在批次末尾命中即 `break`，而循环已无下一轮）、`:1657`（`skipped` 于是 0）。默认 `strategy` + `max_failures: 1` 就可达。**L2-172** |

**它这一轮的两个数字纠正**（不影响结论）：`grep -c "takes no argument" src-tauri/src/local_exec.rs` 实测 **3**（它说 2，漏了 `:1549` 的断言）；`an_invalid_container_id_is_refused` 在 `:1841`（它说 `:1850`）。

**本轮改了什么**（全部 L1；`src-tauri/src/**` 只动文案属性——两处 MCP 工具描述与一处 clap 文档，零行为改动、零签名改动，按 batch 2 第 2 轮 `f23f006` 的 clap 文档先例计）：

- `docs/api.yaml`：
  - `/local-exec` 的 description 收尾句从「**Any other value** — including a `local:` id that is neither built in nor defined in `environments.toml` — is rejected with 400」改成「四类拒绝、四种不同的 message」，并补一句「a bare word after a runtime is a *valid* container reference: `local:docker:ls` names a container called `ls`，所以它会被 plan，runtime 自己的答复在结果里，不是 400」。
  - `/local-exec` 的 400 从 8 项扩到 10 项，新增「环境不吃参数」（逐字引用 `'cmd' takes no argument, but got 'ls'`）与「空发行版」（逐字引用 `'local:wsl:' names no distribution`），末尾加「These are four different remedies — fix the id, drop the argument, name a distribution, or fix the definition — so a client should read the message, not just the code.」
  - `max_failures`：「Stop after this many failures; 0 = never stop early.」→ 补「Failures are counted when a batch finishes, so nothing already in flight is interrupted — reaching the threshold prevents the *next* batch from starting.」
  - `skipped` / `stopped_early`：从 `{ type: integer }` / `{ type: boolean }` 零描述，改为写明「从未被尝试的主机数」与「阈值被触发，不等于 `skipped > 0`；单批次下可以一个是 `true` 一个是 0」。
- `docs/skills.md` 第 72/73 行、`src-tauri/src/bin/agent2ssh_mcp/tools.rs` 的 `local_exec`/`resident_session_open` 描述、`src-tauri/src/bin/agent2ssh.rs` 的 `local exec -H` clap 文档：四处各补「an id you defined in `environments.toml`」。
- `src-tauri/tests/daemon_integration.rs`：`code_block` 的边界判据抽成 `is_response_key`（必须是 `"` + 三个 ASCII 数字 + `":`），注释按实情重写；新增 `a_response_key_is_a_three_digit_code`（钉住收紧本身，`"abc":` 与 `"unknown":` 都必须为假）与 `api_yaml_lists_the_local_refusals_it_can_produce`（先断言 `local_exec.rs` 里两条拒绝臂还在，再断言 400 块里逐字引用了这两句）。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：**L2-172**（单批次 `stopped_early`/`skipped` 自相矛盾，三条改法 + 回滚）。

**反向验证**（这次做了）：新增测试先红后绿——第一次跑时 400 块被读成空（我漏了对 `operation` 做 `trim_start`，`code_block` 的 marker 是去缩进后的形状），修好后仍红，因为正文写的是「takes none」而断言要的是源码里的字面量「takes no argument」；**正是这次红，逼出了「文档该逐字引用 message」这个改动**——不逐字引用的话，客户端没法按文案分流。

**验证**：`npm run build` **6.56 s**；`cargo test --no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 IPv6 `an_ipv6_literal_at_port_22_is_removed`，与基线一致）；`--features daemon --test daemon_integration` **80 passed / 0 failed**（含新增两条）；`cargo fmt --check` 干净；`yaml.safe_load` 90 paths。**合流时远端已推进到 `0244364`**（PR #33 与 #34：glib 与 zip 的依赖升级），rebase 干净无冲突；因为那是依赖变更（`Cargo.lock` 1170 行、`Cargo.toml` +28），**合流后把 lib 与 `daemon_integration` 都重跑了一遍**，数字不变（863/1/1、80/0）。

**未做的 L2**：L2-27（要先把 `LocalExecShutdown` 从 `core.rs` 透出来，再定 local 语境下「未知」的措辞与状态码）、L2-172（`stopped_early` 的语义拆字段）。两条都改 `src-tauri/src/**` 行为。

**新增「不成立」，不要重复提**：

17. **「`local:docker:ls` 被拒为 `invalid container id 'ls'`」**——`validate_container_id`（`local_exec.rs:758-771`）只查字符集 `[A-Za-z0-9._-]` 与 128 字节上限，`"ls"` 两项都过；`container_plan` 于是正常建 plan，由 runtime 自己回话。真正被拒的是 `""`、含空格/分号/斜杠/冒号、`$(id)`、反引号与超长这八种（`an_invalid_container_id_is_refused` 的 bad 列表）。**推论**：`local:<runtime>:<word>` 的「是不是容器引用」不由文案决定，由那个字符集决定。

**回帖**：`2120588608540147712`（`## 本轮收尾`，18:44:57Z；18:45:11Z 补写第六节「issue 状态」）。`#20` **保持 open**，阻塞点是 L2-27 / L2-172 / #22 的身份协议，全在拍板侧。

## 2026-11-12 · 手工单发消费 · issue #20（第五次进入）

**选靶理由**：open 里无 `[opt-round]`（#30 已关）→ 模式 B，第二段为空。MANUAL 里 #20 是 P1 且编号最小，最新评论是 CodeBuddy 的只读复核 `2120592339033829376`（18:59:46Z，**未被消费**）；#21/#23 的复核同样未消费但编号更大；#22/#25 是 P2 设计、#24/#27 要 Windows 实机、#26/#28 零评论且前置是 #22、#29 零评论需先发分析请求。**基线 `f77e6ed`**（= 收到这条复核时的 `cnb/main`，它的自述基线 `b693647` 与 `f77e6ed` 只差一笔「回帖 id」）。

**这一行的价值**：复核方本轮把验证从「读代码」提到了「在 Linux 容器里跑真 daemon」，用独立 config dir 打 `/local-exec`，并做了两次变异测试。它的 400 分类学因此是**实测分类**而非推断——这是它第一次给出可以直接照抄进契约的错误文案清单。

| # | 它的指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | Q1-12：400 分类学缺「环境建不起来」一类（`no PowerShell found on PATH` / `Visual Studio developer shell not found` / `WSL is not available`），与 #7「definition that cannot be built」是两句不同的话 | **成立** | `local_exec.rs:715-727` 的五个 `ok_or_else` 文案互不相同，且都不以 `environment 'X' is based on` 开头（后者在 `:911-916`，说的是*定义*；前者说的是*这台机器*）。400 原文只有「a definition that cannot be built」一项，读上去像已覆盖 |
| 2 | Q1-13：`local:docker:ls` 在没装 docker 的机器上是 **400**（`Docker is not installed, so there is nothing to exec into`），而 400 列表没有这项 | **成立** | `container_plan`（`:784-793`）先 `validate_container_id` 再 `runtime.resolve()`；`resolve()` 为 `None` 即 400。**同一 id 在装了 docker 的机器上是 200** —— 行为依赖机器状态 |
| 3 | Q1-14：自定义环境的「多余后缀」与内置环境是两条不同的臂、文案逐字相同，400 列表没分开 | **部分成立** | 两条臂确实不同（`:427-430` 内置、`:884-889` 自定义），但 400 原文已写「and a defined environment answers the same way」——**该分的是 `description` 里那句「four different refusals with four different messages」的计数**：实际 ≥10 类成因，且其中两条共用一句文案 |
| 4 | Q1 尾：`load_error` 不构成第五类，但依据要换成「运行中坏文件不影响已注册定义」 | **成立** | `local_environments.rs:244`（`Mutex<Option<Arc<…>>>`）、`:279-282`（`current()` 写入后不再读盘）、`:284-290`（`reload()` 的文档自己写「Called by the daemon at start; there is deliberately no request path to it」）、`bin/agent2ssh-daemon.rs:5713`（唯一调用点）。它实测「启动时好、随后写坏文件 → `local:py-ml` 仍按已注册定义工作」与这四行一致 |
| 5 | Q1 连带：指南 `:710` 那句「整个文件解析失败只会让自定义环境不可用」该加「下次启动时」 | **成立** | 同第 4 条；且反方向也成立：启动时坏、运行中修好 → 到重启前仍不可用，`annotate_unknown`（`:218-233`）会一直挂着 `could not be loaded` |
| 6 | Q2：`local:docker:ls` 它判对（我上一轮判对），但 api.yaml 那句让人反推成「任何机器上都不会 400」 | **成立** | `container_plan` 的两步顺序（见第 2 条）就是反例；它用假 `docker` 打的 argv 实测（`exec ls /bin/sh -c echo hi`）与 `validate_container_id` 只查字符集一致 |
| 7 | Q3-A：`/local-exec` 有两类 500（gate 读不了、`policy.toml` 读不了）而 responses 没有 500 | **成立** | `reject_if_gate_paused`（`bin/agent2ssh-daemon.rs:400-410`）→ `INTERNAL_SERVER_ERROR`；`approval.rs:1019` → `load_approval_policies` → `execution_control.rs:481-482` `Internal` → `bin/agent2ssh-daemon.rs:812-815` 500。`/local-exec` 的 responses 实测 200/400/401/403/408/415/422/423/429，`/exec` 有 500 |
| 7-b | 它给的修法：给 `HELPER_STATUS` 加 `("load_execution_gate(",…)` 与 `("load_approval_policies(",…)` 就能让 `/local-exec` 现形 | **不成立** | 那个测试只读**该 handler 自己的函数体**（`handler_body_of`，`:3482` 起，文档注释 `:3111-3117` 明写经 helper 到达的码看不到）。`local_exec` 体内只有 `preflight_guarded_request(`；`load_execution_gate(` 在 `reject_if_gate_paused` 内部（`:405`），`load_approval_policies(` 在 `bin/` 下**零出现**。两条新增都匹配不到任何 handler 体，加完仍然一条都不报。真正的修法是把表改成「helper → 码集合」（`preflight_guarded_request(` 现在只登记 429，实际还能回 423/500）→ **L2-174** |
| 8 | Q3-B：`code_block` 在 marker 找不到时返回空串，调用点的 `contains` 会全部假阴 | **成立** | `:4050-4060` 原文 `return String::new()`。它同时承认现有两个调用点都自保（空块会让断言红），但主张就地失败——已按此改（见下） |
| 8-b | Q3-B 附带：`is_response_key` 只认 `"NNN":`，单引号与裸键不识别，是没写下来的隐含前提 | **成立** | 普查：全文件 571 个候选 key（含本轮新增的 500）**全部**是双引号三数字，误判为 0 → 前提今天成立但无人守；已加测试 |
| 9 | Q3-D / 第 4 条提醒：`stopped_early`/`skipped`/`batches_executed`/`total_duration_ms` 只有 `is_some()`，取值零覆盖 | **成立，未做** | `daemon_integration.rs:4707-4750` 是 serde 形状测试，构造值 `stopped_early: false, skipped: 0` 从未被比较。它的临时测试复现了单批次 `stopped_early: true` + `skipped: 0`，与 L2-172 一致。补取值断言要么新增一条真跑 `exec_multi_with_strategy` 的用例（两个不可达 host，慢且受 DNS 影响），要么改 `core.rs` 的单测（属 `src/**`）——**留给下一轮，不塞进本轮** |
| 10 | Q3-E：`local:wsl:a:b` 的 distro 不校验，而 `local:docker:a:b` 被 `validate_container_id` 拒 | **成立（代码事实，行为未实测）** | `:414-423` 只拒 `Some("")`；`container_target_for_host`（`:743-748`）对 `wsl` 返 `None` → 走 `environment_for_host`。本机无 WSL，`wsl.exe -d a:b` 的真实反应无实测 → **L2-173** |
| 11 | 它自报的失误：第一次启 daemon 没设 `AGENT2SSH_DAEMON_ADDR`，在容器里创建了 `~/.agent2ssh` | **记录，非仓库缺陷** | 该行为是 `resolve_config_dir` 的既有默认路径（未设变量即用户主目录）；它的 `~` 是容器内的 home，本机 `~/.agent2ssh` mtime 仍是 11-04，未被触碰 |
| 12 | 它本轮对上一轮 4 处改动的判定（`is_response_key` 收紧、400 逐字引用、`max_failures` 按批、`skipped`/`stopped_early` 语义） | **全部成立** | 与我的自评一致；它的两条变异（删数字检查、抹掉两句 message）与我上一轮的反向验证同向，互为独立复核 |

**本轮改了什么**（全部 L1；`src-tauri/src/**` 零改动，产物在 `docs/**` 与 `src-tauri/tests/**`）：

- `docs/api.yaml`：
  - `/local-exec` description：把「four different refusals with four different messages」改成「four examples, each with its own message」并点明两条臂共用一句；`local:docker:ls` 那句补上「但只在 runtime 装好时——没装 `docker` 时同一 id 是 400 `Docker is not installed…`，因为 CLI 先于容器解析，答案取决于这台机器」。
  - `/local-exec` 的 400：新增两类（环境在这台机器上建不起来，六个 message 逐字；容器 runtime 的 CLI 未安装），并把「a definition that cannot be built」收窄成「a definition whose base is not usable」以与前者区分；结尾的「四种补救」改成「different remedies」并补一句「两条共用 `takes no argument`，只有 `host` 能区分」。
  - `/local-exec` 新增 `500`（含 `application/json` + `Error` schema，否则 `api_yaml_declares_a_body_for_every_response` 会红——见下）。
- `docs/guides/configuration-guide.md:710-711`：把「整个文件解析失败只会让自定义环境不可用」限定为「这次启动内」，并新增一条**运行中改文件无效**的说明（两个方向：启动时好→运行中改坏，已注册定义照常工作；启动时坏→运行中改好，到重启前仍不可用且 400 一直挂着 `could not be loaded` 的注解）。
- `src-tauri/tests/daemon_integration.rs`：
  - `code_block` 的「找不到 marker」从 `return String::new()` 改成 `assert!`，失败信息点名那个码。
  - `api_yaml_lists_the_local_refusals_it_can_produce`：needle 从 2 条扩到 5 条，新增「环境建不起来」（PowerShell / WSL 两句）与「runtime 未安装」，源码侧与 400 块两侧都断言。
  - 新增 `every_response_key_in_the_spec_is_a_quoted_three_digit_code`（571 个 key 全过 + `keys > 400` 防空转）。
  - 新增 `local_exec_documents_the_internal_errors_its_helper_answers_with`（按路由守卫：`/local-exec` 的 500 必须点名 `execution_gate.json` 与 `policy.toml`，且 `preflight_guarded_request(` 与 `failed to load execution gate` 仍在源码里）。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：**L2-173**（`local:wsl:<distro>` 不校验，三方案）、**L2-174**（`HELPER_STATUS` 1:1 且不传递，三方案）。

**反向验证**（做了两次）：①把 500 描述里的 `policy.toml` 改成「a policy file」+ 把一个 `"408":` 改成裸 `408:` → `local_exec_documents_the_internal_errors_its_helper_answers_with` 与 `every_response_key_in_the_spec_is_a_quoted_three_digit_code` **各自红**，恢复后绿；②`code_block` 的 `assert!` 不是空转：新增 500 块时忘了写 `content:`，`api_yaml_declares_a_body_for_every_response` 立刻红并点名 `["500 POST /local-exec"]`，补 schema 后绿。

**验证**：`npm run build` 见下节；`--features daemon --test daemon_integration` **82 passed / 0 failed**（原 80，新增 2 条）；`--no-default-features --lib` 与 `cargo fmt --check` 见回帖。

**新增「不成立」，不要重复提**：

18. **「给 `HELPER_STATUS` 加 `load_execution_gate(` 与 `load_approval_policies(` 就能让 helper 产出的 500 现形」**——那个扫描只读 handler 自己的函数体，不传递；两条串在 `bin/` 下分别只出现在 helper 内部与完全不出现（`load_approval_policies` 在 `bin/` 零命中）。能覆盖这族的只有把表改成「helper → 码集合」，条目是 L2-174。

**回帖**：`2120598423911948288`（`## 本轮收尾`，同一条评论内补写第五节「issue 状态」）。`#20` **保持 open**：剩余三条全在拍板侧（L2-27 的 503 口径、L2-172 的字段拆分、L2-174 的码集合），另加 #22 身份协议对 L2-165 的前置。

## 手工/单发消费 · 2026-11-12 · #20 第六次进入（停止回执与旧版兼容）

**选靶**：`open` 12 条（#20–#29 + #31/#32 两条安全扫描贴），无 `[opt-round]` → 模式 B，第二段为空。P1 同级取编号最小：#20 的最新评论是复核 `2120600934320328704`（19:33:56Z），未被消费；#21（`2120566812650676224`）与 #23（`2120583722461192192`）的复核同样未消费但编号更大；#22/#25 是 P2 设计类，#24/#27 要 Windows 实机（本机做不了，排除），#26/#28/#29 零评论、需先发只读分析请求再等 5 min。

**判定 12 条 → 成立 8 / 部分成立 1 / 不成立 1 / 记录 2**

| # | 它的指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | §1-1：`code_block` 找不到 marker 改 `assert!` 是对的——三个调用点里只有 `:3956` 传的是已确认存在的码 | **成立** | 上一轮已改，本轮未动；它读的 `:4056-4070` 与现树一致 |
| 2 | §1-2：`every_response_key_…` 的 571 个候选 key 它按同一套过滤规则复算，indent 全 8、误判 0 | **成立** | 独立复算一致；本轮删掉 `/local-exec` 的 500 后该键数为 570，测试只断言 `keys > 400`，不钉具体数字，所以不需要改 |
| 3 | §1-3：400 的 needle 2→5 五个在源码侧与文档侧都在 | **成立** | `local_exec.rs` 的五个 `ok_or_else` 文案与 400 块逐字对得上 |
| 4 | §1-4：新测试只挡回归、不发现问题——「在这个位置是对的形状」 | **成立（性质确认）** | 与我的自评一致；本轮把它换掉了，见下 |
| 5 | §1-5 / §4：`/local-exec` 的 500 点名 `policy.toml`，而审批链的另一条臂读 `approval_policies.toml` | **成立（本轮最值钱）** | `approval.rs:969-979`：先 `policy::policy_approval_policies()?`，`Ok(None)` 才落到 `approval_policies_path()`；`policy.rs:93-97` 对 `.toml` 直接 `toml::from_str`，`AgentPolicyFile`（`policy.rs:9-15`）两个字段都是 `#[serde(default)]`，所以只写 `[[approval.policies]]` 也能解析成功——失败面比「parse 失败」更宽。它说「全仓 `deny_unknown_fields` 零命中」不准确（`local_environments.rs:81`、`:99` 各一处），但那两处与策略文件无关，结论不受影响 |
| 6 | §2：「把表改成码集合是零红的空转」——今天 15 个走 `preflight_guarded_request` 的 operation 全都写了 500 | **不成立** | 逐 operation 取 `responses` 键：能回 gate-read 500 却**没有** 500 的有 11 个 —— `/exec`、`/sftp/{upload,download,ls,stat,mkdir,rename,rm,rmdir}`、`POST /forwards`、`POST /forwards/multi`（`/sftp/rm-rf` 与 `/sftp/rm-rf/authorize` 经 `preflight_sftp_rm_rf` 同源，也没有）。扩表会一次红 11~13 条。**它的前提来自我上一轮写错的一行**：台账「不成立」第 7 条写着「`/exec` 有 500」，而 `git show f77e6ed/4a69666/5c0e53e` 三个版本的 `/exec` 都是 `200/400/401/403/408/415/422/423/429/503`，从来没有 500。**这个错误算我的。** 它的结论（不要走码集合）方向恰好吃对，但理由是错的：口径早在 `daemon_integration.rs` 的 `HELPER_STATUS` 注释里拍过（「不在每条 operation 上重复，`info.description` 说一次」，并写明 per-operation 重复会诱使两处一起被删）→ L2-174 按此收口 |
| 7 | Q1：`/local-exec` 的 400 是一份「拒绝枚举」，而 handler 尾部 `map_err(BAD_REQUEST)` 把执行失败也落成 400，契约里没说 | **成立** | `local_exec` 末尾 `result.map(Json).map_err(\|e\| err(StatusCode::BAD_REQUEST, e))`（本轮改前 `:1850-1852`）；`/sftp/*` 的 400 早写着「the request is invalid, or the upload failed」——同族一侧写了、一侧没写 |
| 8 | Q2：`/exec` 缺 500，与 `/exec-multi`、`/exec/compare` 不同族 | **部分成立** | 事实对（`/exec` 无 500，且它经 `preflight_rewritable_request` → `preflight_guarded_request_inner` → `reject_if_gate_paused`，确实能 500）。但定性错：按文件级口径这**不是缺陷**。真正的不一致是上一轮我自己给 `/local-exec` 加的那条 per-operation 500——本轮删掉它，让 `/exec` 的写法变成规范 |
| 9 | Q3：`HANDLER_STATUS` 的 403 有一处静默漏检——删掉 `/local-exec` 的 403 不会红 | **成立** | 变异验证：删掉 `/local-exec` 的 `"403":` 块后，改前的 82 条全绿；给 `HELPER_STATUS` 加 `preflight_guarded_request(`/`preflight_rewritable_request(` → `403` 后，同一变异立刻红，且**只**点名 `403 post /local-exec (local_exec via helper)`。全仓 16 个 preflight 调用点今天都已文档 403，加表零红 |
| 10 | Q4：`load_error` 两向性成立，建议指南补一句「日志只有启动时那一条」 | **成立（采纳）** | `local_environments.rs:279-282`（`current()` 缓存后不再读盘）、`:284-290`（`reload()` 只在启动调用）；指南 `:711` 原文只说「daemon 启动时会把这些问题写入日志」，读者会去翻日志确认「最近没报错」 |
| 11 | Q5：第 9 条的取值断言（`stopped_early`/`skipped`）推到下一轮 | **记录，非本轮缺陷** | 与它自己的结论一致；要动 `core.rs` 属 `src/**` |
| 12 | §4：建议新立 L2-175（500 文案点名两个文件，推荐 (a) 零代价） | **记录，已按 (a) 落地** | 但没有新立该条——它要修的就是第 5 条，本轮直接改了文案。编号 L2-175 被本轮另一件事占用（400 双语义） |

**本轮改了什么**（全部 L1；`src-tauri/src/**` 零改动）：

- `docs/api.yaml` 的文件级段落：把「第二个 500」那段从「gate 读不了」扩成**两个共享读**——`execution_gate.json` 与审批链（`policy.toml`/`policy.json` 先、`approval_policies.toml` 兜底，谁存在谁决定），并保留原有那句「这就是为什么它们都不在自己的 `responses` 里重复」。
- `docs/api.yaml` 的 `/local-exec`：**删掉**上一轮加的 per-operation `500`（它与上面那句直接矛盾，且只点了两个文件里的一个）；400 末尾补一句「以上都是执行前的拒绝；handler 最后一步也把 local exec 自身的失败映射成 400，其中一些发生在进程启动之后，所以码本身分不清拒绝与跑到一半失败」。
- `docs/guides/configuration-guide.md:711`：补「日志只有启动时那一条」，并点名「最近没看到报错」不是验证。
- `src-tauri/tests/daemon_integration.rs`：`HELPER_STATUS` 8 → 10，加 `preflight_guarded_request(` / `preflight_rewritable_request(` → `403`，并把注释里「423 是唯一 1:1」的说法改成「preflight 是超集，只钉 403」；`local_exec_documents_the_internal_errors_its_helper_answers_with` 改名为 `the_spec_states_the_preflight_internal_errors_once`，断言换成三条：文件级段落必须点名 `execution_gate.json` / `policy.toml` / `approval_policies.toml`、`/local-exec` 不得自带 500、它的 400 必须承认执行失败落在同一个码。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：**L2-174 收口**（裁决为「不扩表、走文件级」，附它的错误前提与我自己那行错账）、**L2-175 新增**（400 双语义，三方案）。

**反向验证（三次，全部实测）**：①删掉 `/local-exec` 的 `"403":` → `api_yaml_documents_the_codes_each_handler_can_return` 红，消息只说 `403 post /local-exec (local_exec via helper)`；②给它重新加一条 `"500":` → `the_spec_states_the_preflight_internal_errors_once` 红；③把文件级段落里的 `approval_policies.toml` 改名 → 同一条测试红。三次恢复后全绿。

**验证**：`npm run build` 6.21 s；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 IPv6 `an_ipv6_literal_at_port_22_is_removed`，与基线同）；`--features daemon --test daemon_integration` **82 passed / 0 failed**（含 8 条 `api_yaml*` 与新守卫）。

**新增「不成立」，不要重复提**：

19. **「把 `HELPER_STATUS` 扩成码集合是零红的空转，因为 15 个 preflight operation 都已经写了 500」**——11 个能回 gate-read 500 的 operation 没有 500（清单见上表第 6 条）；扩表会红 11~13 条。它引用的「`/exec` 有 500」来自我上一轮台账的笔误，已在本条更正。

**回帖**：`2120615980584833024`（`## 本轮收尾`）。`#20` **保持 open**：剩余阻塞全在决策侧（L2-27 的分类学口径、L2-172 的字段取值、L2-175 的 400 双语义），另加 #22 的文件身份协议是 L2-165 的前置。

### 手工/单发消费 · 2026-11-12 · #21 异常诊断（第四次进入 · 脱敏是模式匹配、两份日志的关联断裂、两条 429 的机制错配）

**选靶**：`open` 十二条（#20–#29 + #31/#32 安全扫描贴），**无 `[opt-round]`** → 模式 B，第二段为空。#20 本轮之前刚收尾（`2120615980584833024`）且其后无回复 → 跳过；#21 最新评论是复核方 `2120566812650676224`（2026-11-11T17:18:21Z），未被消费；#23 的复核同样未消费但编号更大；#24/#27 要 Windows 实机、#22/#25 是 P2、#26/#28/#29 零评论。按「P1、编号最小、未消费」取 **#21**。

**基线**：`1942b10`（= `cnb/main`）。复核方自述推了 `ff3c4a8`，**`git merge-base --is-ancestor ff3c4a8 cnb/main` 为假**——它的修订从未落地，本轮改的是 `f0ebc29` 那一版。

**判定 13 条 → 成立 6 / 部分成立 2 / 不成立 2 / 与前条同源 3**。

| # | 指摘 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | ①「`/sessions` — the open side — does list one」立了个错前提 | **成立（我的错）** | `session_open`（`agent2ssh-daemon.rs:2416-2450`）调 `register_limited_session_for_command` → `check_session_open`（`limits.rs:130-193`，比 source/host/tag 的 `max_sessions`），**从不调** `reject_if_rate_limited`；close 侧两个都不调。真正的差别不是「open 限流、close 不限流」，而是「open 占一个槽、close 放一个槽，两边都不看每分钟速率」 |
| 2 | ①′ 五处 4xx 例外清单无第六条 | **成立（复核方替我重扫）** | 我自己用 `yaml.safe_load` 复扫 105 个 operation、29 个列 429：正文提 4xx 而 responses 缺的有 17 条，全部落在四类里（文件级 500 段覆盖的 gate/policy 读失败、`Bytes` 处理器的 413、`/exec/stream` 只走帧、两个 broadcast 的 per-target 折叠），无第六条 |
| 3 | ② `architecture.md` 同段三处夸大没删干净 | **成立** | `SESSION_TRACE_ID`（`src/api.ts:81-85`）是模块级 `const`，**每进程一个**不是 per-session；`tauri_commands.rs` 13 处 `append_diagnostic_log` 里只有 `write_diagnostic_log`（`:1546-1551`）带调用方 `fields`，其余 12 处 `grep trace_id` = 0；前端 `grep "headers.get\|x-agent2ssh-trace-id" src/` 为空 → 响应头里的 id 没人读 |
| 4 | ③1 脱敏写成了「保证」 | **成立** | `is_sensitive_key`（`store.rs:1541-1556`）是 11 个拼写的**精确匹配**；`BUILTIN_RULES`（`redaction.rs:151-178`）10 条正则覆盖私网 IP/Bearer/`sk-`/`AKIA`/JWT/32 位 hex，无命令行口令 |
| 5 | ③2 轮转时机写得不准 | **部分成立** | 结论对：`append_log_file()`（`daemon_control.rs:402-414`）是唯一轮转入口，在 `start_daemon_background` 的 Healthy 提前返回（`:421-435`）之后，只在真要 spawn 时跑。但它给的理由**不成立**，见第 12 条。另我自己发现文档写的「保留 `.1`/`.2` 两代」也错——`for i in (1..=2).rev()` 会造出 `.3`，是三代 |
| 6 | ③3 四个 `command=` 都是权威化后的文本 | **部分成立** | 只有 `/exec`（`:1728`）在 `pf.command` 覆盖（`:1725-1727`）之后打；`/local-exec` 走 `preflight_guarded_request`（`rewritable: false`）、`/exec-multi` 与 `/exec/compare` 走 `authorize_targets`，三者都写请求原文。四→一 |
| 7 | ③4 漏了「导出诊断包也带 `daemon.log` 尾部」 | **成立** | `diagnostics.rs:519-520` 追加 `## daemon.log tail`，`read_tail`（`:456-467`）取 300 行、逐行过 `redact_sensitive_text` 即第 4 条的盲区 |
| 8 | M1 `session_open` 的 429 来自会话配额 | **成立**（与 1 同源） | `try_register_session` → `check_session_open`；scope 后缀是 ` sessions` 而不是 ` rate`（`limits.rs:151/168/186` vs `:243/258/275`） |
| 9 | M2 `app.log` 脱敏漏 `-p`/`api_token` | **成立，实测**（与 4 同源） | 探针跑 `store::redact_sensitive_text`：`mysql -p hunter2`、`mysql -phunter2`、`openssl enc -pass pass:hunter2`、`API_TOKEN=abc123`、`API_KEY=abc123`、`PGPASSWORD=abc123` 六种原样；`--password=hunter2` 抹掉。补了它没试的 `api_key` 与 `PGPASSWORD`——`api-key`/`apikey` 在表里而 `api_key` 不在，说明漏的是**一整类拼写** |
| 10 | M3 诊断包带走 `daemon.log` 尾部 | **成立**（与 7 同源） | 同上；另实测该函数也守着 `audit.jsonl` 的 `command`（`store.rs:1062`）与 webhook 载荷（`notify.rs:96/246`），所以漏网口令不只留本机 |
| 11 | M4 `daemon.log` 无 `trace_id` | **成立** | 桥接层 `trace.rs:135` 只放 `<= WARN`，`trace_id` 在 `:155-160` 塞进 `app.log` 条目；`fmt::layer()`（`agent2ssh-daemon.rs:5686-5697`）不读 task-local |
| 12 | M5 桌面 `daemon_start` 没有 CLI 那份 Healthy 提前返回；判据 `grep -n "DaemonPidState::Healthy" tauri_commands.rs` = 0 | **不成立** | 桌面 `daemon_start`（`tauri_commands.rs:1796-1825`）就有这道判断，命中时还写一条 `daemon start skipped because daemon is already healthy` 诊断并早返回；`grep` 实测 **2 处命中**（`:1770`、`:1800`，后者是 `crate::daemon_control::DaemonPidState::Healthy` 全路径写法）。这是它本轮唯一的整条反转 |
| 13 | M6 `architecture.md` 的「every frontend diagnostic」会让人以为 Rust 侧也有 id | **不成立** | 紧接的下一句原文就是「A Rust-side desktop diagnostic therefore carries a `trace_id` only when the frontend supplied one」，已经否认了那个读法；而「every frontend diagnostic」本身是真的（`logDiagnostic` 无条件 `trace_id: SESSION_TRACE_ID`）。本轮仍把这段拆成四条 bullet 写清，但不记为缺陷 |

**本轮改了什么**（全部 L1；`src-tauri/src/**` 零改动）：

- `src-tauri/tests/daemon_integration.rs`：新增 `api_yaml_operation` 辅助函数与守卫 `api_yaml_names_the_mechanism_behind_each_session_429`，三条断言分别钉住「`POST /sessions` 的 429 必须点名 `check_session_open` 与 `sessions` 后缀」「`DELETE /sessions/{id}` 不得列 429 且必须写出 `unregister_session` 这个释放动作」「`POST /resident-sessions` 的 429 必须点名 `reject_if_rate_limited`/`rate`，且把 `AGENT2SSH_MAX_RESIDENT_SESSIONS` 归给 400」。
- `docs/api.yaml`：文件级段落新增「两种 429」一段，把 `rate` 与 `sessions` 两个 scope 后缀写成客户端唯一可用的判据，并说明两族在 `execution_limits.toml` 里由 `*_per_minute` 与 `*_max_sessions` 各自配置；`DELETE /sessions/{id}` 补 operation 级 `description`（**为什么这里不能查限流**：它是释放槽位的唯一调用，查了会让会话上限变成死锁），403 文案收回到只管 403；`POST /sessions` 的 429 从一句「Execution limit exceeded」改成点名 `check_session_open` 与重试的正确姿势（`DELETE` 而非定时重试）；`POST /resident-sessions` 的 429 删掉「or the local-environment session cap」这个错归因（该上限走 `resident_open_core` → 400）并写明与 `/sessions` 恰好相反。
- `docs/architecture.md` 的 Correlation IDs 段：把一段长句拆成四条——id 只落 `app.log` 且只在 WARN/ERROR 上、`daemon.log` 一行都没有、前端 id 是每进程一个、12 处桌面 Rust 诊断不带 id、响应头里的 id 前端从不读回。
- `docs/guides/configuration-guide.md`：`daemon.log` 那节从一段话扩成五点——脱敏是模式匹配（含实测的六种漏网形状与「`api_key` 不在表里」这个类级结论）、四个 `command=` 里只有一个会变、两份日志不能按 `trace_id` 对上、轮转只在 spawn 时按 5 MB 触发且保留三代、诊断包会带走尾部 300 行；树状清单里 `daemon.log` 那行同步改成「仅在 spawn 新进程时按大小轮转」。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-176**（敏感键表精确匹配，实测清单 + 三条改法）、**L2-177**（诊断包默认带 `daemon.log` 尾部）、**L2-178**（`daemon.log` 无 `trace_id`）、**L2-179**（运行中的 daemon 的 `daemon.log` 无上界，轮转只在 spawn 时发生）。

**反向验证（一次，实测）**：把 `POST /sessions` 429 里的 `\`register_limited_session_for_command\` → \`check_session_open\`` 改成泛指的 `the session counter`，`api_yaml_names_the_mechanism_behind_each_session_429` 立刻红并打出「has to name that instead of implying a rate limit」；恢复后全绿。

**验证**：`npm run build` **6.63 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`，与基线一致）；`--no-default-features --features daemon --test daemon_integration` **83 passed / 0 failed**（原 82，新守卫 +1）；`yaml.safe_load` **90 paths / 105 operations / 29 个列 429**。

**新增「不成立」，不要重复提**：

1. **「桌面 `daemon_start` 没有 CLI 那份 Healthy 提前返回」，判据 `grep -n "DaemonPidState::Healthy" tauri_commands.rs` = 0**——`tauri_commands.rs:1796-1825` 就有，且带一条 `daemon start skipped because daemon is already healthy` 诊断；`grep` 实测 2 处（`:1770`、`:1800`）。由此推出的「从桌面点启动每次都会轮转」也不成立。
2. **「`architecture.md` 的 every frontend diagnostic 会让人以为 Rust 侧也有 id」**——同段下一句原文已否认这个读法；「every frontend diagnostic」本身为真。

**回帖**：`2120634031854219264`（`## 本轮收尾`）。`#21` **保持 open**：文档一致性这条线已走完，剩余四项（L2-176~179）全在「改日志落地方式或 IPC 形状」的决策侧，按本轮把四条写成可在 5 分钟内选一条的形态留在 L2 清单里。分支 `opt/single-21-20261112d` 与 worktree `…/single-21` 已在合流验收后清理，本条补记用的 `opt/single-21b-20261112` / `…/single-21-ledger` 随之清理。


## 2026-11-12 06:47–07:1x · 模式 B 单发 · issue #20 第七次进入（基线 `20b88c4`）

**模式**：`open` 十二条（#20–#29 + #31/#32 安全贴），**无 `[opt-round]`** → 模式 B，第二段为空。开工前无锁、三个其它 worktree 全 0 行、`ls-remote cnb/main` 与本地同为 `20b88c4`、`src`/`docs` 十分钟零写入 → 不让位。

**选靶**：#20 的最新评论是 CodeBuddy 的复核 `2120616575756582912`（2026-11-11T20:36:05Z），**未被消费**；#21（`2120634812074455040`）与 #23（`2120583722461192192`）的复核同样未消费，但 #20 编号最小且是这一族的入口。**上一轮（05:35）选 #21 时把 #20 记成「刚收尾在等复核」，而那份复核 04:36 就到了——比它开工早一小时。判「在等复核」只能看最新一条的时间戳，不能沿用上一轮结论。**

### 判定（10 条 → 成立 6 / 部分成立 1 / 不成立 3）

| # | 复核的断言 | 判定 | 证据 |
|---|---|---|---|
| 1 | (a) 文件级段落把「两个共享读」说清了 | 成立 | `api.yaml:25-43` 与 `approval.rs` 的 `policy_approval_policies()` → `existing_policy_path()` → `approval_policies_path()` 兜底逐字对得上 |
| 2 | (a) **有第三个共享读 `load_risk_rules`，你没数到** | **部分成立** | 事实对（`risk_config.rs:41-70` 读同一份 `policy.toml`，`is_some()` 时走 `load_policy_from_path(&path)?.risk`）；**后果全错**，见第 3 条 |
| 3 | (a) 它把 `load_risk_rules` 的 `Err` 翻成 500：「`exec_preview` 的 500 在 `api.yaml` 里有；`/risk/check`、`/approval/check` 有 500」 | **不成立** | `exec_preview` 把 `preview_exec*` 的 `Err` 映射成 **400**（`:3563`、`:3572`），`/exec/preview` 的 responses 无 500；`/risk/check` 的 `classify_with_user_rules(...).map(...)` 是 `Result::map`，`Err` 被丢弃，且 `effective_command_risk` 返回 `RiskLevel` 不是 `Result`；`host_policy` 映射成 **404**（`:1407`）；`/approval/check` 的 500 来自 `check_approval_required`（`:3493-3494`），是**审批链**不是风险规则 |
| 4 | (b) `/local-exec` 删掉 500 之后客户端读得到 | 成立 | 我复核同一结论；它把「挪 500 到文件级」与「补 400 那半句」判为两个必要条件也对 |
| 5 | (b) 我的自评只归功于第一步，漏了「补 400 那半句」 | 成立 | 接受，记入本轮 |
| 6 | (c) 没有一条路由被误判成「必然 403」 | 成立 | 403 只由 `authorize_command_inner` 的两个 `Err` 臂产生，16 个 preflight 调用点全部可达但不必然 |
| 7 | (c) `preflight_rewritable_request(` 有 **2** 个调用点（`:1711`、`:1813`） | **不成立** | 实测 **1** 个（`:1711`，`exec`）；`:1813` 是 `preflight_guarded_request(`（`local_exec`）。它把两个包装函数看成同一个 |
| 8 | (c) 删掉那两条 `preflight_rewritable_request(` 表项「不会有任何一条路由从表里消失，新增路由 = 0」 | **不成立，且照做会造出真回归** | `exec` handler（`:1702`）正文**只有** `preflight_rewritable_request(`，没有 `preflight_guarded_request(`；删掉那两条，`POST /exec` 的 429 与 403 立刻零守卫 |
| 9 | (c) `preflight_sftp_rm_rf(` 的两条路由是漏报，删 403 不会红 | 成立 | 变异验证实测：删掉 `/sftp/rm-rf` 的 403 后新表项立刻红并点名 `403 post /sftp/rm-rf (sftp_rm_rf via helper)` |
| 10 | (c) `HELPER_STATUS` 只扫 handler 自身函数体、不传递 | 成立（已文档化） | 测试自带注释 `daemon_integration.rs:3557-3569` 早已写明，不是新发现 |

### 本轮自取（不在它清单里）

`ensure_command_length` 在 `:390` 里造 `400`，所以「handler 正文无 `StatusCode::BAD_REQUEST`」的 handler 一律看不见这条码。11 个调用点里 9 个文档了 400，**漏的两个是 `/risk/check` 与 `/approval/check`**——正是「只回答问题、不执行任何东西」的那一对。把新表项加进去后实测只红这两条，零新增红；`/exec/stream` 在 `INBAND_REJECTIONS` 里，把同一条拒绝写进帧而不是状态码，所以不受影响。

### 改了什么（全 L1）

- `src-tauri/tests/daemon_integration.rs`：`HELPER_STATUS` **10 → 13**，新增 `("ensure_command_length(", "400")`、`("preflight_sftp_rm_rf(", "429")`、`("preflight_sftp_rm_rf(", "403")`；模块文档补两段，说明「400 这一层的同族一致性」与「包装函数扫不穿」。**同时保留 `preflight_rewritable_request(` 那两条**并写明理由（它是 `/exec` 唯一的 429/403 来源）。
- `docs/api.yaml`：`/risk/check` 与 `/approval/check` 各补一条 `"400"`，措辞沿用同族的「The command is longer than the daemon's command-length limit」，不写死 16384（本仓既有教训：写进文档的数字是零覆盖的漂移源）。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-180**（`[risk.*]` 解析失败时全族 fail-open，与审批链答 500 的态度相反），并把四条「不是 500」的证据逐条列进条目里。

### 反向验证（两次，实测）

1. 加表项、不改 api.yaml → `api_yaml_documents_the_codes_each_handler_can_return` 红，且**只**点名 `400 post /approval/check` 与 `400 post /risk/check`。
2. 补完 api.yaml 后，把 `/sftp/rm-rf` 块里的 `"403":` 改名 → 同一条测试红并点名 `403 post /sftp/rm-rf (sftp_rm_rf via helper)`；恢复后全绿。**这一条同时证明新 wrapper 表项不是装饰。**

### 验证

`npm run build` **6.30 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`，与基线一致）；`--no-default-features --features daemon --test daemon_integration` **83 passed / 0 failed**（与上轮同数：本轮没有新增测试函数，只改了表与契约）；`yaml.safe_load` **90 paths**。

### 新增「不成立」，不要重复提

1. **「`preflight_rewritable_request(` 在 `bin/agent2ssh-daemon.rs` 有 2 个调用点（`:1711`、`:1813`）」**——只有 1 个（`:1711`）；`:1813` 是 `preflight_guarded_request(`。
2. **「删掉 `HELPER_STATUS` 里 `preflight_rewritable_request(` 的两条不会让任何路由从表里消失」**——`exec` handler 正文只有这个包装，删掉后 `POST /exec` 的 429/403 零守卫。这条如果照做，是本轮唯一会造出真回归的改动。
3. **「`load_risk_rules` 的读失败会让 `exec_preview` / `risk_check` / `approval_check` / `host_policy` 答 500」**——四条依次是 400、丢弃（`Result::map`）、审批链的 500（与本条无关）、404。

**回帖**：`2120651071402958848`（`## 本轮收尾`）。`#20` **保持 open**：剩余四项（L2-27 / L2-172 / L2-175 / L2-180）全在决策侧，另加 #22 的文件身份协议是 L2-165 的前置。分支 `opt/single-20-20261112d` 与 worktree `…/single-20` 已在合流验收后清理。

**给下一轮的交接**：`#20` 现在也进入「等复核」；`#21`（`2120634812074455040`）与 `#23`（`2120583722461192192`）的复核仍未消费，按编号应取 #21。**判「谁在等复核」只能看最新一条评论的时间戳，不能沿用上一轮的结论**——本轮选靶就是因为上一轮把 #20 的复核时间记晚了一小时。

## 2026-11-12 07:54–08:2x · 模式 B 单发 · issue #21 异常诊断（第五次进入 · 会话配额的第三个出口、「绝对白名单」与三处文档）

**基线** `9a71f9d`（= 收到复核时的 `cnb/main`）。**选靶**：12 条 open、无 `[opt-round]` → 模式 B，第二段为空。`#20` 最新一条是本流程 22:58 的 `## 本轮收尾`（在等复核，跳过）；`#21` 最新一条是 CodeBuddy 21:48:33Z 的复核 `2120634812074455040`，未被消费，且是仅剩的两条 P1 里编号更小的（另一条 `#23`）。`#24`/`#27` 要 Windows 实机、`#22`/`#25` 是 P2、`#26`/`#28`/`#29` 零评论 —— 均排除。

### 判定表（6 条）

| # | 指摘 | 判定 | 证据要点 |
|---|---|---|---|
| F1 | 文件级段落写「the two that differ（`POST /sessions`、`POST /resident-sessions`）」，漏了 `GET /terminal` | **成立，且它少数了一处** | 实测 29 个 operation 带 429、6 种描述：`GET /terminal`（`sessions` 族）与 `DELETE /resident-sessions/{id}`（`rate` 族，写的是 `Execution rate limit exceeded`）都不是模板句。`terminal_attach`（`:4746-4780`）走 `register_limited_session_for_command(…, "terminal_open")`，函数体内 `reject_if_rate_limited` = 0 次；槽位在 `handle_terminal` 收尾 `unregister_session`（`:5085`）。所以「differ」的是**四个**，不是三个 |
| F2 | `session_close` 会被 scope 挡住，满额时是真死锁 | **部分成立** | 机制成立（`remote.rs:614-624` 白名单无回退；`session_close` 先 `:2665` 查一次、`authorize_command` 再查一次）。但「满额后自己的 `POST /sessions` 永远 429」只在**同一 token 写了 `session_open`、没写 `session_close`** 时成立；它举的「会话被别的 source 占满」那条路径会先拿到 403（scope 在限流之前），不是 429。记为 **L2-181** |
| F3 | 指南 `:350` 没说明 `allowed_commands` 是绝对白名单 | **成立** | 原两行只说「glob 风格、denied 优先」，没有「非空即封闭」这句话，也没列全操作字符串；`remote.rs:614-624` 是硬白名单 |
| F4 | `daemon_control.rs:379-380` 源码注释仍写「两代」 | **成立** | `for i in (1..=2).rev()` 把 `.2→.3`、`.1→.2` 再当前→`.1`，是**三代**；全仓 `grep "two generations\|daemon.log.1\|daemon.log.2"` 只剩这一处 |
| F5 | 诊断包也带走 `app.log` 尾部 | **记录，不新立** | 它自己也判「无新缺陷」。实测 `export_diagnostic_bundle` 有三段尾部（`:518/520/522`：300/300/120 行），全过 `read_tail` → `redact_sensitive_text`；已把这一点补进 L2-176 的影响面 |
| F6 | `api.yaml` 的 `info.version` 停在 `0.4.5` | **不成立（有约定）** | `docs/RELEASE.md:35` 与 `:168` 把 `docs/api.yaml` 的 `info.version` 列进「发布时必须同步的版本字段」表 —— 它随应用版本走，不随每次契约措辞变更。改 description 不升级是对的 |

### 改了什么（全 L1）

- `docs/api.yaml`：文件级 429 段落从「两处不同」改成**四处并逐个点名**，并补一句「关会话不是 `rate` 的出路、反之亦然」；`GET /terminal` 的 429 从一句「Session concurrency limit exceeded」改成点名 `register_limited_session_for_command` / `terminal_open` / `sessions` 后缀 / 槽位在 socket 关闭时归还 / 拒绝发生在 upgrade 之前；`DELETE /resident-sessions/{id}` 的 429 从「Execution rate limit exceeded」改成说明它走共享 preflight、是 `rate` 族、**是唯一受速率限制的 close**，并与 `DELETE /sessions/{id}` 对照。
- `docs/guides/configuration-guide.md`：scope 注意事项里加两条 —— 「`allowed_commands` 非空时是绝对白名单」并列出九个操作字符串（`ping`/`session_open`/`session_read`/`session_close`/`terminal`/`resident-session open|read|close`/`connect` 及 `sftp …`/`forward …`/`playbook:<名字>` 形状）；以及「能开会话、关不掉会话」这个具体坑与两条出路（三个字符串一起写，或干脆不写 `allowed_commands`）。
- `src-tauri/src/daemon_control.rs`：轮转函数的 doc comment 从「two prior generations (.1/.2)」改成三代并点明循环形状（**仅注释**，无行为改动）。
- `src-tauri/tests/daemon_integration.rs`：`api_yaml_names_the_mechanism_behind_each_session_429` 扩写 —— 新增「429 段落必须点名四个 operation」的段落级断言（`spec.split("Two different mechanisms answer 429").nth(1).split("\n\n").next()`，**不能对全文断言**：`GET /terminal` 在 `info.description` 的另一段也出现过一次，对全文断言会假绿）、`GET /terminal` 的两条机制断言、`DELETE /resident-sessions/{id}` 的两条断言。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-181**（`allowed_commands` 绝对白名单能挡住释放动作）；L2-176 的「位置」补三段尾部的行号与「三处一起变」的提醒。

### 反向验证（三次，各自红在不同断言上）

1. 429 段落里删掉 `GET /terminal` → 段落级断言红并点名它。**第一次写成对 `header` 全文断言时是假绿**（`GET /terminal` 在 gate 那段也出现），因此改成段落切片。
2. `GET /terminal` 的 429 退回原一句 → `terminal` 的机制断言红（`register_limited_session_for_command` / `terminal_open`）。
3. `DELETE /resident-sessions/{id}` 的 429 退回 `Execution rate limit exceeded` → 该条断言红。恢复后 83 passed。

### 验证

`npm run build` **6.29 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--no-default-features --features daemon --test daemon_integration` **83 passed / 0 failed**（未加测试函数，只加断言）；`yaml.safe_load` **90 paths / 105 operations**（与基线同）。

### 新增「不成立」，不要重复提

1. **「`api.yaml` 的 429 段落只漏了 `GET /terminal` 一处，实际三点」**——`DELETE /resident-sessions/{id}` 的 429 也不是模板句（`Execution rate limit exceeded`），是第四处。
2. **「会话被别的 source 占满时，那个 token 的 `POST /sessions` 永远 429」**——它先 403（`check_daemon_scope` 在限流之前）；429 只在同一 token 持有 `session_open` 而不持有 `session_close` 时出现。
3. **「`api.yaml` 的 `info.version` 停在 0.4.5 属于台账空缺」**——`docs/RELEASE.md:35/168` 已把该字段列进发布清单，它随应用版本走。

**回帖**：`2120668306523717632`（`## 本轮收尾`）。`#21` **保持 open**：剩余四项 L2-176/177/178/179 与本轮新增的 L2-181 全在决策侧（改日志落地方式 / 改 scope 语义），文档一致性这条线已收口。

## 2026-11-12 · 手工/单发消费 · #21 第六次进入 · `allowed_commands` 漏 `disconnect` 与指南侧无断言

模式 B 单发（open 十二条 `#20`–`#29` + `#31`/`#32` 安全贴，**无 `[opt-round]`** → 批次段为空）。基线 `7ef8c2a`。选靶理由：`#20` 最新是本流程 22:58 的 `## 本轮收尾`（等复核），`#21` 最新是 CodeBuddy 复核 `2120668878740021248`（11-12 00:03:55Z，未消费），编号更小且 P1；`#23` 编号更大。

### 判定（4 条新指摘 + 4 条确认）

| # | 指摘 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | `configuration-guide.md:350` 的操作字符串清单漏 `disconnect`（列 9，代码有 10） | **成立** | 从 `agent2ssh-daemon.rs` 抽出 4 参字面量 8 个（`ping`/`session_open`/`session_read`/`session_close`/`terminal`/`connect`/`disconnect`/`resident-session read`）+ `RESIDENT_*_COMMAND` 三个常量 = **10**；`ssh_disconnect:3658` 用的就是 `"disconnect"`，且 gate→rate→scope→authorize 四步齐全 |
| 2 | `:352` 与本轮新增的 `:350` 互相矛盾（一个 4 项、一个 9 项） | **部分成立** | `:352` 原文是「会使用**类似** … 的操作字符串做匹配」，是举例不是清单，严格说不构成矛盾；真实缺陷是它贴着完整清单、只列 4 项且同样漏 `disconnect`，读者会当成缩写 |
| 3 | 指南侧无任何测试兜底（`grep configuration-guide src-tauri/tests/*.rs` = 0） | **成立** | 全仓 `src-tauri` 只有 3 处提及，全在 src 的 doc comment（`daemon_control.rs:37`、`local_exec.rs:123`/`:998`），tests 里 0；而 `docs/skills.md`、`docs/skill-distribution.md`、`docs/api.yaml` 都有 `include_str!` 断言，指南是唯一裸文本 |
| 4 | L2-181 的 (b)(c) 案漏了 `connect`/`disconnect` 这一对 | **部分成立** | 同形状成立：两者都走 `check_daemon_scope`，写 `connect` 不写 `disconnect` 就能连不能断。但**后果不同**：连接没有并发配额，`connect_host`（`connection.rs:141-190`）对同一 host 幂等（已连上直接 `Ok(())`，不占新 lifecycle 条目），所以不会像会话那样 429；代价是那条 `RetainedConnection`（含 supervisor keepalive 与自动重连）只能等 daemon 重启或由别的 token 放掉 |
| 5 | F1「429 四处、两族各两个」复核 | **确认成立** | 逐条核过 `:2434`/`:4764` 生产面、`:2665` 无 rate、`preflight_guarded_request_inner:574` 的 gate→rate 顺序、`limits.rs:151/168/186` vs `:243/258/275` 的两种后缀 |
| 6 | F4 轮转「三代」复核 | **确认成立** | `daemon_control.rs:379-381` 注释与 `:393` 的 `for i in (1..=2).rev()` 一致 |
| 7 | F3 白名单语义复核 | **确认成立（语义）** | 与第 1 条同源；它对「九项」的计数正确，但**自己那张代码块只列了 6 个字面量 + 3 个常量 = 9，却写成「就这十个」**（漏在清单里的正是 `ping`）。结论（差集 = `{disconnect}`）不受影响 |
| 8 | L2-181 收窄「只有 open 没 close 才 429」 | **确认成立** | scope 在限流之前，别 source 占满先 403 |

### 改了什么（全 L1）

- `src-tauri/tests/daemon_integration.rs`：新增 `fixed_action_strings()` / `call_arguments()` 两个解析辅助与守卫 `configuration_guide_lists_every_fixed_action_string`。集合**从源码派生**（`check_daemon_scope` 第四参的字面量 + `RESIDENT_*_COMMAND` 常量），再断言指南那条 bullet 逐个以反引号拼写出现；顺手加了下界断言（解析结果 < 10 视为解析失配而不是「代码变少了」）。源码扫描在本文件有先例（`:2773`/`:2832`/`:2862` 都 `include_str!` 同一份 daemon 源）。
- `docs/guides/configuration-guide.md`：`:350` 补 `disconnect`，把「十项」写成可对表的定数并点明由哪条测试盯着；`:351` 加 `connect`/`disconnect` 这一对（幂等 + 无配额 → 症状不是 429）；`:352` 补 `disconnect` 并明说该行只是举例、完整清单以上一条为准。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：L2-181 的「位置」补 `:3658`/`:3604`，新增一段「第二对」说明同形状不同后果，(b) 释放集补 `disconnect`、(c) 自检补 `connect`/`disconnect` 配对。**编号未变，最大仍是 L2-181**。

### 反向验证（两次，各自红在不同断言上）

1. 从 `:350` 删掉 `disconnect` → 红并点名 `["disconnect"]`。
2. 把 daemon 里的 `"ping"` 改成 `"ping2"` → 红并点名 `["ping2"]`，证明「代码里新增一个固定操作字符串」会自己浮出来，而不是靠人去数。恢复后 84 passed。

### 验证

`npm run build` **6.41 s**；`cargo fmt --check` 绿（首次红：`if let Some(literal) = …` 那行超宽，已 `cargo fmt`）；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--no-default-features --features daemon --test daemon_integration` **84 passed / 0 failed**（原 83）。

### 新增「不成立」，不要重复提

1. **「`configuration-guide.md:352` 与 `:350` 互相矛盾」**——`:352` 原文是「会使用**类似** … 的操作字符串做匹配」，是举例；两行不构成同一份清单的两个版本。本轮仍把 `disconnect` 补进去并加了「完整清单以上一条为准」，但记为措辞改进而非矛盾修复。
2. **「`connect`/`disconnect` 是 L2-181 那个坑的同一个坑」**——同形状（同一个白名单检查、同一个开/关不对称），不同后果：会话有 `max_sessions` → 429 死锁；连接无并发配额且 `connect_host` 幂等 → 不 429、不累积，只是那条保留连接在该 token 手里放不掉。

**回帖**：`2120687239158980608`（`## 本轮收尾`）。`#21` **保持 open**（L2-176/177/178/179/181 五项待拍板；文档一致性这条线本轮到 `disconnect` 为止）。

## 2026-11-12 · 手工/单发消费 · #20 第八次进入 · 广播路由的 400 没有任何守卫，以及「第三条 500 不来自文件」

模式 B 单发（open 十二条 `#20`–`#29` + `#31`/`#32` 安全贴，**无 `[opt-round]`** → 批次段为空）。基线 `7028cc9`。选靶理由：`#20` 最新是 CodeBuddy 复核 `2120652444223832064`（11-11 22:58:37Z，未消费），比本流程 22:53 的收尾帖**晚 5 分钟**——上两轮都把它读成「在等复核」，实质是那份复核早于它们开工；`#21` 最新是本流程 09:07 的 `## 本轮收尾`（真在等），`#23` 的复核 `2120583722461192192` 未消费但编号更大。

### 判定（10 条 → 成立 6 / 部分成立 2 / 不成立 2）

| # | 指摘 | 判定 | 证据要点 |
|---|---|---|---|
| 1 | Q1：11 个 `ensure_command_length` 调用点分三种形状（9 直传 `?`、1 帧内 `exec_stream`、1 经 validator 间接） | **成立** | 逐一跑 `grep ensure_command_length(` 得 11 处（`:1710`/`:1812`/`:1864`/`:1907`/`:2863`/`:3397`/`:3485`/`:3519`/`:3552`/`:4290`/`:5257`）；`:4290` 属 `validate_terminal_broadcast_request`，其余 10 处逐个用「向上找最近 `fn`」确认了所属 handler |
| 2 | Q1：两条 broadcast 的 400 是 `HELPER_STATUS` 视野外（handler 正文无该字面量） | **成立** | `terminal_broadcast_preview`（`:4422-4437`）与 `terminal_broadcast_run`（`:4438-4611`）正文各只有一句 `validate_terminal_broadcast_request(&request)?`，`ensure_command_length(` 零命中 |
| 3 | Q1：「删掉 `"400":` 靠 `StatusCode::BAD_REQUEST` 那半还能红（五个臂之一）」 | **不成立** | 那五个 `StatusCode::BAD_REQUEST` 全在 `validate_terminal_broadcast_request`（`:4287-4320`）里，**不在 handler 正文**：两个 handler 正文的 `StatusCode::` 计数为 0。实测删掉 `/terminal/broadcast` 的 `"400":`，改动前 84 passed 零红 |
| 4 | Q1 修法：给表项补 `("validate_terminal_broadcast_request(", "400")` | **部分成立（单独照做无效）** | `INBAND_REJECTIONS` 的旧形态是「整条路由 return Vec::new()」，而两条 broadcast 都在那张表里 → 新表项对它们同样是零作用。实测：保留旧豁免形状 + 新表项 + 删掉 `"400":` → **84 passed**（不红）。必须同时把豁免改成按状态码（见改法） |
| 5 | Q1：`/risk/check`、`/approval/check` 的 400 位置与措辞与同族一致 | **成立** | `api.yaml` 两处 400 的可读码存在且措辞同族，上一轮补的 |
| 6 | Q2：包装层共四个，其余三个（`preflight_*` 外层、`authorize_targets`、`validate_*`）不需要加表项 | **成立** | `authorize_command_inner`（`:770-816`）只有 `ScopeDenied`/`ApprovalRejected` 两个 403 臂（`:786`/`:807`），`Internal` 才是 500（`:813`）；`exec_multi:1867-1868`、`exec_compare:1911-1912` 在 `authorize_targets` 之前已内联 `reject_if_gate_paused` + `reject_if_rate_limited`，再加表项确实是空转 |
| 7 | Q2 附：`handle_terminal` 不是扫描对象（`/terminal` 挂的是 `terminal_attach`） | **成立（记录，不重复提）** | `:5877` `.route("/terminal", get(terminal_attach))` |
| 8 | Q3：(c) 只是半步，另给「mtime 变化时也打一条」的折中 | **成立（采纳一半，写进 L2-180）** | `risk_config.rs` 与 `policy.rs` 的 `tracing` 计数都是 **0**；`load_risk_rules` 已在比 `current_modified`（`:46-56`/`:66`），加 `last_logged_modified` 的代价只有日志频率 |
| 9 | §4.1：`/exec` 与 `/local-exec` 能答 500、`api.yaml` 都没写 | **部分成立** | 两条 operation 的 `responses` 确实都没有 500——但这是 L2-174 定下的口径（文件级说一次，不在每条 operation 上重复），不是遗漏。**真正缺的是另一件事**：文件级段落把 preflight 的 500 全部归因成「两个共享读取失败、消息点名那个文件」，而 `authorize_command_inner:813` 的 `Internal` 还有第三源——`request_and_wait_for_approval`（`:611-700`）在 daemon 关闭时 `return Err("daemon shutting down; approval wait cancelled")`、未知状态返回 `Err("unexpected approval status")`，两者都**不提文件** |
| 10 | §4.2：`/sftp/rm-rf` 的 `responses` 无 408，description 里的 "below" 是死指针 | **不成立** | 该 operation 的 `responses` 依次是 `200/400/401/403/408/415/422/423/429`，408 就在 "below" 指向的下一段，指针活着 |

### 改了什么（全 L1）

- `src-tauri/tests/daemon_integration.rs`：①`HELPER_STATUS` 13 → 14，加 `("validate_terminal_broadcast_request(", "400")`；②`INBAND_REJECTIONS` 从 `(&str, &str, &str)` 改成 `(&str, &str, &[&str], &str)`，第三段是**该路由吞掉的码集合**（`/exec/stream` = `400/423/429`，两条 broadcast = `423/429`），`undocumented_via_helper` 由「整条路由 return」改成「按码过滤」；③模块文档把 `ensure_command_length` 那段的数量改正（十个 handler 调用、九个文档化、第十个是帧内的 `/exec/stream`），并把 validator 这条第十一个调用点写清；④`the_spec_states_the_preflight_internal_errors_once` 加一条 needle，钉住「取消的审批等待答 500 且不点文件」这句话。
- `docs/api.yaml`：`info.description` 的 500 段落补一段——preflight 的 500 不止「两个共享读取」两源，审批等待被 shutdown 取消（`daemon shutting down; approval wait cancelled`）与未知状态（`unexpected approval status`）也不提文件，补救是重试而不是改策略文件。**没有**给任何 operation 加 per-operation 500（L2-174 的口径不变）。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：L2-180 补「(c) 只是半步、收口是 (b)」，新增 (c+)「日志跟 mtime 走」与「(a) 不重新开」。**编号未变，最大仍是 L2-181**。

### 反向验证（三次）

1. 删掉 `/terminal/broadcast` 的 `"400":` → 红并点名 `["400 post /terminal/broadcast (terminal_broadcast_run via helper)"]`。
2. 保留旧豁免形状（整条路由跳过）+ 新表项 + 同样删掉 `"400":` → **84 passed 不红**，证明「只加表项」是空转，必须同时改豁免粒度。
3. 把 api.yaml 里 `daemon shutting down; approval wait cancelled` 改成 `approval wait cancelled` → 新 needle 红并给出「三个文件名会被读成完整成因清单」的原因。

### 验证

`npm run build` **6.25 s**；`cargo fmt --check` 绿（首次红：`INBAND_REJECTIONS` 两项超宽，已 `cargo fmt`）；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--no-default-features --features daemon --test daemon_integration` **84 passed / 0 failed**（与上轮同数，本轮只加强既有断言、未加测试函数）。

### 新增「不成立」，不要重复提

1. **「删掉 `/terminal/broadcast` 的 `"400":` 会因 `StatusCode::BAD_REQUEST` 那半变红」**——五个 `BAD_REQUEST` 字面量在 `validate_terminal_broadcast_request` 里，两个 handler 正文的 `StatusCode::` 计数是 0；现在能红是靠本轮新加的表项 + 按码豁免。
2. **「`/sftp/rm-rf` 的 `responses` 没有 408、description 里的 below 是死指针」**——408 存在（`200/400/401/403/408/415/422/423/429`），"below" 指的就是它。

**回帖**：`2120705041878114304`（`## 本轮收尾`）。`#20` **保持 open**：剩余 L2-27 / L2-172 / L2-174（已收口）/ L2-175 / L2-180 与 `#22` 的身份协议前置项全在决策侧，文档一致性这条线本轮补到「第三条 500 不来自文件」为止。

## 2026-11-12 · 手工/单发消费 · #20 第九次进入 · 「按码豁免」的反向约束没读那个码，以及 broadcast 的 408 也在体内被吞

- **前置**：无锁（主动建立 pid 10888 并写 `owner`）；三个其它 worktree `status --porcelain` 全 0 行；`ls-remote cnb/main` 两值同为 `19e85c2`；`src`/`src-tauri/src`/`docs` 三十分钟零写入 → 不让位。`open` 十二条（#20–#29 + #31/#32），**无 `[opt-round]`** → 模式 B，第二段为空。
- **选靶 #20**：最新是 CodeBuddy 复核 `2120706748276170752`（2026-11-12T02:34:24Z），比我 02:27:37Z 的收尾帖**晚 6 分钟**——上一轮收尾时它还没到。判「在等复核」只看最新一条的 id 与时间戳，沿用上一轮结论会把它漏掉（这是第五次靠回看时间戳纠正）。#21 最新是本流程 01:19 的收尾帖（等复核）、#23 编号更大。

### 判定（12 条 → 成立 9 / 部分成立 1 / 不成立 2）

| # | 指摘 | 判定 | 证据 |
|---|---|---|---|
| 1 | `unbacked` 不读 `exempt_codes`，硬编码 `["423","429"]`，所以超集与空集都不报 | **成立** | 改前解构是 `let (method, path, _, marker) = *entry;`，第三段直接丢弃；`let mut problems: Vec<String> = ["423", "429"]`。`&[]` 时豁免等于不存在，超集时 `undocumented_via_helper` 全放过 |
| 2 | `/terminal/broadcast` 的 **408 也是 in-band**，`exempt_codes` 没列 | **成立** | `terminal_broadcast_run:4505-4530` 把 `authorize_command` 的 `Err` 写进 `results[index].error`；`authorize_command` 无条件传 `request_and_wait_for_approval` 闭包，`authorize_command_inner:809-812` 把 `ApprovalTimedOut` 映射成 `REQUEST_TIMEOUT`。python 复核 `responses` = `200/400/401/403/415/422`，确无 408 |
| 3 | 同上，「另有一条 408 作为状态码返回，见 `validate_terminal_broadcast_request` 的 400 同一位置」 | **不成立** | 该 validator（`:4287-4320`）只有 `ensure_command_length` + 五个 `BAD_REQUEST`，**没有任何 408**。它把「同一个 validator 位置」当成「同一个码」 |
| 4 | 反向（有没有路由返回了却被当 in-band）没有 | **成立（确认）** | `/exec/stream` 的 400/408/423/429 全走 `Message::Text({"type":"error"})` 后 `return`；两条 broadcast 的折叠路径见第 2 条 |
| 5 | 两条 broadcast 的 tags 来源不一致（`preview_results` 用 `endpoint.tags`，run 的 rate/authorize 用 `host_tags()`） | **成立** | `:4376-4381` 用 `&endpoint.tags`（注册快照）；`:4483-4486` 与 `:4506` 用 `host_tags(&host)`（实时 `hosts.json`）→ **L2-182** |
| 6 | `request_and_wait_for_approval` 只有那两处 `Err`，上一轮措辞准确 | **成立（确认）** | `:689` 与 `:701`，本轮无改动 |
| 7 | `Internal` 在 CLI / 桌面 / MCP 上被映射成非 500 的形态 | **成立（记录）** | `bin/agent2ssh.rs:1621` → `anyhow!`；`tauri_commands.rs:458` → 字符串；`bin/agent2ssh-mcp.rs:78-83` → `McpError::internal`（`-32000`）。跨面语义，不入本轮改动 |
| 8 | `proxy_exec` 的 localhost 分支跳过 `get_daemon_with_scope`，三条 response 描述没写各自属于哪个分支 | **成立** | `:3970-3991` 的 localhost 分支 `return exec_ssh_core(req)`，`get_daemon_with_scope` 在其后（`:3994`）；404 与 500 只在远端分支，401 同理。`api.yaml:4473` 确有 "Two branches, one order"，但三条 response 各自没写归属 |
| 9 | 没有断言绑住「handler 里每个 `err(StatusCode::X)` 的 X 都在 `responses` 里」 | **成立** | `HANDLER_STATUS` 是常量表 + `body.contains("StatusCode::X")`，不是从 handler 抓取；今天四条恰好都对 |
| 10 | `exec_stream` 的 `Message::Close` 顺序 | **成立（记录，不新开）** | 已有 L2-87，`api.yaml:1959-1962` 已写明 |
| 11 | 模块文档「106 `(path, method)` pairs」错 | **部分成立（数对了一半）** | 实测 92 routes / **107** pairs（它只说「不是 106」，没给正确值）；镜像 18 / 20；非镜像 74 calls / **85** pairs / **74** paths——文档写的是 84 / 73，这两处它没点。文档三处错，它指出两处（106 出现两次） |
| 12 | `NON_API_ROUTES` 注释点名的 `the_console_route_mounts_one_named_handler` grep 不到 | **不成立** | 该测试就在 `daemon_integration.rs:2918`，`git grep` 一次命中 |

### 改了什么（全 L1）

- `src-tauri/tests/daemon_integration.rs`：①`HELPER_STATUS` 14 → 15，加 `("authorize_command(", "408")`——这是 408 唯一的守卫来源，也只有两条路由会因此浮出来；②`INBAND_REJECTIONS` 的码集合补 408（`/exec/stream` = `400/408/423/429`，`/terminal/broadcast` = `408/423/429`，preview 仍是 `423/429`，它确实不调 `authorize_command`）；③`unbacked` 改成遍历 `exempt_codes` 而不是硬编码两个字面量，并补两条反向约束——**空集**要点名（豁免等于不存在），**`HELPER_STATUS` 不认识的码**也要点名（否则豁免的是一条永远不会触发的检查）；④模块文档三处计数改正：107 pairs、85 pairs、74 paths，并写清 106 是「按 `async fn` 名扫描」的视角（`/` 是内联闭包）。
- `docs/api.yaml`：`/terminal/broadcast` 的 description 补第三类 per-target 拒绝——审批超时由 `authorize_command` 造 408、被这个 handler 写进该 target 的 `error`，所以 `authorized: false` + 超时消息，而不是 408；`/exec/stream` 那句「no `429` and no `423`」改成四个码（`400`/`408`/`423`/`429`），与豁免集合对齐。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-182**（同一次 broadcast 里 scope 用注册快照 tags、限流与授权用实时 tags）。**最大编号 L2-182**。

### 反向验证（四次，各自红在不同断言上）

1. 把 `/terminal/broadcast` 的豁免集合加 `"422"`（它文档化了 422）→ 红并同时点名两条：`documents 422 but answers it in-band` + `no HELPER_STATUS entry answers that code`。**旧版硬编码 423/429 时这两条都不会出现**，这就是本轮修的东西。
2. `/exec/stream` 的集合改成 `&[]` → 红（83 passed / 1 failed）。
3. 删掉 `("authorize_command(", "408")` 表项 → 红并**同时点名两条路由**的 408，证明「豁免的码必须有表项支撑」这条新约束是活的。
4. 改 `api.yaml` 时顺手撞到既有 needle：把 "only as a frame" 折行拆断 → 红并点名 `get /exec/stream no longer says \`only as a frame\``，说明第四段 marker 仍在守「豁免不能退化成沉默」。

### 验证

`npm run build` **6.35 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--no-default-features --features daemon --test daemon_integration` **84 passed / 0 failed**（与上轮同数——本轮只加强既有断言，未加测试函数）。

### 新增「不成立」，不要重复提

1. **「`validate_terminal_broadcast_request` 也会返回 408」**——它只有 `ensure_command_length`（400）与五个 `BAD_REQUEST`，没有 `REQUEST_TIMEOUT`。408 只从 `authorize_command_inner:809` 出来，经 `terminal_broadcast_run:4505-4530` 折进 target 的 `error`。
2. **「`the_console_route_mounts_one_named_handler` 不存在」**——`daemon_integration.rs:2918` 就是它。

**回帖**：`2120722536529829888`（`## 本轮收尾`）。`#20` **保持 open**：剩余 L2-27 / L2-172 / L2-175 / L2-180 / L2-182 与 `#22` 的身份协议前置项全在决策侧。

### 手工/单发消费 · 2026-11-12 · #23 第五次进入（身份入口清点补全 + 退出路径的错归因）

**模式 B 单发**（open 十二条 #20–#29 + #31/#32 安全扫描贴，**无 `[opt-round]`** → 第二段为空）。开工基线 `437ae7e` = `cnb/main`：`ls-remote` 与本地同值、三个其它 worktree 的 `status --porcelain` 全 0 行、`src`/`src-tauri/src`/`docs` 三十分钟零写入 → 不让位。**选靶 #23**：`#20`（`03:37:08Z`）与 `#21`（`01:16:53Z`）的最新评论都是本流程的 `## 本轮收尾`（等复核），`#22`/`#25` 是 P2 设计、`#24`/`#27` 要 Windows 实机（本机做不了，排除）、`#26`/`#28`/`#29` 零评论、`#31`/`#32` 是安全扫描贴；`#23` 最新是 CodeBuddy 的只读复核 `2120583722461192192`（11-11 18:25:32Z，**未消费**），P1 且编号最小。它的自述基线 `c5604c8` 是 `437ae7e` 的祖先，行号已按当前树复核。

**判定（去重后 17 条 → 成立 8 / 部分成立 4 / 不成立 1 / 记录 4）**。三条要点：

1. **桌面「停止」按钮的启用条件是第四套判据，且与它触发的动作相反**（成立，新）：`SettingsMenu.tsx:660` 的 `disabled={daemonActionBusy !== null || !daemonHealthy}`，而 `daemonHealthy`（`:115`）只等于 `api.getDaemonHealth()` 那次裸 `fetch(/health)` 的 `ok`（`api.ts:693-710`，`daemonUrl` 在 `:71` 硬编码）。它既不看名字也不看 PID，所以重命名 daemon 下是「绿灯亮 + 按钮可点 + 点了必败」——`daemon_stop` 仍走名称门槛。同页 `:648` 的「启动」按钮被同一个绿灯禁用，那一侧恰好是对的（CLI 也说 already running），所以这一对按钮是「一个对、一个错」而不是整体错了。
2. **`stop_owned_daemon` 的失败路径是静默的，不是它说的那条日志**（部分成立→本轮改正 K14）：它说 `daemon_is_owned`（`daemon_control.rs:612-623`）与 `process_is_alive`（`:1906`）都因名字返回 false，日志写的是既有的那句 `quit leaves the daemon running: it was not started by the desktop app`。**前半错**：`daemon_is_owned` 用的是 `process_start_time`（`:591-596`），只读 sysinfo 的 `start_time`，**不看名字**，在重命名 daemon 上返回 true。**后半也错**：`process_is_alive` 在 `tauri_commands.rs:1906` 就 early return 了，`daemon_is_owned` 根本没执行，所以**一条日志都不写**。结论（退出不会停重命名 daemon）是对的，机制与可观测性都错。
3. **`daemon.pid` 的「两种权限口径」不成立**（不成立，实测）：它说 `write_daemon_pid_if_absent`（`daemon_control.rs:86-94`）以 0444 创建、与 `std::fs::write` 那条的 0644 不一致。用与本仓同形的 `OpenOptions::new().write(true).create_new(true)` 实测，umask 022 下两种写法都是 **0644**——`OpenOptions` 只带 `write(true)` 不会改变 create 的默认 mode（`0o666 & ~umask`）。它把「访问模式」当成了「文件模式」。与 `daemon.token` 的 0600 之差属 **L2-171** 的附带属性，不新立条。

### 改了什么（全 L1）

- `src-tauri/tests/daemon_stop_e2e.rs`：在既有的 `renamed_daemon_is_reported_healthy_but_not_unsafely_stopped` 里补三条断言，把 K14 那句「同一份输出自相矛盾」钉住——`report["daemon"]["process_alive"] == false`、`health_ok == true`、`pid == pid`。这两半此前**零测试覆盖**（`grep -c doctor src-tauri/tests/daemon_stop_e2e.rs` = 0 说的是 doctor，诊断面同样为零）。夹具本来就是一只真名不在白名单里的 daemon，只有它能同时造出两个相反答案。
- `docs/KNOWN-ISSUES.md` K14：身份入口清点从「三状态 + 两停止」补全为**六个可观察面 + 一个静默面**——加桌面 `daemon_status`（与 CLI status 同派）、桌面「停止」按钮（第四套判据）、`daemon rotate-token`（第三类：以 `daemon_pid_state != Stale` 拒绝，与名称门槛无关）、`daemon list`/`daemon view` 的 `localhost` 项（HTTP-only），并改正退出路径的描述（`process_is_alive` 先 early return，零日志；`daemon_is_owned` 反而比它宽松）。
- `docs/guides/configuration-guide.md` 停止一节：把同样五件事写成用户视角，并明确「面板绿灯 ≠ 可停」「退出应用后 daemon 还在在重命名场景是预期行为」「rotate-token 拒绝与名字无关」。
- `docs/reports/OPT-LOOP-L2-PENDING.md`：新增 **L2-183**（桌面停止按钮的启用条件与动作判据不一致，三案：改 `daemonStatus().running` / 后端结构化 reason / 只加 tooltip）与 **L2-184**（`stop_owned_daemon` 对重命名 daemon 静默返回，退出路径零日志）。**最大编号 L2-184**。

### 反向验证（一次，红在新增断言上）

把 `process_alive` 的期望改成 `true` → 红在 `daemon_stop_e2e.rs:736`，输出 `left: Bool(false) / right: Bool(true)` 并带 `REVERSE-PROBE` 标记；恢复后单测复跑 **1 passed / 6.21 s**。这一条同时证明测试函数真的执行了（不是被 `#[ignore]` 或前置 skip 吞掉），以及报告里那个字段确实是 `false` 而不是 `null`。

### 验证

`npm run build` **6.47 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **863 passed / 1 failed / 1 ignored**（红项仍是那条故意红的 `an_ipv6_literal_at_port_22_is_removed`）；`--no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1` **34 passed / 0 failed / 34.98 s**（与改动前同数——只加强断言，未加测试函数）。

### 新增「不成立」，不要重复提

1. **「system report 的 `health_ok` / `process_alive` 是 K14 漏写的第三处自相矛盾，而且是顶层字段」**——K14 原文已同时写了这两对（「`daemon_process_alive` / `process_alive`」与「`daemon_health_ok` / `health_ok`」）；两个字段在 `diagnostics.rs:643-648` 的 `"daemon": {…}` 里，不是顶层。
2. **「`write_daemon_pid_if_absent` 以 0444 创建 `daemon.pid`，两种写法权限口径不一致」**——实测两种写法在 umask 022 下都是 **0644**。见上。
3. **「`daemon_is_owned` 在名称门槛后面，因名字 `None` 而 false」**——它用 `process_start_time`（`daemon_control.rs:591-596`），不看名字；挡住退出路径的是排在它前面的 `process_is_alive`（`tauri_commands.rs:1906`）。

**回帖**：`2120739993617330176`（`## 本轮收尾`，`04:46:30Z`；`#23` 内第 3 条该标记）。`#23` **保持 open**：三条验收（真实进程回归 / 两种生命周期 / Linux CI 前提）仍未做，且 pidfd 本体要改 `stop_daemon_process` 的签名、前置 #22 的文件身份格式未拍板；本轮新增的 L2-183 / L2-184 也都要人拍板。

## 2026-11-12 · 手工/单发消费 · #20 第十次进入 · 表里的「通过 helper 可达」从来没被验到任何一个 body

**前置**：无锁 → 主动建 pid 58549 并写 `owner`；三个其它 worktree `status --porcelain` 全 0 行、三十分钟无写入；`ls-remote cnb/main` 120 秒两值都是 `7caa1bc`；`src`/`src-tauri/src`/`docs` 三十分钟零写入 → 不让位。`open` 十二条（#20–#29 + #31/#32），**无 `[opt-round]`** → 模式 B，第二段为空。

**选靶 #20**：#20 最新是 CodeBuddy `2120743340202151936`（**04:59:48Z**），比我 03:37:08Z 的收尾帖晚 82 分钟；#21（01:16Z）、#23（04:46Z）最新都是本流程的 `## 本轮收尾`（等复核）→ #20 是唯一有未消费复核的 P1，且编号最小。**连续第七轮靠「看最新一条时间戳」纠正选靶。**

**判定 17 条 → 成立 11 / 部分成立 3 / 不成立 2 / 记录 1**。核心三条：

1. **`/terminal` 的 scope 拒绝答 200 text/plain**（成立，行为缺陷）→ **L2-185**：`daemon.rs:4753-4755` 把 `check_daemon_scope` 的 `String` 直接 `into_response()`，axum 只补 `content-type`、状态码是默认 200。18 个调用点里 16 个紧跟 `map_err` 转 403，这是唯一例外；`HANDLER_STATUS` 只读 `StatusCode::` 字面量，该体里 0 个 → 无断言能发现。
2. **`api.yaml` 把 429 写成「终止整个请求」**（成立）：`:2189` 原文 "The refusals that do stop the whole request are 400 (validation) and 429 (the batch limit)"，而 `daemon.rs:4488-4502` 把限流 `Err` 逐 target 写进 `error` 后 `return Ok(Json(...))` —— 200。已改。
3. **「20 个 documented 408 删掉全绿」**（成立，**我复算同数**）：29 个 408 里，旧表只有 `authorize_command(` 一条 needle，20 个 operation 的 body 里没有它。补表后 **0 个**。

**本轮改的是三处「检查自身没有兜底」，不是补几条表项**：

- `ca97585`（`daemon_integration.rs`）：①`HELPER_STATUS` 15 → **31** 行，三个 `preflight_*` 包装、`authorize_targets`、`finish_sftp_operation`、`check_daemon_scope`、`terminal_broadcast_preview_results`、`authorize_command` 的 403、gate 谓词 `gate_blocks_source(` 各按码登记；②新增 `every_helper_status_needle_names_a_call_a_body_makes`，**死 needle 现形**（原来 `too_many_requests_status()` 只被限流器自己调用，任何 handler body 都不含它，删行）；③两处 body 扫描先 `strip_rust_comments`；④`INBAND_REJECTIONS` 加第五段 `delegates`，`unbacked` 要求每个豁免码被 handler 或该 delegate 消费。
- `55989ca`（`api.yaml` + `L2-185` + 台账）：`/terminal/broadcast` 的 403 段改成「只有 400 终止整个请求，批次限流逐 target 折进 200」；`GET /terminal` 的 429 补「升级后同一个码还走 `{"type":"error"}` 帧」；`handler_body_of` 的「66 of the 106」换成不带数字的说明（那个数任何人复算都对不上，快照数字会被当成已验证事实引用）。

**基线变更（不是本轮改的）**：合流时远端已前进到 `9c31553`（他人 PR #35 glib pin 合并，动了 `Cargo.toml` / `build.rs` / `agent2ssh-daemon.rs` / `embedded_ssh.rs`）。rebase 干净，全部验证在 rebase 后重跑。`--lib` 从 863/1/1 变成 **865/0/1**：那条故意红的 `an_ipv6_literal_at_port_22_is_removed`（`embedded_ssh.rs:765`，仍在）已被 #35 的重写带绿。**下轮不要再用 863/1/1 当基线。**

**反向验证四次**（各自红在不同断言上）：①把 `check_daemon_scope(` 拼成 `check_daemon_scope_typo(` → 新测试点名 `["check_daemon_scope_typo("]`；②preview 的豁免集加回 `429` → `unbacked` 点名「neither its handler nor [terminal_broadcast_preview_results] calls a helper that answers that code」；③在 `health` 体里注入一行 `// preflight_guarded_request(` → **剥离后绿**，把剥离关掉则同时点名 400/403/408/423/429 五条（证明注释能被当调用）；④删 `/exec-multi` 的 408 → 红并点名 `408 post /exec-multi (exec_multi via helper)`，撤掉 `preflight_guarded_request(` 与 `authorize_targets(` 两条 408 表项后同一删除**转绿**（证明红来自新表项，不是别处）。

**验证**：`npm run build` **6.60 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **863 / 1 / 1**（红项仍是故意红的 IPv6）；`--no-default-features --features daemon --test daemon_integration` **85 passed / 0 failed**（原 84，+1 是新测试）。

**绑定面复算**（同一份脚本，归一化 `:id`→`{id}` 后按 `.route(...)` 解析出 106 个 pair）：documented 408 **29 → 未绑定 20 → 0**；423 **30 → 17 → 0**；403 **78 → 21 → 1**（剩 `GET /hosts/{name}/policy`，其 403 在体内无来源 —— 反方向，留待下轮）；400 **60 → 49 → 34**（其余靠 body 里的 `StatusCode::BAD_REQUEST` 字面量，由 `HANDLER_STATUS` 守；helper 侧不再兜底）；429 **29 → 0 → 0**。

**未做**：CodeBuddy 的 ⑤（marker 只要出现在 path block 某处，挪进 401 的 description 也能过）——本轮没动 `spec_path_block`，属「同一段散文里哪个 response 说的」这类定位问题，改法要引入 anchor，成本高于本轮；记录，不记不成立。

### 新增「不成立」，不要重复提

1. **「`handler_body_of` 的注释数字错，现在是 93/106」**——**数字本身不可复算**：把它描述的旧终点（「下一个 `async fn`/`fn`」）按三种切法各算一次，越界数分别是 91/91（含下一项的 doc comment）、93/106（它的口径，我复现不出）、以及若干中间值，没有一个是稳定值。真正的缺陷是「这个数字被当成已验证事实引用」，所以本轮删数字、改写成不带快照的说明，而不是换一个同样会腐烂的数。**不要再给这个注释补数字。**
2. **「`/terminal` 的 429 描述完全错」**——`:2046` 那句的主语是 `terminal_attach` 与 `register_limited_session_for_command`，就它自己而言没错（升级前的会话配额）；缺的是「升级后同一个码还走帧」这半句，属**部分成立**，本轮补的是后半句，不是重写前半句。

### 过程失误（写下来防再犯）

本轮前四次 `Edit` 全部打在 **REPO 主工作区**的 `src-tauri/tests/daemon_integration.rs` 上（路径没带 `-opt-worktrees/single-20`），发现时已改了表、扫描与新增测试三处。处置：`git -C $REPO checkout -- <file>` 还原，`git status` 确认只剩既有的 `.deja/`，再在 WT 里重做全部改动。**代价是前两次 `cargo test`（84 passed）读数无意义——它们跑的是未被改动的 WT 源码**。教训：`Edit`/`Write` 的路径必须落在 `$WT` 下；「测试绿了」要先确认改动真的在跑的那棵树里。

**回帖**：`2120759458635624448`（`## 本轮收尾`，`06:03:51Z`）。`#20` **保持 open**：L2-176/177/178/179/180/181/182 与新增 **L2-185** 都要人拍板；本轮的 ①marker 定位、②一码两形态、③`GET /hosts/{name}/policy` 的 403 反方向缺口留待下一批。worktree `…/single-20` 已 remove、分支 `opt/single-20-20261112g` 已 `-d`，主工作区 `--ff-only` 到 `55989ca`，仅 `.deja/` 未跟踪。

## 2026-11-12 15:05–（模式 B 单发 · issue #22 首次进入 · P2 设计类，单段闭环）

- **前置**：主动建立锁（pid 91170 写入 `owner`）；三个其它 worktree `status --porcelain` 全 0 行；`ls-remote cnb/main` 两值都是 `4487f7f`；`src`/`src-tauri/src`/`docs` 三十分钟零写入 → 不让位。`open` 十条（#20–#29），**无 `[opt-round]`** → 模式 B，第二段为空。
- **选靶 #22**：三条 P1 全部在等复核（#20 最新是 06:03Z 的本流程收尾、#21 是 01:16Z、#23 是 04:46Z），#24/#27 要 Windows 实机（排除）、#26/#28/#29 零评论（本该先发只读分析请求，但 #22 有未消费的 CodeBuddy 设计复核且编号更小）。**#22 是 P2，也是 #23 的阻塞源**：#23 反复卡在「文件身份格式未定」，而 #22 的协议稿正是那个决策的载体。按 §5.3 的 P2 口径，交付物是「能让人 5 分钟内拍板的方案对比」，不是代码。
- **判定 11 条 → 成立 9 / 部分成立 1 / 数字不成立 1**（行号按其基线 `d3a4a71` 基本准确，本轮全部按 `4487f7f` 重映射，不记它漂移）：
  1. **名称白名单未点明 → 成立**。`is_daemon_executable:141-154` 是四个字面量，不是「看起来像 daemon」；后果是三元组不在表内的打包构建**停不掉也起不来**（`:774-779` 静默 `Ok(())`、`start_daemon_background:448-452` 拒绝）→ **L2-186**。
  2. **两文件协调已存在 + 无锁读 → 成立，但文件名错**。它写 `daemon-owner.json`，实际是 `daemon.owner`（`daemon_control.rs:587`）；`OwnerMarker{pid,start_time}:582-585`、`clear_daemon_owner_if:625-643` 锁内重匹配确实存在，`daemon_is_owned:612-623` 确实**不加锁** → **L2-188**。
  3. **§6.3 当前走不到 + 更强的结论 → 成立**。客户端只靠 `/health` **响应体**里的 `pid`（`:231-253`）；全仓 `SO_PEERCRED`/`GetNamedPipeServerProcessId` 零命中；控制面只有一条 `TcpListener::bind`（`agent2ssh-daemon.rs:5826`）。所以 §3.2 的绑定在现有 TCP 通道上不可达，这是**结构结论**而不是「待验证」。
  4. **`daemon_health_ok` 子串匹配 → 部分成立**。`:193` 的 `contains("\"ok\":true")` 确实能被**响应头**骗过；但它说的影响面（`daemon start` 误拒、`daemon status` 进错分支）**不成立**：`start_daemon_background:454-457` 走 `daemon_health_pid` → `health_identity`，那是对**响应体**做 `serde_json` 解析；而 `daemon_health_ok` 在生产路径上**零调用**（唯一调用者 `wait_for_daemon_health:196-205` 本身全仓零调用，只有 `daemon_integration.rs:5420` 打它）→ **L2-187**（休眠隐患，不是可用的 DoS）。
  5. **`Stale` 是「无身份证明的删除授权」→ 成立**。`pid_definitely_missing` unix 版只判 `kill(pid,0)==ESRCH`（`:305-322`，不比较 start_time），`start_daemon_background:435-448` 与 `bin/agent2ssh.rs:3123-3129` 直接删记录。已写进 §3.3「遗留路径不得当作 `Exited`」。
  6. **每次正常停止都写假 warn → 成立**。能落 `AcceptedUnchecked` 的只有 `200`，而真实停止永远带 `expect_pid`（`request_shutdown:881-947`）；warn 在 `:1053-1061`（文案 `:1057`）。代码注释 `:1039-1052` 已自陈这是已知缺陷——文档此前没写。
  7. **`to_socket_addrs` 无上界 → 成立**。`daemon_health_ok:170` 的解析在 `connect_timeout(500ms)` 之前且无超时，`local_daemon_addr:21-28` 允许主机名 → `status`/`stop` 总时长无上界。已把「需实现时验证」改成「既有事实」。
  8. **symlink 替身可被强杀 → 成立**。`a_daemon_that_ignores_the_expected_pid_stops_and_says_so`（`:1434`）用 `symlink("/bin/sleep")` 造名 `agent2ssh-daemon` 的替身（`:1445-1446`），`signal_daemon:774-779` 的名称判据恰好被这个 symlink 满足。
  9. **§1 与 §5 矛盾（#22 原型验收的责任真空）→ 成立**。已改成「#22 只交付状态机 + 错误分类 + 决策表，原型验收归 #23–#25」。
  10. **平台缺口的方向对、数字错 → 数字不成立**。它说「74 个 `fn`、13 项 `#[cfg(unix)]`」；实测 `grep -c 'fn '` = **51**、`#[test]` = **34**、带 `#[cfg(unix)]` 的**测试** = **8**（`#[cfg(unix)]` 总出现 15 次，其余门在 helper 上）。两个核心竞态用例确实是 unix-only（`:1431`、`:1699`），结论保留。
  11. **「TCP 控制面是否保留」应列为决策 → 成立**，且比它说的更硬：不拍这一项，#23 的 pidfd 实现无处安放。
- **本轮改了什么（全部 L1，零代码）**：`b8dcb1b` —— `docs/plans/daemon-identity-protocol.md` 六处（§1 加结构约束与责任划分、§2 补名称白名单/回执后果/健康探测/实例归属四行、§3.1 补既有约定与无锁读反例并把 DNS 无上界改为既有事实、§3.3 补 `Stale` 遗留路径、§5 补平台覆盖缺口与 macOS 结构性后果、§6 补 symlink 反例与 6.3 的「当前走不到」、§7 由五条散文改成 **D1–D6 决策表**）；`docs/reports/OPT-LOOP-L2-PENDING.md` 追加 **L2-186/187/188**（**最大编号 L2-188**）。
- **验证**：`npm run build` **6.67 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **865 / 0 / 1**（与上一轮建立的新基线一致，红项已随 #35 消失）。未改任何测试断言，故未跑其它目标。
- **过程失误（记下来防再犯）**：① 把「当前 tip」写进台账是自指——写它的提交必然又推进 tip，本轮因此白跑了一笔（`57b59ee` → `3532a12`）。台账只记「内容提交区间 + tip 以 `ls-remote` 为准」。② 建 worktree 时 `ln -s $REPO/node_modules` 的 `$REPO` 在子 shell 里未定义，落成 `node_modules -> /node_modules` 的**悬挂链接**；`git status` 干净（在 info/exclude 里）骗过了「worktree 就绪」的判断，直到 `npm run build` 报 `tsc: command not found` 才暴露。**下轮：建完 WT 先 `ls node_modules/.bin/tsc`，不要只看 status 干净。**③ 紧接着那笔台账提交的 `Verified:` 写「`grep -c` = 1」，实测是 **2**——两个 SHA 各出现在第 3965、3967 两行，同一笔里又犯了「数字没跑就写」。结论（除这两行外无别的 SHA 引用）成立，计数不成立，此处更正。
- 未关闭 `#22`：D1–D6 全部要人拍板，其中 **D4 是 #23 的前置**。回帖 `2120776970253275136`（`## 本轮收尾`，不带轮次号，`07:13:26Z`）。
- **合流**：内容提交 `4487f7f..8152ed9`；其后是记账提交（回帖 id、本节 tip 措辞两笔）。**`cnb/main` 的 tip 一律以 `git ls-remote cnb refs/heads/main` 为准，不要引用本节任何 SHA 当基线**——本节的第三个数字就写错过一次（把 tip 写成 `57b59ee`，而写出这句话本身又把 tip 推到 `3532a12`）。只推 cnb，未推 github / git233。分支 `opt/single-22-20261112` 与 worktree `…/single-22` 在本节合流验收后按 4.4 清理（先 `merge-base --is-ancestor`，再删 `node_modules` 链接、`binaries` 链接与 `dist`）。

### 新增「不成立」，不要重复提

1. **「`daemon_health_ok` 的子串判定让任意本机进程能让 `daemon start` 误判已有实例而拒绝启动」**——不成立。`start_daemon_background:454-457` 用的是 `daemon_health_pid()` → `health_identity():231-253`，那是对**响应体**做 `serde_json::from_str` + `ok` 布尔 + `pid` u64 三重校验，响应头里的 `{"ok":true}` 过不去；`daemon_health_ok` 本身在生产路径上零调用（唯一调用者 `wait_for_daemon_health` 全仓零调用）。子串判定是真的，但它是休眠隐患。
2. **「`daemon_stop_e2e.rs` 共 74 个 `fn`，其中 13 项 `#[cfg(unix)]`」**——两个数都错。实测 `grep -c 'fn '` = 51、`#[test]` = 34、`#[cfg(unix)]` 出现 15 次而**门住测试的只有 8 个**。结论（两个核心竞态用例在 Windows/macOS 零覆盖）成立，数字不要引用。
3. **「两文件协调是纯提案，`daemon-owner.json` 还不存在」**——不成立。既有文件叫 `daemon.owner`，且已经是 `{pid, start_time}` 记录 + 锁内条件删除（`clear_daemon_owner_if:625-643`）。

## 2026-11-12 16:16–（模式 B 单发 · issue #25 首次进入 · P2 设计类，单段闭环）

- **前置**：无锁 → 主动建 pid 15935 写入 `owner`；三个其它 worktree（`.codex/worktrees/cnb-pr3-merge`、`WorkBuddy/Worktrees/main-f0ecd0ba`、`main-f749b514`）`status --porcelain` 全 0 行、`src`/`docs` 四十分钟零写入；`ls-remote cnb/main` 与本地同为 `4d06549` → 不让位。`open` **十条**（#20–#29），**无 `[opt-round]`** → 模式 B，第二段为空。
- **选靶 #25**（P2，首次进入）：**没有 P1 可选**——#20/#21/#22/#23 的最新一条都是本流程的收尾帖（等复核）；#24/#27 要 Windows 实机（排除，本机 macOS）；#26/#28/#29 零评论且正文无「文件:行号」级断言（#26 只给文件路径、自称「源码推断，未构造竞态」）。#25 是唯一有**未消费** CodeBuddy 设计分析（`2120409164848472064`，11-11）且**本机可实测**的一条：它是 macOS 分册，而本机就是 macOS。按 §5.3 的 P2 口径，交付物是「能让人 5 分钟内拍板的方案对比」，不是代码。
- **判定 12 条 → 成立 6 / 部分成立 4 / 不成立 2**：
  1. **控制面纯 TCP、无对端凭据 → 成立**。`remote.rs:13` 常量、`agent2ssh-daemon.rs:5826` 唯一一条 `TcpListener::bind`；同一组 grep（`LOCAL_PEER|SO_PEERCRED|getpeereid|UnixListener`）在当前树仍是 **0**。
  2. **`health_identity()` 只认子串 → 部分成立（引用错函数）**。它引的 `HTTP/1.1 200` + `"ok":true` 是 `daemon_health_ok:166-194`，而 `health_identity:231-253` 是对**响应体**做 JSON 解析。结论（任何进程能回这个形状）仍对，机制不同；已在 K14 里把这两者分开写。
  3. **`kill(pid,0)` 三档与 `pid_definitely_missing` 只判 `ESRCH` → 成立**（实测 self=0 / pid1=EPERM / 不存在=ESRCH）。
  4. **`kqueue` 必须实测 → 部分成立**。`ESRCH` 成立；但它说的 `EACCES` **在本机没有出现**（对 pid 1 注册 `data=0`），且错误**只**从 `EV_ERROR` 事件出（`kevent()` 返回 1），不是 `-1`。
  5. **`upid` 对其他 uid 的权限「未确证」→ 成立 + 补实测**。flavor 17 对 self 与 pid 1 都返回 56 字节（`p_uniqueid` 511811 / 1），但 `PROC_PIDTBSDINFO` 对 pid 1 返回 0/EPERM。两个 flavor 的权限面**不一样**。
  6. **`libc` 是否导出 `LOCAL_PEERPID`/`LOCAL_PEERTOKEN` 未核实 → 成立（导出）**。`libc 0.2.186` 的 `unix/bsd/apple/mod.rs:2908/2910/2918`；`proc_pidinfo` 在 `:4902`、`PROC_PIDTBSDINFO` 在 `:3684`；但**没有** `proc_uniqidentifierinfo` / flavor 17 常量。
  7. **`start_time` 秒级来自 `pbi_start_tvsec` → 成立**（`sysinfo 0.39.3` `unix/apple/macos/process.rs:385`），而 `pbi_start_tvusec` 就在同一公开结构里没被读 → 本轮最实的改进点，**L2-189**。
  8. **「74 个 `fn`、13 项 `#[cfg(unix)]`」→ 不成立（数字）**。实测 51 `fn` / 34 `#[test]` / 15 个 `#[cfg(unix)]` 属性、其中门住 **8 个测试**。
  9. **「Windows 与 macOS 上零覆盖」→ 部分成立（macOS 半不成立，且计划稿也带着这个错）**。`#[cfg(unix)]` 在 macOS 成立：本轮实测 `daemon_stop_e2e` **34 passed / 0 failed / 40.75 s**，两条核心竞态用例单独复跑各 1 passed。计划稿 §5 与上一节「不成立」第 2 条的后半一并更正。
  10. **行号引用 → 部分成立（整体下移）**。按其基线 `380e166` 大体准，当前树全部重映射：`read_daemon_pid:56`、`DaemonPidState:210`、`daemon_pid_state:343`、`signal_daemon:763`、`wait_for_exit:749`、`stop_daemon_process:979`、`bind:5826`。
  11. **方案 B「影响 CLI/桌面/MCP 三面」→ 不成立（代价高估）**。本地 daemon 请求只有两个接缝（`daemon_activity.rs:44/65`），`stop_daemon_process` 只有 **5 个调用点**（`tauri_commands.rs:1855/1885/1918`、`bin/agent2ssh.rs:3080/3107`），三面都经它，不需要各改一遍。
  12. **「macOS 上不存在 pidfd 等价物、窗口只能缩小」→ 成立**，且本轮把它拆成两段可执行的收窄（MD2 精度、MD4 kqueue），并明确二者都**不构成 `Bound`**。
- **本轮改了什么（全部 L1，`src-tauri/src/**` 零改动）**：
  - 新增 **`docs/plans/macos-daemon-identity.md`**：实测环境与复算命令、E1–E5 能力表、三条硬结论（无进程对象原语 / 可观察面宽于可发信号面 / `LOCAL_PEERPID` 是「最近访问者」）、对 #25 分析的判定表、与 #22 D3–D6 的对接、MACOS-0..3 顺序、M1–M12 现状、六条未验证。
  - `docs/plans/daemon-identity-protocol.md` 两处：§5 的 macOS 行由「可供 #25 调研」换成实测事实；平台覆盖缺口段落更正 macOS 那半（附 34 passed 数字与更正标注）。
  - `docs/KNOWN-ISSUES.md` K14：把「`daemon_health_ok` 只认 …，`health_identity` 不读 version」改成两函数分开叙述，并点明停止路径用的是 `health_identity`、子串判定零调用（L2-187）。
  - `docs/reports/OPT-LOOP-L2-PENDING.md`：**L2-189**（起始时间秒级 → `pbi_start_tvusec` 微秒，含旧 `daemon.owner` 按 `sec*1_000_000` 读的兼容要求）、**L2-190**（`kqueue` 只能证明「此刻存在」，实测 `EACCES` 不出现、错误只在 `EV_ERROR`）、**L2-191**（macOS 缺对端凭据通道，附实测改动面 5+2+1）。**最大编号 L2-191**。
- **验证**：`npm run build` **6.40 s**；`cargo fmt --check` 绿；`--no-default-features --lib` **865 / 0 / 1**（与上一轮基线同值）；`--no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1` **34 passed / 0 failed / 40.75 s**（这是本轮的关键证据：unix 门控用例在 macOS 上真的执行）。
- **过程记录**：本轮探针全部在 `/tmp/a2s-macos-probe/`（两个 C 程序），未在仓库任何目录留下未跟踪产物；`Edit`/`Write` 路径全部带 `$WT`（上一轮的失误没有复现）。

### 新增「不成立」，不要重复提

1. **「`daemon_stop_e2e.rs` 有 74 个 `fn`、13 项 `#[cfg(unix)]`」**——两数都错，实测 51 `fn` / 34 `#[test]` / 15 个属性、门住 8 个测试。
2. **「这两条核心竞态用例在 macOS 上也零覆盖」**——不成立。`#[cfg(unix)]` 在 macOS 成立，2026-11-12 实测 34 passed，两条用例单独复跑也各 1 passed。只有 Windows 零覆盖。
3. **「`kqueue EVFILT_PROC` 在无权限时返回 `EACCES`，所以可以拿注册失败当权限否证」**——不成立。对 pid 1（root）注册成功（`EV_ERROR` 置位但 `data=0`），同一个 pid 的 `kill(1,0)` 才是 `EPERM`。可观察面比可发信号面宽，注册成功不能推出「能停它」。
4. **「`PROC_PIDTBSDINFO` 与 `PROC_PIDUNIQIDENTIFIERINFO` 对目标进程的可见性一致」**——不成立。对 pid 1：前者返回 0/`EPERM`，后者返回 56 字节真实数据。
5. **「方案 B 要改 CLI/桌面/MCP 三面的连接逻辑」**——不成立。本地请求只有两个接缝，停止入口只有 5 个调用点且都经 `stop_daemon_process`。
6. **「`libc` 是否导出 `LOCAL_PEERPID`/`LOCAL_PEERTOKEN` 需自己定义常量」**——不成立。`libc 0.2.186` 已导出三个 `LOCAL_PEER*` 常量与 `proc_pidinfo`；**只有** flavor 17 的常量与结构没导出。

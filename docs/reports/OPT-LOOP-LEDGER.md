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

**进度载体是 issue 本身**：消费任务靠数 issue 内 `## 第 K 轮收尾` 评论的数量判断续跑位置，不依赖 automation 的记忆或本文件的进度字段。所以单次运行被切断（180 分钟预算用尽）不会丢进度——下次运行从 C+1 轮继续，6 轮总量由评论数保证。

**每轮 3.4 的回帖不可省**：它既是「再发」，也是让 CodeBuddy 在下一轮看到**改后的新代码**并复核自己上一轮的判断。跳掉这一帖，6 轮就退化成 6 次互不相关的单方面改动。

---

**BATCH_COMPLETE** · 2026-11-10 · issue #8 · batch 2「SSH 传输、known_hosts 与认证（russh 侧）」6 / 6 轮已闭环，issue 已关闭。批次汇总见文末「批次汇总 · 2026-11-10 · issue #8」。消费任务此后只报告状态。

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
- 6 轮进度：2 / 6（第 1 轮见下方 `consume 1 / 6`，第 2 轮见 `consume 2 / 6`）
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
- 6 轮进度：2 / 6
- 正文已喂给 CodeBuddy：**28 条「不成立」原文**（preflight 2 + batch 1 六轮 10 + batch 2 第 1 轮行内 D2/D4 两条 + batch 2 第 2–6 轮 14）+ K7/K8/K9/K12/K13 相关空档 + 口径提醒（基线红测、`.github/**` 禁区、四列计数）。范围：`docs/skills.md`、`docs/api.yaml`、`bin/agent2ssh.rs` clap help、`bin/agent2ssh-mcp.rs`、`skills/agent2ssh/SKILL.md` 及 guides 相关段落。

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

BATCH_COMPLETE · 2026-11-10 · issue #7（daemon 控制与停止协议）6 / 6 轮已闭环，issue 已关闭。下一次消费任务只输出一行状态，不开新批次；新批次由生产任务另开 `[opt-round]` issue 后在顶部把我这行删掉。

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

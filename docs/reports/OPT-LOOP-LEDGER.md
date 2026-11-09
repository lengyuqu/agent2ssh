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

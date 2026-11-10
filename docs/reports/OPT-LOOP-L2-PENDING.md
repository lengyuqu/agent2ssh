# L2 待拍板清单（优化轮次）

`src-tauri/src/**` 的行为/签名改动、Windows 语义改动，按消费任务的分级规则**不进 main**，攒在这里等人工决策。每条都写清为什么不能自动落、以及落地时的验证要求。

---

## 2026-11-10 · 来自 issue #2 的消费（文档行为一致性）

### L2-1 诊断文案与它描述的控制流不符

`src-tauri/src/daemon_control.rs:1019-1024` 的 warn 文案是：

```
"daemon did not exit after a guarded shutdown; leaving it running"
```

但紧接着 `:1025` 就 `signal_daemon(pid, start_time, true)` 强制终止，并再 `wait_for_exit(2s)`。文案说「保持运行」，实际是「先杀一次，2 秒后还活着才放弃」。同一次消费已经修掉两处受此误导的用户文档（`docs/guides/configuration-guide.md:377`、`docs/KNOWN-ISSUES.md` K15），但日志本身还在说反话——查日志的人会得到与文档修正相反的信息。

**为什么归 L2**：改动落在 `src-tauri/src/**`。消费任务的自动验证口径只有 `--no-default-features` 的 lib 测试，而这条改动要完整验证需跑 default feature（依赖 `dist/` 与 `src-tauri/binaries/<name>-<triple>` 侧车），超出自动路径。

**建议改法**（2026-11-10 第 2 次收尾后优先级上调）：**前提是 `signal_daemon` 的提前返回要能区分「没发」**。它有两个在 `process.kill()` 之前的 `return Ok(())`（`:732` 快照无该进程、`:735-739` 启动时间/僵尸/名称白名单任一不符），这两处一个信号都不发且是有意的。只要这个区别不可观测，这条分支的任何文案都写不准——写「尝试」也无法让人知道这次到底试没试。所以三步：

1. 让 `signal_daemon` 返回可区分的值（`Signalled` / `Skipped(reason)`），或在那两个 `return Ok(())` 处各记一条诊断；
2. 文案按 `sent` 真/假分叉，把「已经杀过但没杀掉」与「身份已无法确认、拒不发送」写成两句；
3. force 之后的第二次 `wait_for_exit` 结果另记一条诊断（已退出 / 仍在运行），让「杀了但没杀掉」在日志里可区分，而不是只能从返回码反推。

对应地，`DaemonStopError::StillRunning` 现在是这两条路的合流点，运维从报错分不出是哪种；若要分开，需新增一个错误变体（改变对外契约，须单独决策）。

**验证要求**：`npm run build` + `./scripts/prepare-sidecars.sh` 后跑 `cargo test --manifest-path src-tauri/Cargo.toml --lib`（default feature，唯一编译 `tauri_commands` 测试的目标）+ `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`。

### L2-2 `does not report its config directory` 分支无断言

`daemon_control.rs:970` 的拒绝分支（旧 daemon 的 `/health` 不报 `config_dir`）现在没有任何测试钉住：唯一覆盖它的 `legacy_daemon_without_pid_guard_is_not_automatically_stopped` 已由 `f373f2c` 删除，替代品全是「token 被拒」分支。语义上这条分支意味着旧 daemon 在升级前**停不了**，属真实限制，但只有代码可读。

**为什么归 L2**：要补的是 `tests/daemon_stop_e2e.rs` 里的断言。虽然该文件在 `tests/**` 而非 `src/**`，但它需要起真实 daemon 进程验证旧版健康响应，且断言内容涉及「旧 daemon 应被拒绝还是要兼容」的语义选择——那是产品决策，不是自动流程能拍的。

**建议改法**：加一条用例，构造一个 `/health` 响应不含 `config_dir` 的假 daemon（现有 fixture 支持子命令注入），断言 `stop_daemon_process` 返回 `UnverifiedIdentity` 且 message 含 `does not report its config directory`，且 PID 文件保留。

**验证要求**：`cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1`，并在 Windows 腿确认这条分支在 Windows 上同样可达（当前该文件在 GitHub Actions 的 `build` 矩阵里三平台都跑）。

---

## 2026-11-10 · 来自 issue #7 第 1 轮（daemon 控制与停止协议）

本轮核实 26 条指摘（成立 14 / 部分成立 8 / 不成立 4）。下面 16 条要么改 `src-tauri/src/**` 的行为、要么需要新的测试夹具，**不进 main**。A3 与已有 L2-1 是同一处（`daemon_control.rs:1019-1025` 的文案），不重复登记；F1（强制再杀分支无测试）随 L2-1 一起做。

**统一验证要求**：`npm run build` → `./scripts/prepare-sidecars.sh` → `cargo test --manifest-path src-tauri/Cargo.toml --lib`（default feature，唯一编译 `tauri_commands` 的目标）→ `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`；动到停止路径的再跑 `cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1`。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-3 | `daemon_control.rs:867,882,991`；`bin/agent2ssh-daemon.rs:192-208` | 成立，但方向与指摘相反 | 改的是回包形状或对外语义 | 指摘说 `AcceptedUnchecked` 是「死分支」，实测相反：`daemon_shutdown` 核对通过也回 200 `{"ok":true}`，带 `expect_pid` 的停止必然走 `:882`。所以要么让 daemon 在 200 body 里带已核对的 pid（`{"ok":true,"checked_pid":N}`），客户端据字段判定；要么把 `:1001` 的 warn 从「daemon predates it」改成「没有检查回执」，并补一条断言：**新 daemon 的停止不产生这条 warn**（现有 `daemon_stop_e2e.rs:180` 只断言成功与耗时，warn 无人看） |
| L2-4 | `daemon_control.rs:986-988`；Display `:787-791`；`tests/daemon_stop_e2e.rs:765-788` | 部分成立 | 改错误文案 + 收窄测试断言 | `NotAuthenticated` 不是不可达（`daemon.token` 被删后 `/health` 仍公开可答，会走到 `:987`），但它的文案说「daemon 不接受这个 token」，实际触发条件是「本配置没有 token 文件」——一个字都没发出去。两件事要分开措辞；测试的 `\|\|` 应改成断言实际可达的那一条，另起用例覆盖「token 文件消失」 |
| L2-5 | `tauri_commands.rs:1902`（`stop_owned_daemon`）、`:2022`（`confirm_quit`） | 成立 | 改桌面停止/退出判据 | 两处都用 `process_is_alive(pid)` 当前置条件，而它对改名 daemon 返回 false（`daemon_process_start_time` 要求 `process_looks_like_daemon`），`daemon_pid_state` 却回 `Healthy`。后果：改名后的自有 daemon 在退出应用时既不被停、也不弹确认框。改判 `daemon_pid_state(...) != Stale` 或复用 `daemon_is_owned` |
| L2-6 | `daemon_control.rs:38-44` | 成立（低危） | 改错误分叉 | `parse().ok().filter(!=0)` 把「并发写一半 / 空」与「数字但为 0、溢出」压成同一句「invalid … inspect it」。前者应提示稍后重试（指南 `:366` 已这么写），后者才需要人工 |
| L2-7 | `daemon_control.rs:61-69`、`:498` | 部分成立 | 改启动失败路径 | `write_daemon_pid_if_absent` 用 `create_new`，`AlreadyExists` 被 `let _ =` 吞掉。指摘说「实际没保留」不准确（文件若由该 child 写过，PID 就在里面），真正缺的是**结果不可观测**：`Err(AlreadyExists)` 应按成功处理，其余错误单独记诊断 |
| L2-8 | `daemon_control.rs:356-384`、`:429` | 部分成立 | 改日志轮转策略 | 轮转只在真正要 spawn 时发生（`:393` Healthy 提前返回、`:418` 拒绝启动都不轮转），所以指摘给的触发条件（端口被占反复 `daemon start`）走不到；真实缺口是「同配置目录、旧 daemon 仍持有 `daemon.log`，但 PID 文件已不在或地址已改」——此时 `rename` 会把在写的文件搬到 unlinked inode。建议轮转前要求 `daemon_pid_state == Stale`，或改成 `daemon.log.<timestamp>` 保留 N 份 |
| L2-9 | `daemon_control.rs:488-500`、`:508`；`tests/daemon_stop_e2e.rs:1003-1049` | 部分成立 | 需新夹具（kill 失败） | `:508` 的 `remove_daemon_pid_file_if` 返回值被丢弃，`:499` 那条「kill 失败且 child 仍活」的路径没有任何断言。`:1006` 的两个 `publish_pid` 变体都属于「child 被成功杀掉」。可把 `:481-505` 抽成可注入 `try_wait` 错误的函数再断言，不必造忽略 SIGKILL 的 stub |
| L2-10 | `daemon_activity.rs:198-217`；`tauri_commands.rs:2026` | 成立 | 改超时预算 | 四个串行请求各 2s connect + 2s read，最坏 16s；`confirm_quit` 在持有退出流程时同步调用它，期间 UI 冻住。给整个 `fetch_daemon_activity` 一个 3s deadline（逐请求传剩余预算），超预算回 `None` 并记诊断 |
| L2-11 | `daemon_activity.rs:186-195` | 成立 | 改口径 | `resident_sessions` 缺 `dead` 字段算「活着」，`pending_approvals` 缺 `status` 算「不算」，两处默认方向相反，`is_idle()` 的结论随字段漂移翻转。缺字段应整体返回 `None`（unknown），或为每个 list 定义最小 schema |
| L2-12 | `daemon_activity.rs:124-127` | 成立 | 改客户端读取方式 | `read_to_end` 无字节上限，且该客户端被 `daemon_control::health_identity` 用于停止决策；`AGENT2SSH_DAEMON_ADDR` 可以是任意主机（daemon 只 warn 不拦，`bin/agent2ssh-daemon.rs:5725-5731`）。按 `Content-Length` 读 + 1 MiB 硬上限 |
| L2-13 | `daemon_activity.rs:76-91` | 成立 | 公开 API 面 | `#[doc(hidden)] pub fn direct_daemon_request` 唯一使用者是 `tests/daemon_stop_e2e.rs:1242`，却提供了一个「忽略配置直连任意地址」的公开入口。改成 `pub mod testing` 或在 `docs/api.yaml` / `architecture.md` 登记，别用 `doc(hidden)` 两头不占 |
| L2-14 | `bin/agent2ssh-daemon.rs:4147,4153,4182`；`prompt_waiter.rs:256-302` | 部分成立 | async 上下文里的锁策略 | 指摘说「同步持锁会卡住 runtime」过头了：三个 `*_blocking` 持锁期间没有 `.await`，`oneshot::send` 与无界 `mpsc::send` 都不阻塞。真问题是 `.lock().unwrap()`：registry 一旦被 panic 毒化，handler 在 async 里 panic，`install_panic_hook` 只记录，客户端拿不到 500。换 `unwrap_or_else(\|e\| e.into_inner())` 即可 |
| L2-15 | `bin/agent2ssh-daemon.rs:4055-4100` | 成立 | 改路由语义 | `shutdown.cancel()` 之后进 handler 的请求仍会 200 并写一条 `stream_connected` 再立即结束。入口加 `s.shutdown.is_cancelled_now()`（`ws_drain.rs:63` 已有同步判定）回 503，与 `/exec` 的 `SERVICE_UNAVAILABLE` 对齐 |
| L2-16 | `bin/agent2ssh-daemon.rs:1213-1229`；`daemon_control.rs:141-169` | 部分成立 | 改健康语义（对外契约） | `health()` 的 `ok` 恒为 true，`config_dir_available` 才是真实状态；`daemon_health_ok` 只认 `"ok":true`。是否让 `ok` 表示「能服务」是契约决策（`api.yaml` 把 `ok` 列为必填），先拍板再动 |
| L2-17 | `bin/agent2ssh-daemon.rs:1067-1076`、`:1863-1900` | 成立 | 改请求体或文档契约 | `ExecCompareBody` 没有 `source`，限流的 source 维度对它恒为传输层默认值，与 `ExecMultiBody` 的 `source: Option<String>` 不对称。补字段会改 API，至少要先把「compare 恒按传输层身份计」写进 `api.yaml` |
| L2-18 | `bin/agent2ssh-daemon.rs:5009,5428,5937` | 成立（建议） | 加常量要改 src | 2s/4s 只以字面量存在，`GRACEFUL_STOP_WAIT`（8s）与 daemon 的 drain 预算无法靠代码互相验证。提成 `WS_DRAIN_PER_CONNECTION` / `WS_DRAIN_REGISTRY` 并加一条 `assert!(GRACEFUL_STOP_WAIT > 两者之和)` 的测试 |

---

## 2026-11-10 · 来自 issue #7 第 2 轮（CodeBuddy 复核后的新一批）

第二轮指摘 15 条（L2-19…L2-33），核实后**成立 6 / 部分成立 5 / 不成立 2**（另有 2 条已在第 2 轮的 L1 改动里做掉）。这一批里有两条是真正的代码缺陷，其余是观测与措辞问题。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-19 | `:4824`、`:5326`；`:5009`、`:5428` | 部分成立 | 措辞 + 两处实现不一致 | 「每连接 2s」不等于「N 条叠加成 2N」：两个 handler 各跑一个 2s 上限，连接之间**并行**。而 500ms 在两处语义不同——terminal（`:4824`）是**每次 recv 重置**的 500ms 空闲超时，exec（`:5326`）是取消时刻起的**绝对 500ms 截止**。所以「共享一个 500ms 窗口、最短 0.5s」不成立（terminal 的循环在 `event_rx` 立即关闭时根本不等）。文档已按「并行 + best effort」改写（L1）；两处语义是否统一属设计取舍 |
| L2-20 | `:5937` vs `:5926-5929` | 部分成立 | 结论影响常量推导 | 「串行」成立：`with_graceful_shutdown(...).await` 先结束，之后才 `wait_until_empty(4s)`。但「最坏 4s 不是 6s」不成立——2s 的 per-connection 上限在 graceful 阶段已经跑完，registry 再单独计时 4s，**最坏仍是约 6s**。4s 之所以每次都烧满，根因是 L2-26 的 entry 泄漏，那是缺陷，不能用它把上界改小 |
| L2-22 | `:4999-5001`、`:5417-5420`、`:119-130` | 成立 | 改返回类型 + 诊断 | `drained == false` 同时表示「取消后没排空」与「超时被 drop」，`is_ok()` 把两者折成一个 bool。建议让 `run_ws_with_shutdown_deadline` 返回枚举（`Completed` / `Drained` / `Dropped`），两条 warn 分开写 |
| L2-23 | `daemon_control.rs:488-500`、`61-68` | 成立（并入 L2-9） | 同 L2-9 | 比 L2-9 更紧的一层：`child.kill()` 失败 → `write_daemon_pid_if_absent` 返回 `AlreadyExists` 被吞时，状态是「child 仍活着 + PID 文件写的是别人」，没有任何路径能发现。修 L2-9 时一并处理 |
| L2-25 | `:5934-5945` | 成立 | 改返回/诊断 + 契约 | `wait_until_empty` 返回 false 时只 warn，函数仍 `Ok(())`；所以「drains open WebSocket connections」在超时情形下不成立。文档已改成 best effort（L1）；代码侧应把「有几个连接没排空」写进诊断，或让关闭结果带上这个事实 |
| L2-30 | `daemon_stop_e2e.rs:1315`（`#[cfg(unix)]`）vs `:1412` | 成立 | 测试夹具 | Windows 上「409 分支」有覆盖、「200 → AcceptedUnchecked」零覆盖，而 L2-3 要改的正是后者。补一个在 `cfg(windows)` 下也成立的等价用例，至少断言「新 daemon 的停止不产生 predates warn」 |
| L2-31 | `:1212-1228` vs `api.yaml:565` | 部分成立 | 契约 | `health()` 恒回 `version`，`api.yaml` 把它列为可选，`health_identity` 不读。若要用版本号区分新旧 daemon，得先提为必填并让客户端读——但同版本的两个构建仍可能一个有 `checked_pid` 一个没有，所以最终判据只能是字段本身 |
| L2-33 | `:119-130` | 部分成立 | 改名/注释 | 返回值语义是「future 是否在 deadline 前走完」，调用点却当「drain 是否完整」用。改名或补注释，否则 L2-22 的两种 `false` 永远混在一起 |

**L2-26 已撤回**（第 3 轮核实为不成立）：`drain_handle.unregister()` 在 `ws.on_upgrade` 闭包内、`run_ws_with_shutdown_deadline` 的 future 之外——future 在 `:5428` 收口（缩进 8），`unregister` 在 `:5499`（缩进 8），`:5500` 的 `})` 关的是 `on_upgrade`。`0e88d7c` 正是把它从块内挪出来的那笔提交。所以 2s 超时 drop 之后它照常执行，registry 不泄漏，「每次关闭多烧 4 秒」这个前提不存在。详见 ledger 第 11 条。

---

## 2026-11-10 · 第 4 轮：三条 L2 的落地形状（已核实）

第 4 轮不是新缺陷，是对上文三个待拍板项的答复。以下是核实后可以照着做的规格；**行号一律以本轮结束时的树为准**（`daemon_control.rs` 因 `2a7dde2`/`cf09e2c` 的注释扩写整体下移 5 行，上文 L2-1…L2-26 里引用该文件的裸行号需 +5）。

### L2-3 的落地形状：加字段，不换状态码

**否掉换状态码**：`request_shutdown` 里 200 只有两个分支（`:872` 无 body、`:887` 带 body），非 200 全落 `:891/:896` 的 `NotAccepted`，`:1002` 那条 warn 根本不执行。换状态码等于把 warn 变成静默，且只读 status 的旧调用方会把一次成功的停止报成失败。

**采用加字段**，三条理由都已核对：
- `OkBody`（`:1194-1196`）在该文件里出现 **30 次**，是共用类型，不要动它；新增 `ShutdownAccepted { ok: bool, checked_pid: u32 }`，只给 `daemon_shutdown`（`:168`）用。
- 到 `:208` 的三条路径（`Some(Some(_))` 匹配、无 body、省略字段）回的都 `own_pid`，所以「有字段」与「核对过」等价。
- `version` 不动：判据是字段本身，不是版本号——同版本的两个构建也可能一个有 `checked_pid` 一个没有（L2-31 的顾虑就此消掉）。

客户端侧：`:887` 改为按 body 里的 `checked_pid` 与 `expected_pid` 比较分叉，**不要复用 `:910` 的 `reported_pid`**（那是解析 409 错误文案的）。`:1006` 的文案同步改成「accepted the shutdown without reporting which PID it checked」。

**配套断言目前不存在**：全仓 `git grep "daemon predates"` 只命中实现处 `:1006`，`tests/` 里一条断言也没有；`daemon_stop_e2e.rs` 唯一读 `app.log` 的地方在 `:397`，断言的是 `WebSocket drain deadline expired`。要做的是在 `stop_daemon_process_waits_for_a_graceful_exit`（`:180`，它 `Drop` 时会删掉整个临时目录，读日志必须在删之前）里先读 `app.log` 再断言**不含**该句。

**未定的一项**：`:872` 的守卫让 `Accepted` 臂在带 body 的路径上不可达。只做「加字段」就能消掉假阳性；要不要顺手清掉这条不可达臂（会牵动 `request_graceful_shutdown` 的 `matches!` 语义）需要单独拍板。

### L2-25 的落地形状：只加诊断，不改返回值

不改 `wait_until_empty` 的 bool：`ws_drain.rs:402` 断言 `!wait_until_empty(...)`、`:406-407` 直接 `unwrap` 结果，`bin/agent2ssh-daemon.rs:5936-5941` 是 `if !…`——换类型是零收益的破坏。

要加的是诊断：`ws_drain.rs:187` 的 `active_count()` 已经是 `pub`，在 `:5936` 前后各取一次，`tracing::warn!` 带两个数，并让 `append_diagnostic_log` 带同样的 JSON（可 grep）。**文案必须写 `still registered after the deadline`，不能写 `did not drain`**——超时那一刻 handler 的 cleanup 还在跑，计数不等于「没排空的连接数」。

### L2-22 / L2-33 的落地形状：枚举 + 改名的调用点

```rust
enum DrainOutcome {
    Completed,  // future 自己跑完，shutdown 没打断
    Drained,    // 取消后在 grace 内跑完
    Dropped,    // 取消后 grace 用完，future 被 drop
}
```

- `:5010` 与 `:5429` 的 `if !drained` 改成 `matches!(outcome, DrainOutcome::Dropped)`：只对 `Dropped` 记 warn。这顺手修掉一处现有噪声——连接自己先结束时也会记「did not drain before shutdown deadline」。
- `:4813`、`:5316` 的 `let drained` 必须改名 `let outcome`，否则 L2-33 的混用原样保留。
- **`:5986` 的 `!timeout(...).unwrap().unwrap()` 会编译不过**（`!` 没有枚举实现），这是全仓唯一被类型改动打中的断言；`:5992` 的 `flag.load` 语义不变，保留。
- 现在只有 `Dropped` 一条用例，枚举另两臂零覆盖，需补 `Completed`（不 cancel、future 自己结束）与 `Drained`（cancel 后 grace 内返回）两条。

**追加一条级别问题（比命名更重要）**：`daemon_control.rs:1006` 现在用 `warn` 记的是一次**成功**的停止（请求被接受，只是没有检查回执）。加 `checked_pid` 之后，剩下走这条的只有「旧 daemon」与「回了字段但值不符」，但仍然是成功。落地时把它降成 `info`，并让 `component` 承担区分职责（客户端侧是 `daemon_control`，WS 侧是 `daemon_exec_stream` / `daemon_terminal`），比继续在文案里堆形容词有效。

---

## 2026-11-10 · 第 5 轮：线程比它服务的 future 活得久

第 5 轮给 6 条，核实后 **成立 4 / 部分成立 1 / 不成立 1**。N-1 与 N-3 是同一个模式的两次复发：**线程持有资源的时间长于调用方以为的时长**。两者都建议共用「`Arc<AtomicBool>` + 循环内检查」这一套，同批做。

| 编号 | 位置 | 判断 | 建议改法 |
|---|---|---|---|
| N-1 | `forward.rs:298-308`（`join_timeout` 返回 `()`）、调用点 `:231`（`stop_worker`）、`:275`（`Drop`）、`:483`/`:488`（启动失败回滚） | 成立（它说「三处」，实际是**四处**） | `join_timeout` 返回 `bool`；`false` 时 `set_state(StoppingError)` + 诊断。`:231` 之后 `:241`/`:285` 无条件写 `Stopped`，而 worker 可能正阻塞在每连接 60s 的 `connect_embedded_ssh` 里，监听套接字还在 → 立刻 `daemon`/规则重启会 `EADDRINUSE`。已在 `stop_worker` 加注释说明这个状态是「意图」不是「事实」（本轮 L1） |
| N-2 | `embedded_ssh.rs:2089`（`connect_embedded_ssh(&host, 60)`）、`:2072` 的 `thread::spawn`、`:2073-2076` 无条件 `send(Closed)` | 成立 | `spawn_terminal` 加 `stop: Arc<AtomicBool>`，`run_terminal` 的循环在 `WouldBlock` 分支检查；daemon 在 `:5017` 之后置位。顺带的观测缺口：线程失败与正常关闭都发 `Closed`，`Error` 是唯一区分且不带 `terminal_id` |
| N-3 | `bin/agent2ssh-daemon.rs:5079` 线程、`:5111` 截止、`:5122` 检查、`:5291` 默认 60s | 成立 | 同一个 `stop` 标志；`:5316` 的 future 收口后立刻置位。安静远端（`sleep 600`）时线程会空转到自己的 `timeout_secs` 才退——daemon 已退出、PID 文件已删，但进程还持着 SSH 会话 |
| N-4 | `forward.rs`：10 个异步公开函数（`:344`…`:855`）与 `grep -c "tokio::test"` = **0** | 成立 | 最小一条：直接插一个 `RuleState::Starting` 且 `worker: None` 的 `ForwardHandle` 进 `forwards()`，断言 `forward_remove_core` 后列表变短、lifecycle 变 `Closed`。`AppState` 是进程级共享，需要 `#[serial]` + `TestConfigDir` |
| N-5 | `daemon_activity.rs:127` 的 `read_to_end` | 部分成立（**与 L2-12 同一处**） | 「缺 `Content-Length` 时读到上限为止」这半句已写在 L2-12 的改法里，不重复登记；它新提供的价值是「即使地址是本机，`/health` 与 `/gate` 都带 `Content-Length`，所以修法可以比 L2-12 更轻、不必动地址可达性」。并入 L2-12 的说明 |
| N-6 | `ci.yml:214-217`、`:323-325`；`daemon_stop_e2e.rs:317` | **不成立（建议项已存在）** | 事实对：那两处 CI 调用都不设 `AGENT2SSH_TEST_SSH_PORT`，`stop_exits_when_exec_websocket_peer_stops_reading` 在两处都会早退并记 `ok`。但它建议「在报告 `:49` 旁注明」——`CNB-ISSUE-2-WINDOWS-RETEST-2026-11-09.md:59` 已写「23 项不含该用例（`cfg(unix)`），0 项不算通过」，`:122` 又写「CI 没有该场景的 Windows 回归覆盖」。要补的东西已经在了 |

---

## 2026-11-10 · 第 6 轮（末轮）：授权了但没做，说重启了但其实没有

第 6 轮 5 条：**成立 2 / 部分成立 2 / 不成立 1**，另有两处对我前几轮结论的更正（一成立一不成立）。这一批的共同点是**检查与执行之间隔着一次 `await`**。

| 编号 | 位置 | 判断 | 建议改法 |
|---|---|---|---|
| P-1 | `forward.rs:775-786`（`forward_stop_core`）、`:789-805`（`forward_start_core`）；`tauri_commands.rs:1428`、`:1475`；`bin/agent2ssh-daemon.rs:3116-3140`、`:3168-3190` | 成立 | 两处都是「先 `forward_list_core()` 查、再授权（会 `await`，审批可到 300s）、再调 core，而 core 重新拿锁重判状态」。并发两条请求时：stop 的后到者作用在 worker 已 `take()` 的句柄上空转；start 的后到者 `store.insert` 覆盖先到者的句柄，被覆盖的那个 `Drop` 会 `stop.store(true)`。桌面双击即可触发。修法：把状态判定移到授权之前的同一次加锁里，或在 core 里用「版本号/CAS」拒绝第二次 |
| P-2 | `bin/agent2ssh.rs:3062`、`:3089-3090`、`:3098-3102` | 部分成立 | `remove_daemon_pid_file_if(pid)?` 的 `bool` 被 `?` 丢掉（`?` 只传播 `Err`），stop/restart 两条路都不看 `false`——这点属实。但它推的后果（活着的 daemon + 过期的 PID 文件 + 已打印「restarted」）被 `start_daemon_background` 自己的 `:391-424` 挡住了：那条路会先看 PID 状态，`RunningUnhealthy`/`Unverified` 直接报错返回。所以真实影响只是「清理结果不可观测」，与 L2-9 同族，不新立 |
| P-3 | `bin/agent2ssh-daemon.rs:3127-3128`、`:3175-3176`；`execution_control.rs:446`、`:454` | 部分成立 | 「`/forward/:id` 的 scope 拒绝不写审计」属实。但它的对照错：`authorize_command_with_approval` 的 `:446` 是 `check_daemon_scope(...).map_err(...)?`，`?` 提前返回，`:454` 的 `append_decided_exec_audit` 在 scope 拒绝时同样跑不到——两条路一样沉默，不存在「另一条会审计」的不对称。要补的是两边一起补 |
| P-4 | `daemon_control.rs:478-505` | **不成立** | 超时路径**确实会 kill**：`:481` `try_wait()` 为 `None` → `:488` `child.kill()` → `:502` `child.wait()`。只有 `kill()` 返回 `Err` 时才可能留下活着的 child，那正是 L2-9 已登记的那条。它说的「Child 的 Drop 不 kill 所以成孤儿」在 `kill` 成功路径上不会发生 |
| P-5 | `ws_drain.rs:154-184`（实现）、`:326-398`（4 条测试） | 部分成立 | 三点：①`drain_all` 全仓无生产调用点（只有 `ws_drain.rs` 自己与测试）→ 死代码，成立；②`count` 是 `entries.len()`，不看谁真的完成——但 doc 注释已写明「including timed-out ones」，不算缺陷；③`drain_timeout_is_shared_across_connections`（`:390-398`）只断言返回值 `== 3` 与 `elapsed < 500ms`，超时路径同样满足，**测不出它想证明的事**，成立。它说「第一个 entry 超时后其余根本没被等」——技术上对，但 deadline 已过，等也立刻返回，无可观测后果。建议：删掉 `drain_all` 与两条对应测试，或改成「收集仍在 pending 的 id」并断言 |

**两处对我前几轮的更正**：

- **`get_daemons_unified_view` 自指向（成立，并入 L2-10）**：`/daemons/view` 的 handler（`bin/agent2ssh-daemon.rs:4007-4016`）在 daemon 内部调 `remote.rs:460-553`，那里对每个 connected daemon 串行发 3 个 5s 请求（`:479`/`:497`/`:517`/`:537`），无总预算。与 `fetch_daemon_activity` 同族但不同函数——L2-10 的修法要覆盖两处。
- **「`0e88d7c` 把 unregister 挪出 future」这条它判互斥（**不成立**）**：`git show 0e88d7c` 的 diff 里有 `-        drain_handle.unregister().await;`（旧位置，正是新 future 块内的位置）与 `+        drain_handle.unregister().await;`（清理之后、块外）两次改动，同笔还插入了 `+        }, Duration::from_secs(2)).await;`。所以「引入包装器的同一次提交把它放在包装器外面」是准确描述，两个结论不互斥。
- **`resident_sessions` 的 `dead` 口径（部分成立，记录不改）**：`resident_session.rs:658` 在 `wait_for_sentinel` 轮询里刷 `last_active`，所以跑着长命令的会话不会被判空闲（这是对的）；但 `resident_get_core` 的 `dead` 判定（`:978-982`）在 `try_wait` 之后不看 `running`，「shell 端口仍在、命令在跑、parent 已 exit」这一瞬与 `DaemonActivity` 的口径不覆盖。已知可接受，但不要写成「`dead` 就代表没有开销」。

---

## 2026-11-10 · batch 2（issue #8）第 1 轮：russh 传输、known_hosts 与认证

首轮 18 条指摘：**成立 8 / 部分成立 5 / 不成立 5**。落到 L1 的只有注释与文档（`ssh_algo.rs` 模块文档整体重写、`SFTP_SAFE_WRITE` 与 K2 对齐、五处契约注释）；行为级的全在下面。

**行号是本轮核实时的编号**（改动前，基线 `a8f9519`）。本轮的注释改动会让同一文件里的行号下移 5–10 行，按函数名定位。

> **2026-11-10 第 2 轮复核后追加（基线 `8e16f40`）**
>
> - A2 的残留覆盖缺口**仍然开着**，但形状变了：`host_key_algorithm` 的注释现在有据可依（`host_key_algorithm` 由密钥本身推导，`Algorithm::Rsa{..}` 对 `rsa-sha2-256/512`/`ssh-rsa` 一律匹配），仍未验证的是**端到端**——需要一把 RSA 主机键跑完 `trust_host_fingerprint_core` + `get_host_fingerprint_status_core`。现成的可以借力点：`scripts/e2e-ssh-matrix.sh` 的 legacy 段已经把 OpenSSH 8.3 限制成 `HostKey /config/ssh_host_keys/ssh_host_rsa_key` + `HostKeyAlgorithms ssh-rsa`（脚本 `:527-528`），那一台就是 RSA 主机键服务端；矩阵目前只对它跑 exec/拒绝用例，没有 trust→换 hostkey→status 这套断言。
> - C6 的「三件事」已经拆开处理：两侧归一化（仍是 L2，见下）、hashed 条目不可匹配（CLI 文案已改，`remove_system_known_host` 的行为未变）、测试（**已进 main**：`embedded_ssh.rs` 的 `remove_system_known_host_matching` 模块）。其中 `an_ipv6_literal_at_port_22_is_removed` 现在就是红的，它是要修的 `Ok(0)` 的失败断言。
> - D5 的 `u32::try_from` 不可达分支仍未处理（删掉或注明）；`From<SendError>` 臂同样仍无测试。
> - 新增两条本轮复核发现、需要拍板的行为项，见文末「第 2 轮追加」。

| 编号 | 位置 | 判断 | 建议改法 |
|---|---|---|---|
| A2 | `embedded_ssh.rs:152` `host_key_algorithm`；`known_host_key` 的槽位组成 | 部分成立（核心结论错：RSA 归一槽是**对的**，见本轮 L1 注释） | 残留的是覆盖缺口：没有任何测试用 RSA 主机键跑 `trust_host_fingerprint_core` / `get_host_fingerprint_status_core`，唯一覆盖者用 ed25519。补一条：RSA 主机键信任一次后，切 `--hostkey` 在 `rsa-sha2-512`/`rsa-sha2-256`/`ssh-rsa` 之间移动，`trusted` 必须仍为 `true`、`known_hosts.json` 里仍只有一条 `host|ssh-rsa`。需要能配 RSA 主机键的本地 sshd（`scripts/sshd-fixture` 可注入 host key）；若只做 in-memory，退化为 `host_key_algorithm` + `known_host_key` 的单元断言，强度不足以钉住端到端 |
| B1 | `agent.rs:290-311`（pipe 优先）、`:238-247`（`list_identities` 失败置 `None`）；`embedded_ssh.rs:1909`（单次 `connect()`） | 成立 | 两件事一起改：①`connect_stream` 返回候选链（pipe → Pageant），让「管道开着但没有可用身份」能落到第二个 agent；②`list_identities` 失败后允许重连（重建 client），不要把 `None` 变成该连接生命周期内的永久失败。①改的是**认证语义**且在 `#[cfg(windows)]` 里，只能在 Windows 实机验证（装 Pageant、不装 OpenSSH agent 服务），故不进 main |
| B2 | `embedded_ssh.rs:2155-2156`、`:2187`；对照 `:1345-1373` | 部分成立（结构成立；「永久挂死 / 最坏 49.7 天」不成立——终端会话的 `session_timeout_ms(60)` 把上界压在 60 s） | 照 exec 路径做：teardown 前 `set_blocking(true)` + `set_timeout(TEARDOWN_TIMEOUT_MS)`（30 s），再把 `wait_close` 的三种结果分别处理。与 C1 合成一处改，否则改了也不观测 |
| B3 | `embedded_ssh.rs:2064-2080`（`spawn_terminal`）、`run_terminal` 的 `try_recv` 循环 | 成立 | 加 `stop: Arc<AtomicBool>` 并返回 `JoinHandle`；同时把 `try_recv` 的 `Disconnected` 当作 `Close` 处理——现在 drop 掉 `cmd_tx` 后 `Close` 分支永不执行，线程只能靠 `eof()` 或错误退出。与既有的 N-2（daemon 侧同一缺陷）合并做，共用一套标志 |
| B4 | `embedded_ssh.rs:1062-1119`（bridge 主循环）、`:1394-1456`（`drain_channel_output`） | 成立 | bridge 没有总预算也没有「无进展」计数，`Ok(0)` 且未 `eof()` 时 5 ms 一轮空转到进程结束。**判据不能选「一段时间没有字节」**——空闲隧道是合法的；只能选「连续 N 轮 `WouldBlock` 且 `eof()` 仍为假」并配一个足够大的 N（或对端 keepalive 失联）。需要能注入假通道才测得出，建议把 `bridge_tcp_and_channel` 的 `Read` 侧参数化 |
| C1 | `embedded_ssh.rs:1119-1120`、`:2155-2156`、`:2187`；对照 `:1348-1373` | 成立 | 三处 `let _ =` 把「通道没关成功」与「关成功」压成同一个成功返回，而同文件的 exec 路径对 `Ok(true)`/`Ok(false)`/`Err` 分别处理并 `session.abort()`。改成同一套：`Ok(false)` 与 `Err` 都要 `abort()` 并让调用方看到（终端路径的对外语义需先定：关闭失败要不要报给用户） |
| C2 | `mod.rs` `Session::auth_methods`（改动前 `:898-900`）；下游 `embedded_ssh.rs:1719`、`:1785`、`:1797-1803` | 成立 | 二选一，都要拍板：①实现它（只能从失败 `userauth` 的 `remaining_methods` 取，即事后信息）；②删掉下游两处分支与诊断字段，别再每台主机写 `auth_methods: ""`。选②会改变诊断日志的可观测字段，属对外契约 |
| C5 | `sftp_transfer.rs:322-345`（`StallGuard`）、`russh_impl/sftp.rs` 的 `File::write` | 部分成立（行为描述成立，但注释已自述「cannot interrupt a blocked syscall」，不是未披露缺陷） | `File::write` 是 `block_on` 一个无超时的 future，服务器收下连接却不应答 SFTP 包时永不返回，`STALL_TIMEOUT` 无从判定。真正的修法是给 SFTP 会话加写超时（`SftpSession` 的 stream 上套 `tokio::time::timeout`），不是调 `STALL_TIMEOUT`。需要「接受连接但静默」的假服务器才验得了 |
| C6 | `embedded_ssh.rs:563-608`（`remove_system_known_host`）；对照 `:619-633`（`split_host_port`） | 成立 | 两侧地址形态不一致：`port == 22` 只拼裸名、否则只拼 `[host]:port`，而 OpenSSH 对 IPv6 字面量写 `[::1]:22`、对本机名可写 `localhost,127.0.0.1`，且 hashed 条目（`\|1\|salt\|hash`）永远匹配不上。改法：用 `split_host_port` 归一化两侧再比，hashed 条目单独计数并报告「N 条无法匹配」，`removed == 0` 时返回明确的「未找到」而不是静默 0。单测可做（临时 HOME + 手写四种形态的 known_hosts） |
| D1 | `sftp_transfer.rs:484-512` | 成立 | `Write for StallGuard` 的状态机（`written > 0` 刷新 `last_progress`、`written == 0` 且超时返回 `TimedOut`、未超时继续）零测试；现有两条一条只比字节、一条直接调私有 `stalled()`。需要可注入时钟（`Instant` 不能伪造），建议加 `#[cfg(test)] with_last_progress` 或把「现在」抽成字段 |
| D5 | `mod.rs` `SignerFailed` 的 `From<SendError>`（改动前 `:131-136`）与 `u32::try_from` 分支（`:170`） | 成立 | `From` 臂可直测；`u32::try_from` 臂实际不可达（签名 > 4 GiB），建议改成注释写明不可达并去掉错误分支，而不是留着一条永远测不到的路 |

### 2026-11-10 · batch 2 第 2 轮复核：两条新发现（都需拍板）

复核人（CodeBuddy）按 `8e16f40` 的代码给出，两条都在 `src-tauri/src/**`，一条是注释与代码不符（属 L1 但影响的是诊断可信度），一条是行为（L2）。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-13 | `embedded_ssh.rs` `authenticate` 里 `is_publickey_refused` 分支的注释（`:1876-1881`，`8e16f40` 基线）；被调用的 `retry_with_legacy_rsa_signature`（`:2007-2022`）；守卫来源 `transport/russh_impl/mod.rs` `method_pref`（`:1107-1116`） | 成立 | 这一条本身是注释改动（L1），但它现在有了失败断言（`the_legacy_rsa_retry_is_dead_because_the_preference_cannot_be_set`），也就是说**代码事实被钉住了、注释还在说反话**。用户可见的后果不是注释而是行为：对只接受 `ssh-rsa` 的老服务器，真正生效的回退是 `russh_impl` 内部那条（`authenticate_publickey` → `PrivateKeyWithHashAlg::new(key, None)`，`:808-827`），而 `note_legacy_sha1_signature(host, "legacy retry")` 永远写不出来——`detected_by` 只剩 `"transport fallback"` 一个取值，运维按文档去分辨「哪条路回退的」会得出错误结论 | 二选一：①删掉 `retry_with_legacy_rsa_signature` 与整段 `is_publickey_refused` 分支，把注释改成「回退由传输层内部完成，见 `russh_impl`」；②保留分支但注释与 `detected_by` 取值改成「无效分支，永不命中」。两种都要改 `docs/plans/russh-migration.md:564` 第 6 条（它现在写的是「现在靠 `method_pref(SignAlgo, \"ssh-rsa\")`」，与实现相反） |

**最小验证口径**：`cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`（现有断言即覆盖两条分支的取舍，不需要服务端）；真机口径可复用 `scripts/e2e-ssh-matrix.sh` legacy 段那台 `PubkeyAcceptedKeyTypes ssh-rsa` 的主机，但那是 8.3 + `KexAlgorithms diffie-hellman-group14-sha1`，本仓根本不握手，不能用；要真机验就得另起一台只限 `PubkeyAcceptedAlgorithms ssh-rsa` 而 KEX 现代的 sshd。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-14 | `embedded_ssh.rs:1520`（`apply_algo_prefs(...)?`）；`transport/types.rs:96-98`（`MethodType::LangCs/LangSc` 的文档）；`ssh_algo.rs:164-166`、`:155-176`（`supported_algorithms` 的 `_ => return None`）；`russh_impl/mod.rs:1168` | 成立（低危，但是对外契约） | `MethodType::LangCs`/`LangSc` 的文档写「kept so `supported_algs` can answer for them the way libssh2 does」，实际 `supported_algs` **从不**回答它们：`supported_names` 对三者返回空 `Vec`（不是 `Err`），而 `ssh_algo::supported_algorithms` 的 `_ => return None` 从不到达后端——它的 `match` 先按 `MethodType` 分派到 8 个 `OnceLock`，`LangCs/LangSc/SignAlgo` 直接 `None`。所以那句文档描述的是一个不可能发生的调用，读代码的人会以为 `LangCs` 有静态表 | 改 `types.rs:96-98` 的文档，说明这三个变体在 facade 层就被挡掉、后端返回空表只是形状上的兜底；或让 `supported_algorithms` 真的走一次 `supported_algs`（会更慢且仍然得到空表，不推荐） |

**最小验证口径**：`supported_algorithms(MethodType::LangCs)` 已经是 `None`（`narrow_degrades_to_no_preference_never_panics` 对 `SignAlgo` 断言了），补一句同形状的断言到同一个测试里即可，不需要服务端。（**2026-11-10 第 2 轮已补**：`LangCs`/`LangSc` 两条断言已进入该测试，`MethodType` 的文档同笔改写，L2-14 关闭。）

---

### 2026-11-10 · batch 2 第 2 轮追加（基线 `09d49ea`）

第 2 轮 11 条独立指摘：**成立 4 / 部分成立 3 / 不成立 3**，另 1 条信息性成立（N6）、1 条已由 `8e16f40` 修掉（C4）。本轮 L1 只动注释与文档，行为项如下。

**本轮已由 L1 关闭的**：`L2-14`（见上）；`L2-13` 的**注释那一半**——`retry_with_legacy_rsa_signature` 的文档与调用点注释已改成「此路不可达，且有两重独立原因」。`L2-13` 剩下的只有「删不删死分支」。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-15 | `embedded_ssh.rs` `remove_system_known_host` 的 `patterns` 构造（`port == 22` 只拼裸名）；`remove_system_known_host_matching::an_ipv6_literal_at_port_22_is_removed`（**当前红**） | 行为项，且它配套的那条测试**前提已被实测推翻** | 测试的前提是「OpenSSH 把 IPv6 字面量写成 `[::1]:22`，`ssh-keygen -R` 会删掉它」。两半都测过：本机对含 `[::1]:22` 与 `::1` 的文件跑 `ssh-keygen -R ::1`，**只删后者**，前者原样保留——实现与参照一致，不是静默 no-op；`man 8 sshd` 也写明方括号只用于**非标准端口**。所以「实现漏了一种 OpenSSH 会写的形态」不成立，缺的是一种**别的工具**才写得出的形态 | 两条路，先拍板再动。**①对齐参照（推荐）**：删掉或改写这条红测试，注释写清 `[host]:22` 是其他工具写出的形态、`ssh-keygen -R` 同样不删、代价是这类残留只能手工清。**②产品口径「forget 就是删干净」**：`port == 22` 时同时匹配裸名与 `[host]:22`，并把 doc 注释「Equivalent to `ssh-keygen -R`」改成「superset of it」——`forgetting_one_port_leaves_the_other_port_alone` 不受影响（它用 2222，验证过）。**无论选哪条，这条红测试都不该留在 main**：它让 CI 常红，此后每个「全绿」声明都得附带解释，也是本流程唯一必须每次都带的噪声 |
| L2-16 | `russh_impl::ClientHandler::check_server_key` 的 `if let Ok(blob) = key.to_bytes()`（`:415` 起，函数末尾无条件 `Ok(true)`）；上层 `embedded_ssh.rs:331`、`:1665` 的 `None => Err("SSH server did not provide a host key")` | 成立（结构），但触发条件近乎不可达 | 序列化失败时不记 `host_key`、仍返回 `Ok(true)`，`host_key` 留在 `None`，于是上层把「无法序列化这把键」报成「服务器没给主机键」。代价是诊断指向错误方向；但 `PublicKey::to_bytes()` 对已解析的公钥几乎不可能失败，所以这是**错误分类**问题，不是会踩到的 bug | 二选一：①`to_bytes()` 失败时记一条 `warn`（带算法名）再返回 `Ok(true)`，至少留下痕迹；②给 `host_key` 加 `Unserializable` 态并让上层单独成句。两条都改对外文案，需拍板。验证不需要服务端：直接构造 `ClientHandler` 调 `check_server_key` |
| L2-17 | `russh_impl/agent.rs` `list_identities` 的 `if fetched.is_err() { *self.client.try_lock()… = None }`（`:240-246`）；现有测试 `an_unconnected_agent_refuses_instead_of_pretending`（`:748`）只覆盖「从未连过」 | 部分成立（结构成立；`docs/KNOWN-ISSUES.md` §3.1 的共存边界只写了 Windows 的 pipe/Pageant 顺序，但这段代码是跨平台的） | 「连过、`request_identities` 失败一次」之后，本连接生命周期内 `list_identities` 永久 `Err`，且错误串退化成 `the SSH agent is not connected`——与「从未连过」不可区分（`ssh-agent` 中途重启就会踩到）。修它要加状态，改的是对外错误文案；补的测试会**先红**，不能直接进 main | `client` 从 `Option` 换成三态（`NeverConnected` / `Connected` / `Failed(原因)`），或在置 `None` 前存下错误、下次返回时带上；测试：连上 → 让 `request_identities` 失败一次 → 再调，断言第二次文案含原错误。`KNOWN-ISSUES` §3.1 同时补 unix 侧 |

**N2 不新立**：它推的那件事（`remove_system_known_host` 的返回值形状拿不到「有 N 条无法匹配」）就是 `C6` 的第二件事，CLI 文案已改、`usize` 返回值未变，仍在 `C6`。

---

### 2026-11-10 · batch 2 第 3 轮追加（基线 `2c4221a`）

第 3 轮 9 条新编号（19–27）核实后：**成立 3 / 部分成立 3 / 不成立 3**（26 是范围外线索、27 是信息性）。本轮 L1 只动 `skills/`、`docs/` 与两处测试注释，行为项如下。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-18 | `bin/agent2ssh.rs:1928-1931`（`known-hosts forget` 的未命中提示） | 部分成立 | 句子本身是条件式 caveat（"entries stored in the hashed form cannot be matched"），不是「你的条目被哈希了」的因果断言；但实测 `HOME=<tmp> agent2ssh known-hosts forget ::1 --port 2222` 在只含明文 `[::1]:22` 的文件上就打印这句——用户最可能的两个真实原因（端口不符、方括号形态）都没被点名，唯一被点名的恰恰是他几乎踩不到的那一个 | 先只改成 `no known_hosts entry matched {host} (port {port})`，把 hashed 半句留给「文件里确实有 `\|1\|` 行」时。那要求函数把「没找到」与「有但读不到」分开返回（与 C6 同一件事），不是改字面量能做完的 |
| L2-19 | `bin/agent2ssh.rs:1903-1906`；计数来源 `embedded_ssh.rs:500-518` | 成立 | `skipped` 把五种情况压成一个数：hashed 行、字段不足三列、base64 解不开、指纹冲突、其余怪行。实测 `printf 'bad.example ssh-ed25519 !!notbase64!!\n'` → `imported 0 host(s), skipped 1 (hashed or conflicting)`，括号里两个原因都不是它 | `KnownHostImportSummary` 按原因分字段（hashed / malformed / undecodable / conflicting），CLI 分行打印。`--json` 的形状会变，属对外契约，需要拍板 |
| L2-20 | `core.rs:3077-3086`（`import_ssh_config_core` 尾段） | 成立（信息性） | `existing` 是导入前的一次快照，`config.hosts.push(p)` 不回写它，所以**同一次导入**里两个同名 profile 都会落盘（`~/.ssh/config` 里 `Include` 两个文件各有 `Host j1` 即可复现）。`resolve_host`（`session.rs:57-62`）`.find()` 取第一条、`host list` 显示两条、`host rm j1` 一条命令删光——「哪条生效」因此被掩盖。两端都不崩，故记为信息性 | `existing.insert(p.name.clone())` 一行即可让「同名跳过」对同批生效；要保留两条则照 `netcatty_import.rs:181-185` 加 `-2` 后缀（那要改 `flush` 的返回形状） |

**对 L2-15 的补充**：第 3 轮复核给出的证据独立地指向同一结论——「`[::1]:22` 不是 OpenSSH 写出的形态」（对方自建 sshd 实测：`:22` 写裸 `::1`，`:2222` 写 `[::1]:2222`），所以选项 ① 的可信度上升。同时选项 ② 的括注需要收紧：**「两种形态都匹配」只能加在 `port == 22` 分支里**，放到所有端口会打碎 `forgetting_one_port_leaves_the_other_port_alone`（它断言 `:22` 那行逐字节保留）；同效的另一种写法是拿 `split_host_port` 解析后比 `(host, port)`。两条已写进该模块的测试注释。

**N6 是给 `C2` 用的新证据，不是缺陷**：`russh` 的 `authenticate_none` 可用（`channel.rs:809` 的测试已经在调它），所以「实现 `auth_methods`」不必等一次真正的认证失败——可以在 `authenticate` 开头先发一次 `none` 探测。代价是每次连接多一个往返，且老服务器可能把 `none` 记成一次失败尝试（`MaxAuthTries`）。这条只写在这里，供 `C2` 拍板时对照。

---

### 2026-11-10 · batch 2 第 4 轮追加（基线 `bf26f8c`）

第 4 轮（CodeBuddy 第 3 轮复核）11 条：**成立 4 / 部分成立 4 / 不成立 2**，1 条只作观察。L1 只动了 `--help` 与指南里两处「按构建判断」的残留；行为项如下。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-21 | `embedded_ssh.rs` 的 `remove_known_host_core`（`:424-446`）与 `known_host_identity`（`:196-198`） | 部分成立，且**比指摘说的更彻底** | 形态不一致属实：identity 是 `host:port`（IPv6 即 `::1:22`），而 UI 拿到的 `TrustedHostFingerprint.address` 是 `format_host_port` 的 `[::1]:22`，两者互不匹配。但**这个函数没有任何生产调用点**——`tauri_commands` 只暴露 `forget_system_known_host`（写系统 `~/.ssh/known_hosts`），CLI 的 `known-hosts` 只有 `import`/`forget`，前端的 `api.ts` 也没有对应命令；全仓引用只剩它自己和两条测试（`:2604`、`:2748`/`:2756`）。所以「用户在 Hosts 面板点 Forget 删不掉」这条路径不存在——它是一段死代码 | 二选一：①删函数 + 两条测试（并删 `known_host_identity` 在删除路径上的用法）；②保留但让它先 `split_host_port` 归一化再比，同时补一个生产调用点（UI 的「Forget in app store」）。选①还是②是产品决定：现在 UI 上根本没有「忘记应用内信任」的入口 |
| L2-22 | `embedded_ssh.rs` 的 `import_known_hosts_from_ssh`（`:500-522`） | 部分成立（前半成立，后半不成立，见 ledger 23） | 只做 `STANDARD.decode(key_b64)`，不校验解出来的是不是一个 SSH 公钥：截断的合法 base64（20 字节）会被导入并算出一个与真实密钥不同的 `SHA256:` 指纹，而 `ssh-keygen -l` 对同一行报 `is not a public key file`。后果是下一次连接必然报「指纹已变」，用户看到的是自己从没连过的主机 | 解码后加一层结构校验：`russh::keys::PublicKey::from_bytes(&blob)` 能过才收，或至少校验首字段长度前缀与算法名。属输入校验的行为改动，且会改变 `imported`/`skipped` 的计数分布（与 L2-19 同批做） |
| L2-23 | `transport/russh_impl/mod.rs` 的 `check_server_key`（`:420-430`）；`embedded_ssh.rs` 的 `host_key_algorithm`（`:165-175`） | 部分成立（「不可达」不成立） | `HostKeyKind::Unknown` **可达**：`ssh_key::Algorithm` 有 `SkEd25519` 与 `SkEcdsaSha2NistP256` 两个变体（`ssh-key 0.7.0-rc.10/11` 的 `algorithm.rs:119-122`），它们落进 `_ => Unknown`；只是现实里没有服务端用 FIDO 主机键。真问题是 `host_key_algorithm(Unknown)` 返回字面量 `"unknown"`，一旦走到就以 `host:port\|unknown` 落盘并被 `Inspect`/终端横幅原样显示 | 把 `"unknown"` 改成能自解释的值（`"unknown-<algorithm>"`）或让 `Unknown` 走 `to_bytes` 失败那条路（不落盘 + warn），二选一需要拍板；最小改动是给 `_` 臂补一条 `warn` 并保留现有字符串 |
| L2-24 | `embedded_ssh.rs` 的 `trust_or_verify_host_fingerprint` 调用点（`authenticate` 之前） | 成立（设计确认项） | 指纹在**认证之前**取得并落盘，所以「密码写错」这种最常见的失败也会在 `known_hosts.json` 里留下一台从没成功连过的主机的记录。这符合 TOFU 的定义，但与 `2fa_required`/认证结果无关这件事没有任何文字说明 | 要么在 `docs/KNOWN-ISSUES.md` 记一句「信任记录先于认证，失败也会留痕」，要么改成认证成功后才落盘（会改变 TOFU 的语义，需拍板）。**不改代码**的选项更好：这是 TOFU 的正常代价 |
| L2-25 | `ssh_algo.rs`/`embedded_ssh.rs` 的「指纹已变」错误文案 | 成立（低危） | 主机证书场景下，换一把新钥匙 + 陌生 CA 签的证书会被报成 `SSH host fingerprint changed`，而事实是「这张证书的 CA 不认识」。CA 与有效期不检查这件事已在 `check_server_key` 的注释里写明（第 1 轮），缺的只是**错误分类** | 文案加一句「(a certificate from an unexpected CA also lands here)」，或在 `check_server_key` 记下「presented a certificate」，让上层能分叉。改对外文案，需拍板 |

**并入 L2-15 的一条**：`remove_system_known_host` 的重写循环会把 **CRLF 折成 LF**（实测：两行 CRLF 删第一行后本仓 55 字节、`ssh-keygen -R` 56 字节，后者保留 `\r\n`）。这与「Equivalent to `ssh-keygen -R`」矛盾，而 L2-15 无论选①还是②都要动同一个重写循环，**同批做**。它表格里的另一格（删中间行会留一个空行）**不成立**——实测两边字节完全相同。

### 2026-11-10 · batch 2 第 5 轮追加（基线 `60f8262`）

第 5 轮（CodeBuddy 第 4 轮复核）5 条：**成立 2 / 部分成立 2 / 观察 1**，无「不成立」。两条行为项都落在 `src-tauri/src/**`，本轮只加了 landmark 注释（L1），以下是需要拍板的部分。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-26 | `bin/agent2ssh-daemon.rs` 的 `risk_check`（`:3492-3494` 的 `map(\|_\| "user_rule")`）；`risk_config.rs:74` 的 `classify_with_user_rules` | 成立 | 字段名叫 `matched_rule`，取值域却只有 `{null, "user_rule"}`：`classify_with_user_rules` 返回 `Option<RiskLevel>`，命中的 pattern 在函数内部就被丢掉。三处消费者都读得到这个常量——`docs/api.yaml`（已在本轮写成显式取值域）、`/console`（`console.html:1052` 渲染成 `Matched: user_rule`）、`daemon_integration.rs:752`（只断言 `is_null()`）。**本轮另查出两件指摘没提的事**：①`/risk/check` 每个请求调 `classify_with_user_rules` **两次**（`:3492` 直接调、`:3495` 经 `effective_command_risk` → `classify_effective_risk` 再调一次），两次各自 `load_risk_rules()`；规则文件在两次读之间变更或读失败时，`matched_rule` 与 `risk_level` 可以互相矛盾。②CLI 与 MCP 侧同族（`bin/agent2ssh.rs:1454`、`bin/agent2ssh-mcp.rs:1352`）都只回一个 `bool`，没有任何入口回答「命中哪条」 | 让分类函数返回命中信息而不是只返回等级（例如 `Option<(RiskLevel, /* pattern */ &str)>`，或另加 `classify_with_rule_name`），`risk_check` 一次取值同时算 `risk_level` 与 `matched_rule`，顺带消掉双读。属签名改动 + 跨 bin 调用点（daemon / CLI / MCP 共 4 处），且**是对外契约的扩张**（新取值会出现），要拍板 |
| L2-27 | `bin/agent2ssh-daemon.rs` 的 `local_exec`（`:1807` `remote_outcome_unknown: None`；`:1818` 执行错误恒 400，preflight 的 403/423/429 走更早的 `?`，不经过这里）vs `exec`（`:1711` 计算、`:1714-1719` 中断时 503） | 部分成立（「同构」不成立，见 ledger 26） | 不对称属实：`/local-exec` 的 webhook 永远说「远端结果已知」，HTTP 状态永远 400。但指摘说的「同一个 shutdown token、同一种处境」不准确——`core.rs` 的 `exec_local_core_impl` 已经把 `LocalExecShutdown` **降级成一句字符串错误**（"local command stopped during daemon shutdown"）并在那里记了 `append_incomplete_exec_audit`，理由写在同一处注释：local runner 自己持有 deadline 并收割子进程**及其进程组**，所以「不像 SSH 路径那样后面还挂着一个在跑的命令」。**因此这条不能在 handler 里改一行了事**：即使把 `None` 换成 `ssh_shutdown_outcome(...)`，`ExecShutdown` 类型在那之前就已经被抹掉，结果恒为 `None` | 要让 `remote_outcome_unknown` 表达 local 的中断，得先把类型从 `core.rs` 透出来：`LocalExecShutdown` 不走字符串降级，或让 `exec_local_core_impl` 直接返回带 outcome 的错误；随后 `local_exec` 与 `exec` 共用同一段「算 outcome → 写 webhook → 选状态码」。**语义上还要先定一件事**：local 中断时唯一未知的是「已经产生的那部分副作用」，不是在跑的命令，`true` 的文案（"命令可能已发送、远端结果未知"）需要为 local 补一句，否则会把「进程已收割」说成「可能还在跑」。状态码是否也统一成 503 是同一决策的一部分 |
| L2-23（收窄） | 同上一节 | 承接第 5 轮 N-5 | 原写法给了两个选项，其中「让 `Unknown` 走 `to_bytes` 失败那条路」**会把一句可解释的话换成一句假话**：`check_server_key` 只在 `to_bytes()` 成功时才捕获（`mod.rs:419`），失败的路径下 `host_key` 保持 `None`，`embedded_ssh.rs:1676` 报的是 `SSH server did not provide a host key`——而服务端**确实提供了**，只是这套解析不认识。可区分的错误需要新的分支，不是复用现有那条 | 收窄成一个选项：`"unknown"` → `"unknown-<algorithm>"`。它要求 `HostKeyKind` 能带出 `ssh_key::Algorithm` 的名字（现在是纯枚举），属签名改动；若不想动枚举，退路是给 `_` 臂补 `warn!` 把 `{:?}` 的算法名打进日志，字符串仍保持 `"unknown"` |

### 2026-11-10 · batch 2 第 6 轮追加（基线 `e984c35`，末轮）

第 6 轮（CodeBuddy 第 5 轮复核）7 条新编号（N6…N12）+ 1 条不编号的留痕：**成立 3 / 部分成立 1 / 不成立 2 / 信息性 1**。三条成立项都在 `src-tauri/src/**`，本轮只落了 `api.yaml` 的契约缺页（L1）与下面三条 L2。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-28 | `policy.rs:130-142` 的 `save_policy_to_path`、`approval.rs:995` 的 `save_approval_policies` | 成立 | 两处都是 `std::fs::write(path, raw)`，既不设 owner-only，也不走临时文件 + rename；而同目录下的 `hosts.json`（`store.rs:275-278`）与四个规则文件（`write_private_text`，`store.rs:297-318`）都走 `restrict_file_to_owner`（`0o600`）+ `.tmp-<pid>-<uuid>` → rename。后果分两层：**权限**——`policy.toml` / `approval_policies.toml` 落到 umask 决定的模式，而它们描述的是「哪些命令危险、谁能批准」；**原子性**——写到一半崩溃会留下截断文件，下一次 `load_*` 要么解析失败要么读到空策略，而空策略的语义是「什么都不需要审批」。两者都属于「谁能批准什么」这条链，不是纯风格问题 | 把两处换成 `store::write_private_text`（它已经承担建目录 + `0o600` + 临时文件 + rename）。要一并确认两件事：①`write_private_text` 不做锁也不做备份（注释写明这是有意为之，`hosts.json` 才需要），策略文件的写入频率是否也不需要——从调用点看是用户动作，可以不加锁；②`policy.rs` 的 `POLICY_CACHE.invalidate()` 目前写在保存成功之后，换写入函数后要保持这个顺序。属行为改动（文件模式与崩溃语义），需拍板 |
| L2-29 | `tests/daemon_integration.rs:737-753` 的 `http_risk_check_blocked_command` | 成立 | 该测试在 `build_test_router()` 上跑，而 `build_test_router`（`:408`）**不隔离配置目录**，`:752` 断言 `matched_rule.is_null()`——在没有 `risk_rules.toml` 的目录里 `classify_with_user_rules` 恒 `None`，所以这条断言对取值域零覆盖：把 `.map(\|_\| "user_rule")` 改成 `.map(\|_\| String::new())` 它照样绿。**这一条与 L2-26 是一对**：L2-26 改取值域时，会同时改坏「本应覆盖却覆盖不到」的那个测试，而它不会红 | 在测试自己的配置目录里写一份 `[blocked] patterns = ["echo *"]`，再请求 `/risk/check` 并断言 `matched_rule == "user_rule"`。**前提是先给这套 harness 加配置目录隔离**（`store` 的 `CONFIG_DIR_ENV` 是现成手段）——目前 `build_test_router` 的测试共享真实 `~/.agent2ssh/`，盲写会把开发者的 `risk_rules.toml` 覆盖掉。隔离本身会碰到所有走 `load_*` 的测试，属结构性改动，不能顺手做 |
| L2-30 | `daemon.rs` 的 `run_playbook`（`:3711`）与 `terminal_broadcast_run`（`:4402`） | 部分成立 | 行为上都过门：`run_playbook` 正文直接调 `reject_if_gate_paused`（`:3719`）；`terminal_broadcast_run` 正文没有，但它把授权交给 `terminal_broadcast_preview_results`（`:4296`），逐个 target 判。指摘说的「契约测试守不住」因此成立一半：现有两条 `*_route_runs_the_shared_preflight`（`:2210`、`:2283`）只看 handler 正文，照抄给这两个路由，`run_playbook` 会绿（它用了 `reject_if_gate_paused` 这个词）、`terminal_broadcast_run` 会红（它一个词都没有）。也就是说这两个 handler 用的是**两套词**，而这正是「把不变量钉进测试」要暴露的东西 | 二选一：①给 `terminal_broadcast_run` 正文补一次 `reject_if_gate_paused`（**可能重复判**，因为 preview 里已经判过，要先确认幂等）；②把断言从「正文含某个词」改成「正文调用了一个已知过门的 helper」，即对两个 handler 各断言 `terminal_broadcast_preview_results` / `reject_if_gate_paused` 出现。②是测试改写（L1 可落），但它把不变量从「过门」降级成「调了某个函数」，需要人同意这个口径 |

### 2026-11-10 · batch 2 复核之后追加（基线 `2476ec3`）

末轮之后我按惯例复核了一遍自己刚落的 L1，三条都在本轮回帖时记进 L2，因为它们都需要人拍板才能动。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-34 | `bin/agent2ssh-daemon.rs:5129-5153`（`spawn_embedded_exec_stream` 的退出条件）与 `embedded_ssh.rs:1594-1601`（`drain_channel` 的） | 成立 | 同一个原则在同一个 crate 里有两个实现，而且弱的那一个在 WS 路径上：`/exec/stream` 的循环只判 `channel.eof()`，`drain_channel` 判 `channel.eof() && !progressed` 并连续两次才返回。`eof()` 在 `close()`/reader 结束时**立刻**为真（`channel.rs:258-268` 的 `mark_closed`，`state.eof = true` 与 `closed` 同时置位），而队列的填充是另一个 task；`drain_channel` 的注释写的正是「the extra idle passes absorb a trailing packet」。所以一次 Close 紧跟着最后一个 stderr 包时，`/exec/stream` 可以判 eof 并落到 `channel.exit_status()`，把尾巴丢掉——**它正是 `a04e99d` 那句注释所主张的东西的未实现版**。这条测试不出来：`close_without_eof_drains_both_streams_before_reporting_eof`（`channel.rs:661`）离开循环的条件是 `closed` 而且它测的是 `read_to_end`，不是这两个 drain 循环 | 把 `/exec/stream` 的退出条件换成 `drain_channel` 那一条（`eof && !progressed`，计次），或更彻底：抽一个两个调用点共用的 drain helper，让「两端各自读到什么时候可以停」只有一处定义。**这是行为改动**（在 EOF 之后多睡两拍，最长 2 × 10 ms），而且需要一条能证伪的测试：用 `channel.rs` 那套 duplex harness，服务端 `data` + `extended_data` 紧接 `close`（不 `eof`），断言两个流都拿全 |
| L2-35 | `bin/agent2ssh-daemon.rs` 的 `session_write`（`:2460` 起）与 `api.yaml` 的 `SessionWriteRequest`（`:305`） | 成立 | `SessionWriteBody`（`:1085`）只有 `input` / `force` / `source` 三个字段且**不用** `deny_unknown_fields`，所以 `GET /exec` 一族接受的 `command` / `change_id` 在这里被静默丢弃：语义是「`input` 里最后一行完整命令」+ `split_completed_session_commands`，与 `/exec` 的 `command` 不是一回事。文档原先也没写（本轮已在 `SessionWriteRequest.input` 的描述里写明）。行为上不弱——scope/gate/limits/risk/approval 都对**拆出来的那一行**跑过；弱的是可发现性：调用方以为自己在指定 command，实际指定的是「往 shell 里敲的字」 | 二选一：①`SessionWriteBody` 加 `#[serde(deny_unknown_fields)]`，让误传 `command` 变成 400（对既有客户端是破坏性的，但错的是客户端）；②保持宽松，把口径写进 `api.yaml`（本轮已做）并在 `/audit` 里记下 `input` 的原文。需要拍板的是「宁可拒绝也不猜」还是「保持兼容」 |
| L2-36 | `bin/agent2ssh-daemon.rs:5059-5071`（`health()`）、`:5530-5545`（`get_health_snapshot`）、`health.rs:198-205`（`load_health_snapshot`） | 成立 | 本轮给 `/health-snapshot` 补 schema 时发现它一直是错的（`api.yaml` 写裸数组，实际是 `HealthSnapshot` 对象，键名 `load_avg` / `disk_usage` / `memory_usage` 而非 `load` / `disk` / `memory`）——响应表错了整整两个批次没人发现，原因之一是**没有任何测试读这个响应体的字段**：`http_health_snapshot_get_returns_200`（`:1688`）的断言是 `body.get("hosts").is_some() \|\| body.get("error").is_some()`，在一台没收集过快照的机器上走的是 `error` 那一支。同族还有一处：读不到快照时返回的是 `200 {error, hint}`，与 `200` 的快照结构不同形 | ①把 `api.yaml` 的响应表钉进测试：在隔离的配置目录里写一份 `health_snapshot.json`，断言 `GET` 回来的 `hosts[0]` 有 `load_avg`/`disk_usage`/`memory_usage` 三个键（依赖 L2-29 的 harness 隔离）；②`get_health_snapshot` 读不到时改 404 或 204，让「没有快照」不再伪装成一个 200 快照。①是加测试，②是行为改动 |

### 2026-11-10 · batch 4（issue #9）第 1 轮追加（基线 `db60643`）

本轮 5 条指摘（A1–A5）里 4 条落在文档面、已 L1 落地，**没有 `src-tauri/src/**` 行为改动**；另有 1 条我判不成立（A5，见 ledger）。两条需要拍板的在下面。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-37 | `container_runtime.rs:505-512`（`start_local_service`）与 `:539-546`（`select`） | 成立（口径待定，也可能只是文档问题） | `AGENT2SSH_CONTAINER_RUNTIME` 只在 `select` 的 `_` 臂（`:542`）被读，而 `select(None)` 在生产代码里**没有调用点**：daemon 的 `containers_start_service`（`:961`）与 MCP 工具都直接把 `runtime` 交给 `start_local_service`，后者 `explicit` 为 `None` 时走 `available().first()`（`:470-479`），从不读 env。于是同一个 pin 会出现两种结果：`agent2ssh containers runtime` 经 `select(None)`（`bin/agent2ssh.rs:3557`）显示 `wslc`，而 `containers start-service` 仍可能去启动 `apple`。`configuration-guide.md:1352` 把这条 env 的范围写成「容器发现」，所以文档本身没撒谎 | 二选一：①把 `start_local_service` 的 `None` 分支改成 `select(None)`——pin 之后 start-service 会跟着走，但同时会把 `select` 的严格性带进来：env 写了一个不存在的 id 时，从「静默按 `available()` 挑」变成 `Err`；②保持现状，在两处工具描述里写明「这条 env 只影响 `containers runtime` 与发现，不影响 `start-service`」。①更一致但改了失败模式，需拍板 |
| L2-38 | `docs/api.yaml` 的 403 与 `bin/agent2ssh-daemon.rs` 的 `require_admin` / `require_recording_admin` | 本轮已补齐 30 个操作，但**没有任何东西阻止它再次漂移** | 本轮把 36 个 admin 门 path（33 `require_admin` + 3 `require_recording_admin`，共 39 个 operation）逐个补了 403，此前只有 6 个 path 提过。手写清单会漂：新加一个 `require_admin` 调用，`api_yaml_documents_every_daemon_route` 只查 path 在不在，照样绿——这正是这条指摘能存在两个批次的原因 | 三选一：①在路由表旁加一份 `ADMIN_GATED: &[(&str, &str)]` 常量，测试拿它对比 `api.yaml` 的 403 集合（单一来源在代码里，但那张表仍要人维护）；②测试直接扫 `agent2ssh-daemon.rs` 源码找 `require_admin(` / `require_recording_admin(`（零维护，但测试依赖源码文本，改函数名就断）；③给 handler 加 `#[admin_gated]` 属性宏，路由注册时自动派生（最干净，代价是引入 proc-macro） |

### 2026-11-10 · batch 4（issue #9）第 2 轮追加（基线 `38eb70b`）

第 2 轮的指摘来自 CodeBuddy 对本流程第 1 轮回帖的复核（9 条，G1–G9）。文档面 7 条已 L1 落地，剩下的 4 条在下面。**本轮仍无 `src-tauri/src/**` 行为改动。**

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-39 | `src-tauri/web/console.html:543-545`（表头）、`:1115-1123`（行渲染）、`:1104`/`:1122`（`colspan="6"`） | 成立 | 内嵌 Web 控制台的审计表是 6 列（Timestamp/Host/Command/Exit Code/Duration/Risk），`grep -c decision src-tauri/web/console.html` = **0**；桌面端 `src/components/AuditPanel.tsx:37` 已把它渲染成 `Approved (edited)`。于是同一条 `approved_rewritten` 记录，桌面端能看出「有人改写后批准」，`/console` 只能看到一片空白，读起来像「没有判决」。`CHANGELOG.md:37` 那句「the audit panel labels it …」描述的是桌面端，不是这一块 | 加第 7 列 `Decision`，取值表复用 `AuditPanel.tsx:34-41` 的映射。**必须同时改三处**：表头 `<th>`、行渲染的 `<td>`、以及两处 `colspan="6"`（`:1104` 的空态与 `:1122` 的错误态），少改一处表格就错位。属 UI 行为改动，需拍板 |
| L2-40 | `src-tauri/src/core/team_config.rs:78-96`（`import_team_config`）、`:24-45`（`export_team_config`） | 成立 | 导出只剥 `key_path`/`password`/`passphrase`（`:31-38`），保留 `proxy_id`/`group`/`init_command`/`ai_allowed`；导入端 `*existing = next`（`:96`）只把前三个从旧值补回（`:81-95`），**`proxy_id` 不在补回名单里**。所以一份来自别人的导出会把本机 `proxy_id` 清空——`proxy_id` 指向本机代理配置，对别人没有意义，被清掉是纯损失。`group` 同理（未注册的组名会被 `normalize_config` 重置为 `default`） | 二选一：①把 `proxy_id` 加进补回名单（与三个凭据字段并列，但语义不同：凭据是「不能外泄」，`proxy_id` 是「跟着机器走」——更准确的做法是把它归入「本机绑定字段」并在导出时剥掉）；②保持现状，在 `/config/import` 的契约里写明「导入会整体替换主机记录，本机代理设置需重新指定」。②是纯文档，①是行为改动，需拍板 |
| L2-41 | `src-tauri/src/notify.rs:249-254`（Slack vs 默认 JSON 的三元）、`:628-680`（四条出站测试） | 成立 | 四条出站测试都走 `127.0.0.1` 且只断言 `is_ok()`（`:643`、`:668`、`:697`、`:725`），**没有一条断言收到的载荷形状**；`format_slack_message` 的三条测试直接调用函数、绕过三元。所以把 `:250` 的 `format_slack_message(&event)` 换成 `serde_json::to_value(&event)?` 全绿——这正是「测试守着错误的东西」 | 两条路：①把三元抽成 `fn payload_for(url: &str, event: &WebhookEvent) -> serde_json::Value`，给它一条纯函数测试（无网络、无 flake）——**属 src 重构**；②在 `:608` 那套 listener 测试上加一个捕获请求体的用例（L1 可落，但要自己解析 HTTP 请求，Windows CI 上多一条网络测试）。倾向①，但需拍板 |
| L2-42 | `docs/guides/daemon-api-quickstart.md` 的端点总览表 vs `bin/agent2ssh-daemon.rs` 的路由表 | 本轮已补齐 36 个 operation（70 → 105，双向差集为空），但仍然**没有守卫**。注：`70 → 105` 是 **operation** 数，路由数是 **107**（多 `GET /` 与 `GET /console` 两条非 API），这两个数在别处被引用时必须带单位 | 全仓没有任何测试读这张表：`api_yaml_documents_every_daemon_route` 只看 `docs/api.yaml`。这张表曾经漏掉 36 个 operation（含 `/local-exec` 与整个 `/resident-sessions` 族，都是 agent 最常引用的入口），正是在这段时间里没人发现 | 与 L2-38 是同一个方法选型（源码扫 vs 手写清单 vs 属性宏），建议一次拍板、两处一起加断言：`api.yaml` 的 403 与这张表的 operation 集合 |

### 2026-11-10 · batch 4（issue #9）第 3 轮追加（基线 `b756fb5`）

第 3 轮 9 条（B1–B9）里 7 条已 L1 落地（契约补齐与文案收窄），剩下的 3 条在下面。**本轮仍无 `src-tauri/src/**` 改动**——B1 牵出的那条诊断文案 `"daemon did not exit after a guarded shutdown; leaving it running"` 已经登记为 **L2-1**，按门禁不碰，见下方说明。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-43 | `src-tauri/src/approval.rs:108-117`（`ApprovalStatus::Revoked`）、`:761`（写入点）、`:1931`（断言） | 成立 | 契约原先的 `status` 枚举只有 4 个值，`revoked` 是第 5 个且**真的会落盘**。本轮已在 `api.yaml` 补进枚举并写明「今天只有进程内调用者能产生它，没有 HTTP 路由」。剩下的是产品问题：`revoke_approval` 目前只有测试调用者，**要不要给它一条路由** | ①不加：保持现状（已写进描述），客户端不要指望能 filter 到它；②加：`POST /approvals/:id/revoke`，与 approve/reject 同门（admin + scope 检查），并补一条 E2E。②是行为增量，需拍板 |
| L2-44 | `src-tauri/src/notify.rs:200-224`（按钮 URL 拼接）、`approval.rs:901-907`（`approval_action_url` 的形状）、`daemon.rs:5893`（真实路由） | 成立（三重不可用） | `format_slack_approval_notification` 给按钮拼的是 `{approval_action_url}/approve` 与 `/reject`，而路由是 `POST /approvals/:id/approve`（复数、POST）。三条各自都足以让它点不动：①地址不是路由，必然 404；②`is_public_path`（`auth.rs:99-101`）只放行 `/`、`/console`、`/health`、`/metrics`，`/approval/**` 要 bearer token，而浏览器点 Slack 按钮不带 `Authorization`；③`127.0.0.1` 在别人机器上指的是那台机器。对照 `format_slack_message`（`:299-370`）给 `approval_required` 用的是 `/console#approvals-<id>`，那条是**对的**——两条路径本来就不一致。本轮已把文档改成「不要把这些按钮当操作入口」（`configuration-guide.md`），但按钮本身还在发 | 把 `format_slack_approval_notification` 的 buttons 换成 `{base}/console#approvals-{id}`（与 `format_slack_message` 同形），或直接删掉 buttons 只留链接字段。属行为改动，且 Slack 卡片的按钮点击率无法在本仓验证，需拍板 |
| L2-45 | `src-tauri/web/console.html:670`（`API` 初值）、`:816`（connect 后用 `/daemons` 的 `url` 覆盖） | 成立（低危） | 静态打开 console.html 时 `API` 初值是写死的 `http://127.0.0.1:7722`，要等 connect 之后才被 `/daemons` 返回的 `url` 覆盖。所以「daemon 换了端口 + 用文件方式打开 console」这一种组合会在 connect 那一步失败；同源 `GET /console` 不受影响（相对 URL 走这个绝对前缀）。属可发现性，优先级低于 L2-44 | 把 `API` 初值改成同源相对（`""`），仅在需要跨源时才回落到写死地址；或让页面先请求 `/health` 探测当前 origin。属 UI 行为改动，需拍板 |

**关于 B1 牵出的 `daemon_control.rs:1027`**：那句 `"daemon did not exit after a guarded shutdown; leaving it running"` 与紧随其后的 `signal_daemon(pid, start_time, true)` 相反（记完 warn 就强杀），本轮改 `api.yaml` 时顺手改过一版，随后**按门禁撤销**——它在 `src-tauri/src/**` 里，且 **L2-1 早已登记**同一处。它仍待拍板，改法就是一句话：把文案换成「did not exit within the graceful window; terminating it by force」。

### 2026-11-10 · batch 4（issue #9）第 4 轮追加（基线 `684d471`）

第 4 轮的指摘**不是 CodeBuddy 给的**：它对本流程第 3 轮回帖超过协议窗口没有回复（60 秒 × 5 次，第 6 次到 11:46:38 仍是 `total=6`），按「等待超时 → 用已有材料推进、不空等」处理。已有材料是第 2、3 轮同一类缺陷的收尾——把 `docs/api.yaml` 里所有 `type: object` 且无 `properties` 的节点数了一遍：**18 个，5 组**，已全部补成实体（53 → 64 schema，`$ref` 61 个、0 悬空）。补的过程里牵出三件不在「补个 `properties`」射程内的事。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-46 | `docs/api.yaml` 全文 **142 处** `nullable: true` | 成立（口径待定） | 文档头写的是 `openapi: "3.1.0"`，而 `nullable` 是 **3.0** 的关键字：3.1 里空值要写成 `type: [x, "null"]`。严格按 3.1 校验的解析器会**忽略** `nullable`，也就是这 142 处的可空性目前并没有被表达，只是被宽容的渲染器放过了。本轮新增的 `WebDavSyncStatus.local`/`remote` 改用 `$ref` + 描述写可空，没有再加一处 `nullable`；既有的 142 处原样未动 | 二选一：①全量换成 `type: [x, "null"]`（142 处，机械但面积大，`$ref` 与 `$ref` 带兄弟关键字两种形态要分别处理）；②保持 `nullable`（Swagger UI 与多数 codegen 仍认它，代价是文档自称 3.1 却不合规）。取值口径问题，不该我单方面定 |
| L2-47 | `docs/api.yaml` 的 `AgentEvent` / `ApprovalResponse` / `AuditFilter` | 成立（低危） | 本轮把 61 个 `$ref` 全核了一遍：0 个悬空，但有 **3 个组件从来没有被引用过**。不是本轮新加的，是更早批次留下的。可能真的没人用了，也可能有外部工具按名字取用 | ①删掉（schema 数 64 → 61）；②留着并在描述里注明「保留给外部消费者，本仓不引用」；③真接上。选哪个都行，但要一次定，否则下一轮还会被当成新发现 |
| L2-48 | `src-tauri/src/webdav_sync.rs:104` vs `src-tauri/src/tauri_commands.rs:2404` | 成立 | **两个 `pub struct WebDavSyncStatus`**：前者 8 字段、snake_case、daemon 用；后者 11 字段、`rename_all = "camelCase"`、桌面端用。同名不同形，`grep -rn "struct WebDavSyncStatus"` 同时命中两条。本轮已在 `api.yaml` 的组件描述里点名「这是 daemon 序列化的那个，不是桌面端那个」，但代码侧的歧义还在 | 把桌面端那个改名（如 `DesktopSyncStatus`），属 `src-tauri/src/**` 改名 + 调用点替换，需拍板。风险低但要一次改完 |

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

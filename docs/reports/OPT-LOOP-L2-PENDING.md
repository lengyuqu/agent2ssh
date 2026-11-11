# L2 待拍板清单（优化轮次）

`src-tauri/src/**` 的行为/签名改动、Windows 语义改动，按消费任务的分级规则**不进 main**，攒在这里等人工决策。每条都写清为什么不能自动落、以及落地时的验证要求。

---

## 编号撞号登记（2026-11-11，batch 8 第 5 轮核实）

本文件由多批各自从头编号写成，**12 个编号各出现两次、指两件不同的事**：`L2-13`、`L2-14`、`L2-15`、`L2-16`、`L2-17`、`L2-18`、`L2-19`、`L2-20`、`L2-22`、`L2-23`、`L2-25`、`L2-30`。两处散在不同日期的小节里，多数是「daemon 路由/关闭路径」对「`known_hosts`/主机键/认证」，但也有例外（`L2-30` 的两处都是 daemon 侧）。

例：`L2-15` 既是「daemon 在 `shutdown.cancel()` 之后进 handler 的请求仍回 200」，也是「`remove_system_known_host` 的 `patterns` 只认 `[host]:port` 一种形态」。后者在本批被反复引用，前者与本批无关——只写编号会让读者跳到错的那一条。

**引用规则（立即生效）：**

- 引用撞号条目时**必须带主题限定**，写成 `L2-15（known_hosts 形态）` / `L2-15（shutdown 后仍 200）` 这种形式。这个文件里凡出现裸 `L2-13`…`L2-30` 而无主题限定的引用，都是旧引用，需按上下文重判。
- **新条目从 `L2-100` 起编号**，不再取 13–99 的空号——空号看起来安全，实际每次都挑到别人用过的号。**已用：`L2-100`（`forward.rs` accept 循环）、`L2-101`（SFTP 审批用标签快照）、`L2-102`（三条 forward 单条路由的 scope 越权返 403）、`L2-103`（MCP `ssh_events_subscribe` 结构上恒空）、`L2-104`（`tools.rs` 的 `tools/list` 文案）、`L2-105`（daemon 侧 `transfer_id`）、`L2-106`（`ResourceKind::Transfer` 条目从未建成）、`L2-107`（`--risk-override` 的取值与静默丢弃）、`L2-108`（`team_config` 的风险规则路径没跟上 policy）。下一个是 `L2-109`。**
- **上一轮的教训（关帖后补记）**：`L2-100` 立条时我写的是「`/local-exec` 的 webhook 恒说结果已知」——那是错的，`/local-exec` 的 `remote_outcome_unknown: None`（`daemon.rs:1807`）的确恒真，但它**不是**没人做过：同一形状在 `:1711` 的 `/exec` 上工作，两边差的是 `:1700` 那一次 `ssh_shutdown_outcome()`，而 local 侧的类型在 `core.rs:1267` 就被折成了字符串。所以那件事的条目是 **L2-27**，不是新号。**立条前先 `grep` 一遍已经有了什么**，这一条是从「差一点就重复登记」里换来的。
- **同号 + 括号后缀**（目前只有 `L2-23（收窄）`）视为对同号条目的**修订**，不计入撞号，但正文必须写明它在修订哪一条、撤掉了哪个选项。否则下一次按首列扫表的脚本会把它当成一个独立条目。
- 全量重编号要同时改 ledger 里六批的引用，属人拍板项，不在这里做。半途重编号会让「按编号追溯」比现在更坏。

**登记后的一次外部复核（2026-11-11，第 6 轮补记）**：CodeBuddy 按「定义处」重数，结论也是 **12 对、没有第 13 个**，但它把 `L2-23` 判成「同一条的两个版本」、把对数降到 11——**这一条不成立**。它的扫描只看到 `:267` 与 `:281`（后者是 `L2-23（收窄）`，即同一个条目的修订），漏掉了 `:93` 那一条：`daemon_control.rs:488-500` 的 `child.kill()` 失败 + `write_daemon_pid_if_absent` 吞 `AlreadyExists`，与 `HostKeyKind::Unknown` 的 `"unknown"` 字面量毫无关系。12 对里有 11 对确实指两件事，第 12 对（`L2-23`）**也是**两件事，只是它还多带了一个同号修订。它由此提出的「后缀形态没有合法地位」成立，已写成上面第四条规则。

它同一份复核里另指一处**真问题**，与本登记同类：`:91` 的 L2-20 结论末尾写「4s 之所以每次都烧满，根因是 L2-26 的 entry 泄漏」，而 `L2-26` 已在本文件 `:99` 判为**不成立并撤回**（`drain_handle.unregister()` 在 `on_upgrade` 闭包内、future 之外）。它把位置说成 `:251`、把修法说成「改指 `L2-20（entry 泄漏）`」，两处都不对——`:251` 是 L2-20 的另一件事（同名 profile 只落一条），而「entry 泄漏」已经被撤，无处可指。正确修法是删掉那句根因断言。已改。

**第 6 轮补记的补记（关帖后的第二次复核，同日）**：上面那段说「没有第 13 个」——**这个结论我只维持了一半**。按「`L2-17` 与 `L2-22` 各出现两次、且两处**都落在 `known_hosts`/agent 一侧**」重新看：`L2-17` = `/exec/compare` 缺 `source` **对** `list_identities` 置 `None`；`L2-22` = daemon 的 `run_ws_with_shutdown_deadline` 返回值**对** `import_known_hosts_from_ssh` 不验公钥结构。而本段开头那句「多数是『daemon 路由/关闭路径』对『`known_hosts`/主机键/认证』」对这两条**不成立**——它们不是跨主题的对，是同一主题的两半，所以「带主题限定」这条规则对它们**不足以消歧**，必须按内容起名（`L2-17（/exec/compare 缺 source）` / `L2-17（list_identities 置 None）`）。这就是第 13 个：不是漏登记，是**登记里有一句描述与事实不符**。计数按 **13** 读，正文「12 个」与下面这段原文都保留不改写（历史记录）。

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

**L2-10 的预算更正（batch 14 第 2 轮）**：原文「四个串行请求各 2 s connect + 2 s read，最坏 16 s」把两段超时相加了。`daemon_activity.rs:102-110` 的 `request_at` 里 `connect_timeout`、`set_read_timeout`、`set_write_timeout` 用的是**同一个 `timeout` 参数**，全仓调用方恒传 `Duration::from_secs(2)`，所以**逐请求上界是 2 s**，四个串行请求最坏 **8 s**。实测上界受 `ErrorKind` 分派影响（EAGAIN 可能连续重试），不是干净可复算的数；**按「2 s × 4 = 8 s」登记，这是最坏值里唯一可推导的那部分**。

真正的挂起预算比 L2-10 记的更宽，且横跨三个函数——都在 `daemon_control.rs`（复核方这条写错了文件名，写成 `daemon_activity.rs`，但行号是对的）：`daemon_pid_state`（`:343`）→ `daemon_matches_config`（`:283`）→ 两次 500 ms 请求 ≈ 1 s，然后 `request_shutdown` 再 2 s，然后 `wait_for_exit(pid, start_time, GRACEFUL_STOP_WAIT)` 的 8 s（`:651` 定义、`:1057` 调用），最坏 **~11 s**。

---

## 2026-11-10 · 来自 issue #7 第 2 轮（CodeBuddy 复核后的新一批）

第二轮指摘 15 条（L2-19…L2-33），核实后**成立 6 / 部分成立 5 / 不成立 2**（另有 2 条已在第 2 轮的 L1 改动里做掉）。这一批里有两条是真正的代码缺陷，其余是观测与措辞问题。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-19 | `:4824`、`:5326`；`:5009`、`:5428` | 部分成立 | 措辞 + 两处实现不一致 | 「每连接 2s」不等于「N 条叠加成 2N」：两个 handler 各跑一个 2s 上限，连接之间**并行**。而 500ms 在两处语义不同——terminal（`:4824`）是**每次 recv 重置**的 500ms 空闲超时，exec（`:5326`）是取消时刻起的**绝对 500ms 截止**。所以「共享一个 500ms 窗口、最短 0.5s」不成立（terminal 的循环在 `event_rx` 立即关闭时根本不等）。文档已按「并行 + best effort」改写（L1）；两处语义是否统一属设计取舍 |
| L2-20 | `:5937` vs `:5926-5929` | 部分成立 | 结论影响常量推导 | 「串行」成立：`with_graceful_shutdown(...).await` 先结束，之后才 `wait_until_empty(4s)`。但「最坏 4s 不是 6s」不成立——2s 的 per-connection 上限在 graceful 阶段已经跑完，registry 再单独计时 4s，**最坏仍是约 6s**。（原文此处把「4s 每次都烧满」归因于 L2-26 的 entry 泄漏；L2-26 已判不成立并撤回，见下文 `:99`，故那 4s 目前**没有已确立的成因**，也不能拿它把上界改小。） |
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
| L2-17 | `russh_impl/agent.rs` `list_identities` 的 `if fetched.is_err() { *self.client.try_lock()… = None }`（`:240-246`）；现有测试 `an_unconnected_agent_refuses_instead_of_pretending`（`:748`）只覆盖「从未连过」 | 部分成立（结构成立；`docs/KNOWN-ISSUES.md` §3.1 的共存边界只写了 Windows 的 pipe/Pageant 顺序，但这段代码是跨平台的） | 「连过、`request_identities` 失败一次」之后，本连接生命周期内 `list_identities` 永久 `Err`，且错误串退化成 `the SSH agent is not connected`——与「从未连过」不可区分（`ssh-agent` 中途重启就会踩到）。修它要加状态，改的是对外错误文案；补的测试会**先红**，不能直接进 main | `client` 从 `Option` 换成三态（`NeverConnected` / `Connected` / `Failed(原因)`）；测试：连上 → 让 `request_identities` 失败一次 → 再调，断言第二次文案含原错误。`KNOWN-ISSUES` §3.1 同时补 unix 侧。**原「或在置 `None` 前存下错误、下次返回时带上」已删**（batch 8 第 1 轮）：`list_identities` 第二次进来仍在 `:239-242` 拿同一个 `None` 并提前返回 `the SSH agent is not connected`，存下的字段没有读取点——那半句是空分支，要做就等于三态 |

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
| L2-40 | `src-tauri/src/core/team_config.rs:78-96`（`import_team_config`）、`:24-45`（`export_team_config`） | 成立 | 导出只剥 `key_path`/`password`/`passphrase`（`:31-38`），保留 `proxy_id`/`group`/`init_command`/`ai_allowed`；导入端 `*existing = next`（`:96`）只把前三个从旧值补回（`:81-95`），**`proxy_id` 不在补回名单里**。所以一份来自别人的导出会把本机 `proxy_id` 清空——`proxy_id` 指向本机代理配置，对别人没有意义，被清掉是纯损失。`group` 同理（未注册的组名会被 `normalize_config` 重置为 `default`） | 二选一：①把 `proxy_id` 加进补回名单（与三个凭据字段并列，但语义不同：凭据是「不能外泄」，`proxy_id` 是「跟着机器走」——更准确的做法是把它归入「本机绑定字段」并在导出时剥掉）；②保持现状，在 `/config/import` 的契约里写明「导入会整体替换主机记录，本机代理设置需重新指定」。②是纯文档，①是行为改动，需拍板 | **2026-11-11 第 5 轮补记（`init_command` 这条支线已 L1 关闭，L2-40 本身仍 open）**：`init_command` 确实在导出侧保留、在导入侧被丢，但它的丢法与 `proxy_id` 不同——`export_team_config` 从不剥它，所以「包里没有」只可能是「来源机器没设过」，不可能是「分享者关掉了」。据此已在 `import_team_config` 加保留（与 `key_path`/`password`/`passphrase` 并列），并新增 `test_team_config_import_preserves_a_local_init_command` 钉住两个方向。**`team_host_same` 的 11 个比对字段刻意不含它**：含了就会「报 updated 然后保留原值」，即报一个不改的差异。`proxy_id` 那半仍未修——它是**跟着机器走**的本机绑定字段，语义与凭据相反，需要拍板（①导出时剥掉 ②契约里写明导入整体替换）。
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
| L2-46 | `docs/api.yaml` 全文 **136 处** `nullable: true`（`grep -c` 现算；此前写 142 是我把 `grep -c null` 当成了 `nullable: true`，见 ledger 第 6 轮） | 成立（口径待定） | 文档头写的是 `openapi: "3.1.0"`，而 `nullable` 是 **3.0** 的关键字：3.1 里空值要写成 `type: [x, "null"]`。严格按 3.1 校验的解析器会**忽略** `nullable`，也就是这 142 处的可空性目前并没有被表达，只是被宽容的渲染器放过了。本轮新增的 `WebDavSyncStatus.local`/`remote` 改用 `$ref` + 描述写可空，没有再加一处 `nullable`；既有的 142 处原样未动 | 二选一：①全量换成 `type: [x, "null"]`（136 处，机械但面积大；其中 **12 处是 `$ref` 带 `nullable`**，3.1 里 `$ref` 的兄弟不能改 `type`，那 12 处得写成 `anyOf: [$ref, {type: "null"}]` 或按「可空不是契约的一部分」削掉）；②保持 `nullable`（Swagger UI 与多数 codegen 仍认它，代价是文档自称 3.1 却不合规）。取值口径问题，不该我单方面定 |
| L2-47 | `docs/api.yaml` 的 `AgentEvent` / `ApprovalResponse` / `AuditFilter` | 成立（低危） | 本轮把 61 个 `$ref` 全核了一遍：0 个悬空，但有 **3 个组件从来没有被引用过**。不是本轮新加的，是更早批次留下的。可能真的没人用了，也可能有外部工具按名字取用 | ①删掉（schema 数 64 → 61）；②留着并在描述里注明「保留给外部消费者，本仓不引用」；③真接上。选哪个都行，但要一次定，否则下一轮还会被当成新发现 |
| L2-48 | `src-tauri/src/webdav_sync.rs:104` vs `src-tauri/src/tauri_commands.rs:2404` | 成立 | **两个 `pub struct WebDavSyncStatus`**：前者 8 字段、snake_case、daemon 用；后者 11 字段、`rename_all = "camelCase"`、桌面端用。同名不同形，`grep -rn "struct WebDavSyncStatus"` 同时命中两条。本轮已在 `api.yaml` 的组件描述里点名「这是 daemon 序列化的那个，不是桌面端那个」，但代码侧的歧义还在 | 把桌面端那个改名（如 `DesktopSyncStatus`），属 `src-tauri/src/**` 改名 + 调用点替换，需拍板。风险低但要一次改完 |

### 2026-11-10 · batch 4（issue #9）第 5 轮追加（基线 `c3af875`）

第 5 轮的指摘来自 CodeBuddy 对本流程第 4 轮回帖的复核（9 条，C1–C9）：**成立 7 / 部分成立 2 / 不成立 0**。文档面 4 条已 L1 落地（C1 的契约一半、C3、C8、C9 与表头口径），C1 的注释一半落在 `95461ff`（只改文档注释，无行为）。代码侧 5 条在下面，**本轮仍无 `src-tauri/src/**` 行为改动**。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-49 | `bin/agent2ssh-daemon.rs:4012-4021`（`daemons_view`）、`src-tauri/src/remote.rs:534-541`（计数） | 成立 | `daemons_view` 的签名是 `Extension(_auth)`，函数体一次都没读 `auth`；`get_daemons_unified_view()` 对每个远端 daemon 取 `/hosts` 的 `hosts.len()` 累加，用的是 daemon 自己配置的 token。所以**聚合数字与调用方是谁无关**：scoped token 与 admin 看到同一组 `host_count`/`total_hosts`。注意这不是「读到了被 `/hosts` 过滤前的本地配置」，而是「调用方 scope 全程没参与」 | 让 `get_daemons_unified_view` 接受调用方 scope，逐 daemon 用 `scope_allows_observe_host` 过滤后再计数；要动签名 + 跨 bin 调用点，需拍板 |
| L2-50 | `src-tauri/src/remote.rs:943`（`PROTOCOL_VERSION`）、`:966-1000`（比较）、`docs/api.yaml` 的 `VersionCompatibility` | 成立 | `pub const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION")` —— 三个 bin 共享同一个包版本号，所以 `/version-check/{alias}` 比的是**应用版本字符串**。同一版本号的两个构建之间如果路由/语义变了，它照样报 `compatible: true`；两个版本号不同但协议没变的构建则报不兼容。契约与 `daemon-api-quickstart.md:152` 都把它当「协议」发出去了 | ①拆一个整型协议号，`/health` 一并返回，比较协议号（行为增量，要改 `VersionCompatibility` 的形状）；②保持现状，在两处文档写明「这是应用版本比较，不是协议兼容性」。②纯文档但会削弱这个端点的用途，需拍板 |
| L2-51 | `bin/agent2ssh-daemon.rs:1924`（`/audit` 的 `host_scope: audit_host_scope(&auth)`）与 `:1955`（`/audit/export` 的 `host_scope: None`） | 成立 | 两个端点的筛选参数逐字相同（`api.yaml` 的 `/audit` 与 `/audit/export`），但 `/audit` 接受 scoped token 并按 host 过滤，`/audit/export` 只接受 admin 且**不做任何过滤**。今天 scoped token 过不了 `/audit/export`（403），所以不是当场泄漏；问题是**以后放宽权限时导出会漏过 `/audit` 的 scope**，而契约上看不出这两个端点权限模型不同 | ①`/audit/export` 复用 `audit_host_scope`（改了权限语义）；②在 `api.yaml` 的描述里写明「只用 admin token，且不套 host scope」（纯文档）。②更保守，①更一致 |
| L2-52 | `bin/agent2ssh-daemon.rs:3040-3094`（`forward_remove`） | 成立 | 命中分支（`:3048-3089`）走 gate / rate limit / scope / risk + `append_operation_audit`；未命中分支（`:3090-3093`）直接 `forward_remove_core(uuid)` → `BAD_REQUEST`，**不写审计、不查 scope**。于是同一条路由有两种「记不记录」的语义，且未知 id 回 400 而不是 404 | ①未知 id 统一 404；②抽 `find_forward_rule(id)` 让两条分支共用同一段前置检查与审计。都在 handler 层，需拍板 |
| L2-53 | `src-tauri/tests/daemon_integration.rs:129-393`（**20 个 handler 副本**）、`:410`（`build_test_router` 挂副本） | 成立（**结构性，优先级最高**） | 这个测试文件重新实现了 20 个 handler 并在 `build_test_router()` 里挂这些副本，而 daemon 有 107 条路由。副本**已经开始与真身分叉**：`approval_approve` 副本（`:175`）只有 `State/headers/Path` 三个参数，真身（`daemon.rs:3337`）多一个 `body: Bytes` —— 所以真身的「空 body = 按原样批准」与改写路径两条语义，副本里根本不存在；`get_health_snapshot_handler` 副本回 `{error}`，真身回 `{error, hint}`。C2 想补的那条测试如果加在这里，测到的仍然是副本。**推论**：任何「集成测试绿了所以 HTTP 面没问题」的说法，在这个文件上都不成立 | ①用 `#[path = "../src/bin/agent2ssh-daemon.rs"]` 之类引入真 bin 并给 handler 加可见性（axum handler 是普通 `async fn`，可行但要把 bin 改成 lib+bin 结构）；②把 HTTP 面整体交给真 daemon 起的 e2e（`daemon_local_exec_e2e.rs` 那套已经在跑真二进制），删掉副本路由；③保持现状并在文件头写明「这是契约形状的镜像，不是实现测试」。③是零成本的诚实做法，但下次还会有人把它当实现测试读。需拍板 |

### 2026-11-10 · batch 4（issue #9）第 6 轮追加（基线 `b680233`，末轮）

第 6 轮 8 条（A–H）里 6 条成立、1 条部分成立、1 条不成立（E，依据已过时）。文档面已随 `0a79492` 落地；只剩下面这条是产品能力取舍。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-54 | `src-tauri/src/events.rs:20-39` 的 `EventType::HostConnected`（`:29`）、`HostDisconnected`（`:30`）、`ConfigChanged`（`:34`） | 成立 | 全仓 `EventType::HostConnected` / `HostDisconnected` / `ConfigChanged` 只有 `events.rs:92/93/99` 三处**测试**引用，**零生产发布点**；`git log -S` 显示这三个值只有引入、没有「曾经发过后来丢了」。前端早就在注释里承认了：`src/App.tsx:135`、`src/components/NotificationCenter.tsx:18`。契约把三个从未实现的值当既有行为列出来（本轮已在 `api.yaml` 的 `event_type` 描述里加「不要等它们」，但枚举还在） | ①**削**：从 `events.rs` 与 `api.yaml` 删掉三个值，前端两处注释跟着删、`src/types.ts:478/479/485` 的三个字面量一起删。这才符合「不给不存在的行为写契约」。②**补**：连接建立/关闭走 `connection.rs`/`forward.rs`，`config_changed` 挂在 `save_config` 成功之后——但 `App.tsx` 现有的 5s 轮询会与事件重复，得同时改掉轮询。①更诚实、②更有用，需拍板 |

**批次收尾提示**：本批 6 轮已跑完（`BATCH_COMPLETE`），issue #9 关闭。待拍板项累计 **L2-1 ~ L2-54**（编号不连续，缺号是早期批次合并掉的）。下一批开工前应先处理 L2-53（集成测试测副本）与 L2-46（3.1 合规），这两条会影响后续所有契约工作的可信度。

---

## 2026-11-10 · issue #10（前端与 i18n 文案键完整性）第 1 轮追加

**分级口径说明（需要人确认一次）**：消费任务的分级表把 L2 写成「任何 `src-tauri/src/**` 行为/签名改动」，L1 写成「`docs/**`、代码注释、非平台相关测试断言强度」，**前端 `src/**` 的产品代码没有落在任何一格里**。本批主题恰好全在前端，所以本轮采用更保守的读法：**改用户可见行为的归 L2**（即便文件在 `src/`），只把注释与测试落进 main。如果口径相反（前端 UI 改动允许直接落 main），下面三条应当立即执行——它们都不涉及跨平台语义，`npm run build` + `npm test` 即可验证。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-55 | `src/lib/format.ts:14`（`date.toLocaleString()`）、`:24`（`date.toLocaleTimeString([], options)`）；同族 `src/components/AuditCharts.tsx:143`（`toLocaleDateString([], …)`）、`src/components/SFTPPanel.tsx:207`（`toLocaleString(undefined, …)`） | 成立（文案不一致） | `I18nProvider` 把 `<html lang>` 改成 `zh-CN`/`en` 并翻译所有标签，但这四个格式化器都不传 locale，取的是 `navigator.language`。于是中文界面在 en-US 浏览器上把时间渲染成 `11/10/2026, 2:15:03 PM`，同一屏里中文标签配英文日期。**11 个消费点**（第 5 轮补上漏掉的两个）：AuditPanel、FilePreview、SyncPanel、ConfigSnapshotsPanel、RecordingsPanel、LiveActivityPanel、AuditCharts、SettingsMenu、ApprovalDetail、ApprovalQueue、ApprovalWorkbench。**不是未翻译，是同一语言信号两条来源**，所以「翻译字典」永远修不到它 | 二选一：①`formatDateTime`/`formatClockTime` 增加可选 `locale` 参数，调用点传 `language === "zh" ? "zh-CN" : undefined`（**要在 `src/i18n.tsx` 的 context 里暴露 locale，或在 format 里读 `document.documentElement.lang`**——后者更省调用点改动，但让纯函数依赖 DOM）；②保持运行时默认，接受「日期跟随系统、标签跟随应用」。①的代价是 11 个调用点要拿到 language（部分在 `t` 之外）；②的代价是中文用户看到英文日期格式。**落地时务必一次覆盖 5 个 `toLocale*` 面**：`format.ts:26`、`format.ts:36`，加上绕过 helper 的 `SFTPPanel.tsx:207`（mtime 列）与 `AuditCharts.tsx:143`（横轴），只改两个函数的话 SFTP 与审计图仍是英文。属用户可见行为改动，需拍板 |
| L2-56 | `src/components/ui/toast.tsx:153-159`（本轮在其上方加了注释） | 成立（缺陷 / 无障碍） | toast 的关闭按钮是**纯图标**：无 `aria-label`、无 `title`、无文本子节点，屏幕阅读器只读出「button」，视障用户无法知道它关闭什么，也无法判断这是可操作的关闭项。同级按钮都合规（`ui/data-table.tsx:31` 用 `<IconButton title={label}>`；全 `src/` 的 `aria-label=`/`title=` 共 254 处，未包 `t()` 的为 0）。现成的 `zh["Close"]`（`i18n.tsx:190`）可直接用 | 二选一：①加 `aria-label={t("Close")}`（一处，最小）；②换成 `<IconButton title={t("Close")} …>`（与 data-table 同形，但要注意 `IconButton` 不带 `[&_svg]:size-4`，需核对 `X size={13}` 的既有尺寸是否被改变——见口径 4）。两者都改变无障碍树，且 jest-dom 未装，无法用 `toHaveAccessibleName` 断言，只能断 `getAttribute("aria-label")`。**本轮核查过「唯一性」这条加强论据但未能独立证实**：粗扫定位 44 处疑似无名 `<button>`，抽查 3 处（ErrorBoundary:58、Breadcrumb:26、HostList:396）全有文本子节点，是启发式的假阳性；结论只按这一处本身无名字给出 |
| L2-57 | `src/i18n.tsx:948-953`（`initialLanguage()`，本轮在其上方加了注释）、`:961`（`localStorage.setItem`） | 成立（低概率、后果为整树白屏） | 同族的另外两处都防了：`theme.tsx:36-41` 的 `initialTheme()` 包了 try、`App.tsx:126-131` 的 `sidebarCollapsed` 包了 try。这一处没有，而它在 `useState(() => initialLanguage())` 的**首帧**路径上；`main.tsx:37-47` 的挂载顺序是 `ThemeProvider > I18nProvider > ToastProvider > EventsProvider > ErrorBoundary > App`——boundary 在 provider **内部**，所以 `getItem` 抛 `SecurityError`（隐私模式 / 禁用存储 / 不透明 origin）时整棵树挂掉、兜底页也没机会渲染。`src/test-setup.ts:7-23` 给 jsdom 补了 localStorage shim，所以测试永远走不到这条路径。注：`STORAGE_KEY` 与 `theme.tsx:17` 同名不同值**不是缺陷**（两个模块级私有常量，取值 `agent2ssh.language` / `agent2ssh.theme`，互不干扰） | 把 `getItem`/`setItem` 各包一层 try-catch，与 `theme.tsx` 完全同构（失败时回落 `navigator.language` / 静默跳过写入）。零语义风险的健壮性改动，但属行为改动。若要连带加测试，需先让 `test-setup.ts` 的 shim 可被覆盖（现在是「缺了才补」，测试里删掉 `globalThis.localStorage` 会触发 `undefined` 而不是抛错） |

| L2-58 | `src/components/KeysPanel.tsx:194-202`（本轮在其上方加了注释）、`src/components/KeysPanel.test.tsx`（第 3 轮改名） | 成立（**死分支**，比「零覆盖」更重） | 条件是 `algorithm === "ed25519" && preferredAlgorithm === "rsa"`，而 `preferredAlgorithm` 来自 `defaultKeyAlgorithm()` → `tauri_commands.rs:3204` → `keys.rs:190` 的 `default_for_build()`，后者**无条件返回 `Self::Ed25519`**（函数体就一行，注释自述「no probe and no build where the answer differs」）。所以 `preferredAlgorithm === "rsa"` 在生产里永假，这段 `<p>` 从不渲染。**文案本身也过期**：它说「本平台的 SSH 库读不了 Ed25519」，描述的是 libssh2 时代，现在的传输是 russh + ring，能读。当前唯一能让它出现的是 `KeysPanel.test.tsx` 的 `mockResolvedValue("rsa")` | 二选一：①**删**：`KeysPanel.tsx:194-202` 整段 + `KeysPanel.test.tsx` 的「keeps the legacy RSA-build warning…」一条 + `KeyAlgorithm` 的 `preferredAlgorithm` state 链（若删后无他用，`useState` 与 `defaultKeyAlgorithm` 调用也可一并去掉，需核）。②**留**：保留分支并把文案改写为「旧后端」语气，测试改名（本轮已做）——但一个永不渲染的分支保留下来只是给阅读者增加疑问。**建议①**，因为它同时删掉一段会误导人的过时描述 |
| L2-59 | `src/lib/terminal/folds.ts` 全文件、`src/components/TerminalView.tsx:65-66/183-184/261/473` | 成立（**功能未接线**，`folds.test.ts` 517 行守的是它） | `createFoldStore(term, tracker)` 在 `TerminalView.tsx:261` 是**唯一**调用点且不传 options，于是 `maxVisibleLines === null`、`canAutoFold()` 恒 false，自动折叠三方法（`foldActiveOverflow`/`foldRecentOverflow`/`enforceAutoFold`）永不触发。`foldBlock`/`unfoldBlock` 只在 `TerminalViewHandle` 上声明与实现，`TerminalPanel`/`TerminalDrawer`/`App` **无一处调用**，所以 `fold()`/`unfold()` 也不可达。**唯一的活路径是 `unfoldAll`**（`TerminalView.tsx:473`）。注：指摘说「九个方法零生产调用」略有出入，`unfoldAll` 有一处 | 三选一：①**接线**：给 `createFoldStore` 传 `maxVisibleLines`，并把 `foldBlock`/`unfoldBlock` 接到命令块的 UI（工作量最大，但 517 行测试才有意义）；②**标**：本轮已在 `folds.ts` 模块头写明接线状态与「别把绿读成功能可用」；③**删**：连 `folds.test.ts` 与 `command-blocks.ts:187-200` 的 `splitMode: "prompt"` 分支、`prompt.ts` 一起清。②已落，①与③需拍板 |

**同批相关的两处旁证（不进 L2，已核实）**：`command-blocks.ts:187-200` 的 `splitMode: "prompt"` 无生产调用者；`folds.test.ts` 的 fake 缺 `_viewport`，`syncViewport` 的 `vp?.queueSync()` 在测试里从不执行（去掉 `?.` 全绿）——这是 `folds.ts:113-119` 里唯一没有 fake 覆盖的可选链。

## 2026-11-10 · issue #10 第 4 轮追加（产品行为面，7 条指摘 + 1 条问答）

第 4 轮把视角从测试强度转到产品行为。判定 **成立 5 / 部分成立 2 / 不成立 0**。下面 8 条都是 `src/` 前端产品代码的行为改动，按本批口径（改用户可见行为 → L2）进清单；四处最危险的已在代码现场加注释。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-60 | `src/components/approval/ApprovalDetail.tsx:63-71`（`candidateRows`，本轮在其上方加了注释） | 成立 | `host_address` 在 `docs/api.yaml` 的 `ApprovalRequest.context` 与 `src/types.ts` 的 `ApprovalContext` 都声明了（**本行原文写的 `api.yaml:426` 是错的，第 5 轮更正**：e414b63 时 426 行是 `requested_at`，`host_address` 在 434；第 4 轮再往后漂到 441。跨文件引用别写行号，写 schema 名），`candidateRows` 只列 `source`/`matched_policy`/`reason`/`change_id` 四项，**从不渲染地址**。审批工作台是独立模块（`nav.ts` 的 `approvals`），审批者不离开工作台就分不清 `prod-web` 是 `.5` 还是 `.6`。这与我第 3 轮判 A-1 成立的理由是同一个（契约承诺、UI 可静默丢弃），对象换成 `host_address` 一个 | `candidateRows` 加一行 `["Address", ctx?.host_address]` + 一条断言。另注：`host_env`/`host_role` 也声明在 `ApprovalContext`，**契约里同样列了**（`ApprovalRequest.context`，只给类型不给 description）——原文那句「`api.yaml` 未列」是错的，第 5 轮更正；UI 确实不读，删或留另议 |
| L2-61 | `src/components/TerminalView.tsx:161-179`（`copyBlock`，本轮加了注释）、`src/lib/terminal/block-content.ts:85` | 成立 | `extractCommandBlockText` 在 `resolveCommandBlockRange` 返回 `null` 时**返回 `""` 而不是抛错**，`copyBlock` 随即 `writeText("")` 并返回 `{ ok: true, characters: 0 }`，`TerminalPanel.tsx:315-320` 据此显示「copied」。**块已滚出 scrollback 时，用户点复制得到空剪贴板 + 成功提示**。`block-content.test.ts` 有空块的用例吗？没有 | 给 `CopyBlockResult` 加 `reason: "block_scrolled_away"`（或让 `extractCommandBlockText` 抛一个专用错误），`TerminalPanel` 显示对应文案。空串与「内容为空」在当前类型下不可区分，这是根因 |
| L2-62 | `src/components/PlaybooksPanel.tsx:404-407`（Steps 标签，本轮加了注释）、`:245-247`（`switchToVisual`）、`:250-253`（`handleSave`） | **部分成立** | 指摘说「`switchToVisual` 零调用点」**不成立**——`:492` 的「Apply YAML to step editor」按钮就调它。说「YAML edits 静默丢失」**也不成立**——`handleSave` 在 yaml 模式下先 `applyYamlToForm()` 再保存，内容会落盘。**成立的是**：点 Steps 标签直接 `setEditorMode("visual")`，绕过 `applyYamlToForm`，于是可视化表单显示的是旧值，而 `yamlText` 里的编辑看不见了（切回 YAML 还在） | 二选一：①把 `:407` 的 `onClick` 改成 `switchToVisual`；②`disabled={Boolean(yamlError)}` 承认「有错就不许切」。①更直观，但 `applyYamlToForm` 失败时是静默返回 false（无 toast），需要一并补提示 |
| L2-63 | `src/components/Dashboard.tsx:160`（`value={execCount24h ?? "…"}`；注释在 `:155-159`，`(L2-63)` 标记在 `:159`）、`:106-107`（`} catch {` / `setExecCount24h(null)`） | 成立 | `execCount24h` 的 `null` 承担三种含义：未加载 / 加载中 / `listAudit` 失败。三者都渲染 `"…"`。daemon 离线时，「24h executions」是一个**永不结束的加载态**，没有 toast、没有日志、没有测试会红 | 拆成 `{ state: "loading" \| "ok" \| "error", value?: number }`，或加一个 `execCountError` 布尔让 `StatCard` 能显示 `—` 或错误图标。后者改动小，前者更彻底。要动 `StatCard` 的 `value` 契约 |
| L2-64 | `src/components/SetupWizard.tsx:199-203`、`:64-76`（`handleImport`） | 成立 | `!importing && !importError && importedHosts.length === 0` 同时覆盖「还没点过 Import」与「点了但结果是 0 台」，所以**初始状态就显示「No new hosts found. You may not have a ~/.ssh/config file…」**——用户还没点，界面已经在给结论。缺第三个状态（`attempted: boolean`） | 加 `const [attempted, setAttempted] = useState(false)`，`handleImport` 里置 true，文案分支改成 `attempted && importedHosts.length === 0`。属 UI 行为改动 |
| L2-65 | `src/components/NotificationCenter.tsx:40-56`（`onClick`）、`:69-77`（`approval_responded`） | 成立 | 审批 toast 的 Approve/Reject 成功路径**什么都不做**，toast 的消除完全依赖 SSE 推来的 `approval_responded`。SSE 断线时（`useEventsStatus` 会显示 `offline`）toast 永不消失，用户再点一次 → 后端返回 `already Approved`（`approval.rs:700-702`）→ 弹「Failed to approve: … already Approved」，**看起来像第一次批准失败了**。指摘说的「`toastId` 在 `onClick` 闭包里拿不到」属实：`const toastId = showToast({... onClick ...})` 是自引用，`onClick` 定义时 `toastId` 尚未赋值 | 结构性改动：先把 id 建出来再挂 action（`showToast` 拆成「创建 + 补 action」两步），或给 `useToast` 加一个 `update(id, patch)`。**不能只补一个 `.then`** |
| L2-66 | `src/components/SyncPanel.tsx:121-139`（`save`） | 成立 | `setWebDavSyncConfig` 成功之后还有三步：`setForm(saved)`、清密码、`setStatus(await api.getWebDavSyncStatus())`。**最后一步抛错就会进 catch**，弹「保存失败」的红 toast，而配置其实已经存盘、表单也已更新。用户看到红字后重试保存——重复保存无害，但「保存失败」是假消息 | 把 `getWebDavSyncStatus` 挪出 try，或单独 try 它并降级成 warn。属行为改动（错误分类） |
| L2-67 | `src/components/SFTPPanel.tsx:601-605`（remote→remote 的字节数）、`:669-674`（`setLastResult`） | **部分成立** | `return result.uploaded.bytes \|\| result.downloaded.bytes \|\| f.size` 的第三段是**源文件大小**，不代表目标写入量。但只有在后端把两个 `bytes` 都填 0 时才会走到它——真·空文件时 `f.size` 也是 0，所以不产生假成功。语义上是「用源大小冒充落盘量」，实际触发面窄。指摘的另一半（`sftp_exchange` 的中间态无测试）属实：`grep sftp_exchange src-tauri/tests/` 零命中 | 去掉 `\|\| f.size`，让 0 如实显示 0；或改成 `Math.max(uploaded.bytes, downloaded.bytes)`。属行为改动，且要连带确认后端是否真的可能返回 0（`tauri_commands.rs:655-712`） |

## 2026-11-10 · issue #10 第 5 轮追加（`src/lib/**` 纯函数层 + 收敛方案）

第 5 轮按上一轮的要求把方向换成 `src/lib/**`。判定 **成立 4 / 不成立 2**（另 1 条是 L2-55 的扩面，不另列）。这一轮它第一次给了收敛方案（5 个改动吃掉 20 条），我的取舍见本节末。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-68 | `src/lib/terminal/asciicast.ts:27-43`（本轮在循环上方加了注释）、消费面 `src/components/RecordingsPanel.tsx:106-110`、测试 `src/lib/terminal/asciicast.test.ts:16-30` | **成立**（触发面窄） | 过滤掉非单调事件是对的，但 `time` 仍是录制时的绝对秒数，而播放器的 delay 是「与上一个**存活**事件的差」。被丢事件的 Δ 无人记账 → 第一次时间回退之后的整段回放按比例提前（40 分钟的录像可能提前几分钟结束）。现有测试只断言「`b` 不在」，不断言剩余事件的间隔，所以测不到 | 二选一：①过滤时把被丢事件的 Δ 累加到下一个保留事件；②解析期把 time 重写成单调序列（并把原始 Δ 保留在事件上）。②更彻底且不动消费面。测试要补一条「间隔守恒」断言，否则改法本身也没人守 |
| L2-72 | `src/lib/previewLanguage.ts:54-65`（本轮在表上方加了注释） | **成立但低价值** | `NAME_LANGUAGE` 只有 3 个全名，点文件走 `EXT_LANGUAGE["gitignore"]`（不存在）→ `plaintext`；`archive.tar.gz` 走 `gz`、`Makefile.am` 走 `am`。后果不是崩溃，是预览里没有高亮、用户看不出这是配置文件 | `NAME_LANGUAGE` 补 `gitignore`/`env`/`bashrc`/`npmrc`/`dockerignore`；多后缀在 `languageForFile` 里加一段「从右往左试复合扩展名」。**要不要做是产品取舍**——`plaintext` 不是错误状态，只是缺少信息 |
| L2-74 | `src-tauri/src/approval.rs:36`（声明）、`:874`（填充） | **成立**（与 L2-60 方向相反的同族） | 后端每次审批都查最近 5 条审计历史塞进 `context.recent_commands`，而 `docs/api.yaml` 的 `context` 属性表没有这个键、`src/types.ts` 的 `ApprovalContext` 没有这个字段、`grep recent_commands src/ docs/` 零命中。**产生了却没人声明、没人读**。审批卡片上看不到「这台机器最近跑过什么」，而这恰是审批最需要的信息 | 二选一：①补契约 + `candidateRows` 增一栏（与 L2-60 同批做，一次改完两处漏渲）；②后端停止填充（省一次审计查询）。①的产品价值明显更高，但「5 条够不够、命令要不要脱敏」要拍板 |
| L2-71 | `src/components/SFTPPanel.tsx:207`、`src/components/AuditCharts.tsx:143` | **成立（L2-55 的扩面，非新缺陷）** | 与 L2-55 同源。新增的一层是「L2-55 的改法当初只写了两个函数」，落地时这两处绕过 helper 的本地 `toLocale*` 会漏掉，中文界面里 SFTP 的 mtime 列与审计图横轴仍是英文 | 已并入 L2-55 正文（消费点 9 → 11；改法补「5 个 `toLocale*` 面一次改完」）。不另列条目 |
| L2-69 | `src/lib/terminal/highlight.ts:148-159` | **不成立（提出者自撤）** | 它自己跑了两例，结论是「符合注释写的 earlier-rule-wins，不算缺陷」。核对后同意：`sort((a,b) => a.start - b.start \|\| 0)` 的 `\|\| 0` 是 tie 上的 no-op，靠 `Array#sort` 稳定性保规则序；消解是区间调度，不是「裁掉重叠的尾巴」。**但「earlier-rule-wins」这句注释只对同起点成立**——晚声明但起点更靠左的规则会赢。本轮把注释改成准确表述，并补了 3 条断言（`f50ccd1`） | 无（已作为注释 + 测试修正落地） |
| L2-70 | `src/lib/terminal/highlight.ts:138-140` | **不成立** | `regex.lastIndex++` 按码元推进，理论上能把扫描起点扔进代理对中间（`^`/`\\b` 的判定随之改变）。它自己没构造出可观测的错误匹配，我也没构造出：要命中需「零宽规则命中高位代理」与「另一规则从孤立低位代理起匹配」同时成立。**无判据 = 不认** | 无。若将来真有需要，推进量要改成按码点（`text.codePointAt` 判是否代理对），但那是无收益改动 |
| L2-73 | `src/components/SFTPPanel.tsx:137-161`（`remoteJoin`/`joinForKind`）、`src-tauri/src/core.rs:1964/2108/2531/2681/2714/2747`（`Path::new(&remote_path)`）、`:2798`（`entry_path.file_name()`） | **成立，但机制与它说的不同** | 它说「Windows 上 `PathBuf::from("/tmp/a\\\\b")` 把 `\\` 当分隔符，`sftp.stat`/`mkdir` 被打断」，并自判「给不出判据 → 应算不成立」。**真正机制在 `ssh2-0.9.5/src/util.rs:14-37`**：Windows 版 `path2bytes` 把路径里每个 `\` **改写成 `/`** 再发给服务端（Unix 版直接取 `OsStr` 字节，无损）。`Path::to_str()` 本身无损，所以「Path 切分」不是原因。后果：Windows 客户端上，远程文件名含 `\` 时 `stat`/`rename`/`unlink` 全部打到**另一个路径**。第二处是 `:2798` 的 `file_name()`——远程路径含 `\` 时 Windows 上只取到末段，递归删除会把 `/tmp/b` 当 `/tmp/a\\b` 处理。`keys.rs:47-55` 的注释记录过同族陷阱（它用 `is_safe_filename` 直接拒 `\`） | 属 Windows 语义 → 本批不动。两层面：①ssh2 那层只能靠「远程路径不用 `Path` 承载」绕开（换 `&str` 要 wrap 或 fork，代价大）；②我们这层能立刻收紧的是 `:2798`，用 `readdir` 给的原始路径拼，别过 `file_name()`。**要不要进射程需拍板** |

### 收敛方案：它给的 5 案，我的取舍

| 案 | 它标吃掉 | 我的结论 | 理由 |
|---|---|---|---|
| **A 前端「空值三态」** | L2-60/61/63/64/71/74 | **采纳，作为下一批首选** | 6 条同因（一个 UI 值承担两个含义），全在 `src/**`，`npm test` + `npm run build` 可收口。**但要把 L2-71 摘出去**——它是 locale，不是空值。塞进来会让 A 多一个「产品先定日期跟随谁」的前置条件，反而拖住另外 5 条 |
| **B 错误分类与提示保真** | L2-62/65/66/67 | **拆两笔** | L2-66/67 是局部修复（状态查询挪出 try、去掉 `\|\| f.size`），可并进 A 的窗口；L2-65 是结构性改动（`toastId` 自引用），必须单独一笔且先定 `useToast` 契约；L2-62 要先答「`applyYamlToForm` 失败时给不给提示」 |
| **C 死代码三选一** | L2-58/59/21/47/26 | **只采纳「删 KeysPanel 的 rsa 分支」** | 「一次决策管到底」听着对，但这 5 条生命周期不同：L2-58 是**文案已过期**的死分支（删了没人怀念）；L2-59 的 `folds` 是**写完但没接线**的功能（删掉 517 行测试等于承认那个功能不做）。绑在一个决策上，等于让最简单的那个（删死分支）替最需要判断的那个（折叠做不做）背书 |
| **D 契约单方向漂移** | L2-38/42/48/74 | **部分采纳** | L2-74 并入 A（它本质是一个字段的接线）；L2-38/42/48 是「三份清单没有守卫」，选型（源码扫 / 手写清单 / 属性宏）要一次定，但那是**生产批次**的活，不是本循环能落地的 |
| **E 时间与 locale 一个来源** | L2-55 + L2-71 | **不采纳为本批改动** | 它是唯一一条「产品要决定日期跟随应用语言还是跟随系统」的。这个问题不该由我在循环里替用户答。5 个 `toLocale*` 面已写进 L2-55 正文，等拍板 |

## 2026-11-10 · issue #11 第 1 轮追加（`docs/` 声称 vs 代码行为）

第 1 轮 9 条指摘，核实 **成立 6 / 部分成立 2 / 不成立 1**。文档侧（L1）已全部落 main；只剩下面这一条是行为改动。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-75 | `bin/agent2ssh-daemon.rs:637-654`（同一个 `tokio::spawn` 里先后调 `notify_approval_pending` 与 `fire_webhook`）；`notify.rs:74-150`、`:229-282`（两条路径都各自读订阅列表、各自 POST） | 成立 | 一次 `approval_required` 会**发两条** webhook：`notify_approval_pending` 与 `fire_webhook` 都读同一份 `webhook.toml` 的订阅列表，都对 `approval_required` 判过 `events.contains`，于是两条都发。Slack URL 下是两条不同卡片（审批那条四字段 + 死按钮，通用那条两字段 + `Open Approvals`）；非 Slack URL 下是两条 JSON（一条带 `approval_url`，一条是 `WebhookEvent` 形状）。收 Slack 的人会看到重复告警，收自建端点的人会收到两次 POST——按「一次事件一次投递」写幂等的下游会以为审批被提交了两次 | 二选一：①让 `fire_webhook` 跳过 `approval_required`（审批事件由 `notify_approval_pending` 专管），把 `format_slack_message` 里 `:350` 那段 `approval_required` 分支一并删掉；②反过来，把 `notify_approval_pending` 收敛成「只给 `fire_webhook` 补按钮参数」的一个格式函数，投递只留一处。①改动小但会丢掉通用那条的 `Open Approvals` 按钮（那是**可用**的那个），所以实际推荐②：先让 `format_slack_message` 接 `approval_url`，再删掉 `notify_approval_pending` 的发送。属对外投递次数与载荷形状的双重改动，需拍板 |

### 第 2 轮新增（L2-76）· 2026-11-10

第 2 轮 CodeBuddy 未回帖（等待 5 分钟 + issue 静默 80 分钟），按 issue 正文的 KNOWN-ISSUES 核对点自取靶，核实 **成立 3 组 / 部分成立 1**。文档侧（L1）已全部落 main；下面这条是核 K7/密钥类型时顺带读出来的代码不一致。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-76 | `src-tauri/src/keys.rs:113-124`（`list_keys_core` 的类型判定）对 `:645-654`（`import_key_core` 的类型判定） | 成立 | 两处对**同一把密钥**给出不同类型。`import_key_core` 只看 `.pub` 前缀，有 `ecdsa` 分支；`list_keys_core` **先看文件名子串**（`name.contains("ed25519"/"rsa"/"ecdsa")`），且它的 pub 前缀分支**没有 `ecdsa`**。后果两条：①导入一把 ECDSA 密钥、文件名不含 `ecdsa`（例如 `p256-work`），导入响应说 `ecdsa`、Keys 面板列表说 `unknown`；②一把 ed25519 密钥若文件名含 `ecdsa`（例如 `migrated-ecdsa-backup`），列表把它标成 `ecdsa`。桌面 Keys 页的类型是给用户看「这把是什么」，错标会让人拿错密钥去配主机 | 合成一个分类函数：**先按 `.pub` 前缀判**（补 `ecdsa-sha2-` 分支），前缀认不出才退回文件名子串，最后 `unknown`。`list_keys_core` 与 `import_key_core` 都调它，并补两条断言（`p256_work` → `ecdsa`；`migrated-ecdsa-backup` 但 pub 是 `ssh-ed25519` → `ed25519`）。属 `src-tauri/src/**` 行为改动 / IPC 字符串变化，按 L2 处理 |

## 2026-11-10 · issue #12 第 1 轮追加（测试覆盖缺口）

第 1 轮 9 组指摘，核实 **成立 5 / 部分成立 2 / 不成立 2**（另有 1 条它自撤）。L1 侧已落 main：`api.yaml` 补 98 个 operation 的 401 + 三条 resident-session 的 500 + 两条 400 描述；`daemon_integration.rs` 加「方法一致」与「401 齐备」两条断言、文件头改写；`daemon_resident_session_e2e.rs` 加 open/exec 的 gate 断言。下面三条都是行为或观测面改动。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-77 | `bin/agent2ssh-daemon.rs:1828`（`targets_for_exec_multi`）、`:1833`（`authorize_targets`）、`:1844`（`exec_multi_with_strategy`）；`targets_for_exec_multi` 的兜底在 `:791-812`；`core.rs:826`（`resolve_exec_targets`） | 成立（顺序问题，非状态码问题） | `authorize_targets` 在 `:1833` 就 `?` 了，**早于** `exec_multi_with_strategy`。当 `hosts` 里全是不存在的名字时，`expand_exec_targets` 失败 → `:791-812` 用**客户端自报的原始主机名**兜底 → `authorize_targets` 拿这些没解析过的名字跑 scope 与审批，可能弹一张审批卡；卡片被批掉之后，`resolve_exec_targets` 才在 runner 里以 400 `no hosts matched` 结束——**命令根本不会发出，审批卡与它对应的审计条目却已经产生**。另一半：`hosts=[]` 且 `tags=None` 时 `targets_for_exec_multi` 返回空 vec，`authorize_targets` 走 `:846` 的 `targets.is_empty()` 分支，用 `target_host_label` 的 `"no-targets"`（`:827-828`）作为主机名去 authorize——**一个伪造的主机名会被写进审批与审计**。`core.rs` 的三条测试只覆盖 `exec_multi_core`/`exec_multi_with_strategy`，不碰 handler 这一层 | 把「目标解析」前置到 `:1830` 的 gate 判断之前：解析失败或结果为空直接 400，不再进入 authorize。断言落在真 daemon e2e（`daemon_local_exec_e2e`，`daemon_integration.rs` 没挂 `/exec-multi` 的副本）：`POST /exec-multi {"hosts":["no-such-host"]}` → 400 且 body 含 `no hosts matched`，并断言 `/approvals` 计数不变；再补一条「`hosts` 与 `tags` 都空 → 400 且审计里没有 host 为 `no-targets` 的条目」 |
| L2-78 | `bin/agent2ssh-daemon.rs:3134-3146`（`forward_stop`）、`:3190-3202`（`forward_start`）；`forward.rs:783-790`（`forward_stop_core` 的 already-stopped 短路）、`:795-804`（`forward_start_core` 的 already-active 短路）、`:214-246`（`stop_worker` 的 `StoppingError`） | 成立 | 三种「没真的改变什么」在 HTTP 与审计里同形：①对已在跑的规则再 start、对已停的规则再 stop，`*_core` 直接 `return Ok(())`；②`stop_worker` 里 `disconnect` 失败会把状态置成 `StoppingError`（`forward.rs:241-245`），但 handler 只看 `forward_stop_core(...).is_ok()`，照样回 200 `{ok:true}`；③两者都写 `append_operation_audit(..., Some(0), ...)`，`derive_audit_outcome` 判成 `success`。于是「我刚把它拉起来」与「它本来就在跑」、「隧道真断了」与「disconnect 失败但它还占着端口」在响应与审计里**完全不可区分**，只有 `GET /forwards` 的 `state` 字段能看出真相。`api.yaml` 的 200 只声明 `{ok: boolean}`，`stop` 的 400 描述已写明「对已停的规则再停会成功」——行为本身是设计，缺的是**可观测性** | 让 `forward_stop`/`forward_start` 从 `*_core` 读回终态、200 body 增加 `state`（`RuleState` 的小写形式），契约的 200 补该枚举；`stop_worker` 的 `StoppingError` 一并透出（要么进 body，要么改成 409/503 并写进 responses）。审计的 reason 带上 `was_already_active` / `stop_failed`。属契约 + 行为双改，且要定「停止失败该不该是非 2xx」，需拍板 |
| L2-79 | `bin/agent2ssh_daemon/auth.rs:133-158`（`auth_middleware`） | 成立（纯测试缺口） | `bearer_token(...).or_else(|| accepts_query_token(path).then(|| query_token(uri)).flatten())` 这个**组合**没有任何测试：现有用例只测两个零件（`auth.rs:163-170` 测 `accepts_query_token`、`token_matches` 三条、`public_allowlist_is_exact...` 测 `is_public_path`）。把 `.or_else` 改成 `.and_then` 类型同样成立，8 条 auth 单测**全绿**，但语义变成「必须同时给 header 与 query」，即带 Bearer 的正常请求全部 401。这不是假想：`or_else` 与 `and_then` 在这条表达式里只差一个方法名，而中间件本身是全站唯一入口 | 在 `bin/agent2ssh-daemon.rs` 的测试模块里起一个只挂 `auth_middleware` 的小 router（`AppState` 可由 `with_temp_config` 造），补三条：`?token=` 命中 `/terminal` → 放行且 `AuthContext` 被插入；`?token=` 命中 `/hosts` → 401；只有 header 时 `?token=` 被忽略。**为什么按 L2 登记**：`auth_middleware` 是 `pub(super)`，集成测试够不到，这条测试只能写在 `src-tauri/src/bin/**` 内；本轮的 L1 边界是「非平台相关测试断言强度」，新增测试模块不在其内，需确认后再落 |

### L2-78 的加重（第 2 轮复核补充，2026-11-10）

`forward_stop_core`（`forward.rs:783-790`）的短路条件是 `state == Stopped || state == StoppingError`，而 `stop_worker`（`forward.rs:241-245`）在 `disconnect` 失败时正是把状态置成 `StoppingError`。两者合起来的后果比原描述更具体：**一条 `StoppingError` 的规则再调 `stop` 会永远短路成功，`disconnect` 永远不会被重试。** 用户在 `GET /forwards` 里看到 `StoppingError`、再点一次「停止」想清理干净，拿到的是 `{ok:true}` 与一条 `success` 审计，而端口可能仍被占着。

`forward_start_core` 的守卫是 `Active || Starting`（`forward.rs:802-804`），`StoppingError` 不在内，所以 start 能救回来——不是死锁，是「stop 永不重试且每次都报成功」。修法同 L2-78 主线（200 body 带 `state`），另加一条单测：造一个 `StoppingError` 状态的 handle → `forward_stop_core` 返回 `Ok` → 断言 `control.state` 仍是 `StoppingError`，证明它没有重试却报了成功。

## 2026-11-10 · issue #12 第 4 轮追加

第 4 轮 14 项指摘，核实 **成立 8 / 部分成立 3 / 不成立 3**（含两处自撤与一处对我有利的更正）。文档侧（L1）已落 main（`7dddb51`）；下面两条是行为或契约语义改动，需拍板。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-80 | `bin/agent2ssh_daemon/auth.rs:40-62`（`authenticate_token` 的 `load_scoped_daemon_tokens().map_err(…INTERNAL_SERVER_ERROR…)`）、`src/remote.rs:203-215`（`load_scoped_daemon_tokens`，`validate_scoped_daemon_tokens(&file.tokens)?` 在 `:211`） | 成立（行为，且零测试覆盖） | `daemon_tokens.toml` 只要解析不过或校验不过，**任何非 admin token 的请求都拿到 500 而不是 401**：调用方分不清「我的 token 错了」与「服务器坏了」，而**唯一能修这个文件的人（拿 admin token 的）永远看不到这个失败**，因为 admin 分支在 `load_scoped_daemon_tokens` 之前就 `return Ok`。契约已按现状记录（`info.description` 一段 + `/approvals/{id}/approve` 的 500 长文），`grep -rn "failed to load scoped daemon tokens"` 全仓只有 `auth.rs:54` 一处，**没有任何测试断言过这条路径** | 三个方向，需选一个：①保持 500 但补测试（在 `bin/agent2ssh-daemon.rs` 的测试模块里写坏 `daemon_tokens.toml`，断言非 admin token 得 500、admin token 仍 200）；②降级成 401 + `tracing::error!`（安全上更保守，但会让配置损坏彻底不可见，除非有人看日志）；③改成启动时校验一次、坏掉即拒绝启动（`daemon_tokens.toml` 是启动依赖，不是每请求依赖）。我倾向 ①+③ 的组合：启动时校验并大声报错，请求期保持 500，另补那条测试。**这条的真正问题不是状态码，是「坏配置对所有能修它的人不可见」** |
| L2-81 | `bin/agent2ssh-daemon.rs:4450-4462`（`terminal_broadcast_run` 里 `reject_if_rate_limited` 失败后仍 `return Ok(Json(…))`）、`:4485-4489`（每目标的 `Err((_, Json(error)))`）、`:4492-4512`（`all_or_nothing` 的收尾也 `return Ok(Json(…))`） | 成立（契约语义，非 bug） | 三处授权/限流失败**全部返回 200**：每目标的授权失败只写进 body 的 `authorized: false` + `error`；`reject_if_rate_limited` 的 429 被丢弃、改成把每个目标标成未授权；`all_or_nothing: true` 且**全部目标都被拒**时仍是 200。本轮已在 `/terminal/broadcast` 的 operation 描述与 200 说明里写清「per-target 拒绝不会以 4xx 出现」，所以契约不再是错的——但「全部被拒该不该是 403」是个没定过的产品问题：现在下游只能靠解析 body 判断成败，一个只检查 `res.ok` 的客户端会把「一条都没发出去」当成成功 | 定一条规则并落到契约 + 一条 e2e：若「全部目标被拒且 `all_or_nothing: true`」判 403（推荐，因为此路径下没有任何输入被发送，与「部分成功」性质不同），则 `terminal_broadcast_run` 在 `results.iter().all(\|r\| !r.authorized)` 时改返回 403 且 body 仍是逐目标数组；若维持 200，则至少在 200 的 schema 里加一个 `any_authorized` 布尔，让「只看状态码」的客户端有一个可靠的判据。属 `src-tauri/src/**` 行为改动 + 契约双改，需拍板 |

## 2026-11-10 · issue #12 第 5 轮追加

第 5 轮指摘来源是**自取**（CodeBuddy 在 20:19 回帖后 5 分钟内无回复，按协议标「等待超时」后从第 4 轮回帖里自己点名的方向继续）。核出两处行为问题，都与 `Json` 提取器的用法有关；文档侧（L1）分两笔落 main：`6fff9f1`（47 个 operation 补 415/422 + 派生式断言）、`c1ecc72`（把 415/422 的响应体从 `Error` 改成 `text/plain`）。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-82 | `dry_run_playbook_handler`、`post_health_snapshot` 的 `Option<Json<T>>` | 部分解决 | `/shutdown` 已区分空 body 与无效非空 body：后者返回 400，真实 daemon 回归覆盖格式错误、错误类型和 content-type。其余两处仍把解析失败视为无 body：dry-run 可能跳过 playbook 一致性检查，health-snapshot 可能退化为全部主机。 | 对剩余两处决定是否拒绝无效非空 body，修改时同步各自 API 契约和回归测试；本次未改变其行为。 |
| L2-83 | `docs/api.yaml` 的 47 个 `Json<T>` operation（415/422 已按 `text/plain` 记录）；源码侧无处可改，因为拒绝由 axum 生成 | 成立（契约语义，需选方案） | 提取器的拒绝响应是 `(StatusCode, String).into_response()`（`axum-core-0.4.5/src/macros.rs:50-59`），即 `text/plain; charset=utf-8` + 一行消息，**不是** daemon 自己的 `Error` 对象。两份后果：①一个把每个 4xx 都按 `Error` 解析的客户端在 415/422 上会 parse 失败；②更麻烦的是 `JsonSyntaxError` → **400**，而 operation 的 400 本来就存在（handler 自己的），于是「body 不是 JSON」与「handler 拒绝了 body 的内容」在状态码上不可区分，body 体裁还不同。文档侧只能如实记录（已记录），要统一必须动代码 | 三选一：①（推荐）加一个项目内 `ApiJson<T>` 提取器，`Rejection = (StatusCode, Json<ErrorBody>)`，内部 `From<JsonRejection>` 把三种拒绝映射成 `Error`，47 个签名机械替换，并定一条规则：语法错继续 400 还是改 422（改 422 才能与 handler 的 400 分开，但会让所有调用方看到新码）；②47 个 handler 各自收 `Result<Json<T>, JsonRejection>`，把拒绝转成 `err(...)`——改动更大但没有新类型；③什么都不做，只在 `info.description` 里永久保留这段说明（当前状态）。①+「语法错改 422」最能消掉歧义，代价是公开状态码变化，需拍板 |

## 2026-11-10 · batch 2（issue #13）第 1 轮追加

第 1 轮 8 条指摘，核实 **成立 2 / 部分成立 3 / 不成立 3**。L1 侧本轮落 main 的是 `docs/api.yaml` 两段 WebSocket 描述（`/terminal` 的 500 ms 是空闲超时、`/exec/stream` 的 drain 不发 `exit` 帧）与一条把「预算声明」钉到「循环形状」上的断言（`a_websocket_drain_budget_claim_matches_the_loop_it_describes`）。下面三条要动 `src-tauri/src/**`。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-84 | `bin/agent2ssh-daemon.rs:4830`（`tokio::time::timeout(Duration::from_millis(500), event_rx.recv())`，**每次迭代新建**）、`:4783`（`event_task` 每 100 ms 从阻塞通道泵一次）、`:5014`（`run_ws_with_shutdown_deadline(..., 2s)`）；对照 `:5340`（exec 侧 `timeout_at(deadline, …)`，deadline 在取消分支里算一次）、`:5442` | 成立（行为；两处同数不同义） | 终端的 500 ms 是**空闲超时**：只要 500 ms 内有事件就继续转，`Output` 分支只 `continue` 从不 `break`。所以一个持续输出的终端在停机期间不被 500 ms 约束，只能等 `:5014` 的 2 s 把 future drop 掉。改法是动 `src-tauri/src/**` 的循环语义，且会改变客户端在停机期间能收到多少输出（可能少收），属对外行为 | 二选一：①（推荐）把终端 drain 改成 `timeout_at(Instant::now() + 500ms, recv)`，与 exec 一致——同一个 `POST /shutdown` 描述里「up to two seconds for each connection」与 per-route 的「500 ms」只有在①下才同时成立；②保留空闲超时，只维护文档（本轮 L1 已按②落地）。改①时必须同步改 `docs/api.yaml` 的 `/terminal` 描述，`a_websocket_drain_budget_claim_matches_the_loop_it_describes` 会在漏改时变红并点名。回归面：`daemon_stop_e2e.rs` 的 `stop_exits_when_exec_websocket_peer_stops_reading` 与终端相关两条 |
| L2-85 | `daemon_control.rs:986-989`（`UnverifiedIdentity` 的文案）、分支入口 `:982`（`health.as_ref().map(\|i\| i.pid) == Some(pid)` 的 `else`） | 成立（诊断文案） | 文案括号里列了三种原因（health PID / config directory / admin token），但这一支**只可能**是「没有 `/health` 答复」或「`/health` 报了另一个 PID」：`config_dir` 的比对在 `:959-981` 的 `if` 分支里，`admin token` 的比对在 `:991` 之后。打印这句话时，括号里后两种原因都不可能成立，运维会去查两个没参与判定的东西 | 括号收窄成「no `/health` answer, or it named another PID」。**没有测试钉这条分支**：`daemon_stop_e2e.rs:872-877` 只断言 `contains("could not be authenticated")`（首句），加一句「不含 `config directory`、不含 `admin token`」就能把括号钉住。属 `src-tauri/src/**` 文案 + 测试断言 |
| L2-86 | `daemon_activity.rs:205-217`（`fetch_daemon_activity` 只串行读 `/sessions`、`/resident-sessions`、`/forwards`、`/approvals`）、`:180-195`（`summarize_activity` 折四个计数）、`tauri_commands.rs:2016-2049`（`confirm_quit` 以 `is_idle()` 决定弹不弹框） | 部分成立（产品口径） | 一次在途的 `/exec` **不在任何列表里**：`exec`（`bin/agent2ssh-daemon.rs:1665`）不注册 registry，`resident_session.rs:361-367` 的摘要只有 `id/host/label/idle_secs/dead`。所以「有命令在跑」的 daemon 被 `is_idle()` 判成空闲，退出确认框不弹，`shutdown` 直接中断那条命令。要不要把 `/exec` 纳入退出确认是产品决策（`/exec` 自带 `timeout_secs` 上界），且要动 `src-tauri/src/**` | 若要做：daemon 侧先有一条可读的在途视图（在 `exec` 与 `exec_stream` 入口 `register` 一个 owner，或在 `AppState` 里加一个 `AtomicUsize` 计数器 + `GET /exec/activity`），再让 `fetch_daemon_activity` 读它、`DaemonActivity` 加 `running_execs` 字段（`Serialize` 变化会牵动桌面契约与 `quit_confirm_labels` 的文案键）。若不做：至少在 `docs/architecture.md:191` 那段「问 daemon 会丢什么」后面注明 `/exec` 不计入，免得读文档的人以为它是全量 |

**L2-84 的验证前置（batch 14 第 1 轮补）**：上面第 ① 与 ② 两个方案落哪个都**测不出来**，因为 `/terminal` 的 drain 循环只有「静态停机」覆盖：`git grep -c Output src-tauri/tests/daemon_stop_e2e.rs` 为 **0**，33 条用例里没有一条在停机期间持续写终端输出。而 500 ms 空闲超时与 2 s future 上限的分歧**只在这一支上才可见**（静态流量下两者都远小于 2 s，跑不出差异）。所以顺序必须是：先补一条「对端持续输出 + 停机」的 e2e（断言退出耗时 ≤ 2 s + 余量且收到 `Close`），再决定循环语义。该用例不需要真 SSH——`daemon_stop_e2e.rs` 的 `AGENT2SSH_TEST_SSH_PORT` 脚手架用不上，只需一个持续吐字节的本地 mock。

行号按当前树更正（`380e166` 之后整体下移约 36 行）：`timeout(500ms, …)` 在 `:4866-4869`，`Output` 分支在 `:4871`，terminal 的 `run_ws_with_shutdown_deadline(` 起于 `:4855` 而其 grace 参数在 `:5057`（`from_secs(2)`，闭包结束后才传，所以两行相隔 200 行不是笔误），exec 侧 `let mut code = None` 在 `:5363`、其 grace 在 `:5485`。

**第 2 轮复核的回应**：复核方主张「`/local-exec` 够用，`/terminal` 的 WS 路径修不了这条」，理由是 `/local-exec` 在 `:1813` 调同一个 `preflight_guarded_request`、且认领 `/terminal` 会让 `/sessions` 非空从而污染 `confirm_quit` 的回归面。**后半接受、前半不接受**：

- **接受**：认领一次 `/terminal` 会让 `:4842` 注册、`:1055` 占额，`confirm_quit` 的「有活跃会话」分支随之改变——这条 e2e 确实会污染退出流程的回归面，应放进独立测试文件或在用例里显式标注前提。
- **不接受**：`/local-exec` 覆盖不了这个缺口。它走 `run_local_command_capture`（`local_exec.rs`），**没有 WS drain 循环**；而 L2-84 的分歧点正是 `/terminal` 的 `while let Ok(Some(event)) = timeout(500ms, recv())`（`:4866-4869`）与 `run_ws_with_shutdown_deadline` 的 2 s grace（`:5057`）之间。前者的行为只有「已升级的 WS 上持续有 `TerminalEvent::Output`」时才可观测，`/local-exec` 连这个循环都不进。**这条用例必须建 WS**，不能拿 `/local-exec` 代替。
- **它附带的两条发现仍然有用**（落地时会踩）：① `shutdown_reaps_a_running_local_command_and_its_child`（`daemon_stop_e2e.rs:556`）是「本地命令 + 停机」的现成模板，连接与断言骨架可复用（但断言对象不同——它断 PID 被杀，不是 drain 语义）；② daemon 限流默认开启（`limits.rs` 的 `default_true`、`window_secs = 60`、`default_source_per_minute = 30`），同 source 一分钟内超 30 次请求会拿 429，新用例要么把 `execution_limits.toml` 写进 `extra_files`（`start_with_name` 支持），要么留足间隔。

### L2-4 的行号与触发条件更正（本轮复核）

`L2-4` 记的两件事仍然成立，但位置和措辞要按当前树更正：`NotAuthenticated` 的唯一触发点是 `daemon_control.rs:991` 的 `crate::daemon_activity::read_admin_token().is_none()`（**一个请求都没发**），不是 `:986-988`；`Display` 在 `:792-796`。所以它说「the daemon that answered did not accept this config's admin token」时，事实上既没有 daemon 答话、也没有 token 被拒。

**入口清单（第 2 轮补，用以定位落地用例）**：`:991` 在 `:996` 的 `request_shutdown(Some(pid))` 之前，而 `request_shutdown` 自己的 `:846-849` 守卫只有两个入口——`:932` 的 `request_graceful_shutdown()`（传 `None`，无 pid 的旧调用方）与「`:991` 读到 token 之后、`:846` 再读之前文件被删」的竞态（`read_admin_token` 每次重读文件，`daemon_activity.rs:145`）。所以「删 `daemon.token` → `stop_daemon_process`」落在 `:991`，得到 `NotAuthenticated`；而「token 在 daemon 起来之后被换掉」落在 `request_shutdown` 的 401 支，得到 `NotAccepted`（文案反而准确）。**两个变体都偏、偏法相反**，这正是 `L2-4` 要修的那一件事。

`tests/daemon_stop_e2e.rs:772-788` 的 `||` 正是它查不出来的原因：`wrong_admin_token_never_signals_the_real_daemon` 造的是「token 文件被换成错值」，走的是 `request_shutdown` 的 401 支（`NotAccepted`，文案 `did not accept this config's admin token for a shutdown`），**根本到不了** `:991`。两条文案都含 `did not accept this config's admin token`，`||` 两边都能过。落地时要拆成两条用例：删 `daemon.token` → `NotAuthenticated`（文案应说「本配置没有 admin token」）；写错 token → `NotAccepted`（保留现文案）。

### L2-3 的落地形状补一步：`Accepted` 要能带 pid

`L2-3` 那段（`### L2-3 的落地形状：加字段，不换状态码`）只写了「客户端侧 `:887` 改为按 body 里的 `checked_pid` 与 `expected_pid` 比较分叉」，漏了类型这一层：`:887` 返回的 `ShutdownRequest::AcceptedUnchecked` **不带载荷**（`daemon_control.rs:827-843`，`Accepted` 也不带），要比较就得把该臂改成 `AcceptedUnchecked { checked_pid: Option<u32> }`（或新加一臂），并让 `request_shutdown` 的 200 分支先解析 body 再分叉——`reported_pid`（`:910`）解析的是 409 的错误文案，不能复用。落地时按「变体带字段 → 200 分支解析 → 比较 → `:1006` 的 warn 改文案」四步做，缺任何一步都会静默退回原形。

## 2026-11-10 · batch 2（issue #13）第 2 轮追加

第 2 轮是 CodeBuddy 对我上轮四个复核问题的答复，顺带带出两条代码侧问题。逐条核实 **成立 3 / 部分成立 1 / 不成立 2**（其中「不成立」两条都是它对我上轮判断的反驳，见下）。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-87 | `bin/agent2ssh-daemon.rs:5365`（取消分支先发 `Message::Close(None)`）、`:5490-5495`（循环结束后**无条件**再发 `{"type":"exit",…}`，外面套 500 ms timeout）；`grep -n '"type":"exit"'` 全仓只有 `:5493` 一处 | 成立（协议层，不是文案） | RFC 6455 §5.5.1：端点发出 Close 之后**不得**再发数据帧。停机路径上 handler 先 Close、再 break 出 `select!`，然后在 `:5490` 发 exit——客户端按规范应忽略，服务端也可能写进一个已关闭的 socket。后果：客户端拿不到退出码，而「服务端发了但被协议吞了」与「服务端没发」在客户端一侧不可区分；审计里 `exit_code` 仍有值（drain 读到 `Exit` 时），两份记录于是不一致 | 三选一：①（推荐）把 exit 帧移进循环：正常路径在 `Exit` 事件上先发帧再 break，取消路径在 drain 里读到 `Exit` 时先发帧、再 Close；②停机路径干脆不发 exit（契约本轮已写「treat the close as final」），只保留审计；③保留现状。①改客户端可见顺序，需拍板。配一条 e2e：`/exec/stream` 收到 `exit` 后立即 `/shutdown`，断言「close 之后没有 text 帧」 |
| L2-88 | `bin/agent2ssh-daemon.rs:5153-5155`（`session.set_blocking(true); let _ = channel.wait_close(); let code = channel.exit_status().ok();`）；对照 `embedded_ssh.rs:1505-1510`（库里同一件事：`set_timeout(TEARDOWN_TIMEOUT_MS)` + `wait_close_cancellable(&\|\| token.is_cancelled_now())`）、`embedded_ssh.rs:1620`（`session_timeout_ms`） | 成立（资源生命周期，严重度中） | handler 结束时只 drop 了 `stream_rx`，worker 线程要等下一次 `blocking_send` 失败才退（`:5100/:5109/:5122`）——但它在 eof 之后还要走 `:5153-5155`：`set_blocking(true)` 之后 `wait_close()` **没有取消入口**（`channel.rs:473` 传 `None`，而 `:478` 就有可取消变体），上界只剩 `session_timeout_ms(timeout_secs)`（默认 60 s，且 `ExecRequest.timeout_secs` 是 `Option<u64>` 无上界）。所以一次 `/exec` 结束后线程最长还能活 `timeout_secs`，且不响应停机。**库里那段已经把两件事都做对了**（teardown 单独 30 s 且不派生自 `timeout_secs`，并且可取消），daemon 这份是复刻时漏的 | 照 `embedded_ssh.rs:1505-1510` 改：`set_timeout(TEARDOWN_TIMEOUT_MS)` + 把 shutdown token 传进线程闭包并用 `wait_close_cancellable`。断言：停机时进程退出耗时不得随 `timeout_secs` 变长（现有 `stop_exits_when_exec_websocket_peer_stops_reading` 可加一条耗时上界） |

### 第 2 轮里两条「不成立」的反驳（原文保留，请勿重复提）

1. **「`handler_body_of(daemon, "exec_stream")` 拿到的是整个文件（形参括号让 `opening_brace` 的 depth 停在 1），所以新断言会静默变松」— 不成立。**
   实测（临时探针，跑完即删）：`exec_stream` 的 body **13 702 B**、`handle_terminal` **14 261 B**，而源文件 **235 983 B**；两者都不含 `\nasync fn `。原因：`opening_brace` 从 `async fn exec_stream(` 起扫，形参表的 `(` 加 1、其 `)` 减 1，回到 0 之后才在 `-> impl IntoResponse {` 上命中。**不过它指出的失效形态值得防**：本轮已在新断言里加一条守卫（body 内不得出现另一个 `\nasync fn `），并做了破坏性验证——把 `handler_body_of` 临时改成 `&rest[..]`，断言立刻点名 `exec_stream`。

## 2026-11-10 · batch 2（issue #13）第 3 轮追加（自取）

CodeBuddy 在第 2 轮回帖后 5 分钟（21:53–21:58，60 s × 5 轮询）无回复，按协议自取上轮公布的两个靶（`wait_for_exit` / `signal_daemon`）+ 自验上轮两笔 L1。**自验两条通过**（`/exec/stream` 的「sends its one exit after the loop」在非停机路径同样成立——`:5450` 的 `Exit` 臂先 `break`、`:5493` 再发帧，该路径全程无 `Message::Close`；`/terminal` 的 send 失败只结束 drain，`:4851` 之后还有 `:4859` 的 Close 与 `:4860` 的 break，清理照跑）。**新发现两条代码侧问题**：

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-89 | `daemon_control.rs:729-758`（`signal_daemon(pid, start_time, force)`）、唯一调用点 `:1030`（`force = true`）；`grep -rn "signal_daemon("` 全仓 1 个调用 | 成立（死分支） | `force == false` 的 `kill_with(Signal::Term)`（`:750`）与它的 `unwrap_or_else(|| process.kill())`（`:751`）永远不会执行——也就是说「先 SIGTERM、不退出再强杀」的策略在代码里**不存在**，只有一个恒真参数让人以为存在。它已经骗到过一次文档（本轮 `docs/architecture.md:191` 原文就写「`stop_daemon_process` sends `SIGTERM`」，已改） | 二选一：①删掉 `force` 参数与 `kill_with(Signal::Term)` 分支，函数改名 `force_kill_daemon`，把「停止路径只做强杀」写成事实；②接上 SIGTERM-first：`wait_for_exit` 超时后先 `signal_daemon(force=false)`、再等 2 s、仍 `Running` 才强杀。②更贴合「优雅优先」，但一次停止最坏多等 2 s，且 Windows 上 `kill_with(Term)` 返回 `None` 会落到 `process.kill()`（`TerminateProcess`），语义要写进契约。属 `src-tauri/src/**` 行为改动 |
| L2-90 | `daemon_control.rs:996-1016`（`NotAccepted(detail) => return Err(...)`）、`:1019-1035`（只有 `Accepted*` 之后才可能进强杀分支） | 部分成立（口径待定） | `/shutdown` 请求**本身失败**时（连接被拒、读超时、401、409），停止立即以错误结束，**不会**退回强杀——即使身份已在 `:958-990` 核过（pid 与 config_dir 都对）。所以「HTTP 监听卡死但进程还活着」这一种情况，`agent2ssh daemon stop` 只能报错，用户得自己 `kill -9`。契约原文（`api.yaml` 的 `/shutdown`）读起来像无条件两步，本轮已改成「only once a daemon has accepted it」并写明 401/409/连接被拒都不发信号（L1 已落）。**剩下的问题是要不要对传输层失败强杀** | 先定口径：①维持现状（拒绝里含 401/409，强杀会把「发错对象」变成「杀掉对的进程」），只补 `NotAccepted` 的文案给出下一步（`kill <pid>` / `taskkill /PID <pid> /F`）；②把 `NotAccepted` 拆成 `Unreachable`（超时、连接重置）与 `Refused`（4xx），只对 `Unreachable` 且身份已核过的情况允许一次强杀。推荐 ①——身份核过不等于此刻仍是同一个进程（K14 的窗口）。属 `src-tauri/src/**` 行为 + 文案 |

### 第 5 轮（CodeBuddy 复核后）：三分表少了一类 429，以及一条过期的行号引用

**1. `api.yaml` 的 `/shutdown` 三分表漏了第 4 类：`429`（限流）。**
`stop_daemon_process` → `request_shutdown` → `local_daemon_request*` 发的是一个**不带身份字段**的裸 HTTP 请求（`daemon_activity.rs`：hand-rolled，只有 `Authorization`、`Content-Type`、`Content-Length`），所以 middleware 派生的 source 是 `source_from_env("daemon")`，即 `AGENT2SSH_SOURCE` 或字面量 `"daemon"`；host 名从 body 的 `expect_pid` 派生（`local:vs`、`local:wsl:Ubuntu` 这类**带冒号**的名字很常见），`host_tags("local:vs")` 返回空。`reject_if_rate_limited`（`:980`）在 handler 之前跑，用的是**默认开启**的 `ExecutionLimitConfig`（`enabled: true`，`default_host_per_minute: 20`，`window_secs: 60`）。于是：同一 source 或同一 host 名一分钟内第 21 次，`/shutdown` 返回 `429`，`request_shutdown` 落到兜底臂 `NotAccepted("the daemon answered the shutdown request with 429: …")`——「这条路答了，但不是 409、不是 401/403、更不是从未建连」。按 `limits.rs::check_event_limits` 逐字复算 21 次：第 21 次 `host:local:vs rate current=20 limit=20`。**已改 L1**：三分表改成四类，并写明「按分钟、默认限额、可被 `execution_limits.toml` 关掉或改宽」，免得读者以为它不可配置。

**2. 「行号引用」扫法的结果：`src-tauri/src/**` 只有一处过期 —— `transport/russh_impl/agent.rs` 的 `auth.rs:180`。**
扫法：`grep -rnE '(:|`) ?[0-9]{3,5}' src-tauri/src` 得 80 处候选，扣掉端口（`127.0.0.1:7722`）、`on`-位掩码、`duration_ms: 100`、RFC/OSC 编号、JSON 字面量、`0600` 这类权限后，真正「指向别处某一行」的只有三处：`daemon_control.rs` 的 `GRACEFUL_STOP_WAIT` 三个（`:5014`/`:5442`/`:5951`，上一轮已核，仍对）、`redaction.rs` 的 `db/highlight.rs:221`（跨仓库引用 `rssh`，本仓核不了）、这一处。**`agent.rs:18` 原写「the implementation exists at `auth.rs:180`」，不成立**：`russh-0.60.3/src/auth.rs:180` 是 `AgentIdentity::Certificate { .. } => {` 这条 match 臂，**不是** `impl`；`grep -rn "Signer for" src-tauri/src` 只有 `russh_impl/mod.rs:137` 的 `LocalKeySigner`，本仓**没有** `impl Signer for AgentClient`——它在 `russh-keys` 里，本仓只是消费者。结论（不需要包装层）仍然对，错的是「本仓存在那个 impl」与那行号。已删掉行号与「the implementation exists」；`docs/plans/russh-migration.md:472-479` 有同一句，**不改**（它是历史迁移记录）。

**3. 顺带（不新增编号）：** 第 1 条说明 `/shutdown` 也是限流桶的成员。`GET /health` 是公开路径、不过 middleware，所以 pre-flight **看不到**限流；只有 `POST /shutdown`（和 CLI 的 `/activity` 快照）会撞。要彻底排除，可把它们从限流中豁免，那是行为改动，留给下一批决定。

### 第 4 轮：L2-89 / L2-90 的选项收窄（CodeBuddy 答复后）

**L2-89 选 ①（删 `force` 参数与 `kill_with(Signal::Term)` 分支），理由已核实**：
- ② 在 Windows 上**等于 ①**——`kill_with(Term)` 在 sysinfo 里对非 Unix 返回 `None`，落回 `process.kill()`（`TerminateProcess`），所以「SIGTERM-first」在 Windows 上不产生任何新行为；
- 在 Unix 上它只是多一次**可被忽略**的信号，且这次尝试发生在一个刚刚拒绝了 HTTP 的进程上；
- 真正的 K14 收窄不在信号，而在 `request_shutdown` 的 409（daemon 侧一条原子比较 `expected != own_pid`）。`stop_daemon_process` 里 `:950` 取启动时间、`:958` 读 `/health`、`:991` 读 token 是**三次独立读**，中间没有任何东西保证还是同一个进程——这也是 `force` 删掉之后仍然存在的窗口（K14 已记）。

**L2-90 维持「不退回强杀」，但理由换成两条更硬的**（原写「身份核过 ≠ 此刻是同一个进程」不足以拒绝，因为 `signal_daemon` 自己会再核 `start_time`）：
1. **可诊断性**：退回强杀会把「`/shutdown` 没答上」这个明确事实降级成「stop 成功了」，与 L2-1 那句「leaving it running」之后立刻开杀是同一类反话。
2. **拒绝里合法混着 401/409**：`NotAccepted` 同时装「发错对象」与「发不出去」；要退必须先拆成 `Unreachable` / `Refused`，而 `Refused` 那半仍不能退。

**反例照实记下**：唯一值得退的是「daemon 活着但 HTTP 监听卡死」——`:958` 的 `/health` 在 ≤2 s 前刚答过（它是同一轮里更早的一次请求），`signal_daemon` 又会用 `start_time` 挡住复用，所以技术上安全。若将来要做，须同时满足三条：`Unreachable` 分支 + 身份已核过 + `signal_daemon` 复核通过。

## 2026-11-10 · batch 2（issue #13）第 5 轮追加

第 5 轮是对第 3 轮四问的复核（`2120156142427869184`），逐条核实 **成立 2 / 部分成立 2 / 不成立 2**（其中 1 条它主动撤回、1 条它自己改名）。新发现一条代码缺陷：

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-91 | `bin/agent2ssh-daemon.rs:4849-4853`（drain 里 `begin_send()` → `send()` 失败 → `break`，**没有 `end_send()`**）对照 `:4893-4898`（正常转发的两条路径都 `end_send`） | 成立（资源计数泄漏） | 停机时对端已关，drain 里第一帧 send 失败就 `break`，`drain_handle` 的 pending 计数**留在 1**；`DrainRegistry::drain_all` 只看 `pending == 0`，于是它要等满 deadline（`post_shutdown` 的 4 s registry 等待正好在这条路径上），而 `handle_terminal` 自己的清理照跑。只在「停机 + 对端已断开」这个交叉点出现，现有 `daemon_stop_e2e` 的背压用例覆盖的是 exec 那条，不覆盖 terminal | 失败分支补一次 `end_send()` 再 `break`（与 `:4894-4897` 同形），或把 `end_send` 做成 guard 让两条路径自动配对。属 `src-tauri/src/**` 行为修复，配一条 e2e：停机时对端断开 → 断言 registry 在 100 ms 内空而不是等 4 s |

**L2-87 补一条（同轮）**：`exec_stream` 有 **8 个**「命令还没开始就返回」的路径（`:5198` `:5210` `:5221` `:5230` `:5242` `:5275` `:5285` `:5300`：body 不是 ExecRequest、source 非法、命令超长、gate 暂停、限流、授权失败、缺 `force`、未知主机），每条都只发一个 `{"type":"error"}` 就 `return`，**一条 exit 帧都不发**。所以 L2-87 的 ①（把 exit 移进循环）若只改停机路径，这 8 条仍然没有 exit；做 ① 时要顺带定「拒绝要不要有 exit 帧」——本轮已按「没有」写进契约（`api.yaml` 的 `/exec/stream` 新增一句），代码侧维持现状即可。


2. **「本地没有 `daemon.token` 时 `stop_daemon_process` 根本到不了 `:991`，会停在 `request_shutdown` 自己的守卫 `:846-849` 上，所以我给 L2-4 的拆分方案会红」— 不成立。**
   `:991` 在 `:996` 的 `request_shutdown(Some(pid))` **之前**执行（同一函数、语句顺序，`:942` 起）。`request_shutdown` 的守卫只有两个入口：`:932` 的 `request_graceful_shutdown()`（传 `None`，无 pid 的旧调用方）与「`:991` 读到 token、`:846` 再读时文件已被删」的竞态窗口（`read_admin_token` 每次重读文件，`daemon_activity.rs:145`）。所以删掉 `daemon.token` 调 `stop_daemon_process` 确实停在 `:991`，得到 `NotAuthenticated`——而它说「daemon 不接受这个 token」时一个请求都没发出去。**它那张表的方向是对的**（两个变体都偏、偏法相反），只是把「哪条路径到哪一支」接反了；`L2-4` 的备注按上句补了入口清单与竞态说明。

## 2026-11-10 · 手工/单发消费（issue #2）

对 CodeBuddy 复核 `2119861268247908352` 的核实结果中，需要行为改动或产品口径的三条。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-92 | `daemon_activity.rs` 的 `request_at`：`:103` `to_socket_addrs()?`、`:106` `connect_timeout`、`:107-108` 两个 per-syscall 超时、`:124` `write_all`、`:127` `read_to_end` | 部分成立 | 「没有总时限」属实，但承重的那一半是**地址解析**：`to_socket_addrs` 走系统解析器，不吃 `timeout`；`AGENT2SSH_DAEMON_ADDR` 允许主机名（`remote.rs:34` 只把通配符映回 loopback），所以停止线程可以被 DNS 挂住。它给的另一半机制不成立：`/shutdown` 的 handler 是 `Option<Json<ShutdownRequest>>`，axum 在 `require_admin` **之前**就抽完 body，所以「服务端不读 body、客户端阻塞在 4 KiB 写」在这条路径上不会发生；那条 body 实际是 19 字节的 `{"expect_pid":N}`。`read_to_end` / `write_all` 的「对手逐字节滴灌就能越过单个超时」是可构造的，但需要一个病态对端 | 给 `request_at` 一个总 deadline（`Instant + remaining()` 逐次收窄两个 socket 超时），并把解析交给带超时的解析路径（或先试 `SocketAddr::from_str`，失败再解析）。属 `src-tauri/src/**` 行为改动 |
| L2-93 | `bin/agent2ssh-daemon.rs:655-671`（`shutdown.cancelled()` 分支） | 成立（竞态窗口） | `approval_respond(id, false)` 的返回值只用于 `:668` 的日志字段（`"rejected": result.is_ok()`），不参与分支。**窗口比本行原稿写的宽**——`approval_respond_inner`（`approval.rs:678-725`）返 `Err` 有三种原因，都不需要竞态：①`unknown approval: {id}`（内存表里没有，例如 daemon 重启过）；②`has timed out`（TTL，`approval_poll` 可能已把它置 `timed_out`）；③`already {:?}`（已非 `pending`，含已被别人批过）。更糟的是第四种：`append_approval`（`:486-503`）把写盘错误吞进一条 `warn` 诊断，`approval_respond_inner` 仍返 `Ok(())`——于是 `:668` 写 `rejected: true`、`:662` 的 `audit.jsonl` 写 `daemon shutting down; approval wait cancelled`，而 `approvals.jsonl` 末条还停在 `pending`，**没有任何一方说它被回答过**。所以判据不能是 `is_ok()`，必须看 `result` 的**错误原因** | 二选一：①把错误原因接进审计文案（`is_ok()` 时写 cancelled；`Err` 时按三类分别写「已超时」「已被回答」「记录不存在」，写盘失败那条要单独可见）；②维持两笔记录，但在 `api.yaml` / 指南里写明「停机取消与 TTL 超时可能并存，以 `approvals.jsonl` 的状态为准」。属 `src-tauri/src/**` 行为 + 文案 |
| L2-94 | `bin/agent2ssh-daemon.rs:596`（`request_and_wait_for_approval` 第一句 `APPROVAL_COUNT.fetch_add`）、`:1238` 的 `/metrics` 输出 | 成立（命名与语义不符，非缺陷） | 计数发生在**等待开始**，所以拒绝、超时、停机取消都 +1，而低风险与「`force` 越过风险门且无 policy」都不 +1。名字叫 `approvals_total`，读者会当成「批了多少次」或「问了多少次」——两种读法都能自证矛盾。本轮 L1 只补了 `api.yaml` 的描述，没有改代码 | ①改名 `approval_requests_total` 并保留旧键一个版本；或②拆成 `approval_requests_total` / `approval_rejected_total` 两个计数器。属 `src-tauri/src/**` 行为面（`/metrics` 的键集合），需拍板 |

## 2026-11-11 · batch 8 第 1 轮（issue #14，基线 `46a92c4`）

CodeBuddy 首条分析 6 条，核实后 **成立 3 / 部分成立 3 / 不成立 0**；另有 4 条子断言判为不成立（见 ledger 的批次条目）。本表只列需要行为改动或产品口径的。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-95 | `embedded_ssh.rs:536-540`（`import_known_hosts_from_ssh` 的 identity 构造） | 部分成立 | 裸 `host:port` 经 `split_host_port`（`:643-657`）原样返回、端口写死 22，于是 `format!("{hostname}:{port}")` 拼出 `host:2222:22`：条目按一个没有 `HostProfile` 解析得到的 identity 落盘，计数仍是 `imported`。触发面窄——OpenSSH 对所有非 22 端口都写 `[host]:port`，只有手工编辑或外来 `known_hosts` 才有裸形式。指摘同时给的两条叠加问题（`key_type` 是签名算法、`[host]` 与存储层括号形态不等）核实为**不成立** | 让 `split_host_port` 认识裸 `host:port`（或把这种行计入 `skipped` 而非 `imported`），identity 改用 `format_host_port`。改的是落盘内容，需要一条覆盖 `host` / `[host]:port` / 裸 `host:port` 三形态的测试 |
| L2-96 | `sftp_transfer.rs:365`（`STALL_TIMEOUT`）、`:407-416`（`StallGuard::write`）、`russh_impl/sftp.rs:223-231`（`File::write`） | 成立（结论对，机制说反） | `Ok(0)` 在当前传输上不出现：`File::write` 返回 `Ok(min(buf, max_write_len))` 或 `Err`，所以 600 s 分支与 `write_all_shrinking` 的减半分支都不可达。对端不 ACK 时 `russh_sftp` 的 `poll_write` 在 8 个在途 ack 上阻塞，`write_nowait` 的接收端没有被 `request_timeout_secs` 包住，`block_on` 也没有超时 ⇒ 仍然是「无错误地挂着」。指摘把上界说成 60 s 会话超时，但 `write_with_timeout` 只服务 `Channel::write`（shell/exec），不服务 SFTP 句柄 | 给 SFTP 写路径一个真截止时间：在 `File::write` 外面包 `tokio::time::timeout`（**不要**用 `request_timeout_secs`——它只服务 `request()`，对写 ack 无效）。**SFTP 层**的闸门只有 `write_acks` 的 8 个在途请求一处（`russh_sftp` 的 `mpsc::unbounded_channel` 没有背压）；内核 socket 写超时（由连接预算给出，`/sftp/upload` 路径是 60 s，`embedded_ssh.rs:1676-1677`）只拦「TCP 发送缓冲满」，拦不住「窗口一直开着但 SFTP 层不应答 ack」——正是本条的场景，所以它不构成写路径的截止时间，所以要包住 **`write` 与 `flush` 两处**——`File::flush` 的 `poll_drain_writes` 抽的是同一个队列（`poll_shutdown` 也是，但本仓的 `impl Write for File` 没有 `shutdown`）；文案仍由 `StallGuard` 负责「已写多少字节」。**落点必须在 `transport/russh_impl/sftp.rs:223` 的 `impl Write for File`**：那里是 `StallGuard` 唯一能等到的地方；在 `core.rs` 的 `spawn_blocking` 闭包里包 `tokio::time::timeout` 无效（同步闭包里没有 reactor，`block_on` 会互相等）。另注意 `timeout` **不取消**内部 future——超时后 `File` 仍攥着那 8 个 ack，必须先 abort session，否则下一个请求在同一个队列上继续卡。**超时分支的文案必须自己读 `bytes_written`**：`StallGuard::write` 在 `inner.write` 返回 `Err` 时先 `?` 走人，`stalled()` 不会在真错误上被调用，「命名写了多少字节」不会自己出现。需一条能复现「读 TCP、不应答 SFTP」的集成测试 |
| L2-97 | `bin/agent2ssh-daemon.rs:4864-4875`（`/terminal` 的 `connected` 帧）；`embedded_ssh.rs:899`（`auth_method_for_host`）、`:2291-2292` | 部分成立 | `auth_method` 在 `connection_info` 里算出来，终端路径只在诊断日志里用它，`connected` 帧没有这个键；且 `auth_method_for_host` 是**按 host 预测**（password > key_path > agent），不是 `authenticate` 的返回值。指摘称「README 与 `/health-snapshot` 都回」核实为**不成立**：`README.md`、该 daemon、`api.yaml` 三处都没有 `auth_method`。改帧形状属对外契约 | 二选一：①帧里补 `auth_method`，同时把预测值换成 `authenticate` 的返回值（要先把该返回值从 `connect_embedded_ssh` 传出来）；②维持现状，在 `api.yaml` 的 `/terminal` 帧说明里写明「`auth_method` 只进诊断日志」 |
| L2-98 | `embedded_ssh.rs:2134-2136`（认证失败文案） | 成立 | 文案只列 `~/.ssh/id_rsa, id_ed25519, id_ecdsa`，而循环还试 `id_ed25519_sk`/`id_ecdsa_sk`（`:2111-2112`），`keyboard-interactive`（`:1947`）也在尝试列表里。这串字面量属 `src-tauri/src/**` 的用户可见字符串；`docs/reports/evidence/pageant-acceptance-2026-10-23.json` 还留 4 份带**旧版**原文的历史证据（不可改写）。**`russh-migration.md:489` 不需要同步**——它引的是 `"...or default key succeeded"`（不带括号），本来就是旧版串 | 改成「…or a default key in `~/.ssh`…」；历史证据 JSON 不动，在报告里注明「该次运行的文案是旧版」。属 `src-tauri/src/**` 的用户可见字符串 |

### 2026-11-11 · batch 8 第 2 轮（订正与追加）

**L2-96 的「判断」维持原样，第 2 轮的反对意见不成立。** 它主张 `russh_sftp` 每条读包都有 10 s 超时（引 `rawsession.rs:406-414` 的 `run`），超时后连接任务退出、`oneshot` 全部 drop、`poll_write` 于是拿到 `BrokenPipe: write channel closed`。核实：`rawsession.rs` 里**没有** `run`（连接任务在 `client/mod.rs:80`，循环体是 `process_handler(&mut rd, &mut handler)`，无超时）；`rawsession.rs` 全文件唯一的 `runtime::timeout` 在 `:215`，位于 `request()` 内，而 `write_nowait`（`:379-400`）只调 `send()` 并原样返回接收端。`request_timeout_secs` 默认 10 s 属实（`client/mod.rs:46`），但它只约束 `stat`/`open`/`readdir` 这类请求-应答，写 ack 不经过它。`check_write_result` → `BrokenPipe`（`fs/file.rs:96-110`）确实存在，触发条件是**读任务结束**（EOF/关闭），不是「10 s 沉默」。结论：写路径仍无截止时间，L2-96 不变。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-99 | `bin/agent2ssh-daemon.rs:4855`（drain 分支 `TerminalEvent::Error(_) \| TerminalEvent::Closed => break`）对照 `:4900-4904`（正常分支发 `{"type":"error","error":…}` 再 Close） | 成立 | daemon 进入停机 drain 之后，握手成功却让 `request_pty`/`shell`/`init_command` 失败时产生的 `TerminalEvent::Error` 被这个分支吞掉：客户端先收到 `connected`，接着只收到 `shutdown`，失败原因一个字都没有。这个窗口只在停机那一两秒出现，是最难复现、也最容易让人误判成「连上了但没反应」的时刻。`auth_method` 丢的是诊断信息，这里丢的是失败原因本身 | drain 分支补发同样的 error 帧再 break（与 `:4900-4904` 同形）。属 `src-tauri/src/**` 的对外帧行为，需要一条「停机窗口内让 shell 请求失败」的 e2e |

### 2026-11-11 · batch 8 第 3 轮（形态归一合并建议 + 两条补正）

1. **L2-15 / L2-95 / `remove_system_known_host` 的字符串全等是同一个根因，应当一起拍板。** 第 3 轮给出了第二类盲区：`port == 22` 时 `patterns` 只有裸名，所以文件里手写的 `[example.internal]:22` 与裸 `example.internal:22` 都匹配不上、`removed 0`，而 `[::1]:22` 那条红测只是同一根因的第一个实例。三种形态加导入侧的 `host:2222:22` 一次解决的办法是同一个：用 `split_host_port` 解析 host 字段后比 `(host, port)`，不做字符串全等（`forgetting_one_port_leaves_the_other_port_alone` 仍然成立，因为 `(example.internal, 2222) ≠ (example.internal, 22)`）。本轮已在 `embedded_ssh.rs` 的 `patterns` 上方写注释串起三条，**条目编号保留不合并**（合并会让 ledger 里的历史引用全部指错），但作为一次决策提出。
2. **L2-96 的「建议改法」已改**：`request_timeout_secs` 对写 ack 无效，不要把设值当成修法；闸门只有 `write_acks` 一处，在 `File::write` 外包 `tokio::time::timeout` 即可。第 3 轮补充的事实成立：`russh_sftp` 的前置通道是 `mpsc::unbounded_channel`（`client/mod.rs:85`），`send()` 永不阻塞，所以不存在第二条背压。
3. **L2-97 追加一项**：`connected` 帧的 `recording_id` 未录制时序列化为 `null`（键在、值为 null，不是缺键），`terminal_id` 永远非空且是连字符形态。`Uuid` 的 serde 实现与 `.to_string()` 逐字节相同，所以 `:4874` 的 `.to_string()` 不是 bug、只是冗余。改帧形状时把「`recording_id` 可为 null」写进 `api.yaml`。

### 2026-11-11 · batch 8 第 4 轮（自取三条，CodeBuddy 静默 5 分钟）

本轮指摘来自第 3 轮末尾我自己列的三个待核点（CodeBuddy 5 分钟内未回）。

1. **`flush` 也要包进 L2-96 的修法——成立，且把范围扩大了。** `copy_cancellable_with_chunk` 末尾有 `writer.flush()?`，而 `russh_sftp` 的 `File::flush` 第一句就是 `ready!(poll_drain_writes(&mut self.state.write_acks, cx))?`（`fs/file.rs:299-301`），抽干的正是 `write` 用的同一个队列。所以只把 `File::write` 包进 `tokio::time::timeout`，**每次传输的最后一次 flush 仍然可能无界等待**，而它恰好是最容易卡住的地方（队列里还有 8 个未回 ack 时对端沉默）。L2-96 的修法改为「`write` + `flush` 同一处 deadline」。
2. **L2-19 必须先于 L2-95 落地——成立，是顺序约束不是新缺陷。** L2-19 要把 `KnownHostImportSummary.skipped` 按原因拆字段（hashed / malformed / undecodable / conflicting），L2-95 要把裸 `host:port` 计入 `skipped`。后者单独做只是往一个五合一计数器里再塞第六种原因，CLI 文案（`imported N, skipped M (hashed or conflicting)`）会继续给错归因。**先拆后加**，否则 L2-95 会制造一个「数字对了、解释更错」的状态。
3. **`known_hosts.bak` 每次覆盖不是缺陷——不成立，记此以免后续重提。** 实测 `ssh-keygen -R` 同样每次覆盖 `known_hosts.old`：两次连续 `-R` 之后 `.old` 只保留**最后一次调用之前**的状态，与 OpenSSH 逐字一致。本仓 `remove_system_known_host` 写 `known_hosts.bak` 的语义相同，属「与上游一致」，不立项。

### 2026-11-11 · batch 8 第 5 轮（复核第 4 轮 + 撞号登记）

1. **命令是 `L2-19 → L2-22 → L2-95 / L2-15 → L2-96`，第 3 步之前还有一条。** L2-22 那行原文就写着「会改变 `imported`/`skipped` 的计数分布（**与 L2-19 同批做**）」，所以只给 L2-95 加顺序约束是不够的：同一个计数点上有三个条目。另确认 `skipped` 的取值是 **4 个可命名原因 + 0 个兜底**——`embedded_ssh.rs` 的导入循环里 `skipped += 1` 只有四处（hashed、三列不足、base64 解不开、指纹冲突），「其余怪行」被并进「三列不足」那一支，所以 L2-19 的字段集就是四个，不要凭空造第五个或 `other`。
2. **`known_hosts.bak` 的两点澄清（不改代码）**：本仓只有一个 `known_hosts` 文件，所以 `with_extension("bak")` 恒为 `known_hosts.bak`，不存在「第二个文件撞名」的问题；`removed > 0` 才写备份，与 `ssh-keygen -R` 未命中时不产生 `.old` 一致。
3. **L2-96 的落点与取消语义**（已写进条目正文）：必须落在 `transport/russh_impl/sftp.rs:223` 的 `impl Write for File`；不能在 `core.rs` 的 `spawn_blocking` 闭包里包 `tokio::time::timeout`；`timeout` 不取消内部 future，超时后必须 abort session，否则下一个请求在同一个 `write_acks` 队列上继续卡。
4. **撞号登记**见本文件顶部。这是本轮唯一一处「本仓自己的记录坏掉」的发现，优先级高于上面三条代码事项——因为它让「按编号追溯」这个唯一的工作方式失效。

### 2026-11-11 · batch 8 第 6 轮（CodeBuddy 第 3 轮复核的迟到件 + 收尾）

它 16:43:40 那条复核（`2120195699743555584`）当时被后一条挡住没读，直到收尾前才翻到，里面 4 条新指摘。判定：**成立 2 / 部分成立 1 / 不成立 1**。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-100 | `forward.rs:555-590`（accept 循环里无条件 `thread::spawn`）、`:606-617`（`handle_local_connection` 先 `connect_embedded_ssh(&host, 60)` 再碰客户端字节） | 成立 | 本地端口上**只 connect 不说话**的客户端（`nc`、端口探活、容器 healthcheck）仍会付一次完整 SSH 连接：每条一个线程、最长 60 s，且并发数没有上限（`control.connections` 只是计数器，不是闸门）。`bridge_tcp_and_channel` 自己会 `set_nonblocking(true)`，但那是在**连接成功之后**——瓶颈在之前的 SSH 拨号 | 在 `:617` 之前先对客户端做一次带超时的首字节 `peek`（5 s 量级），或把 `connect_embedded_ssh` 挪到「已收到首字节」之后；另加一个并发上限（与 `JumpThreadGuard` 同形）。属 `src-tauri/src/**` 行为，需一条「连上不说话」的用例 |
| — | `sftp_transfer.rs` 的 `STALL_TIMEOUT` 首段（「A deadline turns the hang into a failure that names the write size」） | 成立（**这句话本轮之前是假的**） | `StallGuard::write` 里 `self.inner.write(buf)?` 的 `?` 先返回，`stalled()` 永远不在真错误上被调用——所以「命名写了多少字节」的解释路径连修完 deadline 之后也不会经过 `StallGuard`。已改注释：deadline 必须自己读 `bytes_written` 组文案 | 并入 L2-96 的第 (b) 半 |
| — | `L2-98` 的「为什么不能自动落」 | 部分成立（我的理由写错了） | `russh-migration.md:489` 引的是 **`"...or default key succeeded"`，不带括号**，即它记录的是**旧版**文案；改当前这串带括号的文案**不需要**同步那份文档。证据 JSON 那四份仍不可改 | L2-98 的理由已缩为「用户可见字符串 + 四份历史证据不改写」 |
| — | `jump_chain.rs:88` 每跳一次 `load_config()` | 部分成立（不新立） | 机制成立，但 `load_config` 有缓存，代价可忽略；危害只是「一次连接里不止一份配置快照」的可观测后果（错误信息可能指向已删掉的 profile 名）。它自己也说不是 bug | 把 `resolve_jump_chain` 的结果传下去而不是让 `connect_via_jump_to` 再解析一次；属改善项，不单独立条 |
| — | `bridge_tcp_and_channel` 把 `TimedOut` 当错误上报 | **不成立** | 三个调用点都在非阻塞模式下运行：`:976` 对 jump session 显式 `set_blocking(false)`，`forward.rs:618`/`:668` 同样，而 `bridge_tcp_and_channel` 自己第一句就 `stream.set_nonblocking(true)?`。非阻塞的 `read_side` 返回 `WouldBlock`（`channel.rs:356-358`），不是 `timed_out()`。所以「安静的隧道 60 s 后被 :1266 断开」没有触发条件；`write_with_timeout` 的 `TimedOut` 是写入超时、本就有意 abort，与「空闲」不是一回事 | — |

### 2026-11-11 · batch 8 第 6 轮补记（收尾帖之后 9 秒到达的回复）

第 6 轮收尾帖 17:00:35 发出，CodeBuddy 17:00:44 回了 `2120199995319042048`——即对我第 5 轮两问的答复。本批不再开第 7 轮，按「本轮内吸收」处理（与 batch 2 的「补记」同形）。

它对撞号登记提出三件事：**2 不成立 / 1 部分成立**。

- **「`L2-23` 是同一条的两个版本，撞号应为 11 对」— 不成立。** 它只数到 `:267` 与 `:281`（后者是 `L2-23（收窄）`，同一编号的修订），漏了 `:93`：那是 `daemon_control.rs:488-500` 的 `child.kill()` 失败 + `write_daemon_pid_if_absent` 吞 `AlreadyExists`，与主机键字面量无关。12 对成立，`L2-23` 也成立，只是它多带一个同号修订。它由此提的「后缀形态没有合法地位」成立，已写成登记第四条规则。
- **「`:251` 的 `L2-26` 是死链」— 部分成立。** 死链真存在，但在 `:91` 而不是 `:251`：`:91` 那句「4s 之所以每次都烧满，根因是 L2-26 的 entry 泄漏」依赖的 `L2-26` 已在 `:99` 撤回。它给的修法（改指 `L2-20（entry 泄漏）`）无处可指——「entry 泄漏」本身已被判不存在。已删掉那句根因断言，改为「目前没有已确立的成因」。
- **「`L2-100` 已被第 5 轮小节占用」— 不成立。** 全文 `L2-100` 只出现两处：`:16` 的规则、`:611` 本轮新立的条目。第 5 轮小节里没有这个号。

另给一条新指摘（带行号、带可写的失败断言），判 **部分成立**：

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-101 | `bin/agent2ssh-daemon.rs:1990-2001`（upload）、`:2016-2028`（download）把 `host_tags(&req.host)` 传进 `preflight_guarded_request`；`:569-583` 用它组 `targets` 再进 `authorize_command_inner`（`:717-737` 把 `tags` 塞进 `CommandAuthorizationInput`），`:743` 起等审批，TTL 可到 300 s。执行侧 `core.rs:1910`/`:2055` 的 `*_core_with_source` 签名里**没有** tags，风险只由 `classify_effective_risk(command)` + `host.risk_override` 决定（`:1921-1924`、`:2067-2068`） | 部分成立 | **机制成立**：标签快照取自审批之前，等待窗口最长 300 s，执行侧从不重读标签，也不接收标签。**框架说反了一半**：它称「`/exec` 把标签一路传进执行、SFTP 少传一层」，但在 `core.rs` 里 `tags` 只服务目标展开（`expand_hosts_by_tags`/`resolve_exec_targets`，`:798-826`、`:841`、`:1420`、`:1488`），从不参与风险重判——`/exec` 与 SFTP 有**同一个**「授权按快照、执行按当时事实」的性质，不是 SFTP 独有的少传一层。它的失败模式①（审批期间摘掉 `prod`，已批准的传输照跑）方向上是**更宽松但不构成漏洞**：批准的对象是那条命令，标签消失只是让审批不再必需；失败模式②（审批期间加上 `prod`，于是不经审批就执行）才是真窗口，但那是「审批有时限」这一设计的固有代价，与 L2-77 同族 | 二选一：①审批返回后、执行前用**当前**配置重算一次 `tags` 并要求与审批时一致，不一致即 403（引入一次重读与新错误码）；②维持现状，在 `docs/api.yaml` 的 `/sftp/upload`、`/sftp/download` 与 `configuration-guide.md` 的审批段写明「审批依据是请求时刻的标签快照；审批期间标签变更不会让已批准的请求失效」。属产品口径，需拍板；写 e2e 时用 `daemon_sftp_approval_e2e.rs` 现成的 `assert_no_ssh_connection` 工具即可 |

### 2026-11-11 · 模式 B 单发（issue #2，基线 `b83b9c1`）

CodeBuddy 对上一轮收尾（`2120172790916468736`）的复核 `2120173738653020160`，5 条新指摘 N1–N5。核实后 **成立 3 / 部分成立 2 / 不成立 0**；另有 3 条子断言判为不成立（见 ledger）。本轮 L1 只动 `docs/**`，下面两条是必须由人拍板的。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-102 | `bin/agent2ssh-daemon.rs:3040-3095`（`forward_remove`）、`:3098-3152`（`forward_stop`）、`:3154-3210`（`forward_start`）：三条 handler 都先 `forward_list_core()` 再 `.find(id)`，而 `forward_list_core`（`forward.rs:739-746`）返回**全部** rule、没有 scope 过滤；`check_daemon_scope` 到 `:3062`/`:3119`/`:3175` 才按 `rule.host` 判 | 成立 | 越权结果是 **403 `scope denied`**，不是「像未知 id 一样 400」。于是 scoped token 可以用 403 与 400 之差**枚举**某条隧道是否存在——`api.yaml` 原来那句「an id the token cannot see is answered like an unknown one (400), not with a 403 that would confirm the tunnel exists」正好把这件事说反了（本轮已改正描述）。要变成 400 必须动 handler 的取数顺序 | 二选一：①先给 `forward_list_core` 加 scope 过滤，或把 `.find(id)` 换成「找不到**或不在 scope 内**一律 400 unknown forward」，代价是删掉 403 这条分支、`api.yaml` 与相关测试一起改；②承认这条泄露并在契约里写明（本轮已按②改写 `api.yaml`，若选①要把它改回去）。与 L2-81（广播全拒绝仍 200）同族：都是「handler 的取数范围大于授权范围」 |
| L2-103 | `bin/agent2ssh-mcp.rs:1798-1812`（`McpTool::SshEventsSubscribe`）、`bin/agent2ssh_mcp/tools.rs:851-855`（工具描述）、`events.rs:57-64`（`event_bus` / `subscribe_events`） | 成立（结构上恒空） | 工具自己 `subscribe_events()` 建一个**新** `broadcast` receiver，紧接着在同一个同步块里 `try_recv()` 抽干。`tokio::broadcast` 的新 receiver 没有历史，所以除「订阅与抽干之间恰好有别的线程 publish」外永远是 `[]`；`event_bus()` 又是 `app_state().event_tx`（进程内静态），MCP 进程的 bus 与 daemon 的 bus 是两份。`docs/skills.md` 已按实际行为改写，但工具描述（属 `src-tauri/src/**`）与工具本身仍在承诺「latest events」 | 三选一：①在 daemon 侧给事件加环形缓冲（`publish_event` 目前是纯 `send`，要另存一份 ring），MCP 工具改为查 `GET /events/stream` 或新加的 `GET /events/recent`；②如实改工具描述为「总是空」并把工具标成 deprecated；③删掉工具（MCP 契约破坏，要版本号配合）。属 `src-tauri/src/**` 行为 + 对外契约 |

另外三条不新立（已在别处登记）：

- **N1 的「gate / blocked 只算不拒、应回 423」→ 是 L2-81 的同一件事。** `api.yaml` 早已写明「每目标授权拒绝不改 HTTP 状态」，所以这一条只有「广播目标三个布尔字段没有描述」这半是新的；本轮已在 schema 里补上 `requires_approval` / `authorized` / `approval_granted` 的语义（preview 不发字节、不产生审批卡，`authorized: true` 只表示「暂时没人拒绝」）。
- **N4 的「`approval_count` 与 `approvals_total` 同名不同义」→ 改名/拆分是 L2-94。** 本轮只给 `DaemonMetricsSummary.approval_count` 补了描述，与 `/metrics` 里 `approvals_total` 那段对齐。
- **N5 的「L2-93 窗口比记录的宽」→ 已就地扩写 L2-93**（三种 `Err` 原因 + 被吞掉的写盘失败），未开新号。

---

## 2026-11-11 · batch 9 第 1 轮（issue #15，基线 `f376ee5`）

首条分析 7 条 + 3 条已并入。核实后 **成立 4 / 部分成立 3 / 不成立 0**。本轮 L1 只动 `docs/**`（`api.yaml` 两处 `transfer_id`、`skills.md` 三处、`daemon-api-quickstart.md` 两处），下面两条是必须由人拍板的。

**分类说明（供复核，不同意可整笔回退）**：`tools.rs` 里三处**工具描述字符串**没按 clap 文档先例判 L1，而是登记 L2——依据是本文件 `L2-103` 自己把 `bin/agent2ssh_mcp/tools.rs:851-855` 的「工具描述」标成「属 `src-tauri/src/**`」。同一类东西两套判法会让「按编号追溯」失效，所以本轮沿用 L2-103 的口径：`docs/skills.md` 先改成事实，`tools/list` 的文案等这里拍板后同笔改。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-104 | `bin/agent2ssh_mcp/tools.rs:276`（`max_output_bytes` 描述）、`:546`（`ssh_session_list` 描述）、`:582`（`ssh_risk_check` 描述） | 三处成立，且都是**与 `docs/skills.md` 同一句话的两个版本** | 三处都是发给客户端的 `tools/list` 文案，属 `src-tauri/src/**`（见上方分类说明）。①`:276` 写「Truncate stdout to this many bytes」，而 `types.rs:384-386` 是「stdout **and stderr** … **each**」，且截断保留首尾、中间的字节数记进 `dropped_bytes`；②`:546` 漏了 `backend`——`bin/agent2ssh-mcp.rs:1256-1268` 的 `process` 支与 `:492-513` 的 `daemon` 支**每行都带**，而它是「这条会话会不会随 MCP 进程死掉」的唯一机器可读信号（`skills.md:48` 本轮已补）；③`:582` 把用户规则说成只来自 `risk_rules.toml`，实际 `risk_config.rs:42-45` 先取 `existing_policy_path()`（`policy.rs:54-64` 先 `.toml` 后 `.json`），**policy 存在就根本不读 `risk_rules.toml`**——照契约去改规则文件的 agent 会改到一个不生效的文件（`README.md:224`/`:352` 本来就写着 policy 优先、`risk_rules.toml` 是 legacy） | 三处各改一句，与 `docs/skills.md` 的现行措辞对齐即可。改完只需 `cargo check --no-default-features --bin agent2ssh-mcp`；`cli_smoke` 的 `tool_count == 76` 与 `daemon_integration` 的 `mcp_tools_match_skills_md_documentation` 都只比名字与数量，不受文案影响（已核实） |
| L2-105 | `bin/agent2ssh-daemon.rs:1982-2007`（`sftp_upload`）、`:2008-2034`（`sftp_download`）收到的 `transfer_id`；`core.rs:1949-1952`/`:2094-2100` 的 `sftp_transfer::register`、`:1995`/`:2166` 的 `unregister`；`sftp_transfer.rs:288-297` 的 `cancel_transfer` 全仓唯一调用点是 `tauri_commands.rs:650-651` | 成立（`transfer_id` 在 daemon 侧是死字段） | 取消靠 `register` 放进 `registry()` 的 `Arc<AtomicBool>`，而 `registry()` 是**进程内静态**。daemon 是独立进程（`bin/agent2ssh-daemon.rs`），它注册的 flag 只有 `cancel_transfer` 能翻，而 `cancel_transfer` 的唯一入口是桌面端的 Tauri IPC——**跨进程够不到**。结果：daemon 的 `/sftp/upload` 收下 `transfer_id`、注册一个永远无人翻转的 flag、跑完 `unregister`。桌面端自己的传输不受影响（走 `tauri_commands.rs:628` 的 in-process 路径）。`types.rs:720`/`:735` 的注释「cancel this in-flight transfer via `sftp_cancel`」对 daemon 路径同样不成立 | 二选一：①给 daemon 加取消路由（`POST /sftp/cancel` + admin/scope 校验），顺带把 `api.yaml` 的两处描述改回「另一请求可取消」；②承认不可取消：daemon 的 handler 拒收非 `None` 的 `transfer_id`（400），或忽略并清空，同时改 `types.rs` 两处注释。本轮已把 `api.yaml` 的两处描述按②的事实改写（HTTP 侧没有取消路由）；选①要把那两处再改回去 |

## 2026-11-11 · batch 9 第 2 轮（issue #15，基线 `1bcadb5`）

CodeBuddy 的复核（`2120253081072848896`）判「第 1 轮问题全部真实解决」，三问逐条确认（C1 无第三处入口、L2-105 无反例、`9be53e4` 的方向对），并新批 1 条 D4。本轮 **成立 1 / 部分成立 0 / 不成立 0**，另自取 2 条（来自它给的边角）。L1 只动 `api.yaml` 三处字段描述。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-106 | `sftp_transfer.rs:263-272`（`register` 的 `if let Ok(reservation) = reserve(...)`）、`lifecycle.rs:77`（`Uuid::parse_str(id).is_err() → InvalidId`）、`diagnostics.rs:570-573`（`active_resources` = `Ready` + `Pending`） | 成立 | `reserve` 要求 id 是合法 UUID，而 `transfer_id` 全来自调用方：桌面端 `SFTPPanel.tsx:592` 生成 `sftp-<毫秒>-<随机>`，CLI（`bin/agent2ssh.rs:2419`/`:2449`）与 MCP（`tools.rs:472-490` 的 schema 根本没有该字段）都传 `None`，单测用 `k6-test-transfer`（`sftp_transfer.rs:665`）。**本仓没有一个调用点传合法 UUID**，所以 `if let Ok(...)` 每次都在吞 `InvalidId`，`ResourceKind::Transfer` 条目从未建成 → 在途传输在 `/health-snapshot` 的 `active_resources` 里永远不出现。指摘说「从未被记录」**过强**：裸 HTTP 调用方若传合法 UUID 就会被记录，准确说法是「本仓所有调用点都不是 UUID」 | 二选一：①`register` 里把 `if let Ok` 换成 `match`，`Err` 打一条 warn（承认降级，`active_resources` 仍不含传输）；②不再拿 `transfer_id` 当 lifecycle id，内部另生成 `Uuid::new_v4()` 并维护 `transfer_id → lifecycle_id` 映射，`unregister` 反查。②能真正让传输出现在健康快照里，代价是 `unregister` 多一次查表与「映射丢了怎么办」的处理。另：`ResourceOwner::Headless(Uuid::new_v4())` 使 `close_owner` 永远匹配不上，条目只能靠 `unregister` 的 `close(id, None)` 关——②落地时这条要一并想清 |

## 2026-11-11 · batch 9 第 3 轮（issue #15，基线 `1bcadb5`→`f6f3afd`）

CodeBuddy 的复核 `2120255506575945728` 新批 4 条 → **成立 2 / 不成立 2**（§四.1 与 §四.2 见 ledger 的不成立清单）。另本轮自取 1 条。L1 动了 `approval.rs` 的 rustdoc、`bin/agent2ssh.rs` 的四处提示与一处 clap 文档、`skills.md` 两条、`cli-quickstart.md` 一行。

**「CLI 散文 vs MCP 契约」的分类规则（本轮确立，后续沿用）**：同样是 `src-tauri/src/**` 里的字符串，**人读的**（clap 帮助、CLI 错误提示、rustdoc）判 L1，**机器读的**（`tools/list` 的 `description`、MCP 返回的 schema 文本）判 L2。理由是后者是 agent 的输入契约，改它等于改对外协议；前者只影响人。这也解释了为什么第 1 轮把 `tools.rs` 三处判 L2、本轮把 `bin/agent2ssh.rs` 的提示判 L1。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-104（扩写） | 追加 `bin/agent2ssh_mcp/tools.rs:772`（`ssh_preview_exec` 描述）、`:792`（`ssh_approval_check` 描述） | 成立 | 本轮新发现：`api.yaml` 里写的字段描述**不会进 MCP 响应**——`ssh_preview_exec` 走 `serde_json::to_value(preview_exec(...))`（`bin/agent2ssh-mcp.rs:1675-1690`），`ExecPlan` 是纯 `derive(Serialize)`，serde 不读 OpenAPI。所以 `783cfb3` 修好的是读 `api.yaml` 的人，**读 MCP 响应的 agent 看到的仍是裸 `requires_approval: false`**。`docs/skills.md:48`/`:50` 本轮已补上「false 不代表 `/exec` 会放行」，与 `api.yaml` 对齐；剩 `tools.rs` 两处 | 两处各补一句「`false` only answers this question; `/exec` still demands `force` or an approval for a high-risk command」。与 `L2-103`/`L2-104` 前三处同笔提交即可（同一文件、同一类文字、同一次决策）|
| L2-107 | `bin/agent2ssh.rs:635`（`--risk-override` 的 clap 文档）、`:2010-2016`（`risk_override.and_then(|s| match ... _ => None)`） | 成立 | 两件事：①文档（clap 的 `(low/medium/high)` 与 `cli-quickstart.md:97`）把取值说成闭集，而 `match` 接受 `blocked`——`apply_risk_override`（`core.rs:990-996`）会把它当成覆盖值，于是该主机**每条命令都被拒**，这是文档里从未出现的能力；②**无法识别的值是静默丢弃**：`_ => None` 之后 `HostProfile.risk_override` 为 `None`，程序照样打印 `Saved host 'x'.`。一个拼错的 `--risk-override hihg` 会得到一台没有任何覆盖的主机，且没有任何提示。本轮只按①改文档（`(low/medium/high/blocked)`），②属行为，未动 | ①先定「`blocked` 该不该作为覆盖值」：该留就把 `cli-quickstart.md:97` 与 clap 文档写成事实（本轮已写，若决定不接受则要改回并改代码）；②把 `_ => None` 换成 `bail!("unknown risk level {s}: expected low/medium/high/blocked")`，或至少 `eprintln!` 一条警告。改的是 `host add` 的失败语义（今天永不失败），要配一条 `try_parse` 级用例 |



## 2026-11-11 · batch 9 第 4 轮（issue #15，基线 `99ec6b4`→`d574676`）

CodeBuddy 的复核 `2120258070138740736` 新批 3 条 → **成立 1（§五.2）/ 不成立 2（§五.1、§五.3）**；三个提问（Q1 重新推导、Q2 新文案、Q3 `--risk-override blocked`）均已答复并采纳。它自己推的 `d574676` 里有一处**新引入的错事实**，本轮已回退（见 ledger「不成立」清单第 3 条）。L1 动了 `api.yaml` 两处（回退 + `requestBody.required`）、`cli-quickstart.md` 四处、`bin/agent2ssh.rs` 四处提示、`daemon_integration.rs` 一条断言。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-108 | `core/team_config.rs:36-46`（导出只读 `risk_rules.toml`）、`:109-113`（导入无条件覆盖 `risk_rules.toml`）、`:186-191`（预览按 `risk_rules.toml` 是否存在报 `overwrite`/`new`）；对照 `risk_config.rs:42-43`（`existing_policy_path()?.unwrap_or_else(risk_rules_path)`） | 成立 | `policy.toml` 存在时 `load_risk_rules` **根本不读** `risk_rules.toml`（`README.md:224`/`:352`、`mcp-quickstart.md:583` 都写着 policy 优先、它是 legacy）。于是团队配置的两半都错位：①导出在 policy 时代恒为 `None`，报「未配置」，哪怕对方策略里有一整套 `[risk.*]`；②导入把 `risk_rules.toml` 覆盖掉——用户手写的 legacy 文件被冲掉，而 `summary` 还宣布「risk rules: overwrite」，看起来像生效了。同一函数对 host 的处理是「合并」，对风险规则是「覆盖」，两者都不是「写进当前生效的那份」 | 二选一：①导出/导入都改走 `existing_policy_path()` 优先的那份（导出时把 `[risk.*]` 段落取出；导入时若目标不存在则新建 `policy.toml`），legacy 只在没有 policy 时才碰；②明确只在 legacy 语义下工作：`summary` 加一句「written to `risk_rules.toml`, which `policy.toml` shadows」，并在 `configuration-guide.md` 的团队配置一节写明这条边界。①更符合「团队配置导出的是能用的东西」，代价是要处理 `policy.toml` 里 `[risk.*]` 与 `approval_policies` 同文件的合并 |
| L2-107（补充） | `bin/agent2ssh.rs:2010-2016`（`host add --risk-override`）只改这一处；**:1085** 的 `policy add --min-risk` 保持接受 `blocked` | 同上，补充证据 | 复核给了一条本轮没抓到的证据：`f345a92` 同一笔既加了 clap 文档 `(low/medium/high)` 又加了 `"blocked" => Some(...)`，**没有任何一处文档同时说过 `blocked` 是合法的覆盖值**；`core.rs:3869` 只锁「不能降级 blocked」这一个方向，`cli_smoke.rs:1257` 只用 `--risk-override low`，没有一条正例。所以更可能是顺手多写了一个枚举值，而不是有意的「隔离主机」能力。**但两处 `match` 是同一段代码的两份拷贝，语义不同**：`policy add --min-risk blocked`（「连 blocked 都要审批」）是真的需要这个值，全局禁掉会砍掉它 | 若拍板「拒绝」：`host add` 那处把 `Some("blocked")` 改成走错误分支，`policy add --min-risk` 保留；两处都把 `_ => None` 换成显式报错（拼错返回非零退出，而不是静默存成 `None`）。若拍板「保留」：本轮已把 `cli-quickstart.md:97` 与 clap 文档写成 `(low/medium/high/blocked)`，无需再动，但要把「该主机的命令会被全部拒绝」写进 `configuration-guide.md` 的字段表，否则它仍是一条靠读代码才能发现的能力 |

**第 5 轮复核对 L2-108 拍了方向：选 ①，不走 ②。** 理由按分量：②把一个「导出说 not configured、导入把用户手写文件冲掉还报 overwrite」的错位写进 `summary` 当规范；①两半一起修好，且 `policy.rs:117` 的 `save_policy_approval_policies` 已经是「load → 改 `approval.policies` → save」的形状，`[risk.*]` 照抄即可，不用新建文件。**但它留了一个子问题没定**：导入时若 `policy.toml` 已存在且带 `[risk.*]`，是**整段替换**还是**与现有条目求并集**。复核倾向整段替换（把「导出→导入」当一次完整搬运，与今天 `risk_rules.toml` 的覆盖行为一致，只是目标换对了）。**这一条仍需人拍板**：整段替换意味着从别的机器导入会清掉本机原有的自定义风险规则，而并集意味着同一条规则两边定义不同时无法表达「以哪边为准」。**L2-108 维持 open，等这个子问题定了一起做。**

**第 6 轮补答（CodeBuddy，issue #15 收尾轮的遗留问题之一）：`risk_rules` 缺省该「什么都不做」，不是「清空段落」。** 这条其实不用等拍板——**现状已经是「什么都不做」**，`import_team_config`（`core/team_config.rs`）里写 `risk_rules.toml` 的整个分支包在 `if let Some(ref rules) = export.risk_rules` 内，`playbooks` 同理；`ImportResult::risk_rules_imported` 就是 `is_some()`。所以「导出包里没有 `risk_rules`」时，本地 `risk_rules.toml` 原样保留，`summary` 也不出现 `risk rules:` 一段（`preview_team_config_import` 的 `risk_rules_change` 在 `export.risk_rules.is_none()` 时是 `None`）。**选 ① 之后要做的是「保持这个 no-op」而不是「实现它」**：目标文件从 `risk_rules.toml` 换成 policy，缺省语义不变——文件不动、段落不动、`summary` 不出这一段。已把这条边界写进 `export_team_config` / `import_team_config` 的文档注释，免得下一次又把「缺省」当成「清空」重新讨论。**仍未定的只有一条**：目标带 `[risk.*]` 时整段替换还是求并集。

## 2026-11-11 · batch 9 第 6 轮（issue #15，基线 `418c6db`）· 自取

本轮 5 次轮询（60 秒 × 5）没有新复核，转自取：两处人读文档 + 一条签名级清理。**判定全部成立（自取不涉及「指摘」）**，L1 已落，下面的进 L2。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-109 | `bin/agent2ssh.rs:1585-1606`（`authorize_local_operation` 的 `force: bool` 形参）、9 个调用点 `:2412`…`:2555` | 成立（残留形参） | 9 个调用点**全部**传字面 `false`，而 `SftpCommands::*` 没有任何 `--force` 标志；形参与 `--force` 同名，读代码的人会以为「某天会传 true」。本轮复核独立指出同一点（它说「是设计，不是漏传」——这点我核过也对：`sftp` 从来没有 force 能力）。但**删形参是签名改动**，且要先确认「`sftp` 永不允许 force」是设计而不是待补的功能：daemon 侧 9 个 SFTP handler 也把 `force` 硬编码 `false`，两边一致，看起来是「SFTP 只能靠审批，不能靠 force」的有意设计 | 二选一：①承认设计——删掉 `authorize_local_operation` 的 `force` 形参（内部传 `false`），并在 daemon 侧把 `preflight_guarded_request(..., false, ...)` 的注释写明「SFTP 不接受 force」；②这其实是个功能缺口——给 `sftp` 加 `--force`，同时给 daemon 的 SFTP 请求体加 `force` 字段。①只动签名，②要动契约（`api.yaml` 九条路由的 requestBody + `types.rs` 三个请求结构） |

**第 6 轮定级（CodeBuddy，issue #15 收尾）：** L2-109 **是设计，不是滞后**，证据四条，都能自己验：

1. `CHANGELOG.md:248`（`06fa58c`，2026-09-11）——加 SFTP 四件套的那一笔自己写的：CLI / MCP / 桌面「refuse `high` and `blocked` at the authorisation step before any connection is opened」，**并且**「neither the MCP schema nor the daemon body schema has a `force` field, so those surfaces cannot override the verdict at all」。这是 `f345a92`（2026-06-11）之后、唯一一处直接谈「sftp + force」的文字，它把「没有 force」写成契约，不是写成待办。
2. `cli-quickstart.md:97` 与 `SftpCommands::RmRf` 的 clap 文档（`bin/agent2ssh.rs`）——`rm-rf` 的注释只说「rated high risk … so it can require approval」，**只说审批**，`--force` 一个字没有；同文件里 `exec`、`exec-multi`、`playbook run`、`redact remove`、`highlight remove`、`webdav` 全都有 `--force`，`sftp` 九个变体一个也没有。不是漏了一个，是整组都不一样。
3. `authorize_local_operation`（`bin/agent2ssh.rs:1585`）的 `force` 形参**没有调用点能传 true**：九个调用点全是字面 `false`（`:2412`…`:2555`），`SftpCommands::*` 没有任何 `force` 字段。形参是 `authorize_command_without_approval_handler` 签名（`execution_control.rs:337`，11 个形参）带过来的，不是需求。
4. daemon 侧同向：九个 SFTP handler 调 `preflight_guarded_request(..., false, ...)`，而 `preflight_guarded_request` 的 `force` 形参在 `preflight_guarded_request_inner` 里原样交给 `authorize_command_inner`；`SftpUploadRequest` 等三个请求结构（`types.rs`）都没有 `force` 字段。`api.yaml` 的九条 `/sftp/*` requestBody 也没有。

反面证据也查了：`f345a92` 之后没有任何提交或文档出现「给 sftp 加 `--force`」「sftp force」这类说法；`CHANGELOG.md:342`（桌面 mutation parity）说的是 session / forward / connection 跟着 daemon 的 high-risk approval/force **语义**走，`CHANGELOG.md:177`（rm-rf 走 daemon 审批）更是明写「there is no force or approval-bypassing fallback」。且 `authorize_local_operation` 的拒绝提示自己写着「sftp has neither --force nor daemon forwarding」——**这句话是本轮之前就有的**，即代码自己早就把「没有 force」当既定事实。

**所以选 ①**：删形参、内部传 `false`，并在九个 daemon SFTP handler 旁注明「SFTP 不接受 force」。②（给 `sftp` 加 `--force`）与上面四条正面冲突，且要动 `api.yaml` 九条路由 + `types.rs` 三个结构，属于新功能而不是修缺陷。



## 2026-11-11 · batch 10 第 1 轮（issue #16，基线 `6e8cae3`）

主题是前端与 i18n 文案键完整性。本轮 L1 已落（`src/**` 文案与类型镜像、`api.yaml` 两条契约），下面八条要人拍板。**编号接 `L2-109`**；CodeBuddy 首条分析里自拟的 110–116 与本表不是同一套映射（它把 A1 记作 L2-110，而 A1 经复核**不成立**，见回帖）。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-110 | `src/types.ts:126-158`（`ExecRequest`/`ExecResult`）、`:452-469`（`AuditEntry`）；对照 `docs/api.yaml` 三处 schema 与 `src-tauri/src/types.rs` 三处 `pub side_effect` | 成立（契约宽于镜像） | `side_effect` 在请求、结果、审计三处连续缺失，`grep -c side_effect src/types.ts` = **0**，而 api.yaml 与 Rust 各 3 处。桌面端不传它就是 `None`，所以今天零后果——它是「桌面要不要给一个『声明副作用』入口」的产品问题，不是漏抄一行 | 二选一：①保持现状，在 `types.ts` 三处注明「桌面端刻意不暴露」；②在 `ExecPanel` 加一个可选「副作用说明」输入，`ExecRequest.side_effect` 透传，同时在 `AuditPanel`/`LiveActivityPanel` 列出来（后者连带 L2-117） |
| L2-111 | `src/api.ts:285-296`（`sftpDownload` 不传 `max_mb`）、`src/types.ts`（`SftpDownloadRequest` 镜像无该字段）、`src-tauri/src/tauri_commands.rs:634`（命令直接反序列化 `SftpDownloadRequest`）、`core.rs:2097` | 成立 | 上限默认 **100 MiB**（`core.rs:2097`），超限拒绝文案建议改用 scp/rsync——对 GUI 用户是死路。Rust 侧**不需要改**：`sftp_download` 命令收的就是完整 `SftpDownloadRequest`，前端带上 `max_mb` 即可透传；缺的只是一个界面入口 | ①`api.ts` 增加可选 `maxMb`、`types.ts` 补字段、SFTP 面板加一个「最大体积」输入（默认留空 = 100）；②或只在 `api.yaml` 写明「桌面端暂不可调」并接受 100 MiB 硬上限 |
| L2-112 | `src/components/approval/ApprovalDetail.tsx:186`（「Approve and execute」）、`src/components/ApprovalDialog.tsx:25`（「Confirm execution」）、`:52`（「Confirm and execute」）、`src-tauri/src/bin/agent2ssh-daemon.rs:4521-4547` | 部分成立（动词可辩护，宾语不对） | 「执行」确实发生，但**发生在发起方的会话里**：广播路径把 `format!("{}\r", command)` 写进目标 PTY（`:4535`），`/exec` 路径由 daemon 自己跑。所以三个按钮都在用一个词指两件事，读者会以为点完这条命令在**本窗口**跑。判据是「读者会误解」，不是「按钮是假的」——所以只立条不改 | 改成「Approve」+ 一行说明（「这条命令由发起方执行」）；连带 `ApprovalDetail.test.tsx` 与 `i18n.tsx` 三处键。要先定口径：桌面端到底替谁批 |
| L2-113 | `src-tauri/src/approval.rs:595-621`（`approval_poll` 翻 `TimedOut` 只写盘）、`:791-812`（`approval_list` 同）、`:696-700`（`approval_respond_inner` 的 TTL 分支直接 `Err`）；`src/components/NotificationCenter.tsx:33-79` | 成立（可观测的假状态） | 三条路径都会把 pending 翻成 `timed_out`，**只有**决议成功那条 `publish_event` 了。前端审批 toast 是 `durationMs: null`（不自动消失），只在收到 `approval_responded` 时才 `dismissToast` → **过期的卡片会一直挂着**，上面的「Approve」点下去得到 400 `already TimedOut` | 在 `approval_poll`/`approval_list` 的过期分支里补一次 `publish_event("approval_responded", {id, status:"timed_out"})`（属 `src-tauri` 行为，故只判不改）；或前端加一条「超过 `requested_at + ttl_secs` 就自动 dismiss」的兜底 |
| L2-114 | `src/components/approval/ApprovalWorkbench.tsx:80-91`；`src-tauri/src/store.rs:1522`；`src-tauri/src/execution_control.rs:299-316` | 成立（两个统计名不副实） | ①`auditToday` 数的是「今天写进审计的条目」，而 `list_audit_raw` 从新往旧取到 `limit` 就早停（`store.rs:1522`），今天超过 500 条时这个数**恒等于 500**；②`inFlight` 用 `exit_code === null`，而「被拒/超时/阻断」的条目本来就是 `exit_code: None`（`execution_control.rs:307`），于是过去 15 分钟每一次拒绝都被算成「执行中」 | ①「Decided today」改用 `decision != null` 计数并把文案改成「今日判决」；②「In flight」改成 `decision === null && exit_code === null`。两条都是 `src/**`，但会改变面板显示的数字，需拍板 |
| L2-115 | `src/components/approval/ApprovalDetail.tsx:78-90`（`handleCopy`）、`src/App.tsx:1201-1204`（`onGoExecute`）、`src/components/ExecPanel.tsx:26`（命令初值 `uname -a`） | 成立（本轮只修了文案） | 按钮已由「go to Execution」改成「open Execution」（本轮 L1），因为 `onGoExecute` 只 `setSelectedHost` + `openModule`，**不带命令**；执行页里那个框的初值是 `uname -a`。剩下的一半是 `navigator.clipboard.writeText` 失败时 `.catch(() => setCopied(false))` 什么都不显示——剪贴板被拒（非安全上下文）时用户以为复制成功，跳过去只有一个 `uname -a` | ①把 `approval.command` 透传给 `ExecPanel` 作为初值（改 1 个 prop + 1 处 state），文案可回到「carry」；②或保留现状，复制失败时 `showToast("error", …)`。①更好，但属行为改动 |
| L2-116 | `src/components/CommandPalette.tsx:22-40`（`MCP_TOOLS`）对照 `src-tauri/src/bin/agent2ssh_mcp/tools.rs` | 成立（18 → 16，仍未守住） | registry **76** 条、调色板 **58 → 60** 条（本轮补了 `containers_start_service`/`resident_session_close` 两条真有落点的）。剩余 16 条里 13 条（`ssh_algo_prefs_*` 3 + `ssh_highlight_rule_*` 5 + `ssh_redact_rule_*` 5）只在 `SettingsMenu` 对话框里编辑、没有 module id，3 条（`local_exec`、`resident_session_open/exec`）桌面端**没有任何入口**（`grep -rn local_exec src/` = 0；`ResidentSessionsPanel` 自述 read-only plus close）。补名字会让「跳到同一能力」的承诺变成假话，所以不机械补齐 | 二选一：①给调色板加 `moduleId: null` 形态（激活时提示「此能力只有 MCP/CLI 提供」），然后把 16 条补齐 + 加一条「registry ⊆ palette ∪ 白名单」的源文本契约测试；②保持子集，但加测试钉住「palette ⊆ registry」并断言缺口恰为这 16 条，任何一侧漂移就红 |
| L2-117 | `src/components/LiveActivityPanel.tsx:77-95`（`auditToItem`）、`:97-103`（`needsAttention`）对照 `src/components/AuditPanel.tsx`（`decision` 5 处） | 成立（同一数据两种可读性） | 活动流不渲染 `decision`，于是 `blocked`/`rejected`/`timed_out` 这些「根本没执行」的条目显示成 `exec recorded` + `duration 0ms` + `exit ?`，与「跑完了但没退出码」同形；审计面板有判决列。补它会**改变「注意条」的判定范围**（`needsAttention` 现在按 `kind.includes("approval")` 与 `high/blocked` 风险算），所以不是纯加一列 | 在 `auditToItem` 里带上 `decision`，行尾加一个与 `AuditPanel` 同款的 chip；`needsAttention` 增加 `decision === "rejected" || decision === "blocked"`。属 UI 行为改动 |

**2026-11-11 补记（batch 10 第 1 轮，针对 L2-74）**：L2-74 的前提有一半已过时——`docs/api.yaml:488` 现在**有** `recent_commands`（含 `items` 的三个属性），是后续批次补进去的，所以「契约没声明」那半不用再做。仍缺的只有 `src/types.ts` 的 `ApprovalContext` 字段与任何渲染点（`grep -rn recent_commands src/` = 0）。这条因此收敛为「镜像缺 + 没人渲染」，与 L2-110（`side_effect` 三处镜像缺）同族。批注写在这里而不是插进原表：插在表行之间会把那张表截断成两张。

## 2026-11-11 · batch 10 第 2 轮追加（`/approvals` 的时效性 + 前端两处显示口径）

第 2 轮 9 条指摘，核实 **成立 5 / 部分成立 3 / 不成立 1**。文档侧与 `src/**` 的文案、注释、类型镜像已落 main；下面两条是显示层改动，需拍板。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-118 | `src/components/HostPolicyLine.tsx:35-56`（`MODE_LABEL` 与四个 `summary.*` 段）对照 `src-tauri/src/approval.rs:1130-1150`（`approval_mode_for_risk`）、`execution_control.rs:157-172`（`host_policy_summary`） | 部分成立（A2 的残余） | `ApprovalMode::Auto` 有两种来源：「没有任何 policy 命中」与「policy 命中且 `requires_approval = false`」，`MODE_LABEL.auto = "auto-approve"` 把两者渲染成同一句话。**但它说的「未配策略的主机会显示 `high 自动放行` 却收到审批卡」不成立**：`host_policy_summary` 已把 high 的 `Auto` 改写成 `Required`（`:164-166`，`match mode(High) { Auto => Required, other => other }`），high 一行永远显示 `need approval`。真实缺口只在 low/medium：一台没有任何 policy 的主机会显示 `low auto-approve`，读起来像「已决定放行」，实际是「没有规则，因此不强制审批」 | 二选一：①给 `HostPolicySummary` 加一个「是否命中任何 policy」的布尔，未命中时改说 `no policy — not required`；②维持现状，在 `api.yaml` 的 `HostPolicySummary` 与面板上加一句「auto = 无强制审批，不等于已批准」 |
| L2-119 | `src/components/approval/ApprovalWorkbench.tsx:126-133`（`decidedToday`）、`:150-200`（`history` 分组）对照 `approval.rs:194-200`（30 天保留期）、`:289-290`（读时过滤） | 成立 | 两件事同一个后果——「历史」在 UI 上不可信：①`decidedToday` 按 `requested_at` 判「今天」，且 `status !== "pending"` 把 approved/rejected/timed_out/revoked 全算进去，于是这个数实际是「今天**发起**且已出结果」而不是「今天判决」；②`approvals.jsonl` 只留 30 天，更早的条目在**读的时候**被丢掉，面板没有任何「更早的已丢弃」提示，读者会以为那就是全部历史。契约侧本轮已在 `/approvals` 的 description 写明保留期与读时过滤，UI 侧仍是静默截断 | ①把 `decidedToday` 改成按最后状态变更时间判日（契约需补 `decided_at`），或至少把文案改成「今日发起」；②在历史分组尾部加一行「仅显示最近 30 天（`AGENT2SSH_APPROVAL_RETENTION_DAYS`）」。两条都是 `src/**` 显示改动，需拍板 |

**2026-11-11 补记（batch 10 第 2 轮，针对 L2-110）**：`revoked_at` 与 `approved_command_snapshot` 已补进 `src/types.ts` 的 `ApprovalRequest`（`docs/api.yaml` 与 `approval.rs:95/101` 本来就有），所以「镜像缺」那半不用再做。仍缺的是**没有任何面板渲染这两个字段**——`grep -rn "revoked_at\|approved_command_snapshot" src/` 只命中 `types.ts`。这与 `side_effect`（L2-110 正文）和 `recent_commands`（L2-74 补记）是同一族：**契约与后端都有，前端只镜像不消费**。要不要显示属产品问题，与 L2-110 一并拍板即可，不另立号。

## 2026-11-11 · batch 10 第 3 轮追加（「Copy command and open Execution」的剩余语义）

第 3 轮 4 条指摘，核实 **成立 3 / 部分成立 1**。其中两条在本轮作为 L1 落地（删掉不可见的 `copied` 状态、修正无 daemon 时的兜底文案 + 补测试），剩下两条都归到 **L2-115**，不再另立号——它们与 L2-115 是同一件事的三个面：这个按钮点了之后到底发生什么。

| # | 位置 | 判断 | 为什么不能自动落 |
|---|---|---|---|
| N1 → 并入 **L2-115 ①** | `src/App.tsx:1199-1204`（`onGoExecute` 只 `setSelectedHost` + `openModule("execute")`）、`src/App.tsx` 的 `<ExecPanel hosts={hosts} initialHost={selectedHost} …>`、`src/components/ExecPanel.tsx:16-26`（`Props` 无 command，`useState("uname -a")`） | 部分成立 | 指摘说「按钮名不符」过头了：文案是 `open Execution`，**打开模块**这件事确实做到了（本轮已把复制接上）；缺的是「顺带把命令填进去」这个能力，也就是 L2-115 的①。要落它得加 `initialCommand` prop + `ExecPanel` 的 state 初始化，属行为改动 |
| N4 → 并入 **L2-115 ①** | `src/components/ExecPanel.tsx:25`（`useState(initialHost)`）与 `:20-26`（`useEffect` 只依赖 `[targetHost]`） | 成立（潜伏） | `initialHost` 只在挂载时读一次，之后 prop 变化不生效。今天看不出来是因为切模块会卸载 `ExecPanel`；**一旦按①加 `initialCommand`，「第二张审批卡沿用上一个 host」就会立刻变成真 bug**。所以①落地时必须同时把 `initialHost`/`initialCommand` 改成「prop 变就 set」的同步 effect，测试要覆盖连续两次导航 |
| N2 | `src/components/approval/ApprovalDetail.tsx`（`copied` 状态与 `Check` 图标分支） | 成立 | `onCopyAndExecute` 会切模块（`App.tsx:1196` 是 `{activeModule === "approvals" && …}`），面板在同一个 tick 内卸载，`Check` 一帧都画不出来。**本轮按 L1 删掉 `copied` 与 `Check`**，`handleCopy` 变成 fire-and-forget；「剪贴板被拒时给 toast」仍是 L2-115 的第②半，未做 |
| N3 | `src/components/HostPolicyLine.tsx`（`summary === null` 的兜底数组）对照 `approval.rs` 的 `load_approval_policies`（`:969-980` 无文件返回空 `Vec`）与 `execution_control.rs` 的 `needs_approval`（`:490-492`，`policy.is_some() \|\| (risk == High && !force)`） | 成立 | 兜底文案原写 `low auto-approve` / `med/high need approval`，把 medium 报成需审批。真实默认是 low、medium 都无人管，只有 high 被 `host_policy_summary` 改写成 `Required`。**本轮已改成 `low/med auto-approve` + `high need approval`**，并新建 `src/components/HostPolicyLine.test.tsx`（此前该组件零覆盖）钉住兜底分支与 summary 分支 |

**L2-115 的现状（供拍板时一并看）**：①透传命令到执行页（含 `initialHost` 同步，N1+N4）；②剪贴板被拒时给提示。两条都在 `src/**`，都改用户可见行为。**N2 已作为 L1 关闭**——它不是独立缺陷，是①之前「按钮名不实」的副产物。

## 2026-11-11 · batch 10 第 4 轮追加（`HostProfile.init_command` 的镜像缺口与它造成的真实丢数据）

第 4 轮是自取轮（CodeBuddy 5×60 s + 90 s 无回复，标「等待超时」）。按 issue 正文首查项①「`src/types.ts` / `src/api.ts` ↔ `docs/api.yaml` 漂移」做机械对账：**路由层干净**（`api.ts` 用到的 18 条路径与 `api.yaml` 声明的方法逐条相符），漂移全在 schema 层。7 处类型镜像差异里，6 处是「契约更宽、镜像更窄」的差集（与 L2-110 同族），**只有一处有后果**。

| 编号 | 位置 | 判断 | 为什么不能自动落 |
|---|---|---|---|
| **L2-120** | `src-tauri/src/core.rs:186-244`（`update_host_core` 的 `config.hosts[original_idx] = host.clone()` 整条替换）对照 `types.rs:81-85`（`#[serde(default)] pub init_command: Option<String>`）、`embedded_ssh.rs:2326-2339`（连接后执行它）。**写入方共三处**（第 5 轮更正，原写「唯一写入方」不准确）：`bin/agent2ssh.rs:652/2007/2043` 的 `host add --init-command`；MCP `ssh_add_host`（`agent2ssh-mcp.rs:748`，`serde_json::from_value` 直落 `add_host_core`）；daemon `POST /hosts`（`agent2ssh-daemon.rs:1349`，`Json<HostProfile>`）。后两者与 `host add` 一样走到 `add_host_core` 的 `*existing = host.clone()`，**所以一个只带 `{name, host}` 的 `POST /hosts` 就能把 `init_command` 连同 tags/group/env 一起清空**，不需要 `PUT /hosts/{name}` | 成立（**数据丢失**） | 桌面端那条已经用 L1 修掉了（`types.ts` 补字段 + `AddHostForm` 携带 `editingHost?.init_command`），但**任何其他省略该字段的客户端仍会把它清掉**：`update_host_core` 只对 `password` / `passphrase` 做了「省略即保留」（`:200-216` 有注释解释桌面不回显），`init_command` 没有同样的保护，`config.hosts[idx] = host` 直接覆盖成 `None`。手工改过 `hosts.json`、或用 CLI 设过 `--init-command` 的主机，只要被任何 API 客户端整条 `PUT` 一次，连接后的那条命令就静默消失 | 二选一：①给 `init_command` 加哨兵（沿用 `passphrase` 的 `""` 约定，或改成 `Option<Option<String>>`）并在 `update_host_core` 里「省略即保留」——与既有两处的语义统一，代价是契约要写清哨兵；②承认 `PUT /hosts/{name}` 是**整条替换**，把 `init_command` 暴露到桌面表单里，让每次保存都显式带上它（属产品改动，要加表单控件）。①更彻底，②更符合 REST 语义但要求所有客户端都懂这个字段 |

**另外 6 处契约宽于镜像（只记录，不另立号，与 L2-110 一并拍板）**：`AuditEntry` 缺 `action`/`outcome`/`risk_unclassified`/`side_effect`；`AuditFilter` 缺 `command_pattern`/`host_env`/`host_owner`/`host_role`/`search`（桌面审计面板因此没有这些筛选入口）；`DaemonInfo` 缺 `scope`；`ExecRequest`/`ExecResult` 缺 `side_effect`（即 L2-110 本身）、`ExecResult` 另缺 `dropped_bytes`。`WebDavSyncStatus` 是**同名的前端视图模型**（camelCase 字段），不是镜像，属误报。

**生产方在开批更正里问的那条形状（`let (risk, _)` 忽略 `approved`）**：已核，**数量是 8 个不是 6 个**（它漏了 `ssh_connect`/`ssh_disconnect`），且**无行为后果**——8 处都不经过 `core::exec*` 的 `force` 闸门；真正需要 `approved` 的五个消费点（`exec`、`local_exec`、`proxy_exec`、`run_playbook`、`authorize_targets`）都消费了。**不另立号**，改为在 `authorize_command` 上写一段文档注释说明「哪些调用点必须消费、为什么其余 8 处可以不消费」，避免下一轮再被当成漏翻 force 报上来。

## 2026-11-11 · batch 10 第 5 轮追加（审批失败的卡片残留）

第 5 轮 4 条，核实 **成立 2 / 部分成立 1 / 不成立 1**。两条 L1（两处无保护剪贴板、兜底文案加限定词），下面一条是行为改动。

| 编号 | 位置 | 判断 | 为什么不能自动落 |
|---|---|---|---|
| **L2-121** | `src/App.tsx:462-486`（`handleApprove`/`handleReject` 的 `catch` 只 `showToast`，不把卡片从 `pendingApprovals` 摘掉也不重拉）、`src/api.ts:819-846`（非 2xx 抛 `Failed to approve/reject`）、`src-tauri/src/approval.rs:702-704`（`approval_respond_inner` 对非 pending 返 400） | 成立 | 可构造：请求已被 daemon 处理、但响应在路上丢了（本地超时／连接重启／代理 502），前端走 `catch` → toast「already Approved」→ 卡片**仍留在待办队列**，此后每次再点都得到同一个 400。`useAgentEvents`（`App.tsx:319-325`）只在收到 `approval_requested/responded` 时才重拉，而那条事件正是没收到的那条。**这不是纯 UI 问题**：它让「卡片还在 = 还没决定」这个基本前提失真 | 二选一：①`catch` 里补一次 `GET /approvals`（或 `api.fetchApprovals()`）对齐服务端，代价是多一次往返、且在断线时仍失败；②把 `approval_respond_inner` 的「已决定」映射成 409 + 当前状态，前端把 409 当成功处理（幂等化），代价是改 `approval.rs` 的对外状态码与 `api.yaml`。②根治但属契约改动，①只动前端 |

**顺带更新 L2-115 的范围**：`ContainersPanel.copyCommand` 与 `KeysPanel.copyPublicKey` 本轮已加同一套缺失保护（原来直接 `navigator.clipboard.writeText(...)`），但「写失败要不要 toast」这个产品问题现在有**四个**站点（审批卡、容器、密钥、以及将来的任何复制），应由一个 `copyText` helper 统一决定，而不是四处各写一个 `.catch(() => {})`。

## 2026-11-11 · batch 10 第 5 轮追加（`import_team_config`：服务端自己的合并逻辑丢 `init_command`）

第 5 轮回答生产方第 4 轮的两个提问时，在 `init_command` 这条线上抓到第三处丢失，而且它不在任何客户端里——是服务端自己的合并函数丢的。**已 L1 修掉**，记录在此是因为它与 L2-120 是同一病因的两个面。

| 位置 | 判断 | 关键事实 | 处置 |
|---|---|---|---|
| `core/team_config.rs:87-133`（`import_team_config` 的 `*existing = next`）、`:250-265`（`team_host_same`）对照 `:34-46`（`export_team_config` 只剥 `key_path`/`password`/`passphrase`） | **成立（数据丢失）**，比 L2-120 更难发现 | 导出**保留** `init_command`，导入端只把三个字段从旧值补回，`init_command` 落到包里的值。于是「导入一份来自没设过它的机器的团队配置」= 清掉本机那条命令。更糟的是 `team_host_same` 的 11 个比对字段里**没有**它，所以 `preview` 会把这条主机报成 `hosts_to_skip`（「什么都没变」），**然后把它改掉** | **L1**：`:111` 补 `init_command` 保留；`team_host_same` 上写清「它不进比对字段，因为进了就会报一个不改的差异」；新增 `test_team_config_import_preserves_a_local_init_command`（无则保留 + 有则采用，两个方向都钉） |

**为什么「保留」而不是「按包覆盖」**：与 `password`/`passphrase` 不同，`init_command` 不是凭据，导出**没有理由**剥掉它——所以包里缺这个字段只可能意味着「来源机器没有」，不可能意味着「分享者关掉了」。这一点让「省略即保留」在这里是有据可依的，而不是 L2-120 那种需要哨兵才能表达的选择。

**与 L2-120 的分工**：L2-120 是「客户端用自己拼的残缺载荷覆盖整条记录」，这里的 L2-40 补记是「服务端用包里的残缺记录覆盖整条记录」。两者都需要「省略即保留」才能根治，**但这一处可以就地补，因为不存在「显式清空」的产品需求**（没有任何界面能清空 `init_command`，而 `password` 有）。所以本轮补了，L2-120 仍要拍板。

## 2026-11-11 · batch 10 第 5 轮收尾（L2-121 降成半条，只剩契约那一半）

本轮按「第 6 轮关帖前最该落的一件事」的自问，挑了 **L2-121**——它是本轮唯一一条**会让用户做错事**的缺口（卡片残留 ⇒「卡片还在 = 还没决定」这个前提失真），而不是继续铺新面。

**L2-121 已拆成两半，前半已落、后半仍待拍板。**

| 半边 | 状态 | 落法 |
|---|---|---|
| 前端：`catch` 里不做任何对齐 | **已落（本轮）** | 见下 |
| 契约：把「已决定」幂等化成 409 | **仍在此表**，需拍板 | 改 `approval.rs` 的对外状态码 + `api.yaml`，是契约改动 |

本轮落地的是「**只在确有证据时摘卡**」，一个既不是 ①「无条件重拉」也不是 ②「改契约」的第三条路：

- 新增 `src/lib/approval-decision.ts` 的 `isAlreadyDecided(message)`，只认 `already (Approved|Rejected|TimedOut|Revoked)`。判据是**消息**不是状态码，因为两条路由对每一种拒绝都返 400（`bin/agent2ssh-daemon.rs:3384`、`:3398`），而只有 `approval_respond_inner` 的 `already {req.status:?}`（`approval.rs:699-701`）意味着「已决定」。**rewrite 被 blocked、rewrite 更危险、has timed out、unknown approval 都不能匹配**——它们对应的是「请求仍然 pending」，误匹配会把操作员还没答的请求从队列里抹掉，比残留更坏。同理不写 `already \{[^}]*\}` 这种宽松尾巴，否则将来的 `already blocked by policy` 会把活卡片退役。4 条单测钉住这两侧。
- `App.tsx` 的 `handleApprove` / `handleReject` 的 `catch` 改走 `reportUndecided`：**先原样 toast（措辞不变）**，再仅当 `isAlreadyDecided` 为真时补一次 `api.fetchApprovals()`，**并且只有 daemon 把那一条列为 `non-pending` 才摘卡**。这样 ① 那种「断线时重拉也失败」被处理掉了——不是靠重拉成功，而是靠「拿不到证据就不摘卡」：`fetchApprovals` 在 daemon 不可达时返回 `[]`（`src/api.ts:794-806`），而空表**不是**「已决定」的证据。
- 代价诚实写出：这一路仍会被「400 但不是已决定」的路径**留在队列里**（与修前一致），也不会修正本地 `pending` 之外的任何状态。它只解决「已决定却还挂着」。

**② 仍在此表，且本轮取证后它更不急着做**：把 400 变 409 是契约改动，而前端的可观察后果现在只剩「toast 里仍是 `Failed to approve request: approval … already Approved`」——一个措辞问题，不再是「卡片再也摘不掉」。

**为什么 ② 不归 L1**：`approval_respond` / `approval_approve` 的 400 是 `api.yaml` 里 `/approvals/{id}/approve` 与 `/approvals/{id}/reject` 的 responses，改它要同步契约、Rust 与前端三处，属对外契约。落地要求：`cargo test --no-default-features --lib` + `cargo test --lib`（default feature 才编 `tauri_commands`）+ `npm test`，并在 `api.yaml` 写明 409 携带的 `status` 字段。

## 2026-11-11 · batch 10 第 5 轮收尾（`(no policy)` 的限定词够不够）

上一轮把兜底文案从 `low auto-approve` / `med/high need approval` 改成 `low/med auto-approve (no policy)`。复核结论：**方向对，但词选窄了**，本轮改成 `low/med auto-approve (no policy configured)`（中文 `low/med 自动放行（未配置策略）`）。

- `(no policy)` 读起来像名词短语「没有策略这件事」，可以指「**此刻**没有策略命中」——而那正是 `summary !== null` 分支的情况，两支的限定词就互相打架了。兜底分支的真实前提是**没有配置文件**（`load_approval_policies` 在 `policy.toml` 与 `approval_policies.toml` 都不存在时返回空 `Vec`，`approval.rs:969-980`），所以 `configured` 是必须的，它把「无文件的默认」与「有文件但没命中」分开。
- 但没有采用更显式的「本条只描述无策略时的执行页」：这是一枚 `text-[11px] uppercase` 的一行 chip，带「执行页 / 审批页」两个词会把 11px 的一行推到两行，并把读者的注意力从三个风险档挪到一个元声明上。**取舍写在这里，而不是写进 UI**：限定词负责「不撒谎」（它不再暗示 medium 一定不进卡），注释负责「说全」（`HostPolicyLine.tsx:78-86` 记了两支的语义分工）。若第 6 轮判定仍不够，唯一还能加而不撑爆这一行的是把 high 那份改成 `high need approval (always)`，让「无条件」与「无策略」在视觉上对称——我没有自己拍。

**同步已改**：`i18n.tsx:843` 的 key 与译文、`HostPolicyLine.test.tsx` 两条断言（`i18n.test.tsx:72-74` 会遍历源码里的 `t("…")` 字面量并要求 key 在表里，所以 key 改名必须三处一起动，否则测试先红）。

## 2026-11-11 · batch 10 第 5 轮收尾（`ContainersPanel` 的守卫覆盖范围）

复核结论：**`ContainersPanel` 内部覆盖完整，但它不是最后两处**——同一个缺陷在另外三个站点上还开着，其中两处的结果**比「死按钮」严重**，因为它们会在剪贴板写入**真的失败了**的时候报「已复制」。

| 站点 | 状态 | 说明 |
|---|---|---|
| `ContainersPanel.copyCommand` | **已覆盖**（上一轮） | 唯一调用点 `:261` 的 `IconButton` 也只走这一条；`copied` 状态在同一函数里设，没有第二条路径能翻转图标 |
| `KeysPanel.copyPublicKey` | **已覆盖**（上一轮） | 唯一调用点 `:267`；`k.public_key ?` 已保证文本非空 |
| `McpAgentsPanel.copyCommand`（`:146-150`） | **未覆盖** | 直接 `await navigator.clipboard.writeText(...)` 再 `setCopied`。事件处理器里的 reject ⇒ `setCopied` 不执行 ⇒ 死按钮；`navigator.clipboard` 为 `undefined` ⇒ TypeError 同上。本轮的 `(no policy)` 之外唯一「同一形状还没补」的一处 |
| `AuditPanel.copySelectedAsJson`（`:260-265`） | 有 `.catch`，**但顺序错** | 同 `ContainersPanel` 修前一样的问题：先 `setCopied`/`showToast` 再等 promise。`writeText` 在不可用时**同步抛** TypeError，`.catch` 挂在它返回的 promise 上（根本不返回），于是 toast 已经报「Copied {count} entries to clipboard」——**撒谎的确认**，比静默失效严重 |
| `SettingsMenu.copyConsoleUrl`（`:255`） | 部分覆盖 | `try/catch` 包住了，能落到 `failed`；但 `navigator.clipboard` 为 `undefined` 时抛的是 TypeError，`catch` 能接住 ⇒ 实际可用。**不是缺口** |
| `HelpPanel.copy`（`:30-37`） | 部分覆盖 | 同上，`try/catch` 已够。**不是缺口** |
| `TerminalView`（`:170`、`:184`、`:259`） | 覆盖 | 显式 `?.writeText` 检查 + `clipboard_unavailable` reason。**不是缺口** |

**所以 L2-115 的范围要再扩一次**：不是「四处各写一个 `.catch`」，而是 **`McpAgentsPanel` 一处的保护 + `AuditPanel` 一处的顺序**，加上「复制失败到底该不该给确认」这一个产品口径。三件事都要一个 `copyText` helper 才能一次收口——helper 必须**同步可判**（返回 `{ok:true}` / `{ok:false, reason}` 或抛给调用方），否则调用方又会把确认写在 await 之前。本轮不动：`AuditPanel` 的修法要选「换顺序」还是「换 promise 形状」，`McpAgentsPanel` 的修法要选「静默」还是「toast」，两个都是 L2-115 那个尚未拍板的产品问题。

## 2026-11-11 · batch 10 第 6 轮（收尾轮）：两处更正 + 一处越过 L2 边界的修复

### 一、`9f965f5` 是**未验证的 Rust 行为改动**，直接落在 main 上，且是红的

`9f965f5`（`import_team_config` 保留 `init_command`）改的是 `src-tauri/src/core/team_config.rs` 与 `src-tauri/src/core.rs`，按本流程的定级属 **L2**（`src-tauri/src/**` 行为改动），本应只登记、不动 main。它的 commit message 自己写明「本容器没有 Rust 工具链，`cargo test` 与 `cargo fmt --check` 没有跑」——也就是说这笔是**按未经执行的验证**声称完成的。

本机复跑的结果：`cargo test --no-default-features --lib` **862 passed / 2 failed**，多出来的那条失败正是它新加的 `test_team_config_import_preserves_a_local_init_command`。

**根因不是实现写错，是设计矛盾**：`import_team_config` 里 `if next.init_command.is_none() { next.init_command = init_command }` 是对的（包里没带就保留本地），但 `team_host_same` 把 `init_command` 完全排除在比较之外，而它正是「跳过」的闸门。于是包里**带了**一个不同的 `init_command` 时，11 个字段全等 → 判为 same → 整条跳过 → 包里的新值永远不落地。它的测试第二半（「包里有值则以包为准」）正是在断言这一半，也正是在这里失败。

**已就地修复**（`core/team_config.rs`）：比较条件改成 `incoming.init_command.is_none() || existing.init_command == incoming.init_command`——包里没带才算「相同」，带了且不同就算「不同」，让跳过闸门与保留语义同向。全量复跑 **862 passed / 1 failed / 1 ignored**（仅剩那条故意红的 IPv6），`cargo fmt --check` 绿。

**这条记录的目的不是追责，而是留证据**：L2 边界的意义就是「未经人拍板的行为改动不进 main」，而这一次它进了、且是红的。下次任何人（包括复核方自己）要往 main 推 `src-tauri/src/**`，请先在本机跑 `cargo test --no-default-features --lib`——**没有工具链就不写这条代码**，写进 L2 表比写进 main 便宜。

### 二、更正：`AuditPanel` 的失败形态不是「报了成功」

复核方写「`.catch` 挂在一个根本没返回的 promise 上，于是 toast 已经报了 `Copied N entries`——在看到失败的情况下撒谎」。**方向反了**：`navigator.clipboard` 为 `undefined` 时，`navigator.clipboard.writeText(...)` 在**取属性那一刻**就抛同步 TypeError，`.then` 与 `.catch` 都还没被挂上，所以**成功与失败的 toast 都不会出现**——不是撒谎，是彻底静默（异常走 `window.onerror` 进 `app.log`）。本轮已补守卫并在不可用时给一条明确 toast。

### 三、剪贴板一族收口（五个站点）

| 站点 | 原状 | 现状 |
|---|---|---|
| `ApprovalDetail.handleCopy` | 第 2 轮才接线，且会抛 | `?.writeText` 守卫 + `.catch`（第 2 轮） |
| `ContainersPanel.copyCommand` | 裸调 | 守卫 + `.catch`（第 5 轮） |
| `KeysPanel.copyPublicKey` | 裸调 | 守卫 + `.catch`（第 5 轮） |
| `McpAgentsPanel.copyCommand` | `await` 裸调 | 守卫（第 6 轮） |
| `AuditPanel.copySelectedAsJson` | 取属性即抛 | 守卫 + 明确的「不支持剪贴板」toast（第 6 轮） |

**仍待拍板的只剩「写失败要不要统一提示」**（L2-115）：现在有四处 `.catch` 是空的、一处给了 toast，风格不一。抽 `copyText` helper 时一并定，不要各自为政。

### 四、批末仍 open 的 L2（供下一批按序拍板）

| 编号 | 一句话 | 影响面 |
|---|---|---|
| L2-115 | 复制失败要不要提示 + 是否抽 `copyText`（现五个站点风格不一） | `src/**`，产品口径 |
| L2-118 | `HostPolicyLine` 的 `auto` 一词两义（无策略 vs 策略显式放行） | `src/**` 显示 |
| L2-119 | `decidedToday` 按 `requested_at` 判日 + 30 天保留期在 UI 上静默截断 | `src/**` 显示 |
| L2-120 | `update_host_core` 对省略的 `init_command` 不做保留（桌面端已修，其他客户端仍会清空） | `src-tauri`，需哨兵 |
| L2-121（后半） | 已决定的审批请求幂等化成 409（前端半边已落） | 契约：`approval.rs` + `api.yaml` |

## 2026-11-11 · batch 11 第 1 轮：L2-120 的措辞更正（`PUT /hosts/{name}` 这条路由不存在）

L2-120 的建议里写着「承认 `PUT /hosts/{name}` 是**整条替换**」，但**这条路由不存在**：`docs/api.yaml:1265-1301` 的 `/hosts/{name}` 只有 `delete`，`update_host_core` 的唯一生产调用点是 `tauri_commands.rs:330`（桌面 Tauri 命令），HTTP 侧没有对应入口。所以拍板时：

- 行为缺陷的入口只有 `POST /hosts`（`bin/agent2ssh-daemon.rs:1349` → `add_host_core` → `add_host_core_impl:172-173`），CLI 的 `host add` 与 MCP 的 `ssh_add_host` 走同一条；
- 选项②应表述为「把 `POST /hosts` 的整条替换语义写进契约，并要求客户端提交完整记录」，不要再把 `PUT` 当作既有契约的一部分。

### 同一轮的另一半：HTTP 上的读-改-写会连凭据一起清掉

`GET /hosts` 的每个响应都过 `redacted_for_transport`（`types.rs:105-109`），`password` 与 `passphrase` 一律置 `None`。所以「先 GET 取回整条 → 改一个字段 → 整体 POST 回去」这条看起来最安全的路径，实际会把已存凭据清空——比省略 `init_command` 更难察觉，因为主机名、分组、标签全都对，只有下次连接才失败。

这是**脱敏的正确代价**，不是缺陷。拍板时要一并决定的是「要不要给一个可回传的 `secret_ref` 句柄」或「明确只允许桌面端做部分更新」。本轮只把这条写进两份指南与 `api.yaml`（`a54fcd0`），不改行为。

## 2026-11-11 · batch 11 第 3–6 轮：L2-122（同一份 `execution_gate.json` 读失败，一族 fail-closed，一族 fail-open；第 4 轮补第四条，第 6 轮定落地形状）

三条都出自「契约里 423/429 到底可不可达」这条线，都需要动 `src-tauri/src/**`，本轮只记录。

### 一、`terminal_broadcast_preview_results` 把 gate 读失败当成「未暂停」（最重）

`bin/agent2ssh-daemon.rs:4331`：

```rust
let gate = load_execution_gate().ok();
```

后面是 `gate.as_ref().is_some_and(|status| gate_blocks_source(status, source))`（`:4372-4378`）。`load_execution_gate`（`gate.rs:46-57`）只在**文件存在但读不了或解析失败**时返回 `Err`（文件不存在返回 `Ok(default)`，即 Active）。于是：

| 场景 | `reject_if_gate_paused`（`daemon.rs:381-388`） | `terminal_broadcast_preview_results` |
|---|---|---|
| 文件不存在 | 放行 | 放行 |
| 文件存在、解析失败 | **500，拒绝** | **放行** |

也就是说 `execution_gate.json` 一旦损坏，普通 exec 一族全部 fail-closed，而 `/terminal/broadcast` 与 `/terminal/broadcast/preview` 照常授权并向多条终端发命令。这两条路由恰好是「一次影响 N 个目标」的那一族。

要拍板的是**期望语义**，不是实现细节：gate 读失败时，广播一族该 500 拒绝（与 exec 一致）还是继续？如果选后者，那 `reject_if_gate_paused` 的 500 才是那个需要改的；两者必须统一，不能一族一义。改法也影响契约：现在是「不产 423」的第三条豁免，改成 500 就要把豁免表从 `INBAND_REJECTIONS` 里挪出来。

### 二、`forward_remove|stop|start` 的外部臂不写审计

`forward_remove:3058` 的结构是：

```rust
if let Some(rule) = forward_list_core().await...find(|rule| rule.id == uuid) {
    // gate → 限流 → scope → authorize → append_operation_audit
    return Ok(Json(OkBody { ok: true }));
}
forward_remove_core(uuid)     // :3108
    .await.map(...).map_err(|e| err(StatusCode::BAD_REQUEST, e))
```

命中规则时那条臂会写 `append_operation_audit`（`:3098-3106`），不命中时**什么都不写**。`forward_stop`（`:3140`）与 `forward_start` 同构。

问题不在于「不该删不存在的隧道」，而在于**这个操作在审计里没有痕迹**：同一条 `DELETE /forwards/{id}` 请求，返回 400 还是 200 可以在事后核对，但它有没有进过审计完全取决于规则当时在不在。L2-102 是同一族的取数范围问题，这条应并进去，而不是单独修——单独补 `append_operation_audit` 会让「未命中」也产生一条和「命中」同形的记录，反而更难分辨。

### 三、`/sftp/upload` 没有**调用方**能调的连接预算，`rm-rf` 有

`SftpDirBody` 带 `timeout_secs`（`core.rs:2446` 取默认 60，`:2784` 传给 `connect_embedded_ssh`），`SftpUploadRequest`（`types.rs:711-723`）没有这个字段，`sftp_upload_core_with_source:1910` 也没有对应形参。

两个方向都可能：① 上传也该有连接预算（大文件上传在慢链路上会撞 `connect_embedded_ssh` 的超时，而调用方无法调），那就加字段；② 上传本来就不该有（超时该由传输层自己管），那就把「为什么只有目录操作有」写进 `/sftp/upload` 的描述。

**第 4 轮已跟到底，措辞要收窄**：不是「没有超时」，而是「没有**调用方**入口 + 有一份隐式预算」。`sftp_upload_core_with_source:1957` 传的是字面量 `60`，`connect_embedded_ssh` 把它分成三处（`embedded_ssh.rs`）：`connect_tcp_address` 的 `timeout_secs.min(30)`（`:851`，拨号上限，多地址共享）、`set_read_timeout`/`set_write_timeout`（`:1676-1677`）、`session.set_timeout(session_timeout_ms(60))`（`:1687`，请求-应答）。

所以这条真正要定的是：**这个 60 s 该不该由调用方调**。`/sftp/rm-rf` 能调，`/sftp/upload` 不能，两者走的是同一族 `connect_embedded_ssh`——不对称成立，但「upload 有 bug」不成立。契约层已在第 4 轮写明这段（`api.yaml` 的 `/sftp/upload` 描述）。

### 四、daemon 侧没有 `/sftp/cancel` 路由，`transfer_id` 对 HTTP 调用方是死的

第 4 轮核实：`sftp_cancel` 只存在于 `tauri_commands.rs:650`（桌面 Tauri 命令，`:3657` 注册）。daemon 的 sftp 路由共 10 条（`agent2ssh-daemon.rs:5875-5884`），**没有 cancel**。

于是 `SftpUploadRequest.transfer_id`（`types.rs:721-723`）经 HTTP 传进来后，`sftp_upload_core_with_source:1952-1955` 会 `sftp_transfer::register(id)` 建一个取消标志，但 daemon 进程内没有任何路由能把那个标志置位——注册了、永远等不到取消。

两个方向：① 加 `POST /sftp/cancel`（新路由 = 新契约面 + 权限口径要定，比本条其它项都大）；② 明确「HTTP 上的 `transfer_id` 只为审计/幂等预留，取消只在桌面端」，写进契约即可（**第 4 轮已按 ② 落了描述**，所以这条只需拍板要不要往 ① 走）。

## 2026-11-11 · batch 11 第 5 轮：L2-122 第 1 条的判定（fail-closed）与两条新发现

上一轮把「gate 读失败时广播一族该 fail-open 还是 fail-closed」交回给人拍板。本轮给出判定和判据，**不改代码**：两条新发现都落在 `src-tauri/src/**` 的授权顺序上，动它们不是文档收窄，是本批之外的一笔。

### 一、判定：fail-closed。广播一族与 exec 一族同一个语义

判据只有一条，而且它不依赖 `broadcast` 这个动词：

> **一个决定要不要执行的安全开关，在「读不出来」和「读出来是 Active」之间没有第三种事实。把前者当成后者，等于让一个损坏的文件替操作员按了 resume。**

`execution_gate.json` 是操作员的急停按钮（`agent2ssh pause` 写的是同一个文件）。它读不出来，说明「现在是否急停」这件事**未知**——而未知不是允许。`reject_if_gate_paused` 的 500 早就这么处理了，那句错话写在 `api.yaml:26-32`，措辞故意直白：「An unreadable gate file is an operator-fixable condition, not a crash」。广播一族偏离的不是「exec 的实现方式」，是这条语义本身。

**为什么「一次影响 N 个目标」让这个判定变成必要条件，而不是加分项。** 广播的风险是基数乘积：一次调用 × 最多 32 个目标。执行器本身有 8 个在途槽位、限流按目标逐个记，加一次重试就可能上千次 shell 交互。对单目标路径说「读不出来就放行」，代价是 1 次命令；对广播说同一句话，代价是 32 次。同一个原则在高基数一侧要更严，不能更松——而现在恰好是更松。

**反方向（改成 fail-open）的成本，这里记清楚，免得下一轮又重开。** 若选 fail-open，必须同时改 `reject_if_gate_paused` 的 500——否则同一份 `execution_gate.json`、同一个操作员动作，`/exec` 拒绝而 `/terminal/broadcast` 照跑，这不叫取舍，叫两套语义。而改成 fail-open 会让 `GET /gate` 变成全系统唯一一处告诉你「急停也许没生效」的地方，且它自己也是 500。所以那一半的收益是零，代价是让急停按钮在文件损坏后静默失效。

**落地形状（下一轮可以直接照做，不需要再拍板）：**

1. `terminal_broadcast_preview_results:4331` 的 `load_execution_gate().ok()` 换成 `?` 传播，让两条路由与 exec 一族共用同一个 500 文案（`failed to load execution gate: …`）。

   **`responses` 不加 500（第 5 轮更正）**：`info.description` 的**第二段**（第 3 轮加的）原文是「So every operation that consults the gate can answer 500 even though its body only ever names 423 — which is why none of them lists the gate-read 500 in its own `responses`」。33 个读 gate 的 operation 里 30 个只写 423、500 由那一段统一承担；只给两条广播路由补 500，会让它们成为 33 个里仅有的两个例外——这正是本批反复在修的「同一族里一条写一条不写，从任何一条看过去都像一致」。上一稿把全局段误认成只有 `auth_middleware` 那一条，漏了它后面紧跟的这一段。
2. `INBAND_REJECTIONS` 从 3 条变 2 条（`/terminal/broadcast` 与 `/terminal/broadcast/preview` 出表），`exec_stream` 留表——它的 `reject_if_gate_paused` 用 `.is_err()` 吞掉，升级后仍只发帧，语义上它也没在做决定：升级已经在 socket 里，拒绝一帧与断开连接等价。**这三条要同笔改**，因为第三段断言双向校验「出表的不能再写 423」。

### 二、新发现 A：广播的审批回执**从不影响投递**，只进审计（比第 1 条重）

`terminal_broadcast_run` 的流程是 preflight → 限流 → `authorize_command(...)`（`:4497-4522`，真的会等审批）→ `all_or_nothing` 复查 → 投递。看着对。错在投递条件：

```rust
let sender = ...;                  // 只按 principal + host 过滤（:4534-4552）
for (index, sender) in senders.into_iter().enumerate() {
    if !results[index].authorized { continue; }   // :4558 —— 这个字段 preflight 时已是 true
    ...
}
```

`authorize_command` 的 Err 臂只写 `error`，**不清** `authorized`（对照 `:4497-4522` 与 `:4395`：该字段在 `check_approval_required` 的 `Ok` 臂就已置 true）。所以：

| `authorize_command` 结果 | `authorized` | `error` | 实际投递 |
|---|---|---|---|
| `Ok` | true | — | 投递 |
| `Err`（`ApprovalRejected` 403 / `ApprovalTimedOut` 408 / 风险 400 / 内部 500） | **true** | 被覆盖成那条错误 | **仍然投递** |

`all_or_nothing: true`（默认值）也拦不住：它查的是 `results.iter().any(|r| !r.authorized)`，而全 true。唯一「拦住」的形态是 `all_or_nothing: false` + 目标之间风险不一致——那时先被拒绝的目标保持 true 照发，后面的目标被 `authorized=false`，等于把「被拒的那台照发、没被拒的那台不发」演成了。**拒绝在审批卡上按下去，命令照样送达**，这不是文档层能收窄的，属 `src-tauri/src/**` 行为，建议新立一条 L2-123。

修法（小，但需一条 e2e）：`authorize_command` 的 Err 臂补 `results[*index].authorized = false`，与 `:4502`/`:4509` 的限流臂同形；或把投递条件改成 `results[index].authorized && results[index].error.is_none()`——前者更好，因为「有没有被授权」的字段就该反映这件事。

### 三、新发现 B：同一个 operation 的限流与授权用了两套风险与两套审批事实

同一次 `/terminal/broadcast` 调用里：

- **限流**按 preflight 的目标集算（`:4470-4490`，`check_execution_batch` 是 all-or-nothing，任一目标越限即全拒）；
- **授权**按 `authorize_command` 逐个重算（`:4497-4522`），而它**唯一**的 override 来源是 `command_authorization_target(host).risk_override`（`execution_control.rs:189-207`，读 `hosts.json` 的 `HostProfile.risk_override`）。

于是 `preflight` 与执行之间的差异不止「跳过审批」一件：

1. **风险 override 不继承**：preflight 用 `apply_risk_override(effective_command_risk, host_risk_override(&h))`（`:4382`），`authorize_command` 传的是 `None`（`:4506` 的 `risk_override` 位），函数内部自己再从 `host_risk_override(host)` 取一次（`execution_control.rs:752`）。两处**同源但不同入口**——现在恰好一致，但这是巧合。
2. **审批事实在第二段丢失**：`authorize_command` → `check_approval_required` 只看 `policy.toml`。preflight 把 `risk == High && !body.force` 也算作 `requires_approval`（`:4393-4394`），而这条规则在 `execution_control.rs:487` 里**要等的人答了才叫 approved**。也就是说，一个 `requires_approval: true, force: false` 的高风险命令，`/preview` 说不问，`/broadcast` 会问——`api.yaml` 的 `TerminalBroadcastResponse.authorized` 描述里那句「`authorized: true` with `requires_approval: true` only means 'nothing refused it yet'」正是在说这件事，但它写在错误的位置上：读的人会以为那是 preview 的一家之言，实际是两条路由的**共同**性质。

这两条与 L2-122 第 1 条同族（「preflight 与执行之间的语义漂移」），但修法不同：A 是补一个字段赋值，B 要让 preflight 与执行走**同一个**风险/审批函数。**B 建议并入 L2-101 一起拍**（那一条已经在问「审批依据是请求时刻的快照还是执行时刻的事实」），否则会拆成两个各自都不完整的答案。

### 四、保持原判定（不重开）

1. **「把 `/terminal/broadcast` 与 `/terminal/broadcast/preview` 的路由删掉一半」——不成立，不值得讨论。** preview 的价值正是让人在按下去之前看到 per-target 裁决；第 2 条里的 `authorized` 语义问题该修字段，不该砍路由。
2. **「`read` 一次 gate 就够，所以不应该在 preview 放 gate 检查」——不成立。** `terminal_broadcast_run` 调的就是 `terminal_broadcast_preview_results`，是**同一个函数**；把检查移到 `run` 里就等于让 `run` 直接用 preflight 的结论，那正是第二节那个 bug 的形状（结论被复用、事实被重算）。

**勘误（本轮自查）**：第二节的「新发现 A」被我写反了。`authorize_command` 的 `Err` 臂在 `:4503-4506` **确实**补了 `results[*index].authorized = false`——我只读了 `Ok` 臂和投递条件（`:4534-4560`），没等到读完 `:4503` 就开始写这张表。所以：

- 上表第二行的「仍然投递」**不成立**，那一行作废；
- 投递条件 `if !results[index].authorized { continue; }` 因此是对的，`all_or_nothing` 的两段复查也都在。
- 「新发现 A」整节**撤回**，不立 L2-123。

留下的教训记在这里，因为它比那条错指摘更有用：**判「字段没被清」必须把同一个函数的两个臂都读完**。这一批前面几条（`/shutdown` 的 500、`reject_if_gate_paused` 的 `.is_err()`）都是靠「读完整个臂」才判对的，这次是我自己破了一次。

**第三节「新发现 B」不撤回，但它要收窄一半。** 重核后：

- 风险 override 那一半**不成立**：preflight 走 `host_risk_override(&endpoint.host)`（`:4382`，读 `endpoint.host`），`authorize_command` 在 `execution_control.rs:752` 走 `host_risk_override(host)`（读传入的 `host`，即同一个 `endpoint.host`）。同源同值，`risk_override: None` 是有意让函数内部接管，不是丢字段。
- 审批事实那一半**成立**，而且要说得更准：`authorize_command` 内部 `needs_approval = approval_policy.is_some() || (risk == High && !input.force)`（`execution_control.rs:487`），广播传的是 `request.force`（`:4511` 一带的 `force` 位）。所以「高风险 + 未 force」在**两段**都会被判定为需要审批、都会真的弹卡——`/preview` 不弹只是因为 `terminal_broadcast_preview` 根本没走到 `authorize_command`（它只调 `terminal_broadcast_preview_results`）。

  于是 `api.yaml` 里 `authorized` 的描述**不是错，而是精确的**：「On `/terminal/broadcast/preview` it grants nothing — the route sends no input and raises no approval card, so `authorized: true` with `requires_approval: true` only means "nothing refused it yet"」。它已经把「preview 不弹卡」写在句子里了。

  这一半也不新立条目：它已经是 L2-101 的同一句话（「审批依据是请求时刻的快照还是执行时刻的事实」），而广播这里的答案是「两段都真判、preview 只是不问」——不存在快照漂移。

### 五、`expect_pid` 那句的正确措辞（给 `docs/api.yaml` 的 `/shutdown`）

当前 `api.yaml:1138-1146` 的原文是：

> `{"expect_pid": "1234"}` …stops the daemon without a check and the 409 never fires, and the same goes for a negative or out-of-range PID, or a truncated body. `0` is the exception on the other side: it is inside `u32`, so it parses, gets compared, and is refused with 409 — "out of range" here means outside `u32`, not merely implausible as a PID.

「部分成立」的分歧点只剩一个词：`out-of-range` 指不指「超出 `u32`」。我认为指，理由不是用法习惯，而是缺陷的**分界线在解析这一步**：

- **解析失败**（`"1234"`、负数、> `u32::MAX`、截断的正文）→ axum 把整个 body 当「没发」→ `None` 臂 → 不比对、直接停。
- **解析成功但不像 PID**（`0`、`1`、等于自己的 pid）→ `Some` 臂 → 真比对 → `0`/`1` 落 409，等于自己的 pid 落 200。

同一份契约里两句话在讲同一个分界的两侧，所以措辞应当把**机制**写在前面、把 `0` 写成例外。建议改成（照此替换那三句）：

> A body that does not parse is *dropped*, not refused: `{"expect_pid": "1234"}` — the string a shell produces from `"$PID"` — stops the daemon without a check and the 409 never fires. The same goes for every body the `u32` parse rejects, which is the whole family: a negative PID, a PID above `4294967295`, or a truncated body. `0` is the other case and does *not* belong to that family: it parses, so it is compared, and is refused with 409 because it is not this process. The rule is therefore "parsed ⇒ compared", not "plausible ⇒ compared" — an implausible-but-representable PID is still checked.

改动点只有三处，都为了同一个目的：**把「大于 `u32`」从「不合法」里点名出来**，并让 `0` 的原因从「u32 内」升级成「解析成功 ⇒ 比对」。前者是机制，后者才是读者要的行动指南。

### 五、收尾清单（第 6 轮定形，可直接照做）

第 5 轮把第 1 条判成 fail-closed 后，第 6 轮复核确认了落地形状。**下面五步是一笔改动，缺一步就是编译红或文档说谎。**

> **行号是 `ad9fdf1` 的快照，不是契约。** 本清单里每个 `:NNNN` 都锚在一个**符号**上（函数名、常量名、配置键）；行号漂了按符号重新定位，遇到形状与本清单不一致时，**先当代码变了、别当清单错了**——唯一允许照抄的是文案与「按码豁免」这两条语义。

**第 1 步：把唯一的 `.ok()` 换成带文案的 `?`。**

`bin/agent2ssh-daemon.rs` 里 `load_execution_gate()` 共 3 处消费（`:386` 的 `reject_if_gate_paused`、`:1265` 的 `gate_status`（`GET /gate`）、`:4331` 的 `terminal_broadcast_preview_results`），前两处都 `map_err` 成 500，第三处用 `.ok()` 吞掉——**全仓唯一的孤例**（`gate.rs:83` 那处消费在库内且已 `?`，不在 daemon 路由面上）。`?` 不能直接写：`load_execution_gate` 返回 `anyhow::Result`，handler 要 `(StatusCode, Json<ErrorBody>)`，必须过一次 `map_err`，文案要与 `:386`/`:1265` 逐字相同：

```rust
let gate = load_execution_gate().map_err(|e| {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("failed to load execution gate: {e}"),
    )
})?;
```

`terminal_broadcast_preview_results`（`:4314`）的返回类型随之变成 `Result<Vec<TerminalBroadcastTargetResult>, (StatusCode, Json<ErrorBody>)>`，两个调用点 `:4411`（`terminal_broadcast_preview`）与 `:4429`（`terminal_broadcast_run`）各加一个 `?`——两者本来就返回 `Result<Json<…>, (StatusCode, Json<ErrorBody>)>`，签名不用动。`?` 求值在 `let registry = …lock()` 之后、`drop(registry)` 之前，中途返回时 `MutexGuard` 正常释放。

**第 2 步：豁免表从「按路由」改成「按状态码」。**

改完后两条广播路由的 423 变成 HTTP 可达，而 `/terminal/broadcast` 的 **429 仍然**是 per-target（`terminal_broadcast_run:4469` 的 `if let Err` 不动），`/terminal/broadcast/preview` 则完全不调限流。所以「两条出表、一条留表」这个说法是错的，正确形态是**按码豁免**：

| 改动后 | 423 | 429 |
|---|---|---|
| `GET /exec/stream` | 帧内（留表） | 帧内（留表） |
| `POST /terminal/broadcast` | **HTTP（出表，`responses` 补 423）** | per-target（留表） |
| `POST /terminal/broadcast/preview` | **HTTP（出表，`responses` 补 423）** | 不存在（不调限流，无需豁免） |

于是 `INBAND_REJECTIONS` 的类型从 `[(&str, &str, &str); 3]` 变成 `[(&str, &str, &str, &str); 3]`——第三字段是状态码，第四字段是 marker；三行分别是 `(get /exec/stream 423 …)`、`(get /exec/stream 429 …)`、`(post /terminal/broadcast 429 …)`。第三段断言从「不得声明 423 **和** 429」改成「不得声明**这一条**码」，否则 `/terminal/broadcast` 的 423 会假红。

**第 3 步：两句 `api.yaml` 描述必须同笔改，否则代码改了、合同说谎。**

- `/terminal/broadcast/preview` 的 description 现在写「A paused gate is a per-target verdict here — not a 423」。改后要拆成：**paused** 仍是 per-target 判定；**读不出来**是整条路由 500。这两种情况不再是同一句话。
- `/terminal/broadcast` 的 description 写「folds **both** into every eligible target's `error` and returns 200」——改后只对「限流」和「gate 判为 Paused」成立，对「gate 读失败」不成立。同一段末尾「a preview that reports `execution gate paused` predicts the run exactly」在文件损坏时也失效（两边都 500）。

**第 4 步：`responses` 只补 423，不补 500。** 依据在 `api.yaml` 的 `info.description` **第二段**，原文是「every operation that consults the gate can answer 500 … which is why **none of them** lists the gate-read 500 in its own `responses`」（`ad9fdf1` 时点 `:24-33`）。「none of them」是这段的全部力量：给两条广播路由逐条补 500，它们就成了 33 个读 gate 的 operation 里仅有的两个例外，那句话同时从「盖住全部」退化成「盖住 31 个」。**同笔改动里这两段话一个字都不用动**——paused 仍是 per-target、读失败仍是整条 500，第二段本来就说的是后者。

**第 5 步：可选的 e2e，单独一笔。** `daemon_integration.rs` 跑的是镜像 router，`api_yaml_*` 是文本扫描，所以第 1 步落地后唯一的可验证信号是「`:4331` 的 `.ok()` 消失」，除非新写真 daemon e2e：写坏 `execution_gate.json` → `POST /terminal/broadcast` → 断言 500 且 `error` 含 `failed to load execution gate`。它要起 daemon + 两个活终端目标，比第 1–4 步加起来都大，**不塞进 batch 11 收尾**。

**已知不可断言的一环**：想给「读 gate 的 operation 不得声明 gate-read 500」加扫描断言，做不到——OpenAPI 里 500 不带来源，无法区分 gate-read 500 与配置读失败 500（`GET /hosts`、`/resident-sessions` 都两者兼备）。要断言就得加 `x-500-source` 这类扩展，代价大于缺陷本身。

替代方案不是「加来源」，而是**把来源下移到 `description` 文本**：扫每个读 gate 的 operation 的 `responses["500"].description`，断言不出现 `gate` / `execution_gate.json` 字样。这不需要改 spec 任何结构，只把这条约定变成一个可判定的**文本**规则（哪怕 500 的 `description` 写的是「`info.description` 里说的两种 500」）。第 1 步落地后如果顺手补它，成本约等于零；不补也不算缺口——第 4 步的话在 `info.description` 里仍然如实。

---

## 2026-11-11 · batch 12 第 1 轮：L2-80 / L2-82 / L2-122 第 5 条的落地形状更正，与两个新靶子

本轮的六条指摘全部落在**已经登记过**的三条测试缺口上，所以这一节不新开编号，而是把三条的「怎么落地」改对，并记下两条本轮新查出来的零断言分支。行号是 `b02acef` 的快照，锚符号。

### 一、L2-80（`daemon_tokens.toml` 写坏 → 非 admin 得 500）——落地必须走路由层，且必须用「不可读」而不是「语法错」

`bin/agent2ssh_daemon/auth.rs` 里读文件的唯一位置是 `authenticate_token`（`:40`）的 `:51-56`：

```rust
let scoped_tokens = load_scoped_daemon_tokens().map_err(|e| {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("failed to load scoped daemon tokens: {e}"),
    )
})?;
```

**handler 层写不出这条用例。** admin 短路在 `:44-49`，早于 `:51` 的读文件：

```rust
if token_matches(token, &state.token) {
    return Ok(AuthContext { scope: None, principal: token_principal(token) });
}
```

而 `authenticate_token` 是同步私有函数，调用点只有 `auth_middleware`（`:148`）与 `#[cfg(test)] check_auth`（`:34`）。`daemon.rs` 的 `mod tests` 已经 `use agent2ssh_daemon_auth::{auth_middleware, …}`，所以可走的路是**路由层**：

```rust
Router::new()
    .route("/probe", get(|| async { "ok" }))
    .layer(from_fn_with_state(state, auth_middleware))
```

**断言的判据是「admin 分支不依赖这次读成功」，所以 fixture 只需让 loader 返回 `Err`——语法错就够，不必造权限问题。**（第 2 轮更正本段：上一稿写「必须 `chmod 000`、语法错不行」，那是错的。`authenticate_token` 在 `:44-49` 就返回了，admin 根本不进 `:51` 的读；所以任何 `Err` fixture 都得到 admin 200 / 非 admin 500，与失败点在 `:209` 还是 `:210` 无关。）三条路径分叉在 `src/remote.rs`：

| fixture | 失败点 | 非 admin | admin |
|---|---|---|---|
| 文件不存在 | `:206-208` 提前 `return Ok(Vec::new())` | 401 | 200 |
| 存在但是目录 | `:209` `read_to_string` → `EISDIR` | 500 | 200 |
| 存在但不是 TOML | `:210` `toml::from_str` | 500 | 200 |

**要避开的是第一行，不是后两行。** 缺文件时 loader 返回 `Ok(Vec::new())`，非 admin 落 401，那条 500 臂根本没被走到；只有让 loader 真的 `Err`，断言才在验 500。语法错是最省事的一种，且与 uid 无关——`chmod 000` 在 root 下不生效（本仓容器自测与 `scripts/e2e-container.sh` 常以 root 跑），会把「拒绝读」与「语法错」压成同一个形状；目录（`EISDIR`）也能用，但没有额外好处。悬空符号链接**不能用**：`Path::exists()` 对悬空链接返回 `false`，与「文件不存在」同形，非 admin 得 401。

**推翻一条旧记录（原 L2-80 附带的缓存猜测）**：`SCOPED_TOKENS_CACHE` 确实没有 `invalidate()` 调用点（`remote.rs:200` 定义、`:205` 唯一使用；对比 `policy.rs:126/251`、`store.rs:690/2367`、`notify.rs:64` 都有），**但这不需要修**：`ConfigCache::load_with` 是

```rust
let value = loader()?;          // config_cache.rs:58 —— 失败在这里返回，不写 guard
*guard = Some(Entry { … });
```

失败**不进缓存**，所以「第一次失败会被记住直到 `(mtime, len)` 变化」不成立，下一次调用会重新读。进程内也没有任何写 `daemon_tokens.toml` 的路径（全仓只有 `remote.rs` 的读、测试 fixture、以及 `daemon_resident_session_e2e.rs:62` 的预置写入，都是进程启动前）。**加 `invalidate()` 是纯增噪。**

### 二、L2-82（`Option<Json<T>>` 吞解析错）——三处的后果不同，只有一处有安全含义

提取器这一层已经核到依赖源码：`axum-core-0.4.5/src/extract/mod.rs:119-129`

```rust
impl<S, T> FromRequest<S> for Option<T> where T: FromRequest<S> {
    type Rejection = Infallible;
    async fn from_request(req: Request, state: &S) -> Result<Option<T>, Self::Rejection> {
        Ok(T::from_request(req, state).await.ok())
    }
}
```

所以「任何提取失败都变成没有 body」成立。三处的后果**不能合并成一句**：

| 操作 | 参数 | 坏 body 的后果 |
|---|---|---|
| `POST /shutdown` | `daemon_shutdown:167` | `:204 None => {}` → `:207 SHUTDOWN_REQUESTED.notify_one()` → **关机**。`require_admin` 已在 `:170` 过，所以不是提权，是「本该被 400 拒掉的请求被执行」 |
| `POST /health-snapshot` | `post_health_snapshot:5593` | `:5598-5601` 的 `None => (None, None)` → `:5603-5614` 的 `_ =>` 分支 → **扫描全部已配置主机**（把「指定三台」读成「全部」） |
| `POST /playbooks/{name}/dry-run` | `dry_run_playbook_handler:3834` | `:3837 unwrap_or_default()` → `:3838-3845` 的「路径名与 body 名不一致 → 400」整段跳过 |

**推翻「换签名不红」这个对照**：`daemon.rs:6077/6097/6115` 三条单测直传 `Some(Json(ShutdownRequest{…}))`，把 `:167` 改成 `Json<ShutdownRequest>` 会让它们**编译失败**——是红了，不是绿的。所以这三条钉住的是 handler 分支，缺的是**提取器到 handler 之间**那一段，而不是「分支无断言」。

**但红的方向只有一个，绿的方向是开放的**（batch 14 第 1 轮）：不改签名、只在 handler 内部先判 `body.is_none()` 再解析，那三条单测**仍然全绿**——它们钉分支，不钉提取器。所以「编译失败」不能当成护栏存在的证据。`/shutdown` 现在两道都有：`380e166` 把参数换成 `HeaderMap + axum::body::Bytes` 并显式 `Json::from_request`（编译期），加 `daemon_stop_e2e.rs:1165` 的真进程 e2e（运行期）。**剩余两处一道都没有。**

**「三处共用一条断言」是错的——两处会假绿**（batch 14 第 1 轮）：`daemon_stop_e2e.rs` 的 `RunningDaemon::start_with_name` 只写 `daemon.token` 与 `extra_files`（`:54-59`），配置目录其余为空。于是：

- `/health-snapshot` 的坏 body 落到 `_ =>` 后「扫描全部已配置主机」（`:5607`），空目录下**什么都没扫**，返回 200 + 空数组——与无 body 的结果**相同**；
- `/playbooks/{name}/dry-run` 的坏 body → `unwrap_or_default()`（`:3837`）→「路径名与 body 名不一致 → 400」整段跳过；空 `playbooks` 列表下任何 body 都给同一结果。

所以判据必须拆开：`/shutdown` 用「坏 body → 4xx 且 `/health` 仍 200」（进程还在）；另两处用「**坏 body 与无 body 的行为不同**」——例如 `/health-snapshot` 预置两台主机，无 body → 200 + 2 条，坏 body 收窄前应是 400、收窄后仍是 2 条。同一条断言覆盖三处会给出假绿。

### 三、L2-122 第 5 条的成本重估

原文写「要起 daemon + 两个活终端目标，比第 1–4 步加起来都大」。这句过期了：gate 检查在 `terminal_broadcast_preview_results` 的 **per-target 循环内**（`:4372-4379`），遍历的是 `request.targets`，所以目标为空时根本不执行——「至少一个已注册终端」是必须的，但注册终端只要一次 `/terminal` WS 握手（insert 点在 `:4800`），**不需要真实 SSH 主机**。真实成本 = 一条新文件 + 一个写坏 `execution_gate.json` 的 fixture + 一条 500 断言，与 `daemon_stop_e2e` 同量级。

### 四、新靶子：`start_local_service` 的两条拒绝文案零断言

`src/container_runtime.rs:505-527`：

```rust
let resolved = match explicit.filter(|id| !id.trim().is_empty()) {
    Some(id) => select(Some(id))?,
    None => available()
        .into_iter()
        .find(|r| r.runtime.service_running(&r.binary).is_some())
        .ok_or_else(|| anyhow!("no installed container runtime has a service to start"))?,   // :511
};
…
if resolved.runtime.service_running(&resolved.binary).is_none() {                              // :514
    return Err(anyhow!("{display_name} has no separate service to start"));                    // :515
}
```

两条拒绝文案在 `src-tauri/tests/` 与 `src-tauri/src/` 的测试里**都是零命中**（只有定义处）。而 `docs/skills.md:99` 与 `tools.rs:1052` 把「Docker and wslc have none and are refused rather than silently accepted」当对外契约发出去。

**`None` 臂的两条分支要分开说，不能一起叫「死分支」**：

- `:514` 在该臂**不可达**——`:510` 的 `find` 已经保证 `service_running(...).is_some()`，同一个 binary 再问一次还是 `Some`。
- `:511` 的 `ok_or_else` 在该臂**可达**：`available()`（`:470-479`）只返回**已安装**的 runtime，主机上只装了 Docker/wslc 时 `find` 返回 `None`，`ok_or_else` 就执行。这是 Linux + Docker 的常见形态，不是理论分支。（本轮此前把它记成「两条都是死分支」，那是错的。）

**单测配方**（不依赖真实外部服务、不需要 macOS）：`resolve_executable_in` 走 `PATH`（`path_resolver.rs:54-58`），`binary_candidates()` 对 Docker 写死 `["docker"]`。在临时目录放一个名为 `docker` 的可执行空脚本并把 `PATH` 指向它 →

- `start_local_service(Some("docker"))` 必须返回 `has no separate service to start`，且**不执行任何启动**（`:515` 在 `:520` 的 `start_service` 之前）；
- 同一 fixture 下 `start_local_service(None)` 必须返回 `no installed container runtime has a service to start`。

注意 `unsupported_reason()` 要放行（Docker 全平台支持），所以不能靠 `AGENT2SSH_CONTAINER_RUNTIME` 之外的手段。

### 五、本轮已落一半：派生族的**分母**现在可判定，剩下的是镜像那三个数

`daemon_routes` 原来对读不懂的 `.route(...)` 块静默 `continue`，八个派生族的覆盖面因此没有分母。本轮加了 `daemon_routes_and_drops` + `every_route_call_is_read_as_an_operation`：任何非 `NON_API_ROUTES` 的块读不出来就红。写这条时立刻抓到一处真形状——`daemon.rs:5813-5817` 的

```rust
.route("/", get(|| async { axum::response::Redirect::to("/console") }))
```

是**闭包**挂载，`mounted_handlers` 取到的 handler 名为空，所以 `/` 在 `daemon_routes` 里从来没有条目。它落在 `NON_API_ROUTES` 里，不影响契约；但如果**新加一条 API 路由也写成闭包**，旧代码下八个族会一起静默，现在会红。

仍未钉住的是 `tests/daemon_integration.rs` 头部那三个数（「18 of the daemon's 92 routes」「74 routes that are not mirrored」）：它们是散文，改镜像不会红。要钉得解析测试文件自身的 `build_test_router`，本轮未做——它不影响任何断言的真假，只影响读者对覆盖面的估计。

## 2026-11-11 · batch 12 第 2 轮复核：`mounted_handlers` 的字面一致性（新 L2-123），与两条覆盖面登记（L2-124/125）

本轮是**复核**（读代码、复算，不改行为）。三件事：上一轮我自己提出的「`mounted_handlers` 的字面一致性断言」现在有了确切形状和成本，落成 L2-123；顺手按第 1 轮 Q1.4 的结论复查时，查出两处**覆盖面缺口**（L2-124/125），都不是行为 bug，但都会让某类改动不红。行号基线 `614f355`，按符号引用。

### L2-123 · `mounted_handlers` 把「白名单内路径」与「这里挂的是什么」混成一个事实

**位置**：`tests/daemon_integration.rs:2289-2290`（`if !handler.is_empty() { mounted.push(…) }`）配合 `:2377-2383`（`mounted.is_empty()` → `DroppedRoute`），白名单是 `:2264` 的 `NON_API_ROUTES`。

**为什么不能自动落**：这是「该断言什么」的选择，不是错。它现在的语义是：**任何一个 `.route("…", X)`，只要该路径在 `NON_API_ROUTES` 里、且 `X` 里读不出 `<method>(<ident>`，就当作可解释的丢弃。** 对本仓今天的树它是精确的——我把整棵树按 `daemon_routes_and_drops` 的算法复算了一遍，`.route(` 共 92 次、丢弃 **1** 条（`:5814`，路径 `/`，原因「无 handler」），与 `every_route_call_is_read_as_an_operation` 的 `expected` 逐字一致。

**但「精确」依赖三件事同时成立**，其中两件今天已经在漂：

1. **丢弃路径落在白名单里**——白名单在 `:2264`，断言在 `:2612` 起的 `expected` 里又硬编码了一遍 `/`。两份声明，一份是「集合」，一份是「带原因的单条」。加一条新的白名单路径、或改 `/` 的挂载方式，两处都得手改，且没有任何东西会提示它们该一起动。
2. **`/` 今天只能被闭包挂载，而已**——`serve_console` 是普通的 `async fn`，`get(serve_console)` 是合法调用。所以「`/console` 有一天换成闭包」是一个**能编译、能起服务、语义不变**的改动。
3. **`/console` 被 `mounted_handlers` 正确读出**——今天成立（`:5818` 是 `get(serve_console)`）。

**第 2 件已经不出声了（L2-124，见下）**：`/console` 换成闭包后，`daemon_routes` 会少一条条目，`api_yaml_documents_every_daemon_route` 只检查 `/console` 不在 spec 里、不会红；`every_route_call_is_read_as_an_operation` 会红。所以今天唯一的哨兵是那条新断言——**它对，但它红的时候说的是「解析器读不懂了」，而真正发生的事是「这条非 API 路由换了挂载方式」**，这两句话不对应，第一个照顾它的人大概率直接把 `/console` 加进 `expected`。

**建议改法（可判定的判据）**：把「路径在非 API 集合里」换成「**这一条 `.route` 挂的是闭包字面量**」，用例集只放形状、不放路径：

```rust
// 每个丢弃的 `.route` 段，判据是「段里的实参是闭包字面量」，而不是「路径在白名单里」
fn mounts_a_closure_literal(segment: &str) -> bool {
    // `.route(\s*"[\w/:]+"\s*,\s*(get|post|put|delete|patch)\(\s*(async\s*)?(\|[^|]*\||\|\|)`
}
```

断言：`dropped.iter().all(|d| mounts_a_closure_literal(d.segment))`，并且 `dropped.len()` 只进失败信息、不进期望。这样：

- `/` 的闭包 → 通过（与今天等价）；
- **任何路径**写成闭包 → 通过（同形同待遇，不再依赖白名单）；
- `.route("/console", get(serve_console))` 因为某种 `mounted_handlers` 的退化被误读 → **红**（因为它不是闭包）；
- 有人新加一条闭包 API 路由 → **通过**——这是这个判据唯一的代价，需要人拍板「闭包路由算不算可接受的 API 形状」。若判「不算」，把判据反过来写即可（`!mounts_a_closure_literal` 全部红，`/` 单独列一条显式豁免），两种写法成本相同。

**顺带**：`DroppedRoute` 现在只有 `line/path/reason`，没有段文本，加判据要给它加一个 `segment: &'a str`（`daemon_routes_and_drops` 要加生命周期参数）。这是签名变化，属 `src-tauri/tests/**`，可以落 L1；我没落是因为上面那条产品判断还没定。

**取证方式说明**：本仓工作区没有 Rust 工具链（`command -v cargo` 为空），所以这一节所有数字都是用一个按 `daemon_routes_and_drops` / `mounted_handlers` 原文逐行复写的脚本对**当前树**复算的；`every_route_call_is_read_as_an_operation` 的 `expected` 只有一条（`/` + 原因），与复算逐字一致，这是复写正确的交叉验证。破坏性对照是对 daemon 源文本注入后重跑同一脚本，**没有真的编译**——所以「会红」是按断言源码推的，不是测出来的。

### L2-124 · `/console` 的 GET 挂载没有任何断言

**位置**：`bin/agent2ssh-daemon.rs:5818`（`.route("/console", get(serve_console))`）、`:5603-5605`（`serve_console`）、`:5815-5817`（`/` → `Redirect::to("/console")`）；白名单侧 `agent2ssh_daemon/auth.rs:99`（`is_public_path` 认 `/console`、**不认任何 `/console/…`**）、`:100-101`（`accepts_query_token` 不认 `/console`）。

**事实**：`/console` 既不在 `docs/api.yaml` 里（`NON_API_ROUTES` 把它排除出全部 spec 断言），也**不被任何解析器读到**——它进了 `daemon_routes`，但 `every_route_call_is_read_as_an_operation` 只比 `dropped`，`api_yaml_documents_every_daemon_route` 用 `NON_API_ROUTES` 把它滤掉。所以 `get(serve_console)` 这个字面量在 `src-tauri/tests/**` 里**零命中**（我把 daemon 源按 `mounted_handlers` 复算过：把它换成闭包，`daemon_routes` 少一条条目，**没有一条断言会红**）。

`tests/daemon_integration.rs:668-696` 的 `http_console_serves_key_workflow_entrypoints` 测的是**镜像**：`:145-146` 的本地 `console` 函数 + `:425` 的 `build_test_router().route("/console", get(console))`。所以它断言的 HTML 是真的，路由**挂法**是假的。

**为什么记在这一节**：它不影响任何断言的真假，但它是 L2-123 那个判据的第 2 件成立理由——如果 `/console` 的挂法被闭包替换，今天没有任何东西能说出「换掉的是一条曾用 `async fn` 挂载的路由」。

**建议改法**（两条都可，成本都低）：

- 在 `every_route_call_is_read_as_an_operation` 旁边加一条：解析 `.route("/console"` 的段，断言它 `mounted_handlers` 恰好等于 `[("get", "serve_console")]`，且 `serve_console` 在 daemon 源里是 `async fn`。三行代码，钉住「非 API 路由**用命名 handler** 挂载」这件事，与 L2-123 的「闭包只能是闭包」互为补集；
- 或按 L2-123 的判据统一处理（`/console` 不是闭包，就必须被 `mounted_handlers` 读出来），那时这条就是判据的推论，不必单独立。

**无法自动落的原因**：它是 `src-tauri/tests/**` 的新断言，但判据取决于 L2-123 的产品决定；若那条判「闭包路由可接受」，这一条仍然是必要的补集。

### L2-125 · 头部那三个数：一个「可钉」、两个「不可钉」，且不可钉的原因不是成本

**位置**：`tests/daemon_integration.rs:9`（「18 of the daemon's 92 routes」）、`:17-18`（「74 routes that are not mirrored at all」）。

**先把数复算清楚**（都是 `614f355` 的树）：

| 数 | 复算 | 结论 |
|---|---|---|
| `92` | `.route(` 字面次数 = **92**；解析出 `(path,method,handler)` 的条目 = **106**；去重 `(path,method)` = **106**；去重 path（含 `/console`）= **91** | 92 数的是**调用**，不是路由。正文说「92 routes」，单位错。换成 `(path,method)` 是 106，换成 path 是 91 |
| `18` | `build_test_router`（`:420-449`）里 `.route(` = **18**，但挂的方法数是 **21**（`/webhook/config` 与 `/approval/policies` 各挂 2 个、`/exec/preview` 1 个） | 18 是**路径数**，不是「shape of 18 routes」里那个 route 的单位；与 92 的口径不同（92 是调用数），两个数不可相约 |
| `74` | 92 − 18 = 74 | 这个减法**只在「92 与 18 同一口径」时才对**。92 是调用、18 是路径，74 是两条不同口径相减的产物。它今天的真值（按路径）= 91 − 17 = **74**（`/console` 在两者里都有，所以减出来仍是 74）——**数字对，理由不对** |
| `105`/`106` 指代一致 | spec 的 `(path,method)` = **105**；daemon 的 = **106**，差集是 `/console` | 一致，无需改 |

**为什么只钉得动一半**：18 可以由「解析测试文件自身」稳定得到（`build_test_router` 的 `.route(` 次数，或 `mounted_handlers` 的方法数）。但**「mirrored」这件事在运行时不可判定**：镜像 router 的 18 条里有 `console`、`list_hosts` 这类**同名、同路径、不同实现**的本地函数（`:145`、`:180` 起），源文本能证明「有一条同路径的 `.route`」，不能证明「handler 是 daemon 的」。所以「74 条未镜像」只能钉成「daemon 的 (path,method) 集合减去镜像 router 的 (path,method) 集合 = 74」——它钉的是**形状差**，不是**实现差**，措辞也要跟着改。

**建议改法**：头部改成三个可解析的句子，并把「什么算 mirrored」写明——

- 92 → 「92 `.route(...)` calls, 106 (path, method) pairs」；
- 18 → 「18 paths / 21 operations」；
- 74 → 「74 (path, method) pairs that no mirrored route shares a path with」，并加一句「sharing a path is not sharing an implementation」。

然后加一条断言：解析 `daemon_integration.rs` 自身的 `build_test_router`，把这三个数与正文里的三个数逐字比对。**成本约 40 行，全部在 `src-tauri/tests/**`**，属 L1；我没落是因为它要和 `docs/reports/` 里的表述一起改，且 `105/106/74/18/92` 这几个数在 `docs/architecture.md`、`docs/api.yaml` 的注释里还有副本，需要一次找准范围。

**一条可复算的旁证**（写在这里供落地时做破坏性对照）：给 daemon 加一条 `.route("/console", post(serve_console))` 后，`daemon_routes` 变成 107 条 `(path,method)`，`phantom` 检查不会红（`/console` 在 `NON_API_ROUTES` 里），头部三个数**一个都不会动**。这就是 L2-125 想钉住的那类漂移。

> **这条旁证已失效**（batch 12 第 3 轮）：`the_console_route_mounts_one_named_handler` 现在数 `.route("/console"` 的出现次数，加第二条会红。见本文件末节「batch 12 第 3 轮」。

---

## 2026-11-11 · batch 12 第 3 轮：L2-123 / L2-124 已落地，L2-125 只落了单位（两个数被更正）

上一节（复核小节）登记的三条，本轮全部处理完：两条**已落地**成 L1（`1a0b723` + `3e86b4b`，只动 `tests/daemon_integration.rs`），一条**落了一半**并更正了原文两个数。行号是 `3e86b4b` 的快照，按符号引用。

### 一、L2-123 已落地，但只落一半——「任何路径写成闭包都放行」被有意拒掉

`DroppedRoute` 增 `mounts_closure: bool`，在**丢弃发生时**算好（`mounts_closure(segment)`），所以结构体不持借用、`daemon_routes_and_drops` 的签名不用加生命周期（原建议是加 `segment: &'a str`，见上一节「顺带」那段）。`every_route_call_is_read_as_an_operation` 拆成两条各自可判定的断言：

1. **形状**：每个丢弃段的挂载实参都是闭包（`!drop.mounts_closure` 一律红）；
2. **归属**：每个被丢弃的路径都 ∈ `NON_API_ROUTES`。

第 2 条与 L2-123 建议的「任何路径写成闭包 → 通过（同形同待遇）」**不同**，理由是那条判据有条可构造的漏：

- 一条**未写进 `api.yaml`** 的路由若用闭包挂载，`daemon_routes` 掉一条，而 `api.yaml` 侧的 `phantom` 也没有条目可比对（spec 里根本没写它）——两条网同时漏。L2-123 自己把这点写成「这个判据唯一的代价，需要人拍板」。
- 产品问题该由人拍；L1 车道不该用**撤掉一条护栏**来表达「闭包路由可接受」。要让某条路由享受豁免，就把它登记进 `NON_API_ROUTES`——那是一个必须显式做的动作，不是默认放行。

**破坏性对照**（三次，每次改完 daemon 源后还原，`git diff` 确认无残留）：

| 改动 | 形状断言 | 归属断言 | 旧版（固定 (path, reason) 集合） |
|---|---|---|---|
| `/console` 改成闭包（能编译、语义相同） | 绿（闭包） | 绿（`/console` ∈ 白名单） | 红 |
| `/metrics` 改成闭包 | 绿（闭包） | **红**，报 `line 5821 `/metrics`` | 红 |
| `/console` 写成 `get (serve_console)`（一个空格，仍能编译） | **红**，报「mount a named handler and did not parse」 | — | 红，但文案说「update this expectation deliberately」 |

第一行是**唯一**旧版比新版严的格子，也正是新版为此付出代价的地方：它由 `the_console_route_mounts_one_named_handler` 补上（下一节）。第二、三行说明新判据并不更松——它把「哪条路由被豁免」变成一个必须写下来的决定。

### 二、L2-124 已落地：`/console` 的挂载有断言了

新增 `the_console_route_mounts_one_named_handler`，三件事：

1. `daemon.matches(".route(\"/console\"")` 必须恰为 **1**；
2. 该段 `mounted_handlers` 恰为 `[("get", "serve_console")]`；
3. daemon 源里有 `async fn serve_console(`。

**第 1 条是补上的**：第一版只解析第一个 `.route("/console"` 段，而我用上一节那条旁证（`.route("/console", post(serve_console))`）实测时，测试**仍然是绿的**——axum 把同一路径的第二条 `.route` 合并进第一条，第二段根本没进断言。所以那条被当作「头部数不可钉」的证据的反例，正好是这条测试第一版的漏。`3e86b4b` 补上计数后实测：`/console is mounted by 2 .route(...) calls`。

破坏性对照（已还原）：`/console` 改闭包 → **只有**这条新 test 红，`every_route_call_is_read_as_an_operation` 仍绿（`/console` ∈ 白名单）。这正是 L2-124 说的「静默」，也是这条 test 唯一的存在理由。

### 三、L2-125：单位改了，两个数被更正

| 上一节原文 | 复算 | 结论 |
|---|---|---|
| `build_test_router` 的方法数 = **21** | **20**：18 条 `.route(` 里 `/webhook/config`（get+put）与 `/approval/policies`（get+put）各两个方法，其余 16 条各一个 | 原文多算 1 |
| 「74 是两条不同口径相减的产物（值 91 − 17 = 74 对，理由不对）」 | **74 在调用口径与路径口径下都对**：92 calls − 18 calls = 74；按**调用路径**数是 92 条 distinct − 18 = 74。解析出的 distinct path 是 91，只因为 `/` 是那个唯一的 drop、不产出 entry。换成 `(path, method)` 才是 106 − 22 = **84** | 「74 理由不对」不成立；成立的是「92 与 18 的**名词**不对」（它们都是调用数，不是路由数） |
| 必须同改 `docs/architecture.md` 里的副本 | `git grep -i 'mirror\|镜像' docs/architecture.md` **零命中**，`92`/`74`/`18` 也不在那份文件里 | 不成立：这些数只在测试文件头部两处与本台账里有 |

于是头部改成：`18 of the daemon's 92 .route(...) calls (20 of its 106 (path, method) pairs)`，以及 `The other 74 calls — 84 (path, method) pairs over 74 distinct paths`。另补一句原文没有、复算得来的事实：**路径被镜像 ≠ 方法被镜像**——daemon 在 `/hosts` 与 `/health-snapshot` 上各多挂一个 `POST`，镜像里没有。这与 L2-124 同类：读者会把「18 条路径」读成「这 18 条覆盖了」。

### 四、没落的那一半：不把 74 / 18 / 92 钉成断言

`every_route_call_is_read_as_an_operation` 的文档里已经写明它钉的是「rather than a count a new route would break」——本仓在这条路径上的既定取舍是**不钉计数**。再给头部三个数加一条自解析断言，等于把刚拆掉的漂移源换成另一种：每加一条路由、每改一次镜像都要连带改断言。

留给拍板的措辞：**若要钉，钉的是形状差**（daemon 的 `(path, method)` 集合 vs 镜像 router 的集合），不是「实现差」——源文本无法证明同路径的本地 `console`/`list_hosts` 就是 daemon 的实现。影响面比 L2-125 估的小：只需改测试文件一处。

---

## 2026-11-11 · batch 12 第 4 轮：`get (…)` 的误红格（已修）、链上其余挂载器（已钉）、头部那条 73/74（已改）

`bca51ea` 一笔解决三件事，全在 `src-tauri/tests/daemon_integration.rs`。

### 一、`get (|| …)` 误红——成立，已修

它给的注入 `.route("/", get (|| async { … }))`：Rust 不要求 callee 与 `(` 相邻，所以**能编译、仍是闭包、运行时一字不差**；而 `mounts_closure` 只认 `"get("` 这个子串，`match_indices` 找不到，返回 `false`，第一条断言就报「these `.route(...)` calls mount a named handler and did not parse」。旧版对这条注入是**绿**的（`(path, reason)` 一字未变）。

这是上一轮我引入的**唯一误红格**，而且文案把人指向「教解析器认命名 handler」，真正的修法只是一个空格。这正是 `every_route_call_is_read_as_an_operation` 的文档里反对的那件事：断言红的时候要在说真问题。

修法：先按**词**找 `get`（前一个字符不是标识符字符），跳空白，`strip_prefix('(')`，再跳空白，然后认 `|`（`move` / `async` 作为前缀一并跳过）。`get_x(` 这类仍被 `strip_prefix('(')` 挡掉，不必再单独判「后随标识符」。

实测：注入 `get (|| …)` → **绿**（修前红）；注入 `get (serve_console)`（命名 handler）→ **仍红**。两条一起说明判据区分的是「闭包 vs 名字」，不是「有没有空格」——这也正是它在复核里说的「`mounts_closure` 真正比旧版多看到的东西」。

### 二、链上其余挂载器——结构成立，已钉（覆盖入口，不覆盖语义）

`Router::route_service` / `nest` / `nest_service` / `merge` / `fallback` / `fallback_service` 的文本里**没有 `.route(` 子串**，所以经它们挂的路由既不产出 entry、也不产出 drop——`daemon_routes_and_drops` 甚至会把后面那行吞进上一条 `.route(` 的 segment。新增 `the_router_chain_mounts_paths_only_through_route`：取 `Router::new()` 到 `.with_state(` 之间那段，**先去掉行注释**（一句注释点名 `.route_service(` 不该被当成挂载），再断言六个构建器都不出现。

**覆盖范围要说清**：它钉的是「入口」。它列的另外三条里，(b) `nest` 内部 `.route("/z", …)` 会被读成相对路径 `/z`（假 entry，名字错）、(c) `fallback` 让未列出的动词也有响应、(d) 同名方法重复是 `panic!`——这三条要发生，都得先出现 `.nest(`/`.fallback(`，而那会先被这条钉住。所以是「入口先红」，不是「语义已覆盖」。

实测：`.nest("/zz", Router::new())` 注入 → 红并点名 `.nest(`（**能编译**，其余断言一动不动，正是「零痕迹」的样子）；链上写一句 `// … .route_service( … .fallback( …` 的注释 → 绿。

### 三、头部 73/74——部分成立，已改

它说「那 74 calls 落在 **73** 条不同路径上」：**不成立**。那 74 次调用各自落在 74 条不同路径上，其中一条是 `/`（闭包重定向，不产出 pair）；**73** 是「有 pair 的路径数」。我原句「84 `(path, method)` pairs over 74 distinct paths」字面为真，但容易被读成「84 对分布在 74 条有 pair 的路径」，所以改成分开说：`84 (path, method) pairs over 73 paths, and the one path with no pair is the / redirect`。

### 四、它列的 14 行文档表——只改了 1 行，其余是快照

**改**：`docs/guides/daemon-api-quickstart.md:62`（成立）。那句自己跟自己打架：先说「两个数不要混用」，再把 **107**（= 105 API + 2 条非 API 的 **operation** 总数）叫「107 条路由」。已改成「105 API operation / 107 operation 总数 / 92 条路径」，并把「目前没有测试守着它」更新成点名两条现有守卫（`api_yaml_documents_every_daemon_route`、`the_console_route_mounts_one_named_handler`）——**这张表本身仍无断言**，所以那半句保留。

**不改**，理由是它们是**日期快照**，不是当前声明：

- `docs/reports/redundancy-audit.md:990`（当轮审计的「84 条 `.route(..)` − 2 = 82 paths」）
- `docs/reports/OPT-LOOP-L2-PENDING.md:363`（L2-53 那一行把 107 叫「路由」，同一种名词错，但那是 11-10 的记录）
- `:1224` / `:1283` / `:1289-1291`（上一轮复核原文，已由第 3、4 轮的小节覆盖）
- `docs/reports/OPT-LOOP-LEDGER.md:1050` / `:1065`（历史轮次记录）

改这些等于改写历史，下一个读的人会以为当轮的树就是今天这样。

**政策一句**：面向使用者的现行文档（`docs/guides/**`、`docs/api.yaml`）按今天的树修；带日期的 `docs/reports/**` 保留快照，新的更正写在追加的小节里（本轮两节都是这个做法）。

`docs/skills.md` 与 mcp 系列的「76 个工具」有断言（`mcp_tool_list_contains_exactly_76_tools`），它自己也认，不算缺口。

---

## 2026-11-11 · batch 12 第 5 轮：L2-126 关闭，混合链与链守卫的两个口子登记为 L2-128 / L2-129

### 一、L2-126 已关闭（`94acb22`）——「entry 的 handler 必须解析得到」有断言了

它的原话是「要教解析器读 nest 前缀，得先加这条断言」。**只落了后半句**，而且不依赖 nest：

- `every_parsed_handler_is_a_function_the_daemon_defines`：每条 entry 的 handler 必须在 daemon 里有 `async fn <name>(`。今天是 106 / 106。
- 同笔修了同一个洞更常见的形态：`get(move || …)` 曾被读成 `handler = "move"` → `mounted` 非空 → **不产 drop** → `mounts_closure` 从来没有被调用过。现在读名字处直接拒掉 `move` / `async`。
- **读 nest 前缀本身没做**：它要回答「相对路径怎么拼」「多层 nest 怎么算」「`nest_service` 的通配怎么表达」，而今天树上零例。**判断：等真实用例出现再做，不猜规格。**

### 二、L2-128（新）· 混合链里的闭包仍然隐形

`.route("/x", get(h).post(|| async { … }))`：`mounted_handlers` 只读到 `h`，`mounted` 非空 → **不产 drop** → 那个 POST 闭包既不产 entry 也不产 drop，两条网都不看它。

**为什么本轮不做**：要一条判据，得让 `daemon_routes_and_drops` 多返回一份「既有 entry 又挂闭包」的段（第三个返回值），或把 `mounts_closure` 提成「每段都算」。两者都改签名，而今天 92 段里零例。

**落地面（估）**：解析侧 +15 行、断言 +10 行、破坏性对照 1 次；全在 `src-tauri/tests/**`，属 L1。**优先级中**：要发生得有人同时把命名 handler 与闭包写进一条 `.route`。

**给将来做的人的判据**：**一段里不许混装**——`mounted` 非空且 `mounts_closure(segment)` 为真就红，文案写「同一条 `.route` 上不要混挂闭包与命名 handler」。

### 三、L2-129（新）· 链守卫的三个口子，两个成立、一个不成立

1. **`split("//")` 被字符串字面量骗——成立。** `.route("/c", get(|| { let _ = "//"; })).nest(…)` 写在同一行时，`//` 之后全被截掉，`.nest(` 看不见。今天零例（要「同一行 + 含 `//` 的字符串 + 其后紧跟 `.nest(`」三件事同时成立）。**修法**：把「剥行注释」换成一个跳字符串/字符字面量的小扫描器，约 20 行。**优先级低但不为零**：`.nest` 载荷造出的假 entry，只有当 handler 名恰好是顶层已有函数（如 `get(health)`）时，L2-126 那条断言才不管——那种组合是躲得过两条网的。

   **成本更正（batch 14 第 1 轮，两个方向都修）**：`closing_brace`（`daemon_integration.rs:2583`）的 `match bytes[at]` 第一个分支就是 `b'/' if bytes.get(at + 1) == Some(&b'/')`，随后是块注释、`"` → `string_end`、`'` → `char_end`。也就是说这套跳过逻辑**已经在同一文件里**，不需要重写。但「约 20 行」与复核方说的「约 5 行」都不对：`closing_brace` 不能直接拿来剥注释（它还要数括号），得把这四个分支抽成一个独立的 `code_only(source)`，再让 `closing_brace` 复用它——实测是 +15/−6 量级，收益是**同一份文件只剩一套扫描器**（`parse_route_table` 的 `mounted_handlers` 另有一套懂字面量的，见 `:2620-2658`）。两个数字都别信，按「抽公共扫描器」估。
2. **「其余 builders 不含 `.route(` 这句理由在文本上错」——不成立。** `.route_service(` 第 6 位是 `_` 而不是 `(`：`".route_service(".find(".route(") == -1`。它含 `.route`，不含 `.route(`，而解析器扫的是后者。测试里的原话成立，不改。
3. **守卫终点取第一个 `.with_state(`——成立。** `let app = Router::new()` 之后若先出现 `sub.with_state(s)`，窗口会在那里断掉，其后的 `.nest(` 落到窗口外。今天链上只有一个 `.with_state(`。**修法**：改用「链段内最后一个 `.with_state(`」或「最后一次 `.route(` 之后再取一段」。成本 1 行，但「窗口」的语义会更含糊，故与 L2-128 一起等下次动这个文件时做。

---

## 2026-11-11 · batch 12 第 6 轮（末轮）：L2-128 已落地，L2-129 留给下一批

**L2-128 关闭（`7369ecd`）**。`parse_route_table` 现在返回 `RouteTable { routes, dropped, mixed }`，`mixed` 收「同一条 `.route` 上既挂闭包又挂命名 handler」的段；`a_route_call_does_not_mix_a_closure_with_a_named_handler` 断言它为空。

- **落地形状与估的差别**：没有加第三个返回值，而是把两个返回值收进一个结构体，函数名 `daemon_routes_and_drops` → `parse_route_table`。理由：三元组会让 `daemon_routes` 的 `.0` 变成 `.routes`，一样要改，而结构体顺便给三个字段各写了文档。
- **成本**：+70 / −9（含改名与三处文案），比估的 +25 高；多出来的全是注释与改名，逻辑只有 6 行。
- **破坏性对照**：`/metrics` 写成 `get(metrics).post(|| async { … })` → **只有新断言红**，drop 检查、`api.yaml` 对账、handler 存在性三条全绿。

**L2-129 仍开**（链守卫的两个口子，见上一节 §三 第 1、3 条）。下一批若要动这张表，两件一起做：把「剥行注释」换成跳字符串/字符/注释的小扫描器；窗口终点改用链段内最后一个 `.with_state(`。

---

## 2026-11-11 · batch 14 第 1 轮：L2-130（新）· `/exec/stream` 的 `interrupted` 分不清「没收到 Exit」与「收到 `Exit(None)`」

**状态：开，需人拍板。** 与 batch 7 那条「不成立」判定**不冲突，是它的补集**——那条只覆盖 `Exit(Some(n))`。

### 位置（当前树，`d3a4a71`）

| 事实 | 位置 |
|---|---|
| `code` 的类型是 `Option<i32>` | `bin/agent2ssh-daemon.rs:5363`（`let mut code = None;`） |
| 取消分支 drain 的赋值 | `:5403`（`EmbeddedExecStreamEvent::Exit(exit_code) => { code = exit_code; }`） |
| 主循环的赋值 | `:5478`（同一句，随后 `break`） |
| `interrupted` 判定 | `:5493`（`shutdown_token.is_cancelled_now() && code.is_none()`） |
| 审计文案 | `:5496-5497`（`daemon shutdown interrupted stream; remote outcome unknown`） |
| `ExecCompleted` 事件载荷 | `:5529`（`"interrupted": interrupted`） |
| `Exit(None)` 的生产者之一：超时 | `:5186-5189`（先 `Error("SSH command timed out after {timeout_secs}s")`，再 `Exit(None)`） |
| 生产者之二：worker 出错 | `:5204` |
| 生产者之三：正常路径 `exit_status()` 失败 | `:5198`（`let code = channel.exit_status().ok();`） |

### 判据

`Exit(None)` 被赋给 `Option<i32>` 时，`code = None` 与「从未收到 `Exit`」在类型上同值。于是：

- 一条**在停机前就已经超时结束**的 `/exec/stream`，只要它的 `Exit(None)` 留在队列里被取消分支 drain 读到，`interrupted` 就是 **true**；
- 审计写成 `daemon shutdown interrupted stream; remote outcome unknown`，而真实原因是 `SSH command timed out after Ns`——**那句 `Error` 事件可能已经在 drain 里被发给了客户端，但审计不读它**；
- `ExecCompleted` 的 `"interrupted": true` 同源，webhook 侧无从分辨。

batch 7 判「不成立」时用的论据是「`code.is_some()` 时 `interrupted` 必为 false」——对 `Exit(Some(n))` 成立，对 `Exit(None)` 不成立。所以那条判定不用撤，只需要知道它没有覆盖这一支。

### 为什么不能自动落（L2）

改法有三条，都要动 `src-tauri/src/**` 的行为。**成本排序在 batch 14 第 2 轮被复核方纠正过一次，下面是更正后的**：

1. **`code: Option<Option<i32>>` — 推荐，最便宜。** 只有四处：类型（`:5363`）、两处赋值改成 `Some(exit_code)`（`:5403`、`:5478`）、判定改成 `code.is_none()`（`:5493`）。**不需要给事件加字段**——`"interrupted"` 与 `"exit_code"` 已经同时在线，语义由两者的组合表达。
2. `saw_exit: bool` + `code: Option<i32>` — 名义成本最低、实际最高。判据修好之后 `saw_exit` 与 `exit_code.is_some()` 恒等，是冗余字段；而 `ExecCompleted` 的载荷今天有**四个**发布点（`bin/agent2ssh-daemon.rs:5521`、`core.rs:1118`、`core.rs:1323`、`resident_session.rs:803`），只有第一个带你这条 `"interrupted"`，加字段要四个点一起想清楚。
3. 让线程侧在 `Exit` 之前多播一个 `TimedOut` 事件 — 改动面最大。**它修的是 `Error` 帧（`:5189`、`:5204`），不是审计**：审计在 `:5494` 之后才拼，那时 `code` 已携带信息。这一条与 `interrupted` 的修正彼此独立，应排在其后。

**拆成两步落**（复核方建议，采纳）：

- **第一步（自动可落）**：只修判据——`interrupted` 不再把「收到 `Exit(None)`」当「没收到 `Exit`」。
- **第二步（要拍板）**：统一文案与字段名。`/exec` 停机路径已有一个现成答案——`WebhookEvent.remote_outcome_unknown`（`:1748`），而 `/exec/stream` 对同一件事写的是另一种说法（`embedded_ssh.rs:1385` 的 `SSH request interrupted during daemon shutdown; remote command outcome unknown`）。这一步要问的是「stream 的完成事件是否复用这个字段形状」，**不要造新词**（`exit_seen` 之类）。

**回滚路径**：第一步不改任何对外形状（`interrupted` 只是从「假 true」变回「真 false」），可独立回退；第二步改的是已有字段的措辞与命名，回退面在 `api.yaml` 与四个发布点。

### 同一族的第二个口子：`.is_err()` 抹掉状态码（新，L2-131）

`:5266-5275` 的 `if reject_if_gate_paused(&source, &req.host, &req.command).is_err() { … "execution gate paused" … }` 把 423（paused）与 500（`execution_gate.json` 读失败）压进同一个 `Err`，帧里的 `error` 又固定写 `"execution gate paused"`。而 `docs/api.yaml:24-31` 明说「a client that meets a 500 on one of them should read the message」——这条路径恰好不给 message，于是 gate 文件坏掉时客户端会看到「已暂停」这个错误结论。

它与 L2-130 的 `Error` 帧丢失是兄弟，但独立：L2-130 是审计侧把来源抹平，L2-131 是帧侧把来源抹平。修法：匹配状态码分两句文案（`paused` / `failed to load execution gate`），属 `src-tauri/src/**` 行为改动，登记不动。

**与 L2-130 不合并（batch 14 第 3 轮定）**：两条的判据不是同一个。L2-130 的问句是「`interrupted` 说的事实是真的吗」——修法 1 之后 `interrupted` 在所有分支上都可观测为等价（为假时 `exit_code` 必为 `Some`、为真时必为 `None`），这条线**可以关**。L2-131 的问句是「客户端能不能分辨 423 与 500」——只要 `.is_err()` 还在、还写一个固定字面量，**就不可能靠任何字段分辨**，不是可观测性问题而是信息已被丢弃。合并会逼出第三条修法：「只修 `.is_err()` 留固定文案」，它不修 L2-131（正是 `api.yaml:24-31` 反对的那种）却修完了 L2-130，于是合并项永远关不掉。

**顺带核到的边界（比原文宽，已实测）**：`.is_err()` 抹掉的不止状态码，还有**副作用**。`:5278-5300` 的限流拒绝分支用的是 `body.error`（`reject_if_rate_limited` 在 `:1031` 先写 `write_limit_rejection_audit` 再返回），所以那条拒绝在审计里有记录；而 gate 分支的 `.is_err()` 丢掉的是 423 那一支里 `:421-436` 写的 `append_audit` + `GateRejected` 事件**仍然会发**、500 那一支（`:405-410` 的 `map_err` 在读文件失败时提前返回）**什么都没有**。两码的差别因此不只在帧里，也在审计里：一次「gate 文件坏了」的 `/exec/stream` 请求在审计里可以是**零记录**。这是 L2-131 的加强论据，不另立条。

### 一个可执行的证伪

`daemon_stop_e2e.rs` 今天测不到这一支：要触发，得让一个 `/exec/stream` 先超时（`timeout_secs` 设小 + 远端 sleep）**再**撞上停机。这条用例不需要真实 SSH 主机，但需要一个能被 `timeout_secs` 触发超时的本地命令——`/local-exec` 的命名环境即可。

### L2-132（新，batch 14 第 3 轮）：`/terminal` 的 gate 拒绝不留痕 —— 与 `/exec/stream` 恰好相反

`terminal_attach`（`:4756-4762`）在 `on_upgrade` **之前**判 gate，`:4758` 是 `return e.into_response()`。这条路径**没有一个字节的审计**：`:421-436` 的 `append_audit` + `publish_event(GateRejected)` 在 `:437` 的 `Err(err(locked_status(), …))` 之前就执行完了，所以 423 那一支有记录——但 `:405-410` 的 `map_err` 读文件失败时**直接返回**，500 那一支零记录。

这与 L2-131 是**同一族缺陷的两个相反形态**，而且都不是「文案问题」：

| 路径 | 423（paused） | 500（gate 读失败） |
|---|---|---|
| `/exec/stream` | 帧里写死 `"execution gate paused"`（对）＋审计有记录 | 帧里**也**写 `"execution gate paused"`（错），审计**零记录** |
| `/terminal` | **HTTP 423**（对，契约亦然）＋审计有记录 | **HTTP 500**（对）＋审计**零记录** |

也就是说：**客户端看状态码的路径，审计缺；审计齐全的路径，客户端信息缺。** 两条路由各丢一半，而 `docs/api.yaml` 的 `info.description` 第二段只承诺了「客户端遇 500 要读 message」——它对 `/terminal` 成立、对 `/exec/stream` 不成立；反过来「拒绝必留审计」这条更强的性质，两处都不成文也不成立。

**判据（可执行，不需要真 daemon）**：`load_execution_gate()` 在 `execution_gate.json` 存在但内容不合法时返回 `Err`（`gate.rs:46-54`：`fs::read_to_string` 后 `serde_json::from_str` 的 `?`）。在 daemon 的测试模块里（`with_temp_config` 已具备）写一份坏 JSON 到 `<config_dir>/execution_gate.json`，然后调 `reject_if_gate_paused("mcp", "h", "uptime")`：断言 `status == 500`，并断言 `list_audit_core` **为空**。这条用例与现有的 `:6374 paused_gate_rejects_non_desktop_source_and_writes_audit` 成对——一条钉「paused 有审计」，一条钉「unreadable 没有」，后者正是要改的那个事实，所以它今天会绿、改完会红，是**故意红的**那种写法，登记时要说清。

**修法（L2，需拍板）**：在 `:405-410` 的 `map_err` 里补一次审计（`RiskLevel::Blocked`、reason 用 `format!("failed to load execution gate: {e}")`、`side_effect: None`），以及一条 `GateRejected` 事件带 `"gate_load_failed": true`。要拍板的是**审计条目的语义**：这是一次「命令被拒」还是「一次基础设施故障」？现在 `append_audit` 只接受 `ExecResult`，用 `RiskLevel::Blocked` 表示会让「服务器坏了」与「管理员按了急停」在审计里同形——那正是 L2-130/131 在修的同一类事，别在修它的时候再造一个。可选：新增 `AuditOutcome`/事件类型，或沿用 `Blocked` 但在 `reason` 前缀上钉死 `failed to load execution gate:` 并让 `LEDGER` 记录这个口径。

**成本**：`reject_if_gate_paused` 一个函数、一处 `map_err`、一个字段；测试侧一条（不需要真 daemon，不需要 WS）。比 L2-84 的 WS e2e 便宜得多。

**这条会撞一条现有契约测试（第 3 轮实测）**：`daemon_integration.rs:3792` 的 `claims_budget` 是 `path_block(spec, path).contains("drains for up to 500 ms")`，而 `:3762-3764` 的文档注释写的是「the terminal description **used to** make the budget claim for the idle timeout」。`/terminal` 的当前描述（`api.yaml:1877-1885`，`:1880` 那句）逐字是「output until 500 ms pass with no new terminal event」＋「That 500 ms is an idle timeout, not a budget」，全文**不含** `drains for up to 500 ms`。（注意 `path_block` 的边界是「下一个 `\n  /`」，`/terminal` 的块里只有 `:1862-1932`，所以别在 `:1933` 之后找。）所以循环里的 `assert_eq!` 今天是 `false == false` 通过；文档里那句「used to」是过期叙述（它对 **exec 侧**才成立）。L2-84 改法 ① 落地时必须**连描述文案一起改**（把 `drains for up to 500 ms` 写回 `/terminal` 的描述），两处漏一处就红：漏改循环 → `false vs true`，漏改文案 → `true vs false`。两个方向都红，所以「测试会提醒我」这句在**这一步**上是成立的——但它提醒的是错的那一半（改完循环、文案没改时，红的是 `measures_one_deadline`＝true／`claims_budget`＝false，而提示语 `path … does not claim a 500 ms budget, but … computes a single deadline` 会把责任指向循环）。要在 L2-84 的落地清单里指名。

### L2-133（新，batch 14 第 3 轮）：L2-130 的假绿证伪 ＋ `~52` 的注释是错的

两件事都是本轮核对时掉出来的，都便宜。

**(a) L2-130 的证伪不成立（架构上）。** L2-130 写的是「这条用例不需要真实 SSH 主机，但需要一个能被 `timeout_secs` 触发超时的本地命令——`/local-exec` 的命名环境即可」。**`/local-exec` 不是 `ExecRequest` 的第二个实现，它是另一个 handler，`/exec/stream` 根本到不了它。** `exec_stream`（`:5210`）的请求体里即使 `host`/`source` 写成 `local`，它走的也是 `spawn_embedded_exec_stream` → `exec_ssh_core`（`embedded_ssh.rs` 的 `connect_embedded_ssh`）——`/exec/stream` 不存在本地分支，`types::is_local_host` 在那条路径上没有任何调用点。所以 L2-130 的三条改法里，任何一条的回归测试都**必须**有一个会「接受 TCP 后保持沉默直到超时」的 SSH 对端。

现成的便宜做法（本仓已有，不需要 sshd）：`connect_tcp_address` 的测试形态——在 `127.0.0.1:0` 上起一个 `TcpListener`，接受连接后不写一个字节。`timeout_secs: 1` 的 `/exec/stream` 会在 `connect_embedded_ssh` 的握手阶段超时并返回 `ExecTimeout`（`embedded_ssh.rs:1355-1362` 的 `Display` 就是 `SSH command timed out after 1s`）。这条不依赖 `AGENT2SSH_TEST_SSH_PORT`，与 `tests/connect_deadline.rs`（同目录、同样自绑 loopback）是同一个形状。

**(b) 「the other ~52 daemon handlers」是 95。** `preflight_guarded_request`（`:503`）的文档注释说 `REQUEST_COUNT` 的递增「consistent with the other ~52 daemon handlers」。全仓 `REQUEST_COUNT.fetch_add(1, Ordering::Relaxed)` 实测 **95** 处（`grep -c`）。这个数字本身不影响行为，但它是**别人的引用源**：任何按「52 个 handler 都没有 preflight」推覆盖率的人会漏掉 43 个。要么改成实测数，要么删掉数字只留「consistent with every other daemon handler」。没有理由保留一个过期数——这正是本批反复在修的「写下没核过的数」。

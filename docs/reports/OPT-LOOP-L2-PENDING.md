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
| L2-82 | `bin/agent2ssh-daemon.rs:167`（`body: Option<Json<ShutdownRequest>>`）、`:204`（`None => {}`）、`:207`（`SHUTDOWN_REQUESTED.notify_one()`）；`:3797`（`dry_run_playbook_handler`）、`:5550`（`post_health_snapshot`）；机制在 `axum-core-0.4.5/src/extract/mod.rs:119-129`（`impl FromRequest for Option<T>`，`type Rejection = Infallible` + `Ok(T::from_request(..).await.ok())`） | 成立（行为，且是本轮唯一有安全后果的一条） | `Option<T>` 的拒绝类型是 `Infallible`，所以 `.ok()` **不可能失败**：content-type 不对、JSON 语法错、字段类型错，三种都变成 `None`，与「调用方没发 body」在下游完全同形。`/shutdown` 的后果最重——`{"expect_pid": "abc"}`（shell 里 `"$PID"` 带引号的产物）会让 `Option<Json<ShutdownRequest>>` 变 `None` → 命中 `:204` 的 `None => {}` → **绕过 `:182` 的 409 与 `:199` 的 400 两道守卫**，直接 `notify_one()`。契约原文承诺「naming a different PID is refused with 409」「`expect_pid: null` is a 400」，而真正会发生的第三种情况（字段在、值坏）什么都不报就关机。另两处后果轻：dry-run 的 `playbook` 不一致检查被跳过（`:3801-3807`），health-snapshot 从「指定主机」退化成「全部主机」（`:5557-5560`），在主机多的配置上是一次慢 200 而不是 422 | 换提取器，不是改 status：写一个 `OptionalJson<T>`（签名 `Bytes` + 空 body 判空 + `serde_json::from_slice`），**区分「没有 body」与「body 坏了」**，坏 body 回 `err(StatusCode::BAD_REQUEST, …)`。三处逐一把 `Option<Json<T>>` 换掉，`/shutdown` 的 `None => {}` 分支保持不变（它现在只表示真的没发 body）。断言（需 `src-tauri/src/bin/**` 内的测试模块，集成测试够不到）：`{"expect_pid":"abc"}` → 400 且进程仍在；无 body → 关机；`{"expect_pid":null}` → 400；错 PID → 409。契约在 `6fff9f1` 里已按**现状**写明，改行为时那段也要改 |
| L2-83 | `docs/api.yaml` 的 47 个 `Json<T>` operation（415/422 已按 `text/plain` 记录）；源码侧无处可改，因为拒绝由 axum 生成 | 成立（契约语义，需选方案） | 提取器的拒绝响应是 `(StatusCode, String).into_response()`（`axum-core-0.4.5/src/macros.rs:50-59`），即 `text/plain; charset=utf-8` + 一行消息，**不是** daemon 自己的 `Error` 对象。两份后果：①一个把每个 4xx 都按 `Error` 解析的客户端在 415/422 上会 parse 失败；②更麻烦的是 `JsonSyntaxError` → **400**，而 operation 的 400 本来就存在（handler 自己的），于是「body 不是 JSON」与「handler 拒绝了 body 的内容」在状态码上不可区分，body 体裁还不同。文档侧只能如实记录（已记录），要统一必须动代码 | 三选一：①（推荐）加一个项目内 `ApiJson<T>` 提取器，`Rejection = (StatusCode, Json<ErrorBody>)`，内部 `From<JsonRejection>` 把三种拒绝映射成 `Error`，47 个签名机械替换，并定一条规则：语法错继续 400 还是改 422（改 422 才能与 handler 的 400 分开，但会让所有调用方看到新码）；②47 个 handler 各自收 `Result<Json<T>, JsonRejection>`，把拒绝转成 `err(...)`——改动更大但没有新类型；③什么都不做，只在 `info.description` 里永久保留这段说明（当前状态）。①+「语法错改 422」最能消掉歧义，代价是公开状态码变化，需拍板 |

## 2026-11-10 · batch 2（issue #13）第 1 轮追加

第 1 轮 8 条指摘，核实 **成立 2 / 部分成立 3 / 不成立 3**。L1 侧本轮落 main 的是 `docs/api.yaml` 两段 WebSocket 描述（`/terminal` 的 500 ms 是空闲超时、`/exec/stream` 的 drain 不发 `exit` 帧）与一条把「预算声明」钉到「循环形状」上的断言（`a_websocket_drain_budget_claim_matches_the_loop_it_describes`）。下面三条要动 `src-tauri/src/**`。

| 编号 | 位置 | 判断 | 为什么不能自动落 | 建议改法 |
|---|---|---|---|---|
| L2-84 | `bin/agent2ssh-daemon.rs:4830`（`tokio::time::timeout(Duration::from_millis(500), event_rx.recv())`，**每次迭代新建**）、`:4783`（`event_task` 每 100 ms 从阻塞通道泵一次）、`:5014`（`run_ws_with_shutdown_deadline(..., 2s)`）；对照 `:5340`（exec 侧 `timeout_at(deadline, …)`，deadline 在取消分支里算一次）、`:5442` | 成立（行为；两处同数不同义） | 终端的 500 ms 是**空闲超时**：只要 500 ms 内有事件就继续转，`Output` 分支只 `continue` 从不 `break`。所以一个持续输出的终端在停机期间不被 500 ms 约束，只能等 `:5014` 的 2 s 把 future drop 掉。改法是动 `src-tauri/src/**` 的循环语义，且会改变客户端在停机期间能收到多少输出（可能少收），属对外行为 | 二选一：①（推荐）把终端 drain 改成 `timeout_at(Instant::now() + 500ms, recv)`，与 exec 一致——同一个 `POST /shutdown` 描述里「up to two seconds for each connection」与 per-route 的「500 ms」只有在①下才同时成立；②保留空闲超时，只维护文档（本轮 L1 已按②落地）。改①时必须同步改 `docs/api.yaml` 的 `/terminal` 描述，`a_websocket_drain_budget_claim_matches_the_loop_it_describes` 会在漏改时变红并点名。回归面：`daemon_stop_e2e.rs` 的 `stop_exits_when_exec_websocket_peer_stops_reading` 与终端相关两条 |
| L2-85 | `daemon_control.rs:986-989`（`UnverifiedIdentity` 的文案）、分支入口 `:982`（`health.as_ref().map(\|i\| i.pid) == Some(pid)` 的 `else`） | 成立（诊断文案） | 文案括号里列了三种原因（health PID / config directory / admin token），但这一支**只可能**是「没有 `/health` 答复」或「`/health` 报了另一个 PID」：`config_dir` 的比对在 `:959-981` 的 `if` 分支里，`admin token` 的比对在 `:991` 之后。打印这句话时，括号里后两种原因都不可能成立，运维会去查两个没参与判定的东西 | 括号收窄成「no `/health` answer, or it named another PID」。**没有测试钉这条分支**：`daemon_stop_e2e.rs:872-877` 只断言 `contains("could not be authenticated")`（首句），加一句「不含 `config directory`、不含 `admin token`」就能把括号钉住。属 `src-tauri/src/**` 文案 + 测试断言 |
| L2-86 | `daemon_activity.rs:205-217`（`fetch_daemon_activity` 只串行读 `/sessions`、`/resident-sessions`、`/forwards`、`/approvals`）、`:180-195`（`summarize_activity` 折四个计数）、`tauri_commands.rs:2016-2049`（`confirm_quit` 以 `is_idle()` 决定弹不弹框） | 部分成立（产品口径） | 一次在途的 `/exec` **不在任何列表里**：`exec`（`bin/agent2ssh-daemon.rs:1665`）不注册 registry，`resident_session.rs:361-367` 的摘要只有 `id/host/label/idle_secs/dead`。所以「有命令在跑」的 daemon 被 `is_idle()` 判成空闲，退出确认框不弹，`shutdown` 直接中断那条命令。要不要把 `/exec` 纳入退出确认是产品决策（`/exec` 自带 `timeout_secs` 上界），且要动 `src-tauri/src/**` | 若要做：daemon 侧先有一条可读的在途视图（在 `exec` 与 `exec_stream` 入口 `register` 一个 owner，或在 `AppState` 里加一个 `AtomicUsize` 计数器 + `GET /exec/activity`），再让 `fetch_daemon_activity` 读它、`DaemonActivity` 加 `running_execs` 字段（`Serialize` 变化会牵动桌面契约与 `quit_confirm_labels` 的文案键）。若不做：至少在 `docs/architecture.md:191` 那段「问 daemon 会丢什么」后面注明 `/exec` 不计入，免得读文档的人以为它是全量 |

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
| L2-96 | `sftp_transfer.rs:365`（`STALL_TIMEOUT`）、`:407-416`（`StallGuard::write`）、`russh_impl/sftp.rs:223-231`（`File::write`） | 成立（结论对，机制说反） | `Ok(0)` 在当前传输上不出现：`File::write` 返回 `Ok(min(buf, max_write_len))` 或 `Err`，所以 600 s 分支与 `write_all_shrinking` 的减半分支都不可达。对端不 ACK 时 `russh_sftp` 的 `poll_write` 在 8 个在途 ack 上阻塞，`write_nowait` 的接收端没有被 `request_timeout_secs` 包住，`block_on` 也没有超时 ⇒ 仍然是「无错误地挂着」。指摘把上界说成 60 s 会话超时，但 `write_with_timeout` 只服务 `Channel::write`（shell/exec），不服务 SFTP 句柄 | 给 SFTP 写路径一个真截止时间：在 `File::write` 外面包 `tokio::time::timeout`（**不要**用 `request_timeout_secs`——它只服务 `request()`，对写 ack 无效）。闸门只有 `write_acks` 的 8 个在途请求一处（`russh_sftp` 的 `mpsc::unbounded_channel` 没有背压），所以要包住 **`write` 与 `flush` 两处**——`File::flush` 的 `poll_drain_writes` 抽的是同一个队列（`poll_shutdown` 也是，但本仓的 `impl Write for File` 没有 `shutdown`）；文案仍由 `StallGuard` 负责「已写多少字节」。**落点必须在 `transport/russh_impl/sftp.rs:223` 的 `impl Write for File`**：那里是 `StallGuard` 唯一能等到的地方；在 `core.rs` 的 `spawn_blocking` 闭包里包 `tokio::time::timeout` 无效（同步闭包里没有 reactor，`block_on` 会互相等）。另注意 `timeout` **不取消**内部 future——超时后 `File` 仍攥着那 8 个 ack，必须先 abort session，否则下一个请求在同一个队列上继续卡。**超时分支的文案必须自己读 `bytes_written`**：`StallGuard::write` 在 `inner.write` 返回 `Err` 时先 `?` 走人，`stalled()` 不会在真错误上被调用，「命名写了多少字节」不会自己出现。需一条能复现「读 TCP、不应答 SFTP」的集成测试 |
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

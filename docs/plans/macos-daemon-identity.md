# macOS daemon 身份绑定：实测证据与决策附录

本文件是 [`daemon-identity-protocol.md`](daemon-identity-protocol.md) 的 macOS 分册，服务于 #25。它只做两件事：把该提案里标「待验证」的 macOS 平台能力换成**本机实测**，并把 macOS 侧的取舍收敛成可直接拍板的决策。**本文件不改任何行为**：`src-tauri/src/**` 一行未动，所有结论都能用下面给出的命令复算，凡是没跑的都在 §7 单列。

- 实测主机：macOS 27.2.1（build 26B131），arm64，`csrutil status` = enabled，测试进程 uid 501、普通用户（非 root）。
- 源码基线：`4d06549`（本文所有行号以此为准；结论引用的是这个提交的文件内容）。
- 探针：两个独立 C 程序（源码与原始输出归档在 `docs/reports/evidence/macos-identity-2026-11-12/`），只用 `socketpair` / `sendmsg(SCM_RIGHTS)` / `kqueue` / `proc_pidinfo` / `kill`；未触碰仓库文件，未触碰 `~/.agent2ssh`。

> **2026-11-12 后续复核更正（基线 `17068ff`）**：以下原始表格中的 MD4、M4 和 E3b 已按新实测收窄。`NOTE_EXIT` 单独注册成功不能证明有发信号权限；同时设置 `NOTE_EXIT | NOTE_EXITSTATUS` 对本机 pid 1 返回 `EACCES`。这只说明当前组合、当前时刻的权限检查，不能证明此后 `kill` 必然成功，更不能消除 PID 复用。客户端的 `LOCAL_PEERPID` 在普通服务端响应后等于服务端 pid，但服务端将已连接 fd 交给仍存活的子进程写入后，返回子进程 pid；读数不是监听进程的稳定绑定。三轮原始输出和源码见 [后续探针](../reports/evidence/macos-identity-followup-2026-11-12/output.txt)。

## 0. 结论先行

| # | 决策 | 推荐 | 一句话理由 | 代价 |
|---|---|---|---|---|
| MD1 | macOS 是否也走「必须 Bound 才自动强杀」 | **先不做**，保留「名称白名单 + 起始时间 + `/health` + token」并明写它是 Unbound；等 #22 的 D4 定案后再上 Unix socket（方案 B） | macOS 上没有任何原语能把「读记录 → 观测 → 发信号」变成原子，方案 B 也只能把窗口收窄到「内核给了对端 PID」这一步；先上会与 #22 的协议抢定义 | 0（纯文档） |
| MD2 | 起始时间精度 | **做**：macOS 侧改用公开的 `proc_pidinfo(PROC_PIDTBSDINFO).pbi_start_tvusec`，从整秒降到微秒 | 今天「同一秒启动的两个进程不可区分」在 macOS 上是**构造上必然**（`sysinfo 0.39.3` 只取 `pbi_start_tvsec`）；微秒字段就在同一个公开结构里，对自身实测可读 | 核对全部六个读取行与五个比较点（详见 L2-189 更正）+ `daemon.owner` 的秒级编码（与 #22 D6 绑定）→ L2-189 |
| MD3 | 是否把 `p_uniqueid`（upid）写进记录当判据 | **不作判据**，只允许出现在诊断输出里 | flavor 17 在 macOS 27.2.1 上**确实可读**（实测 56 字节），但它既不在公开 SDK 头文件里也不在 `libc` 里，属未公开 ABI；把它当身份判据等于用「可能明天改名的字段」决定要不要 `kill` | 0（不采纳） |
| MD4 | `kqueue NOTE_EXIT` 能否当「可发信号」的闸门 | 单独 `NOTE_EXIT` 不能；双标志可提供瞬时权限线索 | pid 1 单标志注册成功、`kill(1,0)` 为 `EPERM`；双标志返回 `EACCES`。父子关系、权限变更与 PID 复用仍需另行处理，不能将其当稳定授权或进程绑定 | MACOS-2 的辅助手段 → L2-190 |
| MD5 | 代理 / 端口转发下的 `daemon stop` | **明确拒绝并指向带外**，不尝试穿透 | 实测：`LOCAL_PEERPID` 的语义是「最近一次访问者」，已连接 fd 交给子进程且由其写入后，对端观测到子进程 pid；代理结果仍取决于实际连接和访问路径 | 文案与分支 → 归入 D5 |
| MD6 | 是否保留纯数字 `daemon.pid` | **保留**，新证据另立文件/字段 | 与 #22 D6 的 (b) 一致；本文没有发现必须改格式的理由 | 0 |

MD1、MD2 等待协议决策；MD4 的双标志组合仍需评估父子关系、权限变化和协议语义，不能由单点实测直接拍板。

## 1. macOS 上实际能拿到什么（实测）

复算方式：源码与原始输出已归档在 [`docs/reports/evidence/macos-identity-2026-11-12/`](../reports/evidence/macos-identity-2026-11-12/output.txt)——`clang -O1 -o probe probe.c && ./probe`，第二个程序 `probe2.c` 只测 kqueue 注册形状。下表 PID 与时间值使用归档中的那次执行。

| # | 能力 | 实测结果 | 能证明什么 / 不能证明什么 |
|---|---|---|---|
| E1 | `getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID)` | `socketpair` 两端都读到本进程 pid（19564） | 能：这条 socket 的**最近访问者**是谁。不能：谁 `connect` 的（见 E2） |
| E1b | `LOCAL_PEERCRED` | `struct xucred` 返回 `version=0, uid=501, ngroups=16` | 能：对端 euid/egid 及其附加组。不能：对端 pid（`getpeereid(3)` 就是这一层） |
| E1c | `LOCAL_PEERTOKEN` | 返回 **32 字节**，前 8 字节 `f5010000 f5010000`（auid=501、euid=501，小端） | 能：对端审计令牌（可用于 `audit_token_to_pid`）。代价：要用私有 flavor 号自己解 |
| E2 | fd 经 `SCM_RIGHTS` 交给子进程后 `LOCAL_PEERPID` | 交出去前 = 19564（父），子进程发一个字节后 = **19565（子）** | **关键否证**：`last_pid` 是「最近访问者」，不是「连接所有者」。任何把 `LOCAL_PEERPID` 当「谁拥有这条连接」的方案都是错的 |
| E3 | `kqueue` + `EVFILT_PROC` + `NOTE_EXIT` 注册 | 不存在的 pid → `kevent()` 返回 **1**，事件带 `EV_ERROR`，`data=ESRCH(3)`；自己 / 自己的子进程 / **pid 1** → `EV_ERROR` 置位但 `data=0`（成功） | 能：`EV_ERROR && data==0` 表示「注册那一刻它存在且可观察」。不能：当成权限闸门（pid 1 也成功）；这几例的错误从 `EV_ERROR/data` 返回；仍须处理 `kevent() == -1` |
| E3b | 同一组去掉 `EV_RECEIPT` / 换 `NOTE_EXITSTATUS` | 不存在的 pid 仍是 `EV_ERROR data=ESRCH`；pid 1 的 `NOTE_EXITSTATUS` 也 `data=0` | 这两种单标志未出现 `EACCES`；后续双标志实验对 pid 1 出现 `EACCES`。原 `probe2.c` 最后一行标签写了 `NOTE_FORK|NOTE_EXIT`，实参只有 `NOTE_FORK`，不能按标签解释 |
| E4 | `kill(pid, 0)` | self → 0；pid 1 → `-1 EPERM`；不存在的 pid → `-1 ESRCH` | 与代码现状一致：`pid_definitely_missing`（`daemon_control.rs:308-322`）只把 `ESRCH` 判为「确实不存在」，`EPERM` 落 `Unverified`，方向正确 |
| E5 | `proc_pidinfo` | `PROC_PIDTBSDINFO`：self 返回 full size（`pbi_start_tvsec=1794471890`、`pbi_start_tvusec=979734`）；**pid 1 返回 0 且 `errno=EPERM`**。flavor 17（XNU 的 `PROC_PIDUNIQIDENTIFIERINFO`）：self **56 字节**、`p_uniqueid=514474`；**pid 1 也返回 56 字节、`p_uniqueid=1`** | 能：微秒级起始时间（公开头文件 `sys/proc_info.h:80-81`）。边界：本机 pid 1（其他 uid）返回 0/EPERM；仅此不足以概括所有不同 uid 或同 uid 目标的可见性；flavor 17 反而能读到其他 uid，但它是未公开 ABI（MD3） |

### 1.1 依赖侧核实（原分析标「未核实」的两条）

| 问题 | 实测/读源码结果 |
|---|---|
| `libc` 是否导出 `LOCAL_PEERPID` / `LOCAL_PEERTOKEN` | **导出**。`libc 0.2.186` 的 `src/unix/bsd/apple/mod.rs`：`LOCAL_PEERCRED` `:2908`、`LOCAL_PEERPID` `:2910`、`LOCAL_PEERTOKEN` `:2918`；同一文件 `:3684` 有 `PROC_PIDTBSDINFO`、`:4902` 有 `proc_pidinfo` |
| `libc` / SDK 是否有 `proc_uniqidentifierinfo` / `PROC_PIDUNIQIDENTIFIERINFO` | **都没有**。`grep -rn "PROC_PIDUNIQIDENTIFIERINFO\|p_uniqueid" ~/.cargo/registry/src/*/libc-0.2.186/src` 零命中；`grep -rln uniqidentifier "$(xcrun --show-sdk-path)/usr/include"` 只命中无关的 `kern/kcdata.h`。要用只能自己定义 `const PROC_PIDUNIQIDENTIFIERINFO: c_int = 17` 并按 56 字节裸读 |
| macOS 上 `start_time` 的秒级从哪来 | `sysinfo 0.39.3` 的 `src/unix/apple/macos/process.rs:385`：`let start_time = info.pbi_start_tvsec;`——**只取秒**，微秒字段就在同一个 `struct proc_bsdinfo` 里却没被读 |

## 2. 由此得到的三条硬结论

1. **macOS 上没有「进程对象」这个原语。** 发信号只有 `kill(2)`，按数字寻址；`kqueue` 的 knote 是注册时的一次 `proc_find` 引用，不提供「用这个引用发信号」的入口。所以「核验 → 发信号」的窗口**在任何 macOS 方案里都关不掉**，只能收窄（本文的 MD2/MD4 各收窄一段）。
2. **能观察 ≠ 能发信号。** 实测 E3 与 E4 给出的是相反方向的答案：`pid 1` 上 `kqueue` 注册成功、`kill(1,0)` 却 `EPERM`。任何把「注册成功」当「有权限停它」的判据都会在 root 进程上误判。
3. **`LOCAL_PEERPID` 回答的是「谁刚写了这条 socket」，不是「谁拥有监听」。** E2 是决定性的：fd 一旦被传走，答案就换人。它能支撑的结论上限是「此刻发请求的是哪个 pid」，而这恰好是 §3.2 想让内核回答的那一问——这也是 MD1 里方案 B 唯一真实收益的来源。

## 3. 对 #25 现有分析的判定

对照的是 #25 的第一条 CodeBuddy 只读分析（`2120409164848472064`，基线 `380e166`）。判定只针对**可复算的事实**，不评价方案偏好。

| # | 该分析的说法 | 判定 | 证据 |
|---|---|---|---|
| 1 | 控制面是 TCP，`127.0.0.1:7722`，本机任何进程都能连 | **成立** | `remote.rs:13`（常量）、`agent2ssh-daemon.rs:5826`（唯一一条 `TcpListener::bind`）。行号已漂移：它写 `:5790` |
| 2 | 全仓 `LOCAL_PEER`/`SO_PEERCRED`/`getpeereid`/`UnixListener` 零命中 | **成立** | 同一组 grep 在当前树仍是 **0**；`UnixStream` 只出现在 ssh-agent 通道（`transport/russh_impl/agent.rs:66/339`） |
| 3 | `health_identity()`「只认 `HTTP/1.1 200` + `{"ok":true,"pid":N}`」 | **部分成立（引用错函数）** | `health_identity`（`daemon_control.rs:231-253`）是对**响应体**做 `serde_json::from_str`、校验 `ok` 为 bool、`pid` 转 u64；它引的那句子串判定在 `daemon_health_ok`（`:166-194`），而后者生产零调用（唯一使用点是集成测试，已记为 **L2-187**）。结论「本机任何进程能回这个形状」仍成立，机制不同 |
| 4 | `kill(pid,0)`：`ESRCH`=不存在、`EPERM`=存在但无权限，代码只把 `ESRCH` 判死 | **成立** | E4 三行实测 + `daemon_control.rs:308-322` |
| 5 | `kqueue EVFILT_PROC` 注册：已退出返回 `ESRCH`，无权限返回 `EACCES`，且「手册与实现对不上，必须实测定论」 | **部分成立** | `ESRCH` 成立（E3）；旧的单标志 E3/E3b 未观察到 `EACCES`（后续双标志见上）。后续双标志在 pid 1 上返回 `EACCES`；本次 `EV_RECEIPT` 各例通过 `EV_ERROR` 的 `data` 返回结果，不能据此排除其他参数或系统调用失败时返回 `-1` |
| 6 | `proc_pidinfo(PROC_PIDUNIQIDENTIFIERINFO)` 可读 `upid`，对其他 uid 的权限「未确证」 | **成立 + 补实测** | E5：self 与 pid 1 都返回 56 字节，`p_uniqueid` 分别 514474 / 1。但该 flavor 不在公开 SDK 与 libc（§1.1）→ MD3 |
| 7 | `start_time` 在 macOS 是秒级（sysinfo 读 `pbi_start_tvsec`） | **成立** | `sysinfo 0.39.3` `process.rs:385`；同一结构里的 `pbi_start_tvusec` 未被使用（§1.1） |
| 8 | `libc` 是否导出 `LOCAL_PEERPID`/`LOCAL_PEERTOKEN` 未核实 | **已核实：导出** | `libc 0.2.186` `:2908/:2910/:2918`（§1.1） |
| 9 | `daemon_stop_e2e.rs`「共 74 个 `fn`，其中 `#[cfg(unix)]` 有 13 项」 | **不成立（数字）** | 实测：**51 个 `fn`、34 个 `#[test]`、34 个 `serial`、15 个 `#[cfg(unix)]` 属性**；其中带测试属性的是 **8 个**（另有 5 个夹具和 2 条 `use`）。方向对：两个核心竞态用例确实被 unix 门控 |
| 10 | 「因此 Windows 与 macOS 上这两条路径零覆盖」 | **macOS 这半不成立** | `#[cfg(unix)]` 在 macOS 上成立，那 8 条用例**在 macOS 上照常编译执行**。本轮实测：`cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1` → **34 passed / 0 failed / 40.75 s**，单独点名复跑 `a_daemon_that_ignores_the_expected_pid_stops_and_says_so`（1 passed / 8.45 s）与 `a_stop_that_reached_the_wrong_daemon_stops_nothing`（1 passed / 1.91 s）也都通过。只有 Windows 零覆盖。计划稿 §5 里同一句话也带着这个错，本轮一并改掉 |
| 11 | 行号引用（`daemon_control.rs:62-72` / `:205-215` / `:344-367` / `:759-786` 等） | **大体准，整体下移** | 当前：`read_daemon_pid:56`、`DaemonPidState:210`、`daemon_pid_state:343`、`signal_daemon:763`、`wait_for_exit:749`、`observe_process:732`、`stop_daemon_process:979`、`start_daemon_background:416`、`setsid:478`、`spawn().id():494-497` |
| 12 | 方案 B（Unix socket 对端凭证）「会影响 `api.yaml` 契约测试、CLI/桌面/MCP 三面的连接逻辑」 | **代价被高估** | 本地 daemon 请求只有两个接缝：`daemon_activity.rs:44`（`local_daemon_request`）与 `:65`（`..._with_body`），连 `health_identity` 也走前者；`stop_daemon_process` 的调用点共 **5 处**（`tauri_commands.rs:1855/1885/1918`、`bin/agent2ssh.rs:3080/3107`），CLI/桌面/MCP 全部经它，不需要三面各改一遍 |

## 4. 与 #22 决策表的对接（macOS 侧的真实代价）

| #22 的决策 | macOS 侧的实测影响 |
|---|---|
| D3（强杀前置） | 选 (a)「仅 `Bound` 可强杀」= 按当前已验证方案无法自动强杀（尚未证明不存在其他可满足 `Bound` 的方案），残留 daemon 只能带外 `kill`。选 (c)「分平台」= macOS 保留今天的「白名单名 + 秒级起始时间」，而这个判据已被 `daemon_stop_e2e.rs:1434` 的 symlink 替身用例证伪为「不是绑定」。**没有第三个选项**：MD2 的 µs 精度只能把「同秒碰撞」变成「同微秒碰撞」，不构成 `Bound`。 |
| D4（是否为停止单开本机 IPC） | 若选 (a)，macOS 分支的改动面是：daemon 侧 +1 个 `UnixListener`（现有 `TcpListener` 在 `agent2ssh-daemon.rs:5826`）；客户端侧改 `daemon_activity.rs:44/65` 两个函数；`api.yaml` 需要说明「本机 IPC 不是 HTTP 路由」（否则 `api_yaml_documents_every_daemon_route` 契约测试会把它算成缺文档的路由）。**`stop_daemon_process` 的 5 个调用点不用动。** |
| D5（端点路径/ACL） | macOS 侧无额外障碍：`LOCAL_PEERCRED` 可用（E1b），所以「socket 文件 0600 + 目录 0700 + 对端 euid == 自己 euid」这条三重检查在 macOS 上可实现；但**代理场景必须降级**（E2/MD5）。 |
| D6（记录格式与兼容期） | MD2 会把起始时间从「秒」变成「微秒」；旧 `daemon.owner`（`daemon_control.rs:625-643` 读写的 `{pid, start_time}`）里的 `start_time` 是秒。旧记录是秒级且已丢失微秒：单纯乘以 `1_000_000` 只得到下界，不能无条件与微秒值判等。必须标记单位和精度并定义安全的兼容策略。这条要写进 D6 的兼容表。 |

## 5. 推荐实施顺序（每步可独立回滚）

1. **MACOS-0：只读诊断（不改任何判据）**。给 `daemon_pid_state` 旁边加一个只读的 `describe_identity(pid)`，输出 `PROC_PIDTBSDINFO` 的 `(pbi_start_tvsec, pbi_start_tvusec, uid, comm)` 与 `kill(pid,0)` 的 errno、`kqueue` 注册的「存在/ESRCH」。它让 #21 有料可展示，且**零风险**（不参与任何 `kill` 决策）。→ 这一步就能把 §7 的 M6/M7/M10/M12 从「未做」变成「有诊断可看」。
2. **MACOS-1：µs 精度（L2-189）**。只改起始时间的读取与编码，判据形状不变；先写一条「同一秒内的两个进程现在可区分」的用例，再改实现。
3. **MACOS-2：评估 `kqueue` 用于诊断（L2-190）**。如考虑影响强杀，须先解决 #22 的身份绑定和竞态；必须同时处理 `EV_ERROR/data` 与 `kevent() == -1`。
4. **MACOS-3：Unix socket 控制面（L2-191）**。**依赖 D4 定案**。顺序上必须在 #22 之后，否则会造出第二套并行协议——`/shutdown/guarded` 已经为此付过一次代价（见 K14 末段）。

## 6. 验收矩阵现状（#25 正文的 M1–M12）

| 项 | 状态 | 说明 |
|---|---|---|
| M1 `LOCAL_PEERPID` 基本语义 | **已完成（实测）** | E1 / E1b / E1c |
| M2 fd 传递后 `last_pid` 改写 | **已完成（实测）** | E2，反例已跑出来 |
| M3 代理场景 | 未做 | 需要 Unix socket 控制面先存在（MACOS-3） |
| M4 `EVFILT_PROC` 注册语义 | **部分完成（实测）** | E3 / E3b 与后续双标志测试覆盖本机 self、子进程、pid 1、缺失 PID；沙箱和其他权限组合未验证 |
| M5 `kill(pid,0)` 权限矩阵 | **已完成（实测）** | E4（同 uid / root / 不存在三档；沙箱档未做） |
| M6 PID 复用 | 未做 | 需构造复用，本机无法稳定复现；`upid` 可行但不作判据（MD3） |
| M7 损坏/空/0 PID 文件 | 未做 | 需要真 daemon 跑 CLI；`read_daemon_pid:56-74` 的「保留并报错」目前只有读码依据 |
| M8 并发替换 | 未做 | 已有确定性用例覆盖锁语义，但没造真实双进程竞争 |
| M9 旧 daemon 回执 | 未做 | 由 `daemon_stop_e2e.rs:1434` 的桩覆盖，本轮未新增 |
| M10 权限不足 | 部分 | E4/E5 给出了 `EPERM` 的形状；「受限沙箱」档没造 |
| M11 非 loopback 绑定 | 未做 | 与 #21 的暴露面议题重叠 |
| M12 僵尸 | 未做 | 依赖 `waitid(WNOWAIT)` 路径的真机复核 |

**已跑通的基线**：在本机（macOS 27.2.1 arm64）跑 `cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1` 得 **34 passed / 0 failed / 40.75 s**。这 34 条里含 8 条 `#[cfg(unix)]` 用例，所以「macOS 上这些路径零覆盖」是错的——它们是**在 macOS 上被执行且有断言**的。这也意味着 D3 选 (a) 的代价不是「删掉一段没人测的代码」，而是「放弃一段在 macOS 上有回归保护的自动停止」。

## 7. 未验证与残余风险（不得写成已解决）

1. `pbi_start_tvusec` 的**实际分辨率**没有逐次采样验证：本文只证明「字段可读且非零」（979734 µs），没有证明两个相隔 1 µs 启动的进程一定可区分。要证明它，需要构造高频 fork 循环并统计重复率。
2. `kqueue` 的错误形状只在**本机 27.2.1、arm64、非沙箱**下测过。App Sandbox 或受限 entitlement 的进程上 `EVFILT_PROC` / `kill` 仍可能给 `EACCES`/`EPERM`，本文没有造出那种进程。
3. flavor 17 的 56 字节布局是按 XNU 源码里的字段顺序解释的（前 16 字节 uuid、随后两个 `uint64`）；**没有**在第二个 OS 版本上验证过偏移。这也是 MD3 不采纳它的原因之一。
4. 所有「对 root 进程的观察成功」结论都建立在「目标进程存在且未受限」之上；`pid 1` 是特例里最稳的一个，不能外推到任意 root 进程。
5. `daemon.token` 是文件、复制即共享，这条**没有**被本文削弱；它属于 #22 的 D1（是否排除同用户主动攻击者）。
6. 本轮没有启动任何真 daemon，也没有改 `src-tauri/src/**`，因此「修了没有」不适用于本文——MD2/MD4 之后的行为改动分别是 L2-189 / L2-190。

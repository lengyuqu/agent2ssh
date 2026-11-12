# Daemon 身份绑定与异常恢复协议：设计提案

状态：**待评审；未实现**（2026-11-11 起稿，2026-11-12 修订）

范围：[CNB #22](https://cnb.cool/zdzso/agent2ssh/-/issues/22)，基线 `4487f7f`（起稿时 `380e166`；文中行号已按 `4487f7f` 全部复核，不是起稿时的快照）

关联：[#20 回执](https://cnb.cool/zdzso/agent2ssh/-/issues/20)、[#21 恢复](https://cnb.cool/zdzso/agent2ssh/-/issues/21)、[#23 Linux](https://cnb.cool/zdzso/agent2ssh/-/issues/23)、[#24 Windows](https://cnb.cool/zdzso/agent2ssh/-/issues/24)、[#25 macOS](https://cnb.cool/zdzso/agent2ssh/-/issues/25)

## 1. 结论与边界

提议将**发现实例、连接认证、目标进程绑定、执行停止和清理记录**分开处理。自动强制终止的门槛是：在本次操作中获得由平台提供的目标进程对象，并以已验证的方法证明它属于本次控制连接的目标；此后等待和终止都使用同一对象。不能建立或保持这条绑定，就不按 PID 自动发信号。这个门槛是设计目标，不代表各平台当前已有实现。

身份绑定要解决两类不同问题：一是实例 A 已退出后 PID 被 B 复用，操作不能落到 B；二是发出停止请求的客户端需要判断**回答它的服务端**是否是期望的实例。仅解决第一类，不证明第二类。持久记录、随机 ID、健康响应和共享 token 可辅助发现、认证授权或拒绝不一致，不能各自证明以上两项。

**一条结构约束（2026-11-12 核实）**：当前控制面只有一条 TCP 监听（`agent2ssh-daemon.rs:5826`），全仓没有任何 `SO_PEERCRED` / `GetNamedPipeServerProcessId` 调用（`git grep` 零命中），客户端识别服务端所依据的 `pid` 是 `/health` **响应体里的自报字段**（`daemon_control.rs:231-253`）。因此本节下面 §3.2 的绑定规则在现有 TCP 通道上**不可达**——这不是「实现时再验证」，是传输层没有对端凭据语义。是否为本机停止单开一条 IPC 通道，是 §7 的 **D4**，必须在 #23 动工前拍板。

默认威胁范围涵盖诚实进程重启、旧文件、PID 复用、代理/端口切换、不同用户和权限不足。**能够读取或修改同用户配置、替换二进制或接管控制端点的主动攻击者不在本提案的安全保证内**；不同用户隔离需要依赖经验证的权限/ACL。即便排除此攻击者，也不能用其作为理由省略正常重启、继承或竞态测试。

建议按阶段交付：先做 #20 的回执和 #21 的诊断；**#22 只交付平台无关的状态机、错误分类与本文 §7 的决策表**，原型验收由 #23–#25 各自在自己平台上完成并回报——#22 没有平台归属、也没有三平台 CI，承担「需要真机时序」的验收会形成责任真空。旧版兼容是否允许不绑定的停止请求、空 body 是否继续受理、以及产品是否要求解决同用户对抗，均为**待决事项**，不可在实现时悄悄选定。

## 2. 当前代码事实与需纠正的分析

| 项目 | 当前行为与限制 |
|---|---|
| PID 文件 | `daemon_control.rs::read_daemon_pid` 读取纯数字；非法内容保留并报错。`write_daemon_pid` 在 `.daemon-pid.lock` 下检查旧 PID；无法证明旧进程不存在时拒绝覆盖。锁协调遵守它的进程，不能认证身份。 |
| 现有并发防护 | daemon 先绑定 TCP，再调用 `write_daemon_pid`；`direct_daemon_with_another_port_cannot_replace_a_live_pid` 验证同目录、不同端口的第二个新版 daemon 因现存活 PID 而退出。CodeBuddy 所称「只换端口即可让两个新版实例同时运行」与这条路径不符；旧程序不遵守锁、文件被移除或替换等情形仍不受同样保证。 |
| 状态与停止 | `daemon_pid_state` 通过快照、`/health` 和 token 探测分类；`stop_daemon_process` 还比较健康响应的配置目录，再发带 `expect_pid` 的 `/shutdown`。这些是多次独立观察；`daemon_matches_config` 本身不比较 `config_dir`，不要把两个入口说成同一判据。 |
| 强制终止 | 关闭请求被接受但宽限后仍存活时，`signal_daemon`（`daemon_control.rs:763-792`）重查名称、秒级启动时间并按 PID 强制结束；重查与信号不是原子操作。**这里的「名称」是硬编码四字面量白名单**（`is_daemon_executable:141-154`）：`agent2ssh-daemon` 与 `-aarch64-apple-darwin` / `-x86_64-unknown-linux-gnu` / `-x86_64-pc-windows-msvc` 三个三元组后缀，不是「看起来像 daemon」。打包三元组不在这四项内的构建，在停止路径上等价于 `Unbound`：既停不掉（`:774-779` 判据不中就 `Ok(())` 静默返回、不发信号），也起不来（`start_daemon_background:448-452` 判 `RunningUnhealthy\|Unverified` 后拒绝）。未知状态保留记录。 |
| 请求体 | `380e166` 已让 `/shutdown` 的无效非空 body 返回 400；空 body 仍是管理员可用的无 PID 检查关闭。旧 daemon 可以忽略 `expect_pid`。 |
| 回执 | `request_shutdown(Some(pid))` 把成功 200 归入 `AcceptedUnchecked`；新旧 daemon 的成功回包无法区分。修复及兼容策略归 #20；回执不证明响应者身份。**后果是这条 warn 每次正常停止都写**：能落到 `AcceptedUnchecked` 的只有 `200`，而真实停止永远带 `expect_pid`，所以 `daemon_control.rs:1053-1061`（文案在 `:1057`）在每次成功停止后都会记一条 `daemon accepted the shutdown without checking expect_pid (daemon predates it)`——指控一个并非事实的版本过旧。代码注释 `:1039-1052` 已自陈这是已知缺陷。 |
| 健康探测 | `daemon_health_ok`（`daemon_control.rs:166-194`）用子串判定：`response.starts_with("HTTP/1.1 200") && response.contains("\"ok\":true")`（`:193`），只伪造响应头也能通过。**但它当前在生产路径上零调用**：唯一调用者是 `wait_for_daemon_health`（`:196-205`），后者全仓无调用者，只有一个集成测试（`daemon_integration.rs:5420`）直接打它。真正决定 `start` / `status` 的是 `health_identity`（`:231-253`）对**响应体**的 `serde_json` 解析，头伪造过不去。所以这是休眠隐患，不是今天可用的拒绝服务路径。 |
| 实例归属记录 | `daemon.owner`（文件名不是 `daemon-owner.json`）已经是 `OwnerMarker { pid, start_time }` 两字段（`daemon_control.rs:582-585`）。`mark_daemon_owned:599-609` 在 `.daemon-pid.lock` 内写；`clear_daemon_owner_if:625-643` 在锁内重新匹配 pid **与** start_time 才删；但 `daemon_is_owned:612-623` **不加锁**直接读，判定与使用之间没有互斥。 |

参照源码：`src-tauri/src/daemon_control.rs::{write_daemon_pid, daemon_matches_config, stop_daemon_process, signal_daemon}`、`src-tauri/src/bin/agent2ssh-daemon.rs::{daemon_shutdown, main}` 与 `src-tauri/tests/daemon_stop_e2e.rs::direct_daemon_with_another_port_cannot_replace_a_live_pid`。

## 3. 候选控制协议与信任模型

### 3.1 实例记录和协议数据

继续保留纯数字 `daemon.pid`，供旧客户端读取。可新增版本化 `daemon.instance.json`（**新文件**，与既有的 `daemon.owner` 并存、职责不同：后者只记录「桌面端起的那些 daemon」），包含协议版本、PID、实例随机 ID、启动观察值、平台和控制端点；它**不是密钥或认证凭据**，不得含 token。随机实例 ID 用于关联日志与发现矛盾，不单独授权停止、强杀或覆盖记录；记录丢失、复制、损坏、跨机器迁移、未知版本均进入未绑定/诊断状态。

由 daemon 在现有锁下发布 PID 和实例记录；实例记录以同目录临时文件加原子替换发布，并注明目录持久化的可用性。**两份文件不是一个原子事务**：锁只约束遵守协议的读写者，崩溃可产生单边旧记录。新读者在锁内重读、核对版本、PID 与实例 ID；任一不一致只能拒绝或进入恢复，不能据此判定进程已死。旧程序、不同 inode 的锁或不可靠的网络文件系统不受此协调保证。条件删除须在锁内重新匹配完整记录；旧版只按 PID 清理的行为不能由新协议消除。

**这不是全新的模式，评审成本可以按「既有约定再走一步」来估**：`daemon.owner` 今天已经是「pid + start_time 两字段 + 锁内条件删除」（`mark_daemon_owned:599-609`、`clear_daemon_owner_if:625-643`）。同时它给出了这条要求的**现成反例**——读侧 `daemon_is_owned:612-623` 不加锁，因此「读到标记成立」与「据此动手」之间没有任何保护；`store.rs` 只有阻塞式 `lock_exclusive` / `lock_shared`（`:72-86`），没有 `try_lock`，读时持锁这条捷径不存在。新记录必须把这一对读写都纳入同一把锁，否则会复制同一个洞。

锁内记录相等只是**防文件替换**，不是目标进程身份或退出的证明：若同一 PID 已被复用，旧 PID 字符串仍相等。必须另有针对原实例的退出证据；否则保留记录。记录发布与恢复需要列明损坏文件、同名临时文件、写失败、rename 前后崩溃、锁文件被替换，以及符号链接和非普通文件的处理；不得在不可信路径上跟随链接覆盖其他文件。原子替换只对适用的同文件系统操作提供保证，不等同于断电后持久化，具体文件及目录同步能力应逐平台验证。

锁只用于短时间文件事务，**不在持锁期间等待网络、进程退出或用户确认**。锁获取、控制连接、读写和总体操作都应有独立的可诊断期限或取消方式；锁超时不能被当成锁已释放或进程已退出。限时、错误归类与 CLI/桌面非阻塞交互由 #21 实现；**「系统 DNS 能否受总期限约束」今天已经能回答，而且是坏消息**：`daemon_health_ok:170` 的 `to_socket_addrs()` 排在 `connect_timeout(500 ms)` 之前且**没有超时**，`local_daemon_addr:21-28` 原样采用 `AGENT2SSH_DAEMON_ADDR`（允许写主机名），所以配主机名时 `daemon status` / `daemon stop` 的总时长**没有上界**——这是既有事实，不是待验证项（配置指南已记）。#21 必须先给出总期限口径，不要把分段超时相加当成已证明的上界。

### 3.2 客户端与 daemon 的两个方向

- **客户端识别服务端**：新客户端连接本机控制端点，在同一连接上取得平台能够提供的**服务端/对端**凭据，再进行认证和实例核对。凭据指向哪个进程、何时采样、监听句柄继承/代理会怎样，必须按平台测试。不能把 daemon 在服务端看到的客户端 PID 当成服务端 PID。Windows 命名管道客户端侧应研究 `GetNamedPipeServerProcessId`；`GetNamedPipeClientProcessId` 是反方向，不能替代它。
- **daemon 认证客户端**：daemon 单独检查本地连接的客户端凭据与管理员权限/令牌，并明确代理、不同用户及远程连接策略。这不等于客户端已经核实 daemon。
- **进程对象绑定**：客户端只有在取得的对象与已验证服务端之间存在可证明的关联时才能进入 `Bound`。用 PID 从对端凭据再调用 `pidfd_open` / `OpenProcess` 存在**取凭据后、打开对象前**的复用窗口；新对象仍然存活或记录中的 ID 相等均不能排除这一窗口。平台若不能闭合它，就保持 `Unbound`，不得把它提升为已绑定。
- **直接启动场景**：启动者可在创建时取得平台提供的子进程对象并保持其生命周期，再另行验证控制连接与该对象的关联。`std::process::Child` 在不同平台的等待/终止保证必须实证，不能仅因保存了 `Child` 就无条件声称跨平台具备原子绑定。启动者退出后，独立 CLI 需重新建立绑定，不能假设对象可从磁盘恢复。

本机控制通道是候选方案，不替换现有通用 HTTP API 或远程 daemon 能力。Unix socket 与 Windows 命名管道的具体端点、访问权限、远程访问禁用和平台实现由 #23–#25 验证；远程 daemon 不得被当成本机进程进行 PID 信号操作。Linux `SO_PEERCRED` 给出连接相关凭据，但凭据采样时机、监听 socket 传递和代理必须作为协议前提验证，而不是默认它总是实际处理请求的进程。Windows 管道可有多个实例，不能仅凭名称存在就断言全局单例或安全 ACL。

### 3.3 停止状态机

| 状态 | 允许的动作 | 进入下一状态的条件 |
|---|---|---|
| `Discovered` | 读取记录，报告线索；不发信号 | 建立候选连接 |
| `Connected` | 查询对端、认证授权、读取实例信息；未知或矛盾即拒绝 | 证明连接、记录和进程对象之间满足平台绑定规则 |
| `Bound` | 在同一连接上请求对**该实例**优雅停止；等待同一对象 | 请求已接受，或明确拒绝/失败 |
| `Draining` | 等待已绑定对象退出；仅按产品策略和平台能力允许基于同一对象强制终止 | 确认同一对象已退出；不可观察则 `Unknown` |
| `Exited` | 锁内重新读取并条件清理该实例记录 | 清理成功或记录已被替换，均如实报告 |
| `Unbound` / `Unknown` | 展示证据和恢复指导；不自动强杀、不把不可观察当退出 | 重新发现并验证，或人工带外处理 |

**遗留路径不得当作 `Exited`**：`daemon_pid_state`（`:343-367`）在 `/health` 无应答时用 `pid_definitely_missing`（unix 版 `:305-322`，只判 `kill(pid, 0) == ESRCH`，**不比较 start_time**）判 `Stale`，随后 `start_daemon_background:435-448` 与 `agent2ssh daemon rotate-token`（`bin/agent2ssh.rs:3123-3129`）**直接删记录**。这条路径不经过 `Discovered → Connected → Bound` 中的任何一步，也没有任何身份证明，只有「这个号当前无主」。语义上它足够安全（号空闲即可清），但它**不满足 `Exited` 的绑定条件**：新协议不得把 `Stale` 重命名为 `Exited`，也不得让它在 `Bound` 之外的任何状态触发强杀。

停止请求回执属于协议层：携带 `checked_pid` 可以表明**声称**已检查，但如果服务端本身未认证/绑定，回执可被伪造，不能单独让连接进入 `Bound`。`/shutdown` 的错误 PID 409、空 body 兼容、超时、连接关闭与已接受但未退出必须分别返回；请求失败不能静默重连到另一实例或回退到裸 PID 强杀。是否允许 `Unbound` 状态下仍通过旧 HTTP 发送优雅停止，是产品兼容性决策；即使允许，也必须明确未绑定及可能停错的风险，且**不能把收到 200 当成进程退出的证明**。

请求写出后连接中断时，客户端不知道服务端是否已经接受停止，结果应为 `Unknown`，而非“没有停止”或“已停止”。要重试，必须重新发现并验证实例，且不得把同一端点的新实例当成原请求的目标；协议如引入请求 ID/实例 ID，必须定义去重范围、重启后的失效和状态查询，并注意这些 ID 自身不构成身份凭据。未经实例绑定的响应、409 文案和空 body 都不能补足上述证明。

### 3.4 端点发布与残留

端点的访问控制、唯一配置标识、path 长度、符号链接、重命名、权限变更、不同用户及网络文件系统需逐平台设计。Unix socket 的 pathname 已存在、连接失败，**不能证明旧监听者已死**；无条件 `unlink` 活 socket 可让另一监听者在同一路径出现。启动、探测、清理和重绑定需在经验证的单例协调协议内完成；无法证明安全时拒绝启动并提示恢复。Windows 命名管道应显式定义 ACL、仅本机策略、首实例和并发实例规则；不能假设默认配置就满足这些条件。

## 4. 兼容、恢复和回滚

| 客户端 | daemon | 推荐初始行为与未决项 |
|---|---|---|
| 旧 | 旧 | 保持现状，既有误停风险不因新记录而消失。 |
| 旧 | 新 | 保留数字 PID 和现有 HTTP 合约；旧客户端走旧路径，不获得新绑定保证。是否限制旧 TCP 停止及空 body 是待决的破坏性兼容问题。 |
| 新 | 旧 | 检测不到新通道/回执时显示 `Unbound`；默认不自动强杀。是否仍允许未绑定 HTTP 优雅停止待决，不得无提示回退。 |
| 新 | 新 | 只有平台绑定证明成立才为 `Bound`；能力不足、格式未知、权限拒绝、实例变化均退回 `Unbound`。 |
| 新 | 新但记录/端点矛盾 | 保留 PID 和实例记录，停止自动动作，输出不含 token 的诊断。 |

升级时旧 daemon 正在运行：先诊断并遵守选定的旧版停止策略，不用新实例覆盖其记录。回滚时旧程序忽略新记录，保护会降低；需要说明旧程序清理时不遵守新记录约定，以及残留新 socket/JSON 的处理。不要自动 `adopt` 仅凭 health/token 看似匹配的服务。

异常恢复由 #21 落地：报告配置路径、PID 记录、进程观察、端点/认证结果和权限不足，不输出 token 或秘密。文件损坏或进程身份未知时保留证据；清理 PID 是与停止进程分离的操作。仅在可证明旧进程已退出且锁内重新核对记录未变时，才考虑显式清理；不能证明则不清理。旧版本和不可靠文件系统的竞争必须在界面中说明，不能用用户确认将不确定身份变成已验证身份。

## 5. 平台可行性与未解决问题

| 平台 | 可用于原型的能力 | 未验证/不得声称 |
|---|---|---|
| Linux | `pidfd_open`、`pidfd_send_signal`、Unix socket 对端凭据；研究可直接取得对端 pidfd 或可靠传递进程对象的可用机制。 | 从 peer PID 再打开 pidfd 的窗口仍在；不能仅靠进程仍存在、随机 ID 或同一启动 tick 证明连接与 pidfd 属于同一实例。不同内核、namespace、socket 继承及权限待 #23 实测。 |
| Windows | 创建进程时的句柄、`OpenProcess`、句柄等待及终止；命名管道服务端 PID 查询供客户端侧研究。 | `OpenProcess` 前 PID 复用、管道端点抢占/代理、ACL 和句柄关联尚未解决；拒绝访问不是进程退出。#24 须在 Windows 真机验证。 |
| macOS | **已实测**（见 [macos-daemon-identity.md](macos-daemon-identity.md)）：`LOCAL_PEERPID` / `LOCAL_PEERCRED` / `LOCAL_PEERTOKEN` 三者可取，`kqueue EVFILT_PROC` 可作退出通知，`proc_pidinfo(PROC_PIDTBSDINFO)` 的 `pbi_start_tvusec` 可把起始时间从秒提到微秒。 | `LOCAL_PEERPID` 的语义是**最近访问者**，fd 传递后即换人（实测），不能当「连接所有者」；`kqueue` 注册对其他 uid 的进程**也会成功**（实测 pid 1），因此不构成发信号权限的否证；没有 pidfd 等价物，`kill(pid, …)` 仍按数字寻址，二次观察只缩小窗口。无法达到绑定条件时不自动强杀。 |

**平台覆盖缺口（实测数字）**：`daemon_stop_e2e.rs` 有 51 个 `fn`、34 个 `#[test]`，其中 **8 个**带 `#[cfg(unix)]`，含两个核心竞态用例——`a_daemon_that_ignores_the_expected_pid_stops_and_says_so`（`:1434`，`#[cfg(unix)]` 在 `:1431`）与 `a_stop_that_reached_the_wrong_daemon_stops_nothing`（`:1702`，`#[cfg(unix)]` 在 `:1699`）。`#[cfg(unix)]` 在 macOS 上成立，所以这 8 条**在 macOS 上照常执行**：2026-11-12 实测 `cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1` → **34 passed / 0 failed / 40.75 s**，两条核心用例单独复跑也各 1 passed。因此**只有 Windows 上这两条路径零覆盖**；#24 若复用它们必须先摘掉 `#[cfg(unix)]` 并补平台夹具，不能把「同一份测试文件」当成「全平台矩阵」。**（2026-11-12 更正：本文早前写「Windows 与 macOS 上零覆盖」，macOS 这半是错的。）**

**macOS 的结构性后果**：按 §3.2 的绑定规则，macOS 上若不新增能取对端凭据的本机通道（D4），`kill(pid, …)` 永远只能停在 `Unbound`——即**macOS 没有自动强杀**。这是产品取舍，必须由 D3 明确，不能默认继承 Linux 的行为。

源码/平台资料：Linux [pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html)、[pidfd_send_signal](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html)、[Unix socket](https://man7.org/linux/man-pages/man7/unix.7.html)；Microsoft [OpenProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess)、[管道函数](https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-functions)、[命名管道安全边界](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipes)；Apple [kill](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kill.2.html)。资料只说明原语，不证明组合方案已安全。

## 6. 验证与交付门槛

1. 先写平台无关状态机及错误分类测试：未经绑定不能调用自动强杀；`Unknown` 不能清理；实例改变、拒绝/超时、旧版无回执均不误报成功。模拟测试须覆盖每个转换及不能转换的分支。
2. 用**真实 daemon 和独立临时配置**测试同 token A/B 的健康与停止之间切换、伪造 health/回执、后台进程独立重连、不同目录、端点被抢占、句柄传递、无权限、丢失/损坏记录及并发发布清理。记录操作实际指向哪个进程、两个进程是否存活、文件是否保留；不能只断言状态码。**既有用例已经给出一个必须被推翻的反例**：`a_daemon_that_ignores_the_expected_pid_stops_and_says_so`（`:1434`）用 `symlink("/bin/sleep")` 造了个名为 `agent2ssh-daemon` 的替身（`:1445-1446`），并接受「强杀打到这个替身」为诚实结果。它证明「名称白名单 + 秒级 start_time」不构成绑定；新协议应把该用例的验收改成「必须 `Unbound` 且不发信号」。
3. Linux/Windows 构造“取对端 PID → 原进程退出并回收 → PID 指向另一进程 → 打开对象”的受控时序；不能稳定复用时，用可控适配层及真实对象组合验证拒绝分支，并明确真实 PID 复用未覆盖。证明打开对象后的等待/信号使用同一对象。macOS 必须实测其候选机制，不能沿用 Linux 结果。**这一条在今天的实现上走不到**：客户端识别服务端只靠 `/health` 响应体里的 `pid`（`daemon_control.rs:231-253`），全仓无 `SO_PEERCRED` / `GetNamedPipeServerProcessId` 调用，控制面只有一条 TCP 监听（`agent2ssh-daemon.rs:5826`）。所以它是**给 #23/#24 新实现准备的验收**，不是对现状的补测；把它写成「待验证」会让人以为现有代码已接近该结构。
4. 新旧客户端 × 新旧 daemon、升级/回滚、旧程序不持锁、socket/pipe 残留、重复实例和平台能力不可用均须覆盖；另覆盖停止写出后丢失响应并在 A/B 切换后的重试、同 PID 新实例、长时间持锁及记录两步发布之间崩溃。测试破坏后要能清理子进程并隔离真实 `~/.agent2ssh`。
5. 按 #20、#21 和 #23–#25 分别提交平台测试报告：OS/内核、权限、编译/运行命令、真实或模拟、结果和盲区。更新 `docs/api.yaml`、配置指南、K14 与契约测试；只有所有适用平台达到评审通过的保证，才可声称对应范围完成。

## 7. 需要评审决定

六项决策，每项都能单独拍板；「代价」只计本仓内要动的东西，「失败模式」是拍错之后的可观察后果，「回滚」是不改代码的退路。**D4 是 #23 动工的前置条件**，其余可以先拍后做。

| # | 决策 | 选项 | 代价 | 失败模式 | 回滚 | 倾向 |
|---|---|---|---|---|---|---|
| D1 | 同用户主动攻击者是否纳入本版本 | (a) 明确排除；(b) 纳入并另立可信根 | (a) 0；(b) 需要新的信任根，0600 token 不够 | (a) 同用户可伪造 `/health` 与回执；(b) 范围失控，且没有可验证的交付边界 | 纯文档级 | (a)，但先把 D2 的「未绑定即警示」做出来，让代价可见 |
| D2 | 新客户端遇旧 daemon | (a) 允许未绑定 HTTP 优雅停止 + 显式警示；(b) 拒绝，要求带外恢复；另含「新 daemon 是否继续接受旧客户端的空 body」 | (a) 保留旧 daemon 可用；(b) 升级期所有旧 daemon 无法从 CLI 停 | (a) 停错不报警，警示是唯一护栏；(b) 用户被卡住时会自己 `kill -9`，后果更差 | 只在前端/CLI 文案与分支上，可直接退回 | (a)，**但必须先修 #20 的回执**：今天每次正常停止都写一条假 warn（§2 回执行），在这种状态下加警示等于加噪音 |
| D3 | 强杀的统一前置 | (a) 仅 `Bound` 可强杀；(b) 维持「名称白名单 + 秒级 start_time」；(c) 分平台：Linux 用 `Bound`，Windows/macOS 维持现状 | (a) macOS 与白名单外构建**失去自动强杀**；(c) 两套语义长期并存，文档要写两遍 | (a) 残留 daemon 只能手工处理；(b) symlink 替身可被强杀（§6.2 已有实证用例） | 单点判据，可局部撤回 | (c) 起步、(a) 为终局。选 (a) 时 macOS 的体验必须同时定下来，不能默认继承 |
| D4 | 控制面是否为本机 IPC 单开一条通道 | (a) 新增 Unix socket / 命名管道**专用于停止**，HTTP 只留能力查询与远程；(b) 放弃在传输层做绑定，绑定只对「自己启动并持有子进程对象」成立；(c) 维持 TCP，接受 §3.2 不可达 | (a) daemon 与全部客户端各加一条连接路径；(b) 独立 CLI 永远 `Unbound`；(c) 0 | (a) 两套路径的权限/ACL 要分别验证，且旧客户端不认它；(b) 停止只剩 HTTP，回到今天的误停面；(c) §3.2 永远不会被满足，本文档一半内容作废 | (a) 删监听、客户端回退 TCP，不影响 HTTP | (a)。**不拍这一项，Linux 的 pidfd 实现无处安放** |
| D5 | 若 D4 选 (a)：端点的路径/命名空间、ACL、代理与远程访问策略，配置目录相对路径与跨机器复制的语义 | (a) 配置目录内相对路径 + 目录 0700；(b) 全局路径 + 显式权限；(c) 跟随 `AGENT2SSH_DAEMON_ADDR` | (a) 复制配置目录即失效（可能正是想要的）；(b) 不随配置走，多配置并发要额外协调；(c) 与现有变量语义一致但无隔离 | (a) 跨机器复制的配置在目标机静默 `Unbound`；(b) 同用户其他配置可连；(c) 远程地址与「本机 IPC」语义冲突 | 端点计算集中在 `local_daemon_*` 一处，可换 | (a)，并在 `daemon status` 里明说「此配置目录的端点」 |
| D6 | 两文件记录的恢复规则、兼容期限与回滚约定 | (a) 旧格式长期兼容；(b) 带期限的兼容（下个大版本移除）；(c) 立即只认新格式 | (a) 0；(b) 需要一个版本门；(c) 旧配置直接不可读 | (a) 旧引用永久指向同名新密文（与 #26 的 A10 同族）；(c) 升级即断 | 都在读侧分支 | (b)。与 #26 的阶段 2 共用一份口径，不要各写一套 |

**评审状态**：这是可讨论的提案，不是已经落地的协议；上述决策、平台能力与关联 issue 的实际实现均未完成。本表之前的 §2/§3 已按 `4487f7f` 的源码复核过一遍，凡标「既有事实」的都可用行号复算；凡标「待验证」的都还没有本机证据。

# russh 迁移（P2）交接文档

状态：**待执行**（方案已按源码逐条对账复核，2026-10-22 修订） · 开发机：macOS（`MbpM1Max`）· 收尾机：Windows（`WinR9`）
关联：`docs/reports/WINDOWS-HANDOVER.md`（Windows 未验证面）、`src-tauri/src/ssh_key_format.rs`（迁移后删除）

---

## 0. 这份文档怎么用

- **在 Mac 上开工**：读 §4 建立目标架构 → §5 按阶段推进 → §6 逐文件改 → §9 跑前置与验证。
- **"迁移完成"的判据是 §4.5 那张接口清单**，不是"能编译"也不是"测试没红"。S0 结束时它应当全部有门面落点，S3 结束时全部有 russh 落点。
- **回 Windows 收尾**：直接跳 §10，那是必须在这台机器上做的事，Mac 上做不到。
- 一切"我猜的"都标了 **【待验证】**，落地前先验证，别当结论用。
- **2026-10-22 复核修订**：本版按源码逐条对账，改了四处事实错误（测试基线、agent 认证、`set_timeout` 语义、`cfg(windows)` 计数）、补了 §4.2 的三条契约与 §4.5 的接口清单、补了 `src/bin/agent2ssh-daemon.rs` 这一行、把 `demo/rssh-main` 降级为可选读物。原方案的行号引用与依赖事实经抽查全部准确，未改动。

---

## 1. 目标与验收

把 SSH 传输层从 `ssh2`（libssh2，C）换成 `russh`（Rust 实现；密码学原语由 `ring` 提供，见 §5 探针结果——**不是纯 Rust**，但 `ring` 只要求 C 编译器，不需要 Perl / nmake / NASM / cmake / Homebrew OpenSSL），使三平台都退化为：

```
git clone → npm install → cargo test        # 无 Perl / nmake / NASM / cmake / Homebrew OpenSSL
```

验收不是"能编译"，而是这四条同时成立：

1. `cargo test --lib --no-default-features` = **755 passed**，`TAURI_CONFIG='{"bundle":{"externalBin":[]}}' cargo test --lib` = **763 passed**（2026-10-22 实测，均 0 failed）。演进：735/743（原始方案，已过期）→ 741/749（S0 前）→ 747/755（S0 后，+6 条门面测试）→ **755/763**（移除 ssh-key 后，+8 条 `openssh_key` 测试）。**每阶段开工前用 §9 的命令重测一次并把结果写回本节**——数字会漂，写死的基线只能当参考，不能当门禁；用旧值当"不许下降"的地板会放过真实回归（735 那次就刚好差 6 条）。
2. `npm test` = 22 文件 / **155 tests passed**（实测不变）。
3. `scripts/e2e-container.sh` 对真实 OpenSSH 服务端跑通 exec / SFTP / resume，且在 Windows 上握手成功（这正是当前被迫引入 OpenSSL 的原因）。
4. §4.5 的接口清单逐项有实现——`Session` 22 个方法、`Channel` 14 个（含 `Read`/`Write` 两个 trait）、`Sftp` 9 个、`File` 4 个（含 `Seek`）、`FileStat` 4 个谓词 + 6 个字段。**缺一项就不算迁完**，这一条取代"只换类型"这种无法验收的表述。

---

## 2. 现状：为什么"拿到源码就能编译"现在是假的

| # | 缺口 | 证据 |
|---|---|---|
| 1 | `.cargo/config.toml` 只按 **cwd** 被发现，与 `--manifest-path` 无关 | **只在 Windows 上炸**：仓库根跑 `cargo test --manifest-path src-tauri/Cargo.toml ...` 找不到 `[env]`，退回 Git Bash 的精简 Perl。macOS 不踩（该 config 只有一条 Windows 专用 `[env]`，见 §9 末注）。CI 全部用 `working-directory: src-tauri`，所以 CI 掩盖了这个坑 |
| 2 | `OPENSSL_SRC_PERL` 需要**完整** Perl | Git for Windows 自带精简版缺 `Locale::Maketext::Simple`，且 `which -a perl` 首选就是它 |
| 3 | 环境变量一变就触发 OpenSSL 全量重建 | `openssl-sys` 声明 `rerun-if-env-changed=OPENSSL_SRC_PERL`；同一哈希目录 `543c2086` 先失败后成功＝两次 10 分钟重建 |
| 4 | macOS 侧依赖完全没被记录 | `libssh2-sys` 有 `[target."cfg(unix)".dependencies.openssl-sys]` → 干净 Mac 需 `brew install openssl@3 pkg-config`，README/AGENTS 一个字没提 |
| 5 | sidecar 是 gitignore 的构建产物 | `.gitignore:4` 忽略 `src-tauri/binaries/*`；第 5 行 `!src-tauri/binaries/.gitkeep` 只保住占位文件，不满足 `tauri_build::build()`（它要的是 `binaries/<name>-<triple>` 这个具体路径）→ 干净克隆跑不了默认 feature 的 `cargo test --lib` |

第 1/2/3/4 项**全部由 P2 自然消除**（不再有 OpenSSL，就没有 `[env]` 要发现、没有 Perl 要解析）。第 5 项与传输层无关，迁移后仍需修（见 §10）。

这就是"先做 P2、回头再看 P0/P1 是否存在"的依据：P1 的 Perl 自举方案直接作废，不要投入。

---

## 3. 为什么只能换掉 libssh2

`libssh2-sys 0.3.1` 的全部 feature：

```toml
openssl-on-win32 = ["openssl-sys"]
vendored-openssl = ["openssl-sys/vendored"]
zlib-ng-compat   = ["libz-sys/zlib-ng"]
```

没有 mbedTLS、没有纯 Rust 后端。**留在 libssh2 上，Windows 就永远背着一份从源码构建的 OpenSSL**，这不是配置问题。同理 macOS 的 `cfg(unix) → openssl-sys` 也是硬编码。

---

## 4. 目标架构：同步门面 + russh 内核

### 4.1 为什么是门面而不是整体 async

现有代码是 **thread-per-connection + 阻塞 `std::net::TcpStream` + libssh2**，会话调用点分布：

| 文件 | 行数 | 会话方法调用点数 |
|---|---|---|
| `src/embedded_ssh.rs` | 2254 | 41 |
| `src/core.rs` | 4759 | 19 |
| `src/forward.rs` | 1071 | 5 |
| `src/session.rs` | 351 | 1 |
| 其余（`resident_session.rs` 1049 / `prompt_waiter.rs` 581 / `sftp_transfer.rs` 599 / `ssh_algo.rs` 689 / `ssh_key_format.rs` 421） | — | 类型级引用，随门面走 |

russh 是 async。若把 9500 行一起改成 async，diff 无法 review，而且 Tauri command 层与 CLI 都是同步的，还得在边界上再套一层 —— 等于把 async 成本转嫁到调用方，却仍要写桥接代码。

**结论：只把 async 关在 `src/transport/` 内部。** 对外暴露与今天形状相同的同步 API，尤其是——

```rust
impl Read  for Channel { ... }
impl Write for Channel { ... }
```

**这个杠杆的边界要说清，否则会低估工作量。** `bridge_tcp_and_channel()`（`embedded_ssh.rs:1000`）确实只用 `read / write / write_all / flush / send_eof / eof / exit_status / wait_close / close` 九个，这九个靠 `Read`/`Write` + 五个同名方法就能原样兜住。但"66 个调用点几乎不用改"是**不成立的**，有两处出在 trait 之外：

1. **`channel.stderr()`**——`embedded_ssh.rs:1253` 与 `bin/agent2ssh-daemon.rs:4637` 的排空循环各调一次。它不在 `Read`/`Write` 里。russh 把 stdout 与 stderr 放在**同一条**消息流（`Data` / `ExtendedData`），因此借出式 `stderr()` 游标无法存在；门面必须在 reader task 里双缓冲解复用（见 §4.2 第 3 条）。
2. **`session.host_key()`**——4 个调用点，全部在握手**之后**读。russh 只在 `check_server_key` 回调里给 key，校验时点因此改变（见 §4.2 第 6 条）。

所以准确的说法是：`Read`/`Write` 兜住了**转发桥**这一条最热的路径，其余按 §4.5 的清单逐项对表。全量清单见 §4.5。

### 4.2 必须复刻的六条契约（否则现有代码静默失效）

这六条是门面的规格书。**没有一条能从 russh 的默认行为里"自然获得"**，全部要显式实现并各配一条单测。

1. **阻塞标志与超时是正交的两个旋钮，不是一回事。** 今天 `set_blocking()` 有 9 个调用点（`embedded_ssh.rs` 5、`forward.rs` 2、`bin/agent2ssh-daemon.rs` 2），`set_timeout()` 有 4 个，两者独立使用。`WouldBlock` 是 `set_blocking(false)` 的产物，**不是超时的产物**；把二者合成"`mpsc::recv_timeout(session_timeout)` → WouldBlock"会让 `embedded_ssh.rs:1202` 那个 `set_blocking(true)` 之后的 `wait_close()`（`:1204`）误返 WouldBlock，exec 收尾直接坏掉。门面必须同时暴露 `set_blocking(bool)` 与 `set_timeout(Duration)`，并在 `read`/`write`/`wait_close` 里分别按"是否阻塞"选不同的等待策略。
2. **`WouldBlock` 语义**。`forward.rs:715` 用 `ErrorCode::Session(-37)` 判"暂时没数据"；`bridge_tcp_and_channel` 把 TCP 设成非阻塞，靠 `channel.read()` 返回 `WouldBlock` 驱动双工轮转。非阻塞模式下**无数据即刻**返回 `io::ErrorKind::WouldBlock`（不是等一个超时再返回），这是空转率问题。
3. **`stderr` 必须与 stdout 分开可读，且两者互不阻塞。** `drain_channel_output`（`embedded_ssh.rs`）与 `bin/agent2ssh-daemon.rs:4637` 是"轮流读两个流、任一有进展就继续"的写法。russh 只有一条消息流，所以 reader task 要把 `ChannelMsg::Data` 与 `ChannelMsg::ExtendedData` 分发到两个 `VecDeque`，门面的 `read()` 与 `stderr().read()` 各自从自己的队列取；两者都空且未知 EOF 时才返回 WouldBlock。
4. **`eof()` 是"EOF 已到 **且** 两个缓冲都空"**，不是"收到 `ChannelMsg::Eof`"。排空循环的退出条件明写这个前提（源码注释：`eof()` stays false while stderr data is still buffered, so this is the normal exit），配 `DRAIN_IDLE_PASSES_AFTER_EOF` 吸收尾包。若门面在收到 Eof 消息时就置真，stderr 会被截断。
5. **`exit_status` 在 `wait_close` 之后才可靠**。`ChannelMsg::ExitStatus` 可能在 `Close` 之后才到，门面要在 `wait_close()` 里等齐再返回。
6. **`host_key` 的时点变了，这是控制流改变而非类型替换。** 本仓是 TOFU 三段式：`probe_host_fingerprint`（`embedded_ssh.rs:261`）连上但不认证、读 `host_key()` 拿指纹 → 展示给用户 → `trust_host_fingerprint_core`（`:307`，内部在 `:316` 再探一次）落盘 → 正式连接由 `trust_or_verify_host_fingerprint`（`:214`）比对。russh 只在 `check_server_key` 回调里给 key，所以：**探针必须故意返回 `Ok(true)` 才能读到未知主机密钥**，读完后不认证即断开；正式连接则必须在回调里完成比对并返回 `Ok(false)` 中止。门面的 `Session` 要携带回调期间捕获的 key，`host_key()` 只读这份快照。

> 关于 `set_timeout(0)`：**本仓不存在"永不超时"这种模式**。`session_timeout_ms()`（`embedded_ssh.rs:1288`）把 0 夹到 1 ms，注释写明 "rather than disabling the bound"。门面照此实现，不要引入 russh 侧的"永不超时"分支——那会把现在被刻意消掉的一种挂死重新放回来。

> 关于超时值：`set_timeout` 不是会话级常量，而是**可重设的 per-call 预算**。三个真实用途：`:1333` 命令自身预算（由 `timeout_secs` 派生）、`:1203` `TEARDOWN_TIMEOUT_MS`（注释明确"不是命令的预算"）、`forward.rs:222/263` 的 500 ms（`disconnect()` 前的瞬时预算）。门面按"可随时重设的字段"实现，不要做成初始化时捕获一次。

### 4.3 runtime 边界（最容易踩的坑）

门面内部**自带一个专用 tokio runtime**（独立线程池），不要复用 daemon 的 runtime。

- 同步调用方（CLI、Tauri command、`forward.rs` 的线程）→ `handle.block_on(...)`，安全。
- **daemon 的 async handler 里禁止直接调门面**。`block_on` 在 runtime 线程上会 panic；必须 `tokio::task::spawn_blocking` 包一层。
  今天这些调用点本来就是阻塞的（已经在堵 reactor），迁移不要顺手改语义，但要在 `src/transport/mod.rs` 顶部用注释把这个规则钉住，并逐个标注已有调用点。

### 4.4 门面模块骨架

```
src/transport/
├── mod.rs          # Session / Channel / Sftp / Error / FileStat 的对外形状 + runtime 规则注释
├── legacy_ssh2.rs  # S0 阶段：现有 ssh2 实现（feature = "transport-ssh2"）
├── russh_impl/     # S1 阶段：russh 实现（feature = "transport-russh"）
│   ├── mod.rs
│   ├── session.rs  # 连接、认证、Config.preferred
│   ├── channel.rs  # Read/Write 门面 + reader/writer task + eof/exit_status
│   ├── sftp.rs     # russh-sftp 包装 + FileStat 映射
│   └── error.rs    # russh 错误 → 门面 ErrorKind
└── types.rs        # FileStat、ErrorKind、HostKeyKind（自有类型，不泄漏后端）
```

错误分类用自有枚举替代 libssh2 数字码：

| 现在 | 门面 |
|---|---|
| `ErrorCode::Session(-5)`（KEX 失败，`embedded_ssh.rs:1806`） | `ErrorKind::KeyExchange` |
| `Session(-18) \| Session(-19)`（公钥被拒，`:1827`） | `ErrorKind::PublicKeyRefused` |
| `Session(-37)`（would block，`forward.rs:716`） | `ErrorKind::WouldBlock` |
| `Session(-1)`（opaque，`ssh_key_format.rs:158`） | `ErrorKind::Opaque` |

### 4.5 完整接口清单——这就是"整体迁移"的定义

清单的生成方式可复核：取 `ssh2 0.9.5` 的 `src/session.rs` / `src/channel.rs` / `src/sftp.rs` 全部 `pub fn`，与 `src-tauri/src/` 内实际调用求交。**S0 的 DoD 是这张表逐项勾满**；勾不满就是缝没立完。

**Session：22 个**

| # | 方法 | 调用点 | russh 落点 | 备注 |
|---|---|---|---|---|
| 1 | `sftp()` | 11 | `channel_open_session()` + `SftpSession::new` | 见 §6 `core.rs` |
| 2 | `set_blocking()` | 9 | 门面字段 | §4.2-1，**不可与超时合并** |
| 3 | `authenticated()` | 8 | 门面状态 | |
| 4 | `userauth_pubkey_file()` | 4 | `authenticate_publickey_with` | RSA 走 `PrivateKeyWithHashAlg` + `best_supported_rsa_hash()` |
| 5 | `set_timeout()` | 4 | 门面字段（可重设） | §4.2 末注 |
| 6 | `host_key()` | 4 | `check_server_key` 回调快照 | §4.2-6，**控制流改变** |
| 7 | `disconnect()` | 4 | `disconnect(reason, lang)` | `forward.rs:222/263` 的 500 ms 预算在此生效 |
| 8 | `channel_session()` | 3 | `channel_open_session()` | |
| 9 | `supported_algs()` | 2 | **无等价物** | 改静态表 + `Name::try_from` 的 fail-closed 校验 |
| 10 | `set_tcp_stream()` | 2 | `connect_stream(stream)` | 跳板/proxy 路径（`embedded_ssh.rs:721`） |
| 11 | `method_pref()` | 2 | `Config.preferred.{kex,key,cipher,mac,compression}` | 见 §6 `ssh_algo.rs` |
| 12 | `handshake()` | 2 | `connect` / `connect_stream` 内部 | 主机密钥校验的挂点在回调里 |
| 13 | `channel_direct_tcpip()` | 2 | `channel_open_direct_tcpip()` | 转发（`forward.rs:614`）+ 跳板 |
| 14 | `userauth_password()` | 1 | `authenticate_password` | |
| 15 | `userauth_keyboard_interactive()` | 1 | `KeyboardInteractiveAuthResponse` 状态机 | 见 §6 `prompt_waiter.rs` |
| 16 | `set_keepalive()` | 1 | `Config.keepalive_interval` | `connection.rs:102` |
| 17 | `keepalive_send()` | 1 | `send_keepalive()` | `connection.rs:264` 的手动心跳 |
| 18 | `host_key_hash()` | 1 | 从快照自算 SHA256 | `embedded_ssh.rs:165` |
| 19 | `channel_forward_listen()` | 1 | `channel_open_forward_listen()` | **accept 模型不同**：russh 返回 `(Channel, ChannelOpenFailure)` 流，不是 libssh2 的 listener 轮询（`forward.rs:638`） |
| 20 | `banner()` | 1 | 【待验证】 | `core.rs:1881` 把它传给 SFTP 传输，需确认 russh 是否暴露 |
| 21 | `auth_methods()` | 1 | **无等价物** | 现用于短路"服务端没广告 password 就不试"（`embedded_ssh.rs:1580`），需改写 |
| 22 | `agent()` | 1 | `russh::keys::agent::client::AgentClient` | `embedded_ssh.rs:1749`，**含 Windows Pageant**，见 §10 |

**Channel：14 个**

| 类别 | 方法 |
|---|---|
| trait | `read` / `write` / `write_all` / `flush` |
| 流 | `stderr()` ← §4.2-3 |
| 状态 | `eof()` ← §4.2-4、`exit_status()`、`wait_close()`、`close()`、`send_eof()` |
| 开启 | `exec()`、`shell()`、`request_pty()`、`request_pty_size()` |

**Sftp：9 个** —— `open` / `create` / `open_mode` / `stat` / `readdir` / `mkdir` / `rmdir` / `unlink` / `rename`

> 生成清单时 `shutdown` 曾被算进 Sftp，那是 `TcpStream::shutdown` 的同名噪声，实际没有调用点；`lstat` / `setstat` / `symlink` / `readlink` / `realpath` 也都未使用。S0 落地时按真实调用点收敛到 9 个。

**File：4 个** —— `read` / `write` / `seek`（三个 trait）/ `stat`。`core.rs` 的续传路径对远端文件做了 `seek`，所以门面的 `File` 必须实现 `Seek`，这一点原文整类漏掉了。

**FileStat：4 谓词 + 6 字段** —— `is_file` / `is_dir` / `is_symlink` / `file_type`；`perm` / `size` / `mtime` / **`atime`** / `uid` / `gid`（`core.rs:2204` 有 `stat.atime` 的读取，字段名与语义逐字保持；`core.rs:2118` 起的 5 个格式化函数与它们的测试是现成的对拍集）

**三个"删得掉但要补位"的点**（放这里，比放 §7 更不容易丢）：

1. `supported_algs` 没有 russh 等价物，而它今天**同时承担两件事**：`narrow()` 用它过滤用户配置（`ssh_algo.rs:191`），以及充当"本构建能不能读 OpenSSH 私钥容器"的**能力信号**——`ssh_key_format::reads_openssh_private_keys()`（`:134`）读的就是它的 HostKey 列表（见 `ssh_algo.rs:129-137` 注释）。russh 恒支持 ed25519/ECDSA，后者应直接返回 `true`。
2. `reads_openssh_private_keys()` 有 3 个真实调用点：`keys.rs:821`、`embedded_ssh.rs:1664`、`bin/agent2ssh.rs:3399`。**最后一个是 CLI 的 JSON 字段名**——删函数可以，删字段是 CLI 契约变更，要么保留字段恒 `true`，要么按弃用规则走。
3. `auth_methods` 无等价物。`embedded_ssh.rs:1545/1580/1632` 用它决定要不要尝试密码认证、并拼错误文案。删掉后行为由"不尝试"变成"尝试并失败"，文案要重写。

---

## 5. 分阶段执行

每阶段结束都必须 **两条 cargo 测试命令全绿 + `npm test` 全绿**，否则不进下一阶段。

### S0 — 立缝（**不动任何依赖**）· ✅ 已完成 2026-10-22

1. 建 `src/transport/`，`legacy_ssh2.rs` 里包一层今天的 `ssh2::*`，对外只暴露自有类型。
2. 逐个文件把 `use ssh2::...` 换成 `use crate::transport::...`。
3. 自有 `Channel` 用 `impl Read/Write` 包住 `ssh2::Channel`，`eof/exit_status/wait_close/send_eof/close/request_pty/shell/exec` 一一转发。

**落地结果**：`src/transport/{mod,types,legacy_ssh2}.rs` 共 989 行；23 个 `Session` 方法、14 个 `Channel` 成员、9 个 `Sftp` 方法、带 `Seek` 的 `File`、`FileStat` 六个字段全部到位。libssh2 的 5 个数字码收敛成 `legacy_ssh2::From<ssh2::Error>` 的一处 match。`Cargo.toml`/`Cargo.lock` 零改动。**注意 `Channel::stderr()` 在这里就定成了 `&mut self`**，S1 不必再改两个排空循环。

**DoD**：`grep -rn "ssh2::" src/ | grep -v "src/transport/"` 输出为空——即**模块之外没有任何代码命名后端**（不能只排 `legacy_ssh2.rs`：`mod.rs` 的 `pub use legacy_ssh2::…` 也会命中，而 `types.rs` 的文档注释是刻意写明"镜像 ssh2 的哪个类型"的，属于要保留的记录）；741/749/155 全绿（现场重测值）；`Cargo.toml` 未新增任何依赖；**§4.5 的 22+14+9+4 个方法逐个有门面落点**。
**价值**：这一步在 Mac 和 Windows 上都成立，等于把 9500 行的风险提前清掉；之后换后端只改一个目录。

### S1 — 接 russh（双后端并存）· 🟢 主干已实现并通过真实 sshd，剩余五个缺口

> 依赖层已经在树上（`transport-russh` feature + 五条钉版），`cargo check --features transport-russh` 通过。**剩下的全部工作是 `russh_impl/` 本身**，见本节末「探针结果」。

1. 依赖已就位，不必再抄任何 pin：

```toml
russh = { version = "0.60.3", default-features = false, features = ["rsa", "ring"], optional = true }
russh-sftp = { version = "2", default-features = false, optional = true }
ed25519-dalek = "=3.0.0-pre.6"
pkcs1 = { version = "=0.8.0-rc.4", default-features = false, optional = true }
pkcs5 = { version = "=0.8.0-rc.13", default-features = false, optional = true }
```

> 三个必须记住的坑，理由都在本节末：**必须是 0.60.3 而不是 0.60.1**（0.60.1 的 caret 依赖解不出可行集）；**必须 `default-features = false`**（默认带 `aws-lc-rs`，要 cmake/NASM）；**`ring` 是唯一可选的密码后端**（关掉默认后无后端会 `compile_error!`）。`demo/rssh-main` 仍只作可选读物（§8）。

2. 实现 `russh_impl/`，feature 名 `transport-russh`，默认仍是 ssh2。
3. **认证有四种，不是三种**（已核）：
   - `userauth_pubkey_file`（4 处：`embedded_ssh.rs:1683/1721/1778/1848`）
   - `userauth_password`（`:1583`）
   - `userauth_keyboard_interactive`（`:1601`，回调改状态机）
   - **`session.agent()`（`:1749-1760`）**——`connect()` → `list_identities()` → 逐个 `userauth`，成功时返回认证方式字符串 `"agent"`，源码注释明写"includes Windows Pageant via ssh2's agent connector"。**必须迁 `russh::keys::agent`**，这不是可选项，且它的 Windows 分支（Pageant / 命名管道）是本次唯一横跨传输层与平台层的位置，见 §10。

   另有两个能力探测要一起处理，否则认证路径静默降级：`auth_methods()` 无 russh 等价物（§4.5 补位点 3）、`supported_algs()` 需换静态表（§4.5 补位点 1）。
4. 每条测试在两种 feature 下各跑一遍。

**DoD**：`cargo test --no-default-features --features transport-russh --lib` 与 ssh2 版结果一致；`scripts/e2e-container.sh` 在两套后端下都通过。

#### 探针结果（2026-10-22，实测）· ✅ 已解除，走的是 A

依赖层已经能装了，但结论与原文差得很远，逐条记下来，免得下次重走。

1. **必须用 russh 0.60.3，不能是 0.60.1。** 0.60.1 对 `ed25519-dalek` 用的是 caret（`^3.0.0-pre.6`），cargo 会解析到某个 pre-release，而那个 pre-release 又钉死 `curve25519-dalek =5.0.0-pre.6`，与 russh 自己要的 `^5.0.0-pre.6`（解析到正式版 5.0.0）冲突。0.60.3 把整套 RustCrypto 都钉死了（`elliptic-curve 0.14.0-rc.28` / `primefield 0.14.0-rc.7` / `ecdsa 0.17.0-rc.16` …），**这才是那三行 pin 的真实出处**——它们描述的是 0.60.3 的锁定集，不是应用层要额外加的东西。

2. **`default-features = false` 是必须的，`ring` 是唯一可选项。** 默认 feature 是 `["flate2", "aws-lc-rs", "rsa"]`，其中 `aws-lc-rs` 要 cmake + NASM——正是要摆脱的东西。关掉默认后 russh 会 `compile_error!`，只能选 `ring`；`ring 0.17.14` 携带 `*-nasm.o` 预编译对象、build.rs 不调 nasm，**Windows 仍只需 MSVC**。所以 §1 的"纯 Rust"改成"Rust 实现 + ring"，但"Windows 只需 MSVC"的目标仍成立。

3. **真正的阻塞是 `ssh-key`，已按 A 方案移除。** `ssh-key 0.7.0-rc.10` 与 russh 钉的 RustCrypto 预发布集**没有交集**（`curve25519-dalek` / `ed25519-dalek` / `pkcs8` 三处都撞），不是加 pin 能解决的。改用 `src/openssh_key.rs`（150 行）自己写 OpenSSH 私钥容器，公开钥派生仍交给 `ed25519-dalek`。

4. **现在实际生效的依赖面**（写进 `Cargo.toml`，都带注释）：

   ```toml
   russh = { version = "0.60.3", default-features = false, features = ["rsa", "ring"], optional = true }
   russh-sftp = { version = "2", default-features = false, optional = true }
   ed25519-dalek = "=3.0.0-pre.6"   # 跟着 russh 的钉版，否则 curve25519-dalek 无解
   pkcs1 = { version = "=0.8.0-rc.4", default-features = false, optional = true }
   pkcs5 = { version = "=0.8.0-rc.13", default-features = false, optional = true }
   ```

   `pkcs1` / `pkcs5` 是**为了让 russh 自己编译过**而钉的：它们转正后改了 API（`pbkdf2_sha256_aes256cbc` 改名、`RsaPrivateKey<B>` 重新泛型化），不钉就是 `russh` 编译失败。

5. **`cargo tree -d` 结果**：`ed25519` / `curve25519-dalek` / `ecdsa` / `elliptic-curve` / `pkcs5` 全部单副本；只有 `pkcs1` 有 0.7.5（`rsa 0.9`，本仓用它写 PKCS#1）和 0.8.0-rc.4（russh）两份，属于可接受的并行。

**实现进度（2026-10-22）**：`src/transport/russh_impl/` 已落地，Session 与 Channel 的六条契约全部实现。

- **真实 sshd 验证通过**：`exec_fixture` 四条全绿（含 `drains_stdout_and_stderr_off_one_channel_without_deadlocking` 与超时截止那条）。这两条正是双缓冲解复用和"两旋钮"写错时最先倒的用例。
- **门面签名调整**：`host_key` / `host_key_hash` 改为返回 owned 字节——libssh2 能从会话里借出切片，russh 只能在握手回调里拿到 key，门面统一持有副本。
- **基线**：`--features transport-russh` = 752（755 − 3，少掉的正好是 `legacy_ssh2` 自己的映射测试，该后端在 feature 下不编译）；默认后端仍 755。

**剩余五个缺口**（都在 `russh_impl` 顶部列出，各自返回明确的 unsupported 错误，不假装可用）：

| 缺口 | 影响 | 说明 |
|---|---|---|
| `Sftp` | `core.rs` 的 SFTP 路径全部失败 | 需要接 `russh-sftp`，并把 `File` 的 Read/Write/Seek 包在 `block_on` 上 |
| `channel_forward_listen` | 远程转发不可用 | russh 把转发的通道**推进 handler**，不是发 listener，是模型差异不是改名 |
| `agent` | agent / Pageant 认证不可用 | Windows 分支无法在本机验证，盲写比明示更糟 |
| `auth_methods` | 「服务端没广告 password 就不试」的短路消失 | russh 不暴露该列表 |
| `banner` | `core.rs:1881` 拿不到登录横幅 | 需要在 handler 里捕获 |

`set_keepalive` 另有半缺口：握手后设置不再生效（russh 在 connect 时固定 interval），`keepalive_send()` 仍可用。

### S2 — 切默认

把默认后端改成 `transport-russh`，ssh2 保留一个 feature 便于回滚。至少经过一次 release 观察再删。

### S3 — 删

见 §7。

---

## 6. 文件级改动地图

| 文件 | 行数 | 现状 | 改法 | 风险 |
|---|---|---|---|---|
| `src/transport/*` | 新增 | — | 见 §4.4 | §4.2 六条契约逐条实现；**两旋钮（阻塞/超时）+ stderr 双缓冲**是主要工作量 |
| `src/embedded_ssh.rs` | 2254 | 认证（4 路，含 agent）、host key 校验、exec、PTY、`bridge_tcp_and_channel` | 换类型；`host_key_algorithm()`（`:152`）的 `HostKeyType` → 自有 `HostKeyKind`，**字符串保持原样**（写进 `known_hosts.json`）。**但 `host_key()` 的 4 个调用点要重排**：`probe_host_fingerprint`（`:261`）与 `trust_host_fingerprint_core`（`:307`）是"连上不认证只看密钥"的探针，russh 下探针必须在 `check_server_key` 里返回 `Ok(true)` 才能拿到未知密钥；正式连接的比对要在回调里返回 `Ok(false)` 中止。见 §4.2-6 | 高：认证状态机 + host key 控制流 + 41 个调用点 |
| `src/core.rs` | 4759 | SFTP 批量操作、`FileStat` 格式化（`:2118`–`:2236`）、`create_sftp_dir_all`、续传路径的远端 `seek` | `ssh2::FileStat` → `transport::FileStat`（字段逐字对齐，**含 `atime`**）；`OpenFlags/OpenType` → 自有枚举/位集；`File` 需带 `Seek` | 中：5 个格式化函数 + 1 处测试构造 |
| `src/forward.rs` | 1071 | `Arc<Mutex<Option<Session>>>`、双向桥、`-37` 分类 | Session 换成门面（内部是 `Handle` + tokio Mutex）；`ssh_error_is_would_block` → `ErrorKind::WouldBlock` | 高：转发是阻塞线程 + 非阻塞语义混合 |
| `src/ssh_algo.rs` | 689 | `Session::method_pref()` 字符串偏好 + `supported_algs()` 能力探测 | `apply_algo_prefs()` 改成产出 `client::Config.preferred.{kex,key,cipher,mac,compression}`（`Cow<Vec<Name>>`）；**配置 JSON 里的算法名不许变**。两处 `supported_algs()`（`:163` 缓存探测、`:228` fail-closed 校验）都要换：russh 无运行期能力查询，改为静态白名单 + `Name::try_from` 的 `Err` 作为"拼错算法名"的等价失败（现文案会列出 supported 列表，需换成静态表内容） | 中：`kex-strict-*` 这类协议标记要过滤（参考 `algorithms.rs:141`）；**`narrow()` 的过滤语义不能丢** |
| `src/ssh_key_format.rs` | 421 | 整套 libssh2 密钥格式探测（WinCNG 只认两种 PEM 头）；`reads_openssh_private_keys()`（`:134`）读 `ssh_algo::supported_algorithms(HostKey)` 当能力信号 | **整文件删除**；`inspect_key_file` 若 UI 还要，改用 `russh::keys::decode_secret_key` 试解析。**删之前先补位**：`reads_openssh_private_keys()` 有 3 个调用点（`keys.rs:821`、`embedded_ssh.rs:1664`、`bin/agent2ssh.rs:3399`），最后一个是 **CLI 的 JSON 字段名**，russh 恒支持 ed25519/ECDSA，保留字段恒 `true` | 低（删代码），但漏一个调用点就是编译错、删字段则是 CLI 契约变更 |
| `src/session.rs` | 351 | PTY 会话注册表 | 跟随类型改名 | 低 |
| `src/resident_session.rs` | 1049 | 常驻会话 | 跟随 | 低 |
| `src/prompt_waiter.rs` | 581 | keyboard-interactive 交互 | libssh2 回调 → russh 的 `KeyboardInteractiveAuthResponse` 状态机 | 中：回调变状态机，时序不同 |
| `src/sftp_transfer.rs` | 599 | 传输调度 | 跟随 | 低 |
| `src/connection.rs` | 400 | 连接与 deadline、`set_keepalive(true, SSH_KEEPALIVE_SECS)`（`:102`）、手动 `keepalive_send()`（`:264`） | 换门面；deadline 用 tokio timeout；keepalive 拆成 `Config.keepalive_interval`（自动）与 `send_keepalive()`（手动）两处 | 低 |
| **`src/bin/agent2ssh-daemon.rs`** | **5928** | **原文漏了这一行。** 自带一条 exec 流式排空循环：`:4615/:4667` 切 `set_blocking`、`:4637` 读 `channel.stderr()`、按 `WouldBlock` 分派 | 跟随门面类型；**这是 §4.2-3 与 §4.2-1 的第二个消费者**，双缓冲与阻塞标志改错会在这里表现为事件流卡住或丢 stderr | 中：文件大、测试覆盖靠 `daemon_local_exec_e2e` |
| `tests/exec_fixture.rs`、`tests/local_exec.rs` | — | 需真实 sshd（`AGENT2SSH_TEST_SSH_PORT` / `AGENT2SSH_TEST_SSH_KEY`） | 不改断言，靠环境跑 | — |

---

## 7. 迁移完成后可以删掉的东西

**删除前必须先补位的三处**（否则不是删代码，是删能力）：

- `ssh_algo::supported_algorithms()` 与它喂出来的 `ssh_key_format::reads_openssh_private_keys()`：换成"恒 `true`"的常量实现，3 个调用点见 §4.5 补位点 1、2。**`bin/agent2ssh.rs:3399` 那个 JSON 字段名要保留**。
- `ssh_algo::apply_method()` 里那圈 `supported_algs()` fail-closed 校验：改成静态白名单，保留"拼错的算法名必须报错、不许静默通过"的语义。
- `embedded_ssh` 里 `auth_methods()` 驱动的"没广告 password 就不尝试"短路：改成尝试后失败，并重写错误文案（§4.5 补位点 3）。

之后可以删：

- `src-tauri/src/ssh_key_format.rs`（421 行）
- `src-tauri/Cargo.toml`：`ssh2`、`[target.'cfg(windows)'.dependencies]` 里那条 `ssh2 = { features = ["openssl-on-win32","vendored-openssl"] }` 及其 40 余行注释（第 114–162 行）
- `src-tauri/.cargo/config.toml`（`OPENSSL_SRC_PERL` 的 `[env]`）
- `scripts/win-perl.cmd`
- `.github/workflows/ci.yml`：两处 `Add build tools for the bundled OpenSSL (Windows)` 步骤（`choco install nasm` + `win-perl.cmd` 自检，第 240–250、415–425 行，已核），以及第 221–239、396–414 行的解释性注释
- `AGENTS.md` 中关于 vendored OpenSSL / Perl / NASM 的全部段落，改为一条"Windows 只需 MSVC 工具链"

净减：约 500 行代码 + 一次 10–15 分钟的源码构建 + 一个未记录的平台依赖。**但净增的是门面本身**（`src/transport/` 预计 1200–1800 行，双缓冲解复用与"阻塞/超时两旋钮"占大头）。"净减 500 行"不是整次迁移的收益口径，别拿它当立项理由。

---

## 8. 参考实现：`demo/rssh-main`（**本机不存在，属可选读物**）

**先纠正一个前提**：`demo/` 在 `.gitignore:13`，`git ls-files` 里没有任何条目，本机也没有这个目录。所以：

- 不要在 §5 的依赖 pin 上依赖它——四行 pin 已内联到 §5 S1 第 1 步，那是权威值。
- 下面这张表是**读代码时的索引**，不是开工前置。拿不到 `demo/rssh-main` 不影响 S0，也不阻塞 S1 的骨架。
- 想补齐的话从 rssh 上游取对应版本；`docs/rssh-design-analysis.md`（已跟踪，37 KB）**替代不了**它——那份是能力 gap 分析，没有 russh 版本号、没有 `Config.preferred` 的用法记录。

不要加 path 依赖，只当**读的样板**。对应关系：

| 本项目 | rssh 参考文件 | 说明 |
|---|---|---|
| `src/embedded_ssh.rs`（会话/通道） | `src-tauri/src/ssh/client.rs`（35K） | `client::Handle<SshHandler>`、`check_server_key` 钩子（`:187`）、转发通道路由（`:426`） |
| `src/ssh_algo.rs` | `src-tauri/src/ssh/algorithms.rs` | `config.preferred.kex/key/cipher/mac/compression`（`:17`–`:21`） |
| `src/prompt_waiter.rs` | `src-tauri/src/ssh/auth.rs`（21K） | `KeyboardInteractiveAuthResponse` 状态机 |
| `src/forward.rs` | `src-tauri/src/ssh/forward.rs`（45K） | 本地/远程转发 + 动态通道注册 |
| `src/core.rs`（SFTP 部分） | `src-tauri/src/ssh/sftp.rs`（36K） | `SftpSession::new(channel.into_stream())`（`:151`） |
| `src/known_hosts.json` 逻辑 | `src-tauri/src/ssh/known_hosts.rs` | 他们直接读写系统 `~/.ssh/known_hosts`，我们自建 → **只借 API，不借策略** |
| 跳板机 | `src-tauri/src/ssh/bastion.rs` | 我们有 proxy/jump 逻辑 |

注意差异：rssh 全程 async、用 `Arc<tokio::sync::Mutex<Handle>>`；我们用同步门面，**不要照搬它的锁结构**。

---

## 9. macOS 开工前置

```bash
# 1. 迁移期间仍要链 libssh2，这是临时前置（P2 完成后消失）
brew install openssl@3 pkg-config

# 2. 前端产物（tauri_build 要求 frontendDist 存在）
npm install && npm run build

# 3. 默认 feature 的 lib 测试需要 sidecar；Mac 上没有 aarch64-apple-darwin 的那三个
export TAURI_CONFIG='{"bundle":{"externalBin":[]}}'

# 4. 基线（先确认起点干净，再动手；数字以本次实测为准，写回 §1）
npm test
cargo test --lib --no-default-features   # 当前 = 755（S0 后 747，S0 前 741，原始方案 735）
TAURI_CONFIG='{"bundle":{"externalBin":[]}}' cargo test --lib   # 当前 = 763（S0 后 755，S0 前 749）
```

> 第 3 步的 `TAURI_CONFIG` 与第 4 步的第二条要一起用，否则 `build.rs` 会去找不存在的 sidecar（§2 第 5 项）。**基线必须现场重测再写进 §1**：验收门禁认的是当场数字，不是这份文档里的历史值。

真实握手验证需要一个容器运行时（`agent2ssh containers runtime` 会自动选 Docker Desktop 或 Apple `container`）：

```bash
docker run -d --name a2s-sshd -p 2222:22 a2s-sshd:local   # 镜像见 scripts/sshd-fixture
AGENT2SSH_TEST_SSH_PORT=2222 cargo test --manifest-path src-tauri/Cargo.toml \
  --no-default-features --test exec_fixture -- --test-threads=1
```

> Mac 上 `--manifest-path` 形式不踩 §2 的坑（那个坑只在 Windows 需要 `[env]` 时存在），但**建议统一用 `cd src-tauri && cargo ...`**，与 CI 一致。

---

## 10. 回到 Windows 必须做的事

Mac 上**无法**完成的，逐条验收：

1. **`cfg(windows)` 半边编译**。Mac 上没有 `x86_64-pc-windows-msvc` 链接器，交叉 `cargo check` 不成立。必须在 WinR9 或 CI 的 `windows-latest` 腿跑：
   ```bash
   cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
   cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
   cargo test --manifest-path src-tauri/Cargo.toml --lib
   ```
   `#[cfg(windows)]` **`src/` 内实为 33 处**（原文写 44，不可复现）：`local_exec.rs` 10 / `tauri_commands.rs` 8 / `store.rs` 3 / `shell_list.rs` 3 / `path_resolver.rs` 3 / `daemon_control.rs` 2 / `bin/agent2ssh.rs` 2 / `container_terminal.rs` 1 / `container_discovery.rs` 1。**结论仍然成立**：没有一处落在传输层那 10 个文件里，所以 Mac 上跳过这些是安全的。但注意——

   **`agent` 认证是这条结论的唯一例外。** `embedded_ssh.rs:1749` 的 `session.agent()` 注释明写"includes Windows Pageant"，迁到 `russh::keys::agent::AgentClient` 后，Windows 侧要走 Pageant / `SSH_AUTH_SOCK` 命名管道那条分支。它在 `embedded_ssh.rs` 里，靠 `cfg(windows)` 之外的条件（运行期探测）分派，所以上面那 33 处计数并没有覆盖它。**这一项必须在 Windows 上用真实 Pageant 实测**，见本节第 2 条的验收补充。
2. **真实 OpenSSH ≥10 握手**。这是当初引入 OpenSSL 的唯一理由（WinCNG 只有 MODP DH，OpenSSH 10 删光了 `diffie-hellman-group*`）。换成 russh 后要在 Windows 上实测：ed25519 公钥、curve25519 KEX、chacha20-poly1305、AES-GCM 全部可用。
   ```bash
   ./scripts/e2e-container.sh          # 或 wslc / docker
   ```
   **反向验证**：`agent2ssh keys algorithm` 应仍回答 `ed25519`。

   **同一轮还要验两条容易被漏的**：

   - **四条认证路径各通一次**：公钥文件、密码、keyboard-interactive、**agent（Pageant）**。第四条是原文漏掉的那条，它在 Windows 上是独立代码路径，Mac 上只跑得通 `SSH_AUTH_SOCK` 那一半。
   - **TOFU 三段式**：先连一台未知主机 → 应弹指纹确认（不是直接拒绝也不是直接信任）→ 确认后再连应通过 → 手改 `known_hosts.json` 里的指纹后再连应报 "fingerprint changed"。这条验的是 §4.2-6 那个"探针必须故意接受未知密钥"的改法没有把安全语义改坏。

3. **sidecar 兜底（第 5 项缺口，与传输层无关）**。干净克隆时 `src-tauri/binaries/*` 不存在 → `tauri_build::build()` abort。建议在 `build.rs` 里先探测，缺失时注入 `TAURI_CONFIG={"bundle":{"externalBin":[]}}`（打包任务里 sidecar 一定已存在，行为不变）。修完 `cargo test --lib` 才算真的"克隆即可编译"。
4. **锁工具链**。当前无 `rust-toolchain.toml`（各机 rustc 漂移）、`package.json` 无 `engines`、无 `.nvmrc`（CI 用 node 20）。
5. **清 CI**。删掉两处 OpenSSL 工具链步骤后，确认 `windows-latest` 腿仍绿。
6. **Windows 既有问题**（与本次迁移独立，别混在一起修）：`docs/reports/WINDOWS-HANDOVER.md` 记录 `cli_smoke` 三个补全测试栈溢出、unit tests 跑 50 分钟无输出。

---

## 11. 验证矩阵

| 验证项 | macOS | Windows | CI |
|---|---|---|---|
| `npm test`（155） | ✅ | ✅ | ✅ ubuntu / win / mac |
| `cargo test --lib --no-default-features`（755） | ✅ | ✅ | ✅ |
| `cargo test --lib`（763，tauri feature） | ✅ 需 `TAURI_CONFIG` 或 sidecar | ✅ | ✅ `tauri-unit-tests`（ubuntu） |
| `cargo check --features daemon` | ✅ | ✅ | ✅ |
| `exec_fixture`（真实 sshd） | ✅ 容器 | ✅ wslc/docker | ✅ `real-ssh-e2e` |
| `daemon_local_exec_e2e` / `daemon_resident_session_e2e` | 部分 | ✅ | ❌ 未覆盖 |
| Windows `cfg` 编译 | ❌ | ✅ | ✅ |
| OpenSSH 10 握手 | ✅ | ✅ 必须 | ✅ |
| **四条认证路径**（公钥/密码/kbd-interactive/**agent**） | agent 只覆盖 `SSH_AUTH_SOCK` | ✅ **含 Pageant** | ❌ 无 agent 夹具 |
| **`stderr` 与 stdout 分离**（`drain_channel_output` / daemon 事件流） | ✅ 单测 | ✅ | ✅ |
| **阻塞标志 × 超时预算两旋钮** | ✅ 单测 | ✅ | ✅ |
| **TOFU 三段式**（未知 → 确认 → 变更检测） | ✅ | ✅ | ❌ 需交互 |

---

## 12. 风险与未决项

| # | 项 | 处理 |
|---|---|---|
| 1 | **`WouldBlock` 与阻塞标志的语义复刻不准** → 转发桥静默卡死、空转，或 exec 收尾坏掉 | S0 就把 `bridge_tcp_and_channel` 的读写路径用门面跑一遍，单测要覆盖**非阻塞下无数据即刻返回 `WouldBlock`**（不是"等超时再返回"），以及**阻塞模式下 `wait_close()` 不被 `WouldBlock` 短路** |
| 2 | **runtime 边界**：daemon async 里调同步门面会 panic | `transport/mod.rs` 顶部写死规则；逐个标注调用点，加 `spawn_blocking` |
| 3 | `russh` 0.60 的 RustCrypto 预发布锁 | **已解决**：必须 `russh 0.60.3`（0.60.1 的 caret 依赖解不出可行集）+ `default-features = false` + `ring` + `pkcs1/pkcs5` 钉版。理由与实测见 §5「探针结果」 |
| 4 | 算法名字符串兼容性 | `SshAlgoPrefs` 的 JSON 是用户配置，**名字不许变**；`kex-strict-*` 是协议标记要过滤；`narrow()` 的白名单语义要保留 |
| 5 | host key 算法名兼容性 | 已写进 `known_hosts.json`，`ssh-rsa` / `ecdsa-sha2-nistp256` 等字符串必须逐字保持 |
| 6 | RSA / SHA-1 遗留路径 | 现在靠 `method_pref(SignAlgo, "ssh-rsa")`（`embedded_ssh.rs:1842`）→ russh 用 `PrivateKeyWithHashAlg` + `best_supported_rsa_hash()` |
| 7 | 性能 | 控制面，不是吞吐路径。`aws-lc-rs` 会引入 cmake/NASM，**默认 feature 里就带着它**，必须显式关掉 |
| 8 | ~~【待验证】russh 0.60 的 MSRV~~ | **已执行**：rustc 1.98.1 下 `russh 0.60.1` 与 `0.60.3` 均可解析到编译阶段（失败点是依赖冲突，不是 MSRV） |
| 9 | 【待验证】`russh-sftp` 与 libssh2 SFTP 的权限位语义差异 | 用 `core.rs:2118` 那 5 个格式化函数的现有测试对拍 |
| 10 | ~~`set_timeout(0)` 等价物~~ | **这一条撤销。** 本仓把 0 夹到 1 ms（`session_timeout_ms()`，注释 "rather than disabling the bound"），**不存在"永不超时"模式**。按原写法加"永不超时分支"会重新引入现在被刻意消掉的挂死 |
| 11 | **`stderr` 双缓冲解复用** → 做错表现为 stderr 静默丢失，或 `read()` 与 `stderr().read()` 互相阻塞 | §4.2-3；`drain_channel_output`（`embedded_ssh.rs`）与 daemon 事件流（`bin/agent2ssh-daemon.rs:4637`）各一条单测，断言 stdout/stderr 都完整 |
| 12 | **`host_key` 的时点变更** → 探针拿不到未知主机密钥；或比对挪进回调后安全语义被改坏 | §4.2-6；把 §10 的 TOFU 三段式列入验收，不只跑单测 |
| 13 | **`supported_algs` 无等价物** → 用户拼错算法名从"报错"退化成"静默忽略" | 静态白名单 + `Name::try_from` 的 `Err`；`reads_openssh_private_keys` 恒 `true` 且保留 CLI JSON 字段 |
| 14 | **agent 认证横跨传输层与平台层** → Mac 上测通不等于 Windows 上通 | §5 S1-3、§10 第 1/2 条；Windows 上用真实 Pageant 实测 |
| 15 | **`ssh-key` 与 russh 的预发布钉版互斥** → S1 装不上 | **已解决（走 A）**：`ssh-key` 已从依赖里删除，私钥容器改由 `src/openssh_key.rs` 自写，公开钥派生仍用 `ed25519-dalek`。验证方式是对 `ssh-keygen -y` 对拍，不是对我们自己的解析器 |

---

## 13. 回滚

S1/S2 阶段 SSH 后端由 feature 切换，回滚只需改默认 feature（或 `--features transport-ssh2`），**不涉及数据迁移**。S3 之后回滚等于 revert 一个提交，`ssh2` 相关代码在历史里可查。

S3 里有两类东西**不能靠 revert 恢复干净**，删之前先想清：

1. `ssh_key_format.rs` 的删除。它不只是提示文案的辅助——`unusable_key_hint()` 的调用点要一并清理，且 `reads_openssh_private_keys()` 的**能力信号身份**（`ssh_algo.rs:129-137` 的注释就是为它写的）和 **CLI JSON 字段**（`bin/agent2ssh.rs:3399`）都要有替代物。删完再 revert 会把 `supported_algorithms` 一起带回来，与 russh 后端并存时那套"libssh2 能力探测"已经没有意义。
2. §7 里那三处"先补位再删"的替换（静态白名单、恒 `true` 的能力信号、`auth_methods` 短路改写）。它们是**行为等价性**的载体，回滚单独一笔提交容易只回滚一半，留下"没有 libssh2 却还在查 libssh2 能力"的半截状态。

门面（S0）本身是可回滚的：`legacy_ssh2.rs` 保留在 S2 之后，`--features transport-ssh2` 一直是那条退路。

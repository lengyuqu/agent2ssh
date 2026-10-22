# russh 迁移（P2）交接文档

状态：**待执行** · 2026-10-22 · 开发机：macOS（`MbpM1Max`）· 收尾机：Windows（`WinR9`）
关联：`docs/reports/WINDOWS-HANDOVER.md`（Windows 未验证面）、`src-tauri/src/ssh_key_format.rs`（迁移后删除）

---

## 0. 这份文档怎么用

- **在 Mac 上开工**：读 §4 建立目标架构 → §5 按阶段推进 → §6 逐文件改 → §9 跑前置与验证。
- **回 Windows 收尾**：直接跳 §10，那是必须在这台机器上做的事，Mac 上做不到。
- 一切"我猜的"都标了 **【待验证】**，落地前先验证，别当结论用。

---

## 1. 目标与验收

把 SSH 传输层从 `ssh2`（libssh2，C）换成 `russh`（纯 Rust），使三平台都退化为：

```
git clone → npm install → cargo test        # 无 Perl / nmake / NASM / cmake / Homebrew OpenSSL
```

验收不是"能编译"，而是这三条同时成立：

1. `cargo test --lib --no-default-features` = **735 passed**，`cargo test --lib` = **743 passed**（当前基线，不许下降）。
2. `npm test` = 22 文件 / **155 tests passed**（不变）。
3. `scripts/e2e-container.sh` 对真实 OpenSSH 服务端跑通 exec / SFTP / resume，且在 Windows 上握手成功（这正是当前被迫引入 OpenSSL 的原因）。

---

## 2. 现状：为什么"拿到源码就能编译"现在是假的

| # | 缺口 | 证据 |
|---|---|---|
| 1 | `.cargo/config.toml` 只按 **cwd** 被发现，与 `--manifest-path` 无关 | 仓库根跑 `cargo test --manifest-path src-tauri/Cargo.toml ...` 必炸；CI 全部用 `working-directory: src-tauri`，所以 CI 掩盖了这个坑 |
| 2 | `OPENSSL_SRC_PERL` 需要**完整** Perl | Git for Windows 自带精简版缺 `Locale::Maketext::Simple`，且 `which -a perl` 首选就是它 |
| 3 | 环境变量一变就触发 OpenSSL 全量重建 | `openssl-sys` 声明 `rerun-if-env-changed=OPENSSL_SRC_PERL`；同一哈希目录 `543c2086` 先失败后成功＝两次 10 分钟重建 |
| 4 | macOS 侧依赖完全没被记录 | `libssh2-sys` 有 `[target."cfg(unix)".dependencies.openssl-sys]` → 干净 Mac 需 `brew install openssl@3 pkg-config`，README/AGENTS 一个字没提 |
| 5 | sidecar 是 gitignore 的构建产物 | `.gitignore:4` 忽略 `src-tauri/binaries/*`，而 `tauri_build::build()` 要求它存在 → 干净克隆跑不了默认 feature 的 `cargo test --lib` |

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

`bridge_tcp_and_channel()`（`embedded_ssh.rs:1000`）和所有读循环只用到 `read / write / flush / send_eof / eof / exit_status / wait_close / close`。**只要门面的 `Channel` 实现这两个 trait，这 66 个调用点几乎不用改。** 这是整次迁移最重要的杠杆。

### 4.2 必须复刻的三个行为（否则现有代码静默失效）

1. **`WouldBlock` 语义**。`forward.rs:715` 用 `ErrorCode::Session(-37)` 判"暂时没数据"；`bridge_tcp_and_channel` 把 TCP 设成非阻塞，靠 `channel.read()` 返回 `WouldBlock` 驱动双工轮转。门面的 `read` 必须：**超时内无数据 → `io::ErrorKind::WouldBlock`**，实现上用 `mpsc::recv_timeout(session_timeout)`，不能用无限 `recv()`。
2. **`set_timeout` 是会话级全局量**（`embedded_ssh.rs:1286` 注释：`0` = 永不超时）。门面要把它变成门面自己的字段，所有阻塞原语都按它取超时。
3. **`exit_status` 在 `wait_close` 之后才可靠**。`ChannelMsg::ExitStatus` 可能在 `Close` 之后才到，门面要在 `wait_close()` 里等齐再返回。

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

---

## 5. 分阶段执行

每阶段结束都必须 **两条 cargo 测试命令全绿 + `npm test` 全绿**，否则不进下一阶段。

### S0 — 立缝（**不动任何依赖**）

1. 建 `src/transport/`，`legacy_ssh2.rs` 里包一层今天的 `ssh2::*`，对外只暴露自有类型。
2. 逐个文件把 `use ssh2::...` 换成 `use crate::transport::...`。
3. 自有 `Channel` 用 `impl Read/Write` 包住 `ssh2::Channel`，`eof/exit_status/wait_close/send_eof/close/request_pty/shell/exec` 一一转发。

**DoD**：`grep -rn "ssh2::" src/ | grep -v "src/transport/legacy_ssh2.rs"` 输出为空；735/743/155 全绿；`Cargo.toml` 未新增任何依赖。
**价值**：这一步在 Mac 和 Windows 上都成立，等于把 9500 行的风险提前清掉；之后换后端只改一个目录。

### S1 — 接 russh（双后端并存）

1. 加依赖，**版本与锁必须逐字照抄 `demo/rssh-main/src-tauri/Cargo.toml`**：

```toml
russh = "0.60.1"
russh-sftp = "2"
# russh 0.60.x 依赖 RustCrypto 预发布 API，必须锁死，否则依赖解析漂到不兼容的 pkcs8
ecdsa = { version = "=0.17.0-rc.16", default-features = false }
ed25519 = { version = "=3.0.0-rc.4", default-features = false }
elliptic-curve = { version = "=0.14.0-rc.28", default-features = false }
```

2. 实现 `russh_impl/`，feature 名 `transport-russh`，默认仍是 ssh2。
3. 认证只需三种（已核）：`userauth_pubkey_file`(×5)、`userauth_password`、`userauth_keyboard_interactive`。**没有 SSH agent 依赖**，不用迁 `russh::keys::agent`。
4. 每条测试在两种 feature 下各跑一遍。

**DoD**：`cargo test --no-default-features --features transport-russh --lib` 与 ssh2 版结果一致；`scripts/e2e-container.sh` 在两套后端下都通过。

### S2 — 切默认

把默认后端改成 `transport-russh`，ssh2 保留一个 feature 便于回滚。至少经过一次 release 观察再删。

### S3 — 删

见 §7。

---

## 6. 文件级改动地图

| 文件 | 行数 | 现状 | 改法 | 风险 |
|---|---|---|---|---|
| `src/transport/*` | 新增 | — | 见 §4.4 | 门面的 `WouldBlock`/`set_timeout` 语义 |
| `src/embedded_ssh.rs` | 2254 | 认证、host key 校验、exec、PTY、`bridge_tcp_and_channel` | 只换类型；`host_key_algorithm()`（`:152`）的 `HostKeyType` → 自有 `HostKeyKind`，**字符串保持原样**（写进 `known_hosts.json`） | 高：认证状态机 + 41 个调用点 |
| `src/core.rs` | 4759 | SFTP 批量操作、`FileStat` 格式化（`:2118`–`:2236`）、`create_sftp_dir_all` | `ssh2::FileStat` → `transport::FileStat`（字段对齐 `perm/size/mtime/uid/gid`）；`OpenFlags/OpenType` → 自有枚举 | 中：5 个格式化函数 + 1 处测试构造 |
| `src/forward.rs` | 1071 | `Arc<Mutex<Option<Session>>>`、双向桥、`-37` 分类 | Session 换成门面（内部是 `Handle` + tokio Mutex）；`ssh_error_is_would_block` → `ErrorKind::WouldBlock` | 高：转发是阻塞线程 + 非阻塞语义混合 |
| `src/ssh_algo.rs` | 689 | `Session::method_pref()` 字符串偏好 | `apply_algo_prefs()` 改成产出 `client::Config.preferred.{kex,key,cipher,mac,compression}`（`Cow<Vec<Name>>`，`Name::try_from(&str)`）；**配置 JSON 里的算法名不许变** | 中：`kex-strict-*` 这类协议标记要过滤掉（参考 `algorithms.rs:141`） |
| `src/ssh_key_format.rs` | 421 | 整套 libssh2 密钥格式探测（WinCNG 只认两种 PEM 头） | **整文件删除**；`inspect_key_file` 若 UI 还要，改成用 `russh::keys::decode_secret_key` 试解析 | 低（删代码），但要同步删调用点 |
| `src/session.rs` | 351 | PTY 会话注册表 | 跟随类型改名 | 低 |
| `src/resident_session.rs` | 1049 | 常驻会话 | 跟随 | 低 |
| `src/prompt_waiter.rs` | 581 | keyboard-interactive 交互 | libssh2 回调 → russh 的 `KeyboardInteractiveAuthResponse` 状态机 | 中：回调变状态机，时序不同 |
| `src/sftp_transfer.rs` | 599 | 传输调度 | 跟随 | 低 |
| `src/connection.rs` | 400 | 连接与 deadline | 换门面；deadline 用 tokio timeout | 低 |
| `tests/exec_fixture.rs`、`tests/local_exec.rs` | — | 需真实 sshd（`AGENT2SSH_TEST_SSH_PORT` / `AGENT2SSH_TEST_SSH_KEY`） | 不改断言，靠环境跑 | — |

---

## 7. 迁移完成后可以删掉的东西

- `src-tauri/src/ssh_key_format.rs`（421 行）
- `src-tauri/Cargo.toml`：`ssh2`、`[target.'cfg(windows)'.dependencies]` 里那条 `ssh2 = { features = ["openssl-on-win32","vendored-openssl"] }` 及其 40 余行注释
- `src-tauri/.cargo/config.toml`（`OPENSSL_SRC_PERL` 的 `[env]`）
- `scripts/win-perl.cmd`
- `.github/workflows/ci.yml`：两处 `Add build tools for the bundled OpenSSL (Windows)` 步骤（`choco install nasm` + `win-perl.cmd` 自检，第 240–250、415–425 行），以及第 221–239、396–414 行的解释性注释
- `AGENTS.md` 中关于 vendored OpenSSL / Perl / NASM 的全部段落，改为一条"Windows 只需 MSVC 工具链"

净减：约 500 行代码 + 一次 10–15 分钟的源码构建 + 一个未记录的平台依赖。

---

## 8. 参考实现：`demo/rssh-main`（同仓库，已是 russh 0.60）

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

# 4. 基线（先确认起点干净，再动手）
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features   # 期望 735
cargo test --manifest-path src-tauri/Cargo.toml --lib                          # 期望 743
```

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
   44 处 `#[cfg(windows)]` 已核：**没有一处属于 SSH 传输层**（集中在 `local_exec.rs` 10 / `tauri_commands.rs` 8 / `store.rs` 3 / `shell_list.rs` 3 / `path_resolver.rs` 3 / `daemon_control.rs` 2 及对应 tests），所以这些在 Mac 上跳过是安全的。
2. **真实 OpenSSH ≥10 握手**。这是当初引入 OpenSSL 的唯一理由（WinCNG 只有 MODP DH，OpenSSH 10 删光了 `diffie-hellman-group*`）。换成 russh 后要在 Windows 上实测：ed25519 公钥、curve25519 KEX、chacha20-poly1305、AES-GCM 全部可用。
   ```bash
   ./scripts/e2e-container.sh          # 或 wslc / docker
   ```
   **反向验证**：`agent2ssh keys algorithm` 应仍回答 `ed25519`。
3. **sidecar 兜底（第 5 项缺口，与传输层无关）**。干净克隆时 `src-tauri/binaries/*` 不存在 → `tauri_build::build()` abort。建议在 `build.rs` 里先探测，缺失时注入 `TAURI_CONFIG={"bundle":{"externalBin":[]}}`（打包任务里 sidecar 一定已存在，行为不变）。修完 `cargo test --lib` 才算真的"克隆即可编译"。
4. **锁工具链**。当前无 `rust-toolchain.toml`（各机 rustc 漂移）、`package.json` 无 `engines`、无 `.nvmrc`（CI 用 node 20）。
5. **清 CI**。删掉两处 OpenSSL 工具链步骤后，确认 `windows-latest` 腿仍绿。
6. **Windows 既有问题**（与本次迁移独立，别混在一起修）：`docs/reports/WINDOWS-HANDOVER.md` 记录 `cli_smoke` 三个补全测试栈溢出、unit tests 跑 50 分钟无输出。

---

## 11. 验证矩阵

| 验证项 | macOS | Windows | CI |
|---|---|---|---|
| `npm test`（155） | ✅ | ✅ | ✅ ubuntu / win / mac |
| `cargo test --lib --no-default-features`（735） | ✅ | ✅ | ✅ |
| `cargo test --lib`（743，tauri feature） | ✅ 需 `TAURI_CONFIG` 或 sidecar | ✅ | ✅ `tauri-unit-tests`（ubuntu） |
| `cargo check --features daemon` | ✅ | ✅ | ✅ |
| `exec_fixture`（真实 sshd） | ✅ 容器 | ✅ wslc/docker | ✅ `real-ssh-e2e` |
| `daemon_local_exec_e2e` / `daemon_resident_session_e2e` | 部分 | ✅ | ❌ 未覆盖 |
| Windows `cfg` 编译 | ❌ | ✅ | ✅ |
| OpenSSH 10 握手 | ✅ | ✅ 必须 | ✅ |

---

## 12. 风险与未决项

| # | 项 | 处理 |
|---|---|---|
| 1 | **`WouldBlock` 语义复刻不准** → 转发桥静默卡死或空转 | S0 就把 `bridge_tcp_and_channel` 的读写路径用门面跑一遍，加一个"超时无数据返回 WouldBlock"的单测 |
| 2 | **runtime 边界**：daemon async 里调同步门面会 panic | `transport/mod.rs` 顶部写死规则；逐个标注调用点，加 `spawn_blocking` |
| 3 | `russh` 0.60 的 RustCrypto 预发布锁 | 逐字照抄 rssh 的四行 pin；`cargo tree -d` 检查没有第二份 ecdsa/ed25519 |
| 4 | 算法名字符串兼容性 | `SshAlgoPrefs` 的 JSON 是用户配置，**名字不许变**；`kex-strict-*` 是协议标记要过滤 |
| 5 | host key 算法名兼容性 | 已写进 `known_hosts.json`，`ssh-rsa` / `ecdsa-sha2-nistp256` 等字符串必须逐字保持 |
| 6 | RSA / SHA-1 遗留路径 | 现在靠 `method_pref(SignAlgo, "ssh-rsa")`（`embedded_ssh.rs:1842`）→ russh 用 `PrivateKeyWithHashAlg` + `best_supported_rsa_hash()` |
| 7 | 性能 | RustCrypto 弱于 OpenSSL，但用在控制面；如未来有吞吐诉求再单独评估 aws-lc（**会引入 cmake/NASM，等于回到今天的坑，不轻易换**）|
| 8 | 【待验证】russh 0.60 的 MSRV 与当前工具链 | S1 第一步先 `cargo add` 后 `cargo check` 探一次 |
| 9 | 【待验证】`russh-sftp` 与 libssh2 SFTP 的权限位语义差异 | 用 `core.rs:2118` 那 5 个格式化函数的现有测试对拍 |
| 10 | 【待验证】`set_timeout(0)` 在 russh 下的等价物 | 需要 russh 侧显式"永不超时"分支 |

---

## 13. 回滚

S1/S2 阶段 SSH 后端由 feature 切换，回滚只需改默认 feature（或 `--features transport-ssh2`），**不涉及数据迁移**。S3 之后回滚等于 revert 一个提交，`ssh2` 相关代码在历史里可查。

唯一不可逆的是 `ssh_key_format.rs` 删除 —— 它只是提示文案的辅助，`unusable_key_hint()` 的调用点要在 S3 一并清理，别留悬空引用。

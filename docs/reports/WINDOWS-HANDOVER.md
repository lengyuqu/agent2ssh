# Windows 验证交接记录

日期：2026-10-22（macOS 开发机；Windows 部分由维护者在 Windows 上用独立 harness 验证）

本文记录 **没有在 Windows 上验证过** 的部分、CI 上观察到的现象，以及在 Windows 上复核时的做法。所有结论都只来自 macOS（Docker / Apple container）和 GitHub Actions 的 `windows-latest`，**没有任何一项是在 Windows 真机上跑出来的**。

## 1. CI 上的 Windows 现状

`build (windows-latest, x86_64-pc-windows-msvc)` 任务的观察（均为 `main` 分支 push）：

| Run | Commit | Windows 结果 |
|-----|--------|--------------|
| 39602994694 | `055bb34` | `cli_smoke` 三个补全测试失败：`thread 'main' has overflowed its stack` |
| 39629626028 | `b4a390e` | "Rust unit tests" 步骤运行约 50 分钟无输出，被手动取消 |
| 39637586186 | `82e22b1` | 单元测试、daemon 集成测试、`cli_smoke` 通过；"Rust CLI contract tests" 运行 30+ 分钟无输出，被手动取消 |
| 39643660739 | `4d24c11` | **全部步骤通过**（含 `cli_smoke`、`cli_contract`、connect deadline） |

要点：

- **补全栈溢出已修复并在 CI 验证**。原因是 `CompleteEnv::complete()` 在进程主线程（Windows 默认 1 MiB 栈）上构建 clap 命令树，未优化构建装不下；现在整段放进 8 MiB 线程（`src-tauri/src/bin/agent2ssh.rs` 的 `main`）。macOS 上用 `ulimit -s 1024` 复现并验证过同一机制。
- **两次"无输出挂起"原因未查明**。它们发生在不同步骤（库单元测试、`cli_contract`），同一批代码下一轮又全部通过，所以更像偶发（runner 或某个带子进程/网络的测试），而不是确定性死锁。GitHub 不提供进行中任务的日志，取消后日志也没有拿到。**如果在 Windows 上能复现，优先怀疑会起 CLI 子进程或做网络/DNS 解析的测试**（例如 `cli_contract` 里的 `ping` 不存在主机、`b39_*` 系列）。建议在 CI 上给 Windows 任务加 `timeout-minutes`，让挂起变成带日志的失败。
- Windows 构建使用 OpenSSL 后端（`ssh2` 的 `openssl-on-win32` + `vendored-openssl`，见 `src-tauri/Cargo.toml`），不是 WinCNG；此前文档里关于 WinCNG 的限制（无 curve25519、读不了 OpenSSH 私钥容器）对当前构建不适用，但下面第 2 节的内容仍需在真机上确认。

## 2. 本轮改动中依赖平台、需要在 Windows 上验证的部分

| 项 | 改动位置 | 为什么要在 Windows 验证 |
|----|----------|-------------------------|
| 用户证书认证（`<key>-cert.pub` 自动识别） | `embedded_ssh.rs` `user_certificate_for` / `authenticate` | 不同 libssh2 加密后端对证书公钥文件的处理可能不同；macOS/Linux 的 OpenSSL 后端已对 OpenSSH 8.3–10.3 验证通过（ed25519、RSA、ECDSA） |
| RSA 被拒后用 `ssh-rsa`（SHA-1）重试 | `embedded_ssh.rs` `retry_with_legacy_rsa_signature` | 依赖 `Session::method_pref(SignAlgo, …)` 在握手之后生效，这是 libssh2 的行为，需确认 Windows 构建同样如此 |
| 密钥交换失败提示 | `embedded_ssh.rs` `handshake_with_hint` | 只改了错误文案，但 `-5` 的触发条件取决于后端算法集 |
| `host add --passphrase` | `bin/agent2ssh.rs` | 口令走凭据库；Windows 的文件权限 ACL（`icacls`）路径需确认没有影响 |
| Dropbear 上传使用 16 KiB 写入 | `sftp_transfer.rs` `upload_chunk_for_server` | 与平台无关，但 Windows 上的 Dropbear/嵌入式设备实测可补一次 |
| 各密钥类型（ed25519、RSA-3072、ECDSA 256/384/521，OpenSSH 与 PEM 容器；明文、带口令、证书） | 矩阵脚本 | 后端换成 russh 后所有格式都可读，`keys algorithm` 的推荐恒为 ed25519；仍需在真机跑一遍矩阵确认 |
| 旧算法服务端（仅 SHA-1 DH、CBC、HMAC-SHA1、`ssh-rsa`） | 矩阵脚本 legacy 段 | `algo set` 能放开哪些算法取决于后端的 `supported_algs` |
| `known_hosts.json` 按 `host:port` 只记一个主机密钥类型 | `embedded_ssh.rs` `trust_or_verify_host_fingerprint` | 已知限制，与平台无关：改了主机密钥算法偏好或服务器新增更优先的密钥类型，会被报 "fingerprint changed"，需要重新信任 |

## 3. 在 Windows 上复核的做法

### 3.1 连接矩阵（`scripts/e2e-ssh-matrix.sh`）

脚本在 macOS 上对 Docker 通过 198 项、对 Apple container 通过 57 项，**从未在 Windows 上运行**。要在 Windows 上跑，需要注意：

- 用 Git Bash。脚本已 `export MSYS2_ARG_CONV_EXCL='/config;/home/'`（与 `e2e-container.sh` 同一做法），防止 Git Bash 把远端路径改写成 `C:/Program Files/Git/...`，**这一行也没有在 Windows 上验证过**。
- 运行时由 `agent2ssh containers runtime` 选择（Docker → wslc → Apple）。Apple container 无法拉取 `lscr.io`（跨域认证被拒），脚本会自动换成 `ghcr.io` 地址；wslc 上的镜像拉取没试过。
- 依赖 Git for Windows 自带的 `ssh-keygen`（含 `-m PEM`）、`ssh-agent`、`ssh-add`、`base64`、`head -c`、`cmp`。缺 `ssh-keygen` 时加密密钥和证书用例会跳过并提示，缺 `ssh-agent` 时 agent 用例跳过。
- `connects()` 把 `HOME` 指向空目录，避免应用回退到真实的 `~/.ssh/id_*`（`embedded_ssh.rs` 在密钥失败后会依次尝试默认密钥）。**这个隔离在 Windows 上不可靠**：`dirs::home_dir()` 在 Windows 走 Known Folder API，不读 `HOME` 或 `USERPROFILE`。因此在 Windows 上，"应被拒绝"的用例有可能被开发者自己 `%USERPROFILE%\.ssh` 下的密钥放行。复核时要么在没有默认密钥的账号/虚拟机里跑，要么先把该目录挪走；更稳妥的做法是给应用加一个仅测试使用的 home 覆盖变量（目前没有）。
- 临时目录里的私钥 ACL：系统 `ssh.exe` 会拒绝权限过宽的私钥（`e2e-container.sh` 已为此跳过独立校验）。矩阵里的私钥由 `ssh-keygen` 写在 `mktemp -d` 下，应用本身读取不受影响，但如果换用系统 `ssh` 做交叉校验会遇到。
- 服务端镜像：`lscr.io/linuxserver/openssh-server` 的 `version-8.3_p1-r1`、`8.8_p1-r1`、`9.3_p2-r1`、`9.9_p2-r0`、`10.3_p1-r1`，加 `alpine:3.20`（Dropbear + `openssh-sftp-server`）。端口从 22130 起。

### 3.2 其它建议在 Windows 上单独确认的点

- `agent2ssh keys algorithm --name` 的输出（当前恒为 ed25519，`reads_openssh_private_keys` 字段恒为 true）。
- 带 `-cert.pub` 的密钥在 Windows 路径（含反斜杠、含空格的用户目录）下是否仍被识别：`user_certificate_for` 用 `file_name()` 拼接，应当无关路径风格，但没实测。
- `cli_contract` 里起子进程的测试（见第 1 节的挂起怀疑）。
- `local:vs`、`local:wsl`、`local:cmd` 等 Windows 专有环境不在本轮改动范围内，沿用 `REGRESSION-LOG.md` 的 W1 记录。

## 4. 没有覆盖、与平台无关的缺口

供对照，避免把它们误认为 Windows 问题：多轮 keyboard-interactive（二次验证）、FIDO/安全密钥、跳板机矩阵、OpenSSH 7.x 之前的真实旧版本（目前用限制算法的 8.3 模拟）、除 OpenSSH 与 Dropbear 外的服务端实现（Windows 自带 OpenSSH、网络设备等）。

与平台无关的已知问题汇总在 [../KNOWN-ISSUES.md](../KNOWN-ISSUES.md)。

## 5. 附记：2026-10-22 在 WinR9 上的复核结果

本节由 Windows 侧的实测补充，不改动上文任何结论，只回答"这些条目现在是什么状态"。

### 5.1 第 1 节记录的两个 CI 症状：均不复现

| 症状 | 记录时的现象 | 2026-10-22 实测 |
|---|---|---|
| `cli_smoke` 三个补全测试 | `thread 'main' has overflowed its stack` | `cargo test --no-default-features --test cli_smoke` = **33 passed, 0 failed**（0.44s） |
| "Rust unit tests" 步骤 | 约 50 分钟无输出后取消 | `cargo test --lib` = **746 passed**，28s；含编译 1m46s |

`cargo test --lib`（默认 tauri feature）在 2026-10-22 之前需要 `TAURI_CONFIG` 或预先存在 sidecar，现在两条路都不用：`build.rs` 会在 `binaries/` 里找不到本 target 的 sidecar 时自行注入 `bundle.externalBin: []`，并打印一行指出 `scripts/prepare-sidecars.sh`。把 `binaries/*.exe` 全部移走后重跑 `cargo test --lib` 验证过。

### 5.2 第 3 节里已经过时的条目

- **`reads_openssh_private_keys` 字段已不存在**：`ssh_key_format.rs` 随 libssh2 后端一起删除，因为它存在的理由（WinCNG 只认两种 PEM 头）随后端消失了。`keys algorithm --name` 仍回答 `ed25519`，但不再有"本构建能不能读 OpenSSH 容器"这个问题。
- **`HOME` 隔离在 Windows 上不可靠**那条仍然成立，而且现在更容易观察到：agent 认证是 `embedded_ssh` 的第一顺位，`~/.ssh/id_*` 是最后一顺位。要证明某个用例走的是 agent 而不是默认密钥，做法不是挪 `HOME`（`dirs::home_dir()` 走 Known Folder API），而是**清空 agent 后看同一命令是否失败**——2026-10-22 用这个负对照验证了 agent 路径。

### 5.3 本轮真实跑过的内容

| 项 | 命令 | 结果 |
|---|---|---|
| Docker 容器化 E2E | `AGENT2SSH_CONTAINER_RUNTIME=docker ./scripts/e2e-container.sh` | 9 passed, 0 failed |
| wslc 容器化 E2E | `AGENT2SSH_CONTAINER_RUNTIME=wslc ./scripts/e2e-container.sh` | 9 passed, 0 failed |
| 单元测试（无默认 feature） | `cargo test --lib --no-default-features` | 738 passed |
| 单元测试（tauri feature） | `cargo test --lib` | 746 passed |
| CLI smoke | `--test cli_smoke` | 33 passed |
| clippy ×3 | CLI+MCP / daemon / tauri lib | 全绿，`-D warnings` |
| agent 认证（真机） | `ssh-agent` + `ssh-add` 两个身份，只授权第二个 | exit 0，且负对照失败 |

**没有跑过的**：PuTTY Pageant 本身（未安装，代码里 Pageant 优先、`\.\pipe\openssh-ssh-agent` 兜底，只有兜底路径有运行时证据）；`e2e-ssh-matrix.sh` 的全矩阵（依赖 Git for Windows 的 `ssh-keygen`/`ssh-agent`/`cmp`，本轮未执行）。

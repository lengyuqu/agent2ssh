# Windows 验证交接记录

日期：2026-10-22（macOS 开发机；Windows 部分由维护者在 Windows 上用独立 harness 验证）；2026-11-04 更新见第 6 节，同日在 Windows 真机完成第 6 节待办，结果见第 7 节。

本文记录 CI 上观察到的 Windows 现象、平台相关改动的复核做法，以及 Windows 真机结果。第 1–4 节写于 2026-10-22，结论只来自 macOS（Docker / Apple container）和 GitHub Actions 的 `windows-latest`；第 5 节（2026-10-22）和第 7 节（2026-11-04）是 Windows 真机实测，后者覆盖当前基线，与上文冲突时以第 7 节为准。

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
- Windows 构建使用 OpenSSL 后端（`ssh2` 的 `openssl-on-win32` + `vendored-openssl`，见 `src-tauri/Cargo.toml`），不是 WinCNG；此前文档里关于 WinCNG 的限制（无 curve25519、读不了 OpenSSH 私钥容器）对当前构建不适用，但下面第 2 节的内容仍需在真机上确认。（**已过时，见第 6 节**：russh 迁移后 `ssh2`/`openssl-sys` 不再编译或链接，传输层是 `russh` + `ring`，三平台同一算法集。）

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

脚本在 macOS 上对 Docker 通过 198 项、对 Apple container 通过 57 项；**2026-11-04 已在 Windows 真机（Git Bash + Docker）跑通 190 项，见 §7.4**。以下注意事项仍然适用：

- 用 Git Bash。脚本已 `export MSYS2_ARG_CONV_EXCL='/config;/home/'`（与 `e2e-container.sh` 同一做法），防止 Git Bash 把远端路径改写成 `C:/Program Files/Git/...`；**2026-11-04 在 Windows 上确认这一行是必需的**（未设置时 SFTP 远端路径被改写成 `C:/Program Files/Git/config/...`）。
- 运行时由 `agent2ssh containers runtime` 选择（Docker → wslc → Apple）。Apple container 无法拉取 `lscr.io`（跨域认证被拒），脚本会自动换成 `ghcr.io` 地址；wslc 上的镜像拉取没试过。
- 依赖 Git for Windows 自带的 `ssh-keygen`（含 `-m PEM`）、`ssh-agent`、`ssh-add`、`base64`、`head -c`、`cmp`。缺 `ssh-keygen` 或 agent 时相关用例会计入 SKIP；`AGENT2SSH_MATRIX_REQUIRE_FULL=1` 会在开容器前拒绝缺失的 keygen/agent。Windows 系统 agent 默认不修改；只有显式设置 `AGENT2SSH_MATRIX_WINDOWS_AGENT=1` 才会加入测试密钥，异常中断可能阻止清理。
- `connects()` 将 `HOME` 指向空目录，但不能假定该覆盖在 Windows 上隔离 `dirs::home_dir()`；应在无个人默认密钥的隔离账号或虚拟机中验证。显式 `key_path` 认证失败后直接返回错误，不会继续尝试默认密钥；默认密钥回退只可能影响没有显式 key/password 的 profile。Windows 实际路径、agent 和 `--cidfile` 行为仍未实测。
- 临时目录里的私钥 ACL：系统 `ssh.exe` 会拒绝权限过宽的私钥（`e2e-container.sh` 已为此跳过独立校验）。矩阵里的私钥由 `ssh-keygen` 写在 `mktemp -d` 下，应用本身读取不受影响，但如果换用系统 `ssh` 做交叉校验会遇到。
- 服务端镜像：`lscr.io/linuxserver/openssh-server` 的 `version-8.3_p1-r1`、`8.8_p1-r1`、`9.3_p2-r1`、`9.9_p2-r0`、`10.3_p1-r1`，加 `alpine:3.20`（Dropbear + `openssh-sftp-server`）。端口从 22130 起。

### 3.2 其它建议在 Windows 上单独确认的点（2026-11-04 已确认，见 §7.5）

- `agent2ssh keys algorithm --name` 的输出（当前恒为 ed25519，`reads_openssh_private_keys` 字段恒为 true）。
- 带 `-cert.pub` 的密钥在 Windows 路径（含反斜杠、含空格的用户目录）下是否仍被识别：`user_certificate_for` 用 `file_name()` 拼接，应当无关路径风格；**2026-11-04 已用 `…\work dir with spaces\cert key` 实测通过**（§7.5）。
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

- **`reads_openssh_private_keys` 字段仍在，值恒为 `true`**：`ssh_key_format.rs` 确实随 libssh2 后端一起删除了，但这个 JSON 键被保留在 `bin/agent2ssh.rs` 里、硬编码 `true`——`48b1d17` 的提交信息写明「JSON field stays, hardcoded true, because scripts」，理由是不破坏已经在解析它的脚本（`docs/plans/russh-migration.md` §4.5 也是这么定的）。本节此前写成「字段已不存在」是错的，与同文件 §3.2、§7.5 两处「恒为 true」互相矛盾；2026-11-10 按代码更正。`keys algorithm --name` 仍回答 `ed25519`；「本构建能不能读 OpenSSH 容器」这个问题确实没有了，字段留在那里只是因为 CLI 契约，不是因为还有东西可探测。
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

**Pageant 补充验收（2026-10-23）已通过**：Windows 11 上的 PuTTY / Pageant / plink 0.85 对 Docker 中的 OpenSSH 10.3，通过 Ed25519、仅授权第二个身份、RSA-3072（服务端对 RSA 签名仅接受 SHA-2）及 1 MiB SFTP 往返；空 agent、未授权密钥和关闭 Pageant 均按预期失败，重新加载后恢复。全新配置目录和不含密钥文件 / 密码的 host 隔离了本轮配置，OpenSSH agent 服务保持停止且禁用，诊断日志确认 `auth_method = agent`。密钥由应用生成，经临时 Alpine 容器的 `putty` 包所提供的 `puttygen` 转为 PPK。此前加载后卡住的现象没有复现，旧根因未查明；本轮未修改传输实现。OpenSSH 命名管道可打开但无可用身份时仍不会再试 Pageant，不能把本轮结果扩大为两种 agent 共存时的自动回退保证。完整证据见 [Pageant 验收报告](PAGEANT-ACCEPTANCE-2026-10-23.md)。

`e2e-ssh-matrix.sh` 的全矩阵（2026-10-22 已在本机跑通：OpenSSH 8.3/8.8/9.3/9.9/10.3 + Dropbear + legacy，**190 passed / 0 failed**，见 [../KNOWN-ISSUES.md](../KNOWN-ISSUES.md) §3）。

## 6. 2026-11-04 交接更新（基线 `e312166` / v0.4.5）

第 1、2 节写于 libssh2 时代，其中三处与当前 main 的事实矛盾，先纠正再列 Windows 侧待办：

**已过时的描述（勿据此建模）**：
- §1 的「Windows 构建使用 OpenSSL 后端」：russh 迁移（`2658b5e`，2026-10-22）后传输层是 `russh` + `ring`，`ssh2`/`libssh2-sys`/`openssl-sys` 不再编译或链接。原 §2 表格中「不同 libssh2 加密后端对证书公钥文件的处理可能不同」的验证理由随之失效。
- §2 的「known_hosts.json 按 host:port 只记一个主机密钥类型」：v0.4.5 已改为按 `host:port|<算法>` 键控，双算法主机切换 hostkey 偏好不再误报「fingerprint changed」——该条从已知限制变成待验证的新行为。
- §2 的「RSA 被拒后 ssh-rsa 重试」：v0.4.5 两条回退路径都设 `used_legacy_sha1_signature` 并写 warn 日志；10-22 的 K10（RSA 证书 legacy 签名）已随 russh 0.64.1 解决。

**Windows 真机待办（按优先级）—— 2026-11-04 已全部完成，结果见第 7 节**：
1. **基线**：`main` 拉到 `e312166` 后跑 `cargo test --lib --no-default-features`、`cargo test --lib`、`--test cli_smoke`（34 项，含新 `cli_approvals_clean_dry_run_then_compact`）、`--no-default-features --features daemon --bin agent2ssh-daemon`（24 项，新增的 CI 目标）、clippy ×3。
2. **v0.4.5 新行为**（11-02/03 的提交均未经 Windows 真机）：SFTP stall guard（Dropbear/OpenSSH 上传停滞报进度，K2 复测）；known_hosts 双算法键控；证书拒绝三因可辨；SHA-1 回退的 warn；`execution_limits.toml` 热加载；`approvals clean --dry-run`；0.4.4 的用户自定义环境（`environments.toml` 的 `cmd`/`pwsh`/`vs` 基座）。
3. **矩阵**：Git Bash 下 `./scripts/e2e-ssh-matrix.sh`（§3.1 的注意事项仍有效：不要依赖 `HOME` 隔离，使用隔离账号或虚拟机；系统 agent 默认不修改，明确同意后才可选择 opt-in；`MSYS2_ARG_CONV_EXCL` 未在 Windows 验证）。
4. **§3.2 的点**：带空格/反斜杠路径下 `-cert.pub` 的识别；`cli_contract` 起子进程测试的挂起复现尝试。

## 7. 2026-11-04 Windows 真机验证结果（基线 `6606621` / v0.4.5）

第 6 节四项待办已在 Windows 11（10.0.26300，rustc 1.99.1，Docker 29.9.2，Git Bash）上完成。`6606621` 相对 `e312166` 只多两个文档提交，代码相同。

### 7.1 基线

| 命令 | 结果 |
|---|---|
| `cargo test --lib --no-default-features` | 826 passed / 0 failed / 1 ignored（28.5 s）；修复下条缺陷后 827 |
| `cargo test --lib`（tauri feature） | 843 passed / 0 failed / 1 ignored（36.4 s）；修复后 844 |
| `--test cli_smoke` | 34 passed（0.42 s，含 `cli_approvals_clean_dry_run_then_compact`） |
| `--no-default-features --features daemon --bin agent2ssh-daemon` | 24 passed（1.08 s） |
| clippy ×3（CLI+MCP / daemon / tauri lib，`-D warnings`） | 全绿 |
| `--test cli_contract` | **40 passed / 0 failed（6.64 s）**，未复现挂起 |

### 7.2 v0.4.5 新行为

| 项 | 结果 |
|---|---|
| SFTP 8 MiB 往返（K2 复测） | OpenSSH 10.3：上传 648 ms / 下载 1077 ms；Dropbear 2024.85（Alpine 3.20）：上传 827 ms / 下载 1077 ms；两侧 SHA-256 均一致，无停滞。停滞报告本身由 `a_stalled_peer_reports_how_far_the_transfer_got` 覆盖；真机未诱导 600 s 超时 |
| known_hosts 双算法键控 | 同一 127.0.0.1:22222 下 `…\|ssh-ed25519` 与 `…\|ssh-rsa` 两条目共存；把 ed25519 条目的指纹改坏后仍报 `SSH host fingerprint changed`（负对照） |
| 证书拒绝三因 | 容器 sshd 只信任测试 CA：合法证书通过；错误 principal、过期、不受信 CA 分别给出各自的文案（`valid for ["somebody-else"]` / `expired at …` / 提示 `TrustedUserCAKeys`） |
| SHA-1 回退 warn | 见 7.3：发现并修复误报；真阳性为 `PubkeyAcceptedAlgorithms ssh-rsa` 下 RSA 认证，记录 `detected_by: "transport fallback"` |
| `execution_limits.toml` 热加载 | `per_minute = 1` 时第 2 次 `/local-exec` 得 429；改为 100 后**不重启** daemon，第 3 次通过，日志出现 `execution limits reloaded` |
| `approvals clean --dry-run` | `{dropped:1, kept:1, dry_run:true}`，文件哈希不变；真实清理后 1 行 |
| 用户自定义环境 | `cmd`/`pwsh`/`vs` 三种基座的 prelude、env、cwd 均生效（vs 的 `where.exe cl` 指向 MSVC 14.51.36231）；daemon 运行中新增的环境被拒（加载一次、不热更新），重启后生效 |

### 7.3 发现并修复的缺陷：非 RSA 登录被误报为 SHA-1 回退

`used_legacy_sha1_signature` 的记录条件是 `first.success() && hash_alg.is_none()`，而 `rsa_hash_alg` 对任何非 RSA 密钥都返回 `None` —— 所以每次 ed25519/ECDSA 认证成功都会写一条 `ssh authenticated with the legacy ssh-rsa (SHA-1) signature`（真机日志两次连续出现，其中第二次的连接还因指纹变化并未完成认证）。这与该功能提交说明「no RSA key was used 应为 false」直接矛盾，属实现缺陷，与平台无关。

修复：条件加上密钥类型（`first_succeeded && is_rsa && hash_alg.is_none()`，抽成纯函数并加 `only_rsa_without_a_sha2_choice_counts_as_legacy`）。真机复验：ed25519 与现代 RSA 不再告警；对只接受 `ssh-rsa` 的 sshd，RSA 认证仍正确记录 warn。`CHANGELOG.md` 已记。

### 7.4 矩阵

`./scripts/e2e-ssh-matrix.sh`（Docker，Git Bash）：**190 passed / 0 failed / 8 skipped**，与 macOS 基线一致。

| 服务器 | 结果 |
|---|---|
| openssh:8.3 / 8.8 / 9.3 / 9.9 / 10.3 | 各 33 passed / 0 failed / 1 skipped（ssh-agent identity） |
| dropbear | 22 passed / 0 failed / 2 skipped（证书不支持、ssh-agent） |
| openssh:8.3+legacy | 3 passed / 0 failed / 1 skipped（K12：russh 无 SHA-1 kex） |

- `MSYS2_ARG_CONV_EXCL='/config;/home/'` 确实必需：未设置时 Git Bash 把远端 `/config/...` 改写成 `C:/Program Files/Git/config/...`（本次实测）。设置后矩阵正常。
- `HOME` 隔离在 Windows 仍不可靠，但矩阵使用独立 `AGENT2SSH_CONFIG_DIR`，真实 `~/.agent2ssh` 未被触碰；agent 用例因本机 OpenSSH agent 服务未运行而跳过，Pageant 路径沿用 2026-10-23 验收结论。
- 矩阵脚本另有一处小瑕疵已修：内层 `sh -c` 脚本的注释里用了反引号（`` `PerSourcePenalties` ``），整段在双引号内，外层 bash 会把它当命令替换执行，日志出现两行无害的 `command not found`。改为无反引号注释后单跑 8.3 复验：36 passed / 0 failed / 2 skipped，噪音消失。

### 7.5 §3.2 遗留点

- `keys algorithm --name --json` 回答 `ed25519`，`reads_openssh_private_keys` 恒为 `true`，与 §3.2 的预期一致。
- 带空格与反斜杠的证书路径（`…\work dir with spaces\cert key` + `cert key-cert.pub`）下证书被正确识别并认证成功。
- `cli_contract` 40 项在 6.64 s 内跑完，起子进程的测试未复现 CI 上的无输出挂起。CI 已为每个 job 加 `timeout-minutes`，再发生会变成带日志的失败。

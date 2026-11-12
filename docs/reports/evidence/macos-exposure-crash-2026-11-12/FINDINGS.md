# CNB #28 — macOS/Linux 跨进程暴露面实测与真实崩溃注入

日期：2026-11-12。仓库基线：`cnb/main` = `17068ff`。全部实测使用**合成标记**与
`tempfile` 隔离目录（`AGENT2SSH_CONFIG_DIR` 指向临时目录），未读取、写入或显示任何真实凭据，
未触碰 `~/.agent2ssh`。原始输出见同目录 `raw-exposure.txt` / `raw-tty.txt` / `raw-crash.txt`。

## 0. 环境（实测）

| 项 | 值 |
|---|---|
| OS | macOS 27.2.1 (26B131), arm64, `Darwin Kernel Version 27.2.0 ... RELEASE_ARM64_T6000` |
| SIP | enabled |
| 调用者 | uid 501 (`yuqu`), gid 20(staff), admin 组 |
| 工具链 | Apple clang 21.0.0 (clang-2100.4.1.1)、rustc/cargo 1.99.1、Python 3.14.10 |
| CLI | `src-tauri/target/debug/agent2ssh`，`cargo build --no-default-features --bin agent2ssh` |

## 1. 跨进程 argv / environ 暴露面

`procargs_probe.c` 用 `sysctl(CTL_KERN, KERN_PROCARGS2, pid)` 读取目标进程的执行路径、argv
与 environ，逐段打印 NUL 分隔字符串。归档输出中环境变量**只保留变量名**，值一律 `<redacted>`；
仅合成标记对应的两行保留原文作为证据。

| # | 项目 | 结果 | 判定 |
|---|---|---|---|
| E1 | 同 uid 子进程：argv 标记 `A2S_EXPOSURE_ARGV_7F3C`、env 标记 `A2S_EXPOSURE_ENV_9B2E` | `KERN_PROCARGS2` 返回 `len=32236 argc=4`；两个标记**都可见** | **实测**：同 uid 进程的 argv 与 environ 均可读 |
| E2 | `ps -p <pid> -o args=` / `ps auxww` | 均可见 argv 标记（`PS_AUXWW_HIT_COUNT=1`） | **实测**：命令行可见性与 `KERN_PROCARGS2` 一致 |
| E3 | 对照组：另一个同 uid 进程（父 zsh） | `len=3279 argc=3`，正常返回 `/bin/zsh -c ...` | **实测**：探针本身可用，E4 的 EINVAL 不是探针缺陷 |
| E4 | pid 1 (`/sbin/launchd`, root, SIP 保护) | `KERN_PROCARGS2` → `ERROR size errno=22 (EINVAL)`；但 `ps -p 1 -o args=` 正常返回 `/sbin/launchd` | **实测**：失败的是这个 sysctl，不是「进程信息全部不可读」。**errno 是 EINVAL 而非 EPERM，原因本轮未确证** |
| E5 | 其他 root 进程（pid 96 `logd`、pid 98 `smd`） | 同样 `EINVAL(22)` | **实测**；`ps -U root` 列出 196 个 root 进程，命令行本身可见 |
| E6 | hidepid 等价物 | `sysctl -a | grep -i hidepid` 无命中；`mount` 前 6 项无 `/proc` 挂载（APFS sealed root、devfs、各系统卷） | **实测**：macOS 无 `/proc` hidepid 语义，Linux 的 hidepid 结论**不能**移植 |

**结论**：K8 中「旧 `--password` / `--passphrase` 的值会落在进程参数中」在 macOS 同 uid
场景下成立且已实测——`KERN_PROCARGS2` 与 `ps` 都能读到 argv。加上 shell history 的持久化点
（属源码/使用方式推论，非本轮实测），保留旧参数做脚本兼容的代价是真实存在的。
`--password-stdin` / `--passphrase-stdin` 把值移出 argv 的收益也就有了本机证据。

**明确未验证**（不得写成平台结论）：

- Linux `hidepid=1/2`、不同 uid、`ptrace_scope` 组合的可见性矩阵——本机无 Linux 环境；
  11-06 那次是 Linux 容器且 `ptrace_scope=0`、`/proc` 未做 hidepid 重挂载，只代表该容器。
- root / SIP 保护进程上 `KERN_PROCARGS2` 返回 EINVAL 的根因（内核限制？SIP？sysctl 语义？）——
  本轮只记录现象，未做内核级归因。
- 其他 macOS 版本、网络文件系统、不同权限模型下的可见性。

## 2. 真实 `kill -9` 崩溃注入（不再是构造 log 的确定性模拟）

### 手法（不改生产代码）

`src-tauri/tests/fixtures/crash_hook.c` 编译为 dylib，经 `DYLD_INSERT_LIBRARIES` 注入**真实的
`agent2ssh` CLI 进程**，用 `__DATA,__interpose` 拦截 `rename` / `unlink`：

- `before-rename` = `rename(tmp, hosts.json)` 之前，即 `store_commit::begin()` 之后；
- `before-finish` = `unlink(.commit.json)` 之前，即 hosts rename 之后、`finish()` 之前。

命中时 hook 写 `A2S_HOOK_DIR/<stage>.fired` 并 `pause()`；驱动脚本从**进程外部**发送
`SIGKILL`，进程以 `returncode == -9` 死亡，`pause()` 永不返回。这样磁盘上留下的是真实写入
路径在该时点的状态，而不是手工拼出来的 log。替换函数走 `renameat` / `unlinkat`，因为
`RTLD_NEXT` 对 macOS 插入镜像返回 NULL（首次 smoke 因此 SIGSEGV，已修正并有 `hook_smoke.c`
对照：`stage=none` 正常退出 0）。

驱动：`run_crash_injection.py`（证据与可复现脚本）；回归：
`src-tauri/tests/store_crash_injection.rs`（macOS-only，`#[serial_test::serial]`，3 项）。

### 结果（Python 驱动 0 失败；Rust 回归 3 passed / 0 failed）

| Case | 崩溃瞬间状态（实测） | 重启后（全新 CLI 进程） | 判定 |
|---|---|---|---|
| `before-rename` | `.commit.json` 存在；`hosts.json` 仍只有 `seed`；`hosts.json.tmp.<pid>` 完好且 sha256 == `log.hosts_sha256`；`secrets.enc` sha256 == `log.secrets_sha256` | `recover()` Case 3 重放：hosts 含 `crash-target`，随后 `after-recovery` 也落地；log 与 tmp 清除；`hosts.secrets_generation == secrets.enc.generation`；解密后 `seed` 与 `crash-target` 的口令均解析为对应合成标记 | 通过 |
| `before-finish` | `.commit.json` 存在；`hosts.json` sha256 == `log.hosts_sha256` 且已含 `crash-target`；临时文件已被 rename 消耗 | `recover()` Case 1 只清 log，无双重重放、无数据丢失；代次对齐；两条引用解密可解析 | 通过 |
| `lost-tmp` | 在 `before-rename` 崩溃后删除 log 指向的 tmp 文件 | 写入被拒绝，stderr 含 `cannot be replayed`；`hosts.json` 字节不变；log 保留供检查；`must-not-land` 未落地 | 通过 |

所有 case 的 stdout / stderr 与配置目录内每个文件都经过合成标记扫描，无泄漏。

### 一处值得交给 #26 的实测事实

`lost-tmp` 状态下磁盘是：`hosts.secrets_generation=1`、`secrets.enc.generation=3`，
`secrets.enc.entries` 已含 `host:crash-target`，而 `hosts.json` 仍是只有 `seed` 的旧版本，
其密码字段是裸 `$agent2ssh-secret$`。当前 `recover()` 在此处 **fail-closed（拒绝写入）**，
所以本路径没有静默用错密文；但这份状态说明「裸 marker + 代次」不足以让引用自证密文身份，
与 #26 阶段 2「恢复旧 `hosts.json` 快照时旧引用会静默指向同名新密文」是同一个根因。
该结论是**实测状态 + 源码语义推断**，不是「已复现错配登录」。

### 关于「可重放但临时文件丢失」是否需要显式修复命令

现状：`recover()` 保留 log 并拒绝后续写入，直到人工检查；CLI **没有**显式修复命令，
用户要恢复写入只能手工删除 `.commit.json`，且此时 hosts 与 secrets 代次已经不一致、
库里还留着孤儿密文。建议（**产品决策，本轮不实施**）：提供一个显式、带确认与审计的
修复命令，例如 `agent2ssh config repair-commit --discard-pending`，打印将保留的 hosts 版本、
代次差与孤儿条目后由用户确认；**不得自动丢弃**，因为无法排除目标 hosts 其实是更新的配置。
该决策与 #22 的 D6 / #26 阶段 2 共用一份口径，建议合并评审。

## 3. TTY 行为（`run_tty_probe.py`）

| 项 | 实测输出 | 判定 |
|---|---|---|
| PTY（真实 termios 行规程）首次 `secrets set-password` | 输出仅 `New master password: \r\nConfirm master password: \r\nMaster password set.\r\n`；输入含合成标记 + 中文字符 + 退格，**标记从未回显** | 通过 |
| 非 TTY 重定向 stdin | stdout 仅 `Master password set.`，stderr 空，无交互提示、无标记 | 通过 |

边界：这是 `openpty()` 伪终端，具备真实内核 ECHO 语义，但**不是** Terminal.app 的人工肉眼确认；
Windows Terminal / ConPTY 与经典 conhost 属于 #27，不在本单范围。既有
`tests/store_transaction.rs::interactive_initial_password_requires_matching_confirmation_without_echo`
已用同一 pty 路径做回归，本轮实测与它一致。

## 4. 实测 / 推断 / 未验证 清单

| 分类 | 内容 |
|---|---|
| 实测 | E1–E6 暴露面；两个时点的真实 SIGKILL 与三分支恢复；引用解密解析；代次对齐；PTY 无回显；非 TTY 无提示；合成标记零泄漏 |
| 推断（有源码依据） | shell history 持久化点；`lost-tmp` 代次差与 #26 裸 marker 问题的关联；显式修复命令的建议形态 |
| 未验证 | Linux hidepid/ptrace_scope/跨 uid 矩阵；root 进程 `KERN_PROCARGS2` EINVAL 的根因；Windows 真机（#27）；Terminal.app 人工确认；真实断电 / 内核崩溃（本轮只覆盖进程级 SIGKILL）；`fsync` 持久性 |

**CI 边界（需人工决定）**：`.cnb.yml` 第 17–21 行逐一枚举 Rust `--test` 目标，
`store_transaction` 与新增的 `store_crash_injection` 均不在清单内。按仓库既有约定
（L2-196 把 `.cnb.yml` / `scripts/**` 列为禁区），本轮**未改 CI**；新增回归目前只能手工执行。
该目标只在 macOS 有效，Linux runner 上编译为 0 tests，纳入 CI 时需要平台条件。
FINDINGS 与 K8 均按此口径记录，不宣称它已被 CI 强制门覆盖。

## 5. 复现

```bash
cd src-tauri && cargo build --no-default-features --bin agent2ssh
cd ../docs/reports/evidence/macos-exposure-crash-2026-11-12
python3 run_exposure_probe.py  > raw-exposure.txt 2>&1
python3 run_tty_probe.py      > raw-tty.txt      2>&1
python3 run_crash_injection.py > raw-crash.txt   2>&1
cd ../../../../src-tauri && cargo test --no-default-features --test store_crash_injection -- --test-threads=1
```

## 6. 文件

| 文件 | 用途 |
|---|---|
| `procargs_probe.c` / `run_exposure_probe.py` / `raw-exposure.txt` | macOS argv/environ 暴露面探针、驱动、原始输出 |
| `run_tty_probe.py` / `raw-tty.txt` | PTY 与管道实测 |
| `run_crash_injection.py` / `raw-crash.txt` | 真实 SIGKILL 三 case 驱动与原始输出 |
| `hook_smoke.c` | DYLD interpose 是否生效的最小对照程序 |
| `src-tauri/tests/fixtures/crash_hook.c` | 注入 hook 的**唯一源码**（测试与证据脚本共用） |
| `src-tauri/tests/store_crash_injection.rs` | macOS-only 回归测试（含解密验证） |

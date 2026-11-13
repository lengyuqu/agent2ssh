# CNB #27 — Windows 实机验证：stdin 无回显、管道 CRLF 与 rename 重试

日期：2026-11-12。仓库基线：`cnb/main` = `f98bd83`（本地 `workbuddy/main-cd4d72f6` 分支，测试提交见本轮 commit）。
全部实测使用**合成标记**，每个用例都有独立的 `AGENT2SSH_CONFIG_DIR` 临时目录；未读写任何真实凭据，
`~/.agent2ssh` 在本轮前后无任何文件被修改（`find ~/.agent2ssh -newermt 2026-11-12` 为空）。
原始输出见同目录 `raw-*.json`；驱动脚本见 `*.py`（`python <script> <cli.exe> <show_secret.exe> <outdir>`）。

## 0. 环境（实测）

| 项 | 值 |
|---|---|
| OS | Windows 11 `10.0.26300.9818`（主机名 `machenikeR9`） |
| 账户 | `Administrator`，管理员令牌（非受限账户） |
| 工具链 | rustc/cargo 1.99.1，Python 3.15.2 |
| Shell | `cmd.exe`（Windows 11 自带）、PowerShell 7.6.9、Windows PowerShell 5.1.26100.9818 |
| 终端 | ConPTY（pywinpty 3.1.2）、经典 conhost（wexpect 4.0.0 读屏幕缓冲区） |
| CLI | `src-tauri/target/debug/agent2ssh.exe`，`agent2ssh 0.4.5`，`cargo build --no-default-features --bin agent2ssh` |
| 辅助 | `show_secret.rs`（同目录，证据工具：用 lib 解密已存口令并以 hex 打印；编译到 `src-tauri/examples/` 用一次后移出仓库，源码随证据归档） |

## 1. `--password-stdin` 在 cmd.exe 与 PowerShell 下的实际字节

驱动：`run_windows_stdin_probe.py`，原始：`raw-stdin-probe-before-bom-fix.json`（修复前）、
`raw-stdin-probe.json`（修复后）。"存储值"一列是 `show_secret` 用 lib 的 `get_secret` 解密出的
hex，不是从 JSON 输出推断的。

| # | 用例（真实命令形状） | 存储值（hex → 文本） | 判定 |
|---|---|---|---|
| A1 | `cmd /c echo M\| agent2ssh host add … --password-stdin` | `5359…4543484f` → `SYNTHETIC_CMD_ECHO` | **实测通过**：cmd 管道自带的 CRLF 被裁掉，值里没有 `\r` |
| A2 | `cmd /c agent2ssh … < secret.txt`，文件为 `M\r\n` | `SYNTHETIC_CMD_REDIRECT` | **实测通过**：只裁一个行尾 |
| A3 | 文件为 `  M  \r\nsecond line  \r\n` | `2020…2020 0d0a 7365…20`，即 `  M  \r\nsecond line  ` | **实测通过**：仅最后一个 `\r\n` 被裁，内部 CRLF 与首尾空格保留 |
| A4 | `cmd /c type NUL\| agent2ssh … --password-stdin` | — ；`Error: secret input is empty` | **实测通过**：退出码 1，未写 `hosts.json` |
| A5 | `cmd /c type bad.bin\| …`，`bad.bin` = `C3 28`（非法 UTF-8） | — ；`Error: secret input must be UTF-8` | **实测通过** |
| B1 | pwsh 7：`"M" \| & agent2ssh … --password-stdin` | `SYNTHETIC_PWSH_PIPE` | **实测通过** |
| B2 | PS 5.1：`"M" \| & agent2ssh … --password-stdin` | 修复前 `efbbbf 5359…`（U+FEFF + M）；修复后 `5359…` | **实测发现缺陷，已修**（§5） |
| B3 | PS 5.1 `$OutputEncoding` 诊断 | `type=System.Text.ASCIIEncoding`、`preamble=0` | **实测**：5.1 的 `$OutputEncoding` 是 ASCII 且无前导码，但 native 管道仍写入 UTF-8 BOM —— 说明该 BOM 不受 `$OutputEncoding` 控制 |
| B4 | PS 5.1 显式 `$OutputEncoding = UTF8Encoding($false)` / `UTF8Encoding($true)` / `[Console]::OutputEncoding` / 二者组合 | 无 BOM 设置：仍是 1 个 BOM；**带 BOM 设置：2 个 BOM** | **实测**：调用方无法去掉第一个 BOM，只能由 CLI 剥离 |
| B5 | pwsh 7 / PS 5.1：`… --password-stdin < file` | pwsh 7：`ParserError … "<"运算符保留供将来使用`；5.1：`The '<' operator is reserved for future use` | **实测通过**：两个 shell 都不支持 `<` 重定向，必须改用管道 |
| B6 | pwsh 7：`Get-Content -Raw file \| & agent2ssh …`，文件末尾 `\r\n` | `SYNTHETIC_GETCONTENT\r\n`（残留一个 CRLF） | **实测**：`-Raw` 保留文件末尾换行，PowerShell 又追加一个，两个里只裁掉一个 |
| B7 | pwsh 7：`Get-Content file \| & agent2ssh …`（不加 `-Raw`） | `SYNTHETIC_GETCONTENT` | **实测通过**：推荐写法 |
| B8 | pwsh 7：`$null \| & agent2ssh … --password-stdin` | `Error: secret input is empty` | **实测通过** |

**`--passphrase-stdin` 的覆盖状态。** 上表 15 个用例全部是 `--password-stdin`。两条路径在
`bin/agent2ssh.rs` 里合并成一次 `read_redirected_host_secret()` 调用（`password_stdin || passphrase_stdin`
→ 同一个 `stdin_secret`，再分别落到 `password` / `passphrase` 字段），所以「shell 的字节对两者一致」
在源码上是**推断**，不是本轮的实测结论。收尾时补了一条回归（见 §5 的
`passphrase_stdin_takes_the_same_redirected_path`），覆盖 cmd 管道与 PS 5.1 管道两条最苛刻的路径，
断言密文可解出同一个值、`hosts.json` 的 `password` 字段仍为空、明文不出现在磁盘上；它随
`windows-latest` 的 CI 腿运行，**本机尚未单独跑过**，因此这里仍标为「有回归、没真机数字」。

补充实测（cmd 自身语义，非 CLI 缺陷）：

- `cmd /c type` 对一个以 `FF FE` 开头的文件输出 **0 字节**（`probe_cmd_details.py`），所以「非法 UTF-8」
  的用例必须避开 UTF-16 BOM，否则测的是 cmd 的文件嗅探。A5 因此用 `C3 28`。
- 本轮的所有 cmd 调用都把 exe 目录前置到 `PATH`（而不是在命令行里写带引号的绝对路径）：cmd 的
  `/c` 在「首参数带引号 + 命令里还有第二处引号」时会剥错引号并报
  `The filename, directory name, or volume label syntax is incorrect`。要写成
  `cmd /c ""C:\path\x.exe" … < "C:\tmp\f.txt""`（**外层再包一对引号**）才正确，已在
  `probe_cmd_details.py` 中实测；回归测试走 `PATH`，避开这个与 CLI 无关的坑。

## 2. `secrets set-password` 的真终端无回显

两个后端分别测：ConPTY（`CreatePseudoConsole`，Windows Terminal / VS Code 走这条路）与经典
conhost（`cmd.exe` 独立窗口走这条）。判定不看「屏幕上像不像」，而是三件事：提示出现、口令字符串
**没有**出现在终端输出里、事后用同一口令能让新进程解锁该库。

| # | 后端 | 用例 | 结果 |
|---|---|---|---|
| C1 | ConPTY | 正常输入 + 确认 | 提示可达；`SYNTHETIC_PTY_PASSWORD_9Q` 不在 transcript；`secrets status` 用该口令 `unlocked=true`；退出码 0 |
| C2 | ConPTY | 退格：`abX` + DEL + `c` | 解锁口令为 `abc`；transcript 无回显 |
| C3 | ConPTY | 非 ASCII：`合成口令-测试` | 该口令可解锁（UTF-8 解码与逐字符退格都正确） |
| C4 | ConPTY | Ctrl+C（`0x03`） | `Error: master password input cancelled`，退出码 1；库未创建；shell 未被打断 |
| C5 | conhost | 正常输入 + 确认 | `prompt1/prompt2/completed` 均 true；屏幕缓冲**不含**口令；该口令可解锁 |
| C6 | conhost | 退格 `abX` + DEL + `c` | 存储值 `abc`，屏幕只显示 `abXc` 的键入轨迹（无回显） |
| C7 | conhost | 非 ASCII `合成口令-测试` | 屏幕不含口令；该口令可解锁 |
| C8 | conhost | Ctrl+C | 屏幕出现 `cancelled`；`secrets.enc` 未创建 |

**结论（实测）**：`raw_terminal::RawTerminal::enable()` 的 `SetConsoleMode` 去
`ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT` 在 **ConPTY 与 conhost 上都成立**；
`ENABLE_PROCESSED_INPUT` 关掉后 Ctrl+C 以 `0x03` 字节到达 CLI 并被当作取消，而不是杀死进程。
退格由 CLI 自己按 UTF-8 续字节回退（ConPTY 与 conhost 都验证了中文口令）。

未验证：真实键盘（两个后端都用 `WriteConsoleInput`/PTY 写入模拟）、其他 Windows 版本、
IME 组合输入。

## 3. `replace_file_atomic` 的三种 Windows 状态

驱动：`probe_rename.py`，原始：`raw-rename-probe.json`。基线：一次普通 `host add` 为 669 ms
（CLI 启动占绝大部分）。

| # | 情形 | 结果 | 判定 |
|---|---|---|---|
| D1 | 目标 `hosts.json` 带只读属性 | 退出码 0；`readonly_before=true`、`readonly_after=false`、配置已更新 | **实测通过**：`clear_readonly_attribute` 生效 |
| D2 | 目标被另一进程以 `FILE_SHARE_READ\|WRITE`（**无** `FILE_SHARE_DELETE`）打开 | 退出码 1；`failed to replace …\hosts.json after 6 retries (another process may still hold it open)` + `拒绝访问。 (os error 5)`；`hosts.json` 字节不变 | **实测通过**：失败可见、旧配置保留 |
| D3 | 同上，但句柄在 300 ms 后释放 | 退出码 0，耗时 674 ms | **实测通过**：退避窗口足够等到短命读者 |
| D4 | 目标被带 `FILE_SHARE_DELETE` 的句柄打开（Rust `File::open` 的形态） | 退出码 0 | **实测**：普通的「另一个 CLI 正在读」不会阻塞替换 |
| D5 | 目录里有残留 `hosts.json.tmp.999999`（模拟崩溃遗留） | 退出码 0；残留文件仍在且未被误删 | **实测通过**：临时文件名含 pid，残留不影响下一次保存 |
| D6 | `icacls` 失败时是否 fail-closed | **未验证**：三种构造手法都被实测排除 | 见下 |

D2 的耗时：1319 ms 总时长 − 669 ms 基线 ≈ **650 ms**，与源码的
`10+20+40+80+160+320 = 630 ms` 退避吻合（差值是一次额外文件操作的开销）。

D6 为什么没测成（三次尝试，都是实测排除，不是没试）：

1. **PATH 前置伪造的 `icacls.exe`**：拦不住。`CreateProcess` 的搜索顺序是 exe 目录 → 当前目录
   → `System32` → `PATH`，`System32\icacls.exe` 始终先命中（`case_icacls_failure` 返回 0、配置被
   更新）。
2. **FAT32 卷**：用 `diskpart` 建 64 MB 固定 VHD、格式化为 FAT32、挂载成盘符，然后
   `icacls E:\probe.txt /inheritance:r /grant:r "<user>:(F)"` —— **返回 0**
   （`Successfully processed 1 files; Failed processing 0 files`）。FAT32 不支持安全描述符，
   但 `icacls` 不把它当失败，因此触发不了 CLI 的错误分支。VHD 用完已 detach 并删除，卷不残留。
3. **deny 自己的 WRITE_DAC**：`icacls file /deny "<user>:(WDAC)"` 之后再改 ACL，仍返回 0。
   文件 owner 隐含 `READ_CONTROL`/`WRITE_DAC`，deny ACE 拦不住 owner 改自己文件的 DACL；而
   `write_private_bytes` 的临时文件由运行用户 `create_new` 创建，owner 就是运行用户。

所以「同一用户在单机上把 `icacls` 弄失败」这条路不成立：真实失败需要「文件不归运行用户所有且
ACL 拒绝 `WRITE_DAC`」「安全软件拦截」「受限令牌去掉写 DACL」或网络卷等条件之一，本轮都做不到。
**源码事实**（不是实测）：
`write_private_bytes` 在 rename **之前**调用 `restrict_file_to_owner(&tmp_path)?`，失败即清理临时
文件、目标不变（fail-closed）；`save_config_inner` 在 rename **之后**再调一次
`restrict_file_to_owner(&path)?`，这一次失败会让命令报错，但配置已经落盘 —— 也就是「权限加固失败
不会回滚已提交的写入」，与「拒绝写入」不是一回事。受限账户下的实际表现列入未验证。

未验证：SMB/网络文件系统上的 rename 语义、真实断电与目录 fsync、其他 Windows 版本。

## 4. D5：Windows 上「标记串不在 argv」怎么断言

`GetCommandLineW()` 只能读**调用进程自己**的命令行，所以它只能证明测试进程（标记从未进过它的
argv）；要证明**被测的 CLI 子进程**，必须读进程表。实测结论：

- `sysinfo` 可以读同用户子进程的 cmdline，但**必须**用
  `refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet))`；
  默认的 `refresh_processes` 不采集命令行为空（第一次实现因此拿不到 `cmd()`，测试失败）。
- 子进程必须还活着：让它在 `--password-stdin` 上阻塞读 stdin 是最稳的窗口。
- 落地为 `tests/host_secret_stdin.rs` 的 `the_secret_never_reaches_the_child_command_line`：
  断言子进程 argv 含 `--password-stdin` 且**不含**标记，同时 `GetCommandLineW` 对测试进程也不含标记。

因此 D5 的答案是：**Linux 用 `/proc/<pid>/cmdline`；Windows 用 `sysinfo` + `refresh_processes_specifics`
（`with_cmd`），`GetCommandLineW` 只能做自身断言**。macOS 仍按 #28 的结论（`KERN_PROCARGS2`），
不在本单范围。

## 5. 本轮发现并修复的缺陷：PowerShell 5.1 管道 BOM 污染口令

**现象（实测）**：Windows PowerShell 5.1 执行 `"口令" | agent2ssh host add … --password-stdin`
时，管道前三个字节是 `EF BB BF`（UTF-8 BOM），CLI 把 `U+FEFF` 当成口令的第一个字符存进库。
后果：用同一个口令去解锁永远失败，而屏幕上看两个值是「一样」的；`$OutputEncoding` 无论怎么设
（ASCII、无 BOM UTF8、带 BOM UTF8）都去不掉第一个 BOM，带 BOM 设置反而会叠成两个。

**修复**（`src-tauri/src/bin/agent2ssh.rs`）：

- `read_redirected_host_secret`：裁掉一个行尾后，若字节以 `EF BB BF` 开头则丢弃这三个字节，
  再做空值判断（纯 BOM 输入因此报 `secret input is empty`，而不是存一个 `U+FEFF` 口令）。
- `read_secret_line` 的非 TTY 分支：`strip_prefix('\u{feff}')`，覆盖 `secrets set-password`
  从重定向 stdin 读新口令的路径。

**回归测试**（`tests/host_secret_stdin.rs::windows_shells`）：

| 测试 | 覆盖 |
|---|---|
| `cmd_echo_pipe_drops_the_crlf_it_adds` | A1 |
| `cmd_redirect_trims_one_ending_and_keeps_the_interior` | A2 + A3 |
| `cmd_pipes_reject_empty_and_invalid_utf8_input` | A4 + A5 |
| `powershell_pipes_arrive_without_a_bom` | B1 + B2（B2 是修复的锚点） |
| `powershell_refuses_angle_bracket_redirection` | B5 |
| `the_secret_never_reaches_the_child_command_line` | D5 |
| `passphrase_stdin_takes_the_same_redirected_path` | §1 的 passphrase 覆盖缺口的回归（cmd 管道 + PS 5.1 管道） |

**反向验证**（证明 B2 的测试不是空转）：临时把剥离条件加上一个环境变量开关，令
`A2S_REVERT_BOM_STRIP=1` 时跳过剥离，重跑：

```
assertion `left == right` failed
  left: "\u{feff}windows-ps51-pipe-marker"
 right: "windows-ps51-pipe-marker"
test result: FAILED. 0 passed; 1 failed
```

恢复后同一条测试通过。开关已删除，不留在源码里。

## 6. 验证边界

**本机测试结果**（Windows 11 10.0.26300，`--no-default-features`）：

| 命令 | 结果 |
|---|---|
| `cargo test --test host_secret_stdin` | **11 passed / 0 failed**（原有 5 + 新增 6）。收尾补的 `passphrase_stdin_takes_the_same_redirected_path` 未在本机跑过，全量应为 12 |
| `cargo test --lib store::` / `secrets::` / `russh_impl::channel` | 29 / 21 / 11 passed |
| `cargo test --lib`（全量） | 856 passed / 4 failed / 1 ignored |
| `cargo clippy --bin agent2ssh --test host_secret_stdin -- -D warnings` | 通过 |
| `cargo fmt --check` | 通过 |

全量 `--lib` 的 4 个失败**不是本轮回归**，也不是凭据路径：

- 3 个 `embedded_ssh::remove_system_known_host_matching::*` 依赖用 `HOME` 把
  `dirs::home_dir()` 指向临时目录，而 Windows 上 `dirs::home_dir()` 走 Known Folder API，
  `HOME` 覆盖无效（`WINDOWS-HANDOVER.md` §5.2 已记）；本机
  `C:\Users\Administrator\.ssh\known_hosts` 不存在，测试因此在真实 home 上找不到文件。
- `an_ipv6_literal_at_port_22_is_removed` 是源码里注释为 "Pinned as a failure while the
  question is open" 的基线红（对应 L2-15）。

顺带修了两处 Windows-only 的基线 clippy 告警（不在本轮功能范围内，但会让 Windows 上的
`clippy -D warnings` 红）：`store.rs` 的 `permissions.set_readonly(false)`（该 lint 针对 Unix
语义，这里只在 `#[cfg(windows)]` 分支编译，加 `#[allow]` 与注释）、`channel.rs` 一个多余的 `mut`。
`--all-targets` 在 Windows 上仍有既有告警（例如 `daemon_integration` 的 `never_loop`），未在本轮处理。

**实测**（本机、本版本）：§1 全部 A/B 用例、§2 全部 C 用例、§3 的 D1–D5、§4 的 sysinfo 手法、
§5 的缺陷复现与修复，以及上表的测试与 lint 结果。

**推断**（有源码依据，未构造实机场景）：`icacls` 失败在受限账户下的 fail-closed 行为（§3 D6）；
SMB 上的 rename 重试是否够用。

**未验证**：其他 Windows 版本（本机 10.0.26300）、受限账户/低完整性令牌、真实键盘与 IME、
真实断电、网络文件系统、`GetCommandLineW` 之外的进程表读法（如 WMI）在其他权限模型下的可见性。
`icacls` 失败路径见 §3 D6：三种构造手法都实测排除，不是「没试」。

## 7. 对 K8 的影响

K8 里「Windows 真机未验证」这一段可以按本轮结果收窄：

- 删掉三项已验证：`SetConsoleMode` 在 ConPTY/conhost 下的无回显、cmd/PowerShell 管道与 CRLF、
  rename 重试与只读属性。
- 保留并改名：`icacls` 在受限账户下的失败行为（未验证）；新增「PowerShell 5.1 的 BOM 已由 CLI
  剥离，5.1 用户不需要设置 `$OutputEncoding`」这条事实（不是限制，是行为说明）。

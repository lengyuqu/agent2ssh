# CNB issue #2 Windows 复测报告（c011b52）

日期：2026-11-09
仓库：`D:\vibercodeing\agent2ssh`（git233 `origin`）
被测提交：`c011b5275204da2a0a015e33bb3f6125ad8d7e76`（`Recognize exited Windows daemon processes`）——HEAD 即该提交
平台：Windows 11 专业版 26H2 / 10.0.26300.9818 / x86_64，**全部为 Windows 原生**，无 WSL 结果

## 1. 结论

`c011b52` 修好了参考报告 §7 记录的 5 项停止测试：`daemon_stop_e2e` 从 **18 passed / 5 failed（76.16 s）** 变为 **23 passed / 0 failed（34.94 s）**，全程无 `exit cannot be verified`。真实 SSH 关闭用例首次在 Windows 上**跑通**（1 项，越过原 `stop_daemon_process` 失败点）。未发现新缺陷；保守分支（查询失败 / 权限不足）已在 2026-11-10 用受限令牌实测，见 §5。

后续复核发现一处归因分歧：复核意见认为上一轮失败属于进程快照层，本机突变实验证明决定层是句柄判据层，见 §5.1。`5af3e8d` 本身只改测试与 CHANGELOG，其新增断言在合并树上通过。

## 2. 基线与仓库状态

| 项 | 值 |
|---|---|
| HEAD 完整 SHA | `c011b5275204da2a0a015e33bb3f6125ad8d7e76` |
| 是否包含修复提交 | 是（HEAD == c011b52） |
| 拉取方式 | `git fetch` → `git merge --ff-only origin/main`，`00b7240..c011b52`，3 个提交（`c73be49`、`d0b9219` merge、`c011b52`） |
| 工作区 | 拉取前后均 clean；未 reset / clean / stash；已有未提交文件：无 |
| 变更规模 | 5 文件 +107/−6（`daemon_control.rs` +61，`store.rs`、`host_secret_stdin.rs`、文档） |

CNB 远端 `https://cnb.cool/zdzso/agent2ssh.git` 在复测时无可用凭据（`ls-remote` 报 `could not read Username`），当时沿用 git233，代码一致。

复测完成后远端出现 `5af3e8d Verify daemon exit through retained child`（只改 `CHANGELOG.md` 与 `daemon_stop_e2e.rs`，不动产品代码），已合并为 `cad69a6`；本报告的结论对应 c011b52 的产品代码，5af3e8d 的新增断言已在合并树上单独验证（§5.1）。

## 3. 环境

| 项 | 版本 |
|---|---|
| OS | Windows 11 专业版 26H2，10.0.26300.9818，x64 |
| CPU / RAM | AMD Ryzen 9 7940H（8C/16T） / 63.2 GB |
| Rust | rustc 1.99.1、cargo 1.99.1 |
| Node / npm | v26.13.0 / 11.20.0 |
| 容器运行时 | Docker 29.9.2（linux engine） |
| SSH 客户端（校验 fixture） | OpenSSH_10.5p1（MSYS2/Git Bash） |

## 4. 逐项结果

所有命令在仓库根目录执行，`AGENT2SSH_CONFIG_DIR=C:\Users\Administrator\AppData\Local\Temp\a2s-retest-config`（集成测试各自再建一次性目录）。默认特性库测试**未设** `TAURI_CONFIG`：先跑 `npm run build` 生成 `dist/`，`build.rs` 在缺 sidecar 时自行注入空 `externalBin`（与参考报告一致）。

| # | 命令 | 结果 | 耗时 | 日志 |
|---|---|---|---|---|
| 1 | `npm run build` | **PASS**（exit 0，vite 14.54 s） | — | `A1-npm-build.txt` |
| 2 | `--no-default-features --lib daemon_control::tests -- --nocapture` | **PASS** 13 passed / 0 failed / 0 ignored（836 filtered） | 0.56 s | `A2-...txt` |
| 3 | `--lib daemon_control::tests -- --nocapture` | **PASS** 13 passed / 0 failed / 0 ignored（853 filtered） | 0.56 s | `A3-...txt` |
| 4 | `--features daemon --test daemon_stop_e2e -- --test-threads=1 --nocapture` | **PASS** 23 passed / 0 failed / 0 ignored | 34.94 s | `A4-daemon_stop_e2e.txt` |
| 5 | `--test exec_fixture -- --test-threads=1`（真实 SSH） | **PASS** 5 passed / 0 failed / 0 ignored | 16.77 s | `B1-exec_fixture.txt` |
| 6 | `--features daemon --test daemon_stop_e2e shutdown_interrupts_an_active_ssh_request -- --exact --nocapture` | **PASS** 1 passed / 0 failed（`running 1 test`，22 filtered） | 2.76 s | `B2-shutdown-ssh-request.txt` |

### 4.1 Windows 专属测试确实执行（不是 0 项）

`--list` 与实际运行都包含，且均为 `ok`：

- `daemon_control::tests::exit_observation_handles_exited_child_with_retained_handle`
- `daemon_control::tests::a_live_process_handle_is_not_signaled`

`daemon_stop_e2e` 在 Windows 上仍是 23 项，`stop_exits_when_exec_websocket_peer_stops_reading` **依旧不存在**（`cfg(unix)`）——0 项不算通过。

### 4.2 参考报告中 5 项失败用例的复测

| 用例 | 上一轮（e4c25a1） | 本轮（c011b52） |
|---|---|---|
| `stop_daemon_process_waits_for_a_graceful_exit` | FAILED | **ok** |
| `stop_closes_an_active_event_stream_and_exits` | FAILED | **ok** |
| `stop_exits_with_an_idle_exec_websocket` | FAILED | **ok** |
| `shutdown_cancels_a_request_waiting_for_approval` | FAILED | **ok** |
| `exiting_old_daemon_preserves_replacement_pid_in_the_same_config` | FAILED | **ok** |

全量日志中 `exit cannot be verified` 出现 **0 次**，`panicked at` **0 次**。总耗时从 76.16 s 降到 34.94 s，与「每项不再耗满 `GRACEFUL_STOP_WAIT = 8 s`」一致。

### 4.3 真实 SSH 关闭路径

fixture：仓库 `scripts/sshd-fixture`（`alpine:3.20`）构建的 `a2s-sshd:issue2`，容器 `a2s-sshd-retest-1794232082`，`127.0.0.1:22222→22`；服务端 **OpenSSH_9.7p1**（OpenSSL 3.3.7），**ed25519 公钥认证**（临时 `ssh-keygen` 密钥，注入 `/root/.ssh/authorized_keys`，700/600），busybox `sh`、`/tmp`、`sleep` 均可用。环境含 `AGENT2SSH_TEST_REQUIRE_SSH=1`。容器与临时密钥在测试后删除，未触碰现有容器或生产主机。

`shutdown_interrupts_an_active_ssh_request` 本轮通过，说明它已经越过上一轮卡住的 `daemon_stop_e2e.rs:436` 停止点，并完整走完断言：`HTTP/1.1 503`、`remote command outcome unknown`、审计 `reason` 同时含本次调用原因与「远端结果未知」、`side_effect == "test effect"`、`change_id == "test-42"`、`daemon.pid` 已由 daemon 自行删除。耗时 2.76 s（上一轮 10.18 s 且失败）。

## 5. 修复实现与保守分支核对

`c011b52` 新增 `process_handle_is_signaled`（Windows）：`OpenProcess(PROCESS_SYNCHRONIZE)` + `WaitForSingleObject(h, 0) == WAIT_OBJECT_0`，**仅在 `snapshot.is_none()` 时**参与 `definitely_missing` 判定；`OpenProcess` 失败（`handle == 0`）直接返回 `false`，不消费子进程退出状态。这正好覆盖上一轮的根因（sysinfo 已移除 + 进程对象仍可打开）。

独立探针 `probe-handle-signaled.rs`（同版本 windows-sys，非仓库代码）在本机结果：

```
access=PROCESS_SYNCHRONIZE (process_handle_is_signaled)
pid 0:     OpenProcess=false GetLastError=87 signaled=false
pid 65534: OpenProcess=false GetLastError=87 signaled=false
pid 4:     OpenProcess=true  GetLastError=0  signaled=false
pid 5168:  OpenProcess=true  GetLastError=0  signaled=false
self:      OpenProcess=true  GetLastError=0  signaled=false
exited child <pid>: OpenProcess=true GetLastError=0 signaled=true  sysinfo_listed=false
child exit status: Some(259)
```

- **已退出的子进程**（退出码 259，`GetExitCodeProcess` 会误认 STILL_ACTIVE）：句柄 signaled=true、sysinfo 不列出 —— 正是修复要判定的情形，独立复现成功。
- **活着的进程**（自身、4、5168 等）：signaled=false —— 不会误判为退出。
- **查询失败**：探针用的是 `PROCESS_SYNCHRONIZE`——与 `process_handle_is_signaled` 相同的访问掩码，**不是** `pid_definitely_missing` 的 `PROCESS_QUERY_LIMITED_INFORMATION`。`pid 0` 与不存在的 `pid 65534` 都得到 `OpenProcess=false, GetLastError=87`，函数按 `handle == 0` 分支返回 false。
- **权限不足（`ACCESS_DENIED`）**：本轮（2026-11-09）未实测，2026-11-10 补上了。用受限令牌（deny-only Administrators + Everyone/Users 限制 SID）启动同一探针，对 `services.exe`（1680）与 `lsass.exe`（1712）的 `OpenProcess(PROCESS_SYNCHRONIZE)` 稳定返回 `ACCESS_DENIED(5)`，函数即为 false；同一子进程对自己仍能打开，排除「令牌本身失效」。三轮一致。日志：`evidence/cnb-issue-2-denied-2026-11-10/P4-restricted-token.txt`。保守链条（`handle == 0 → false` → `pid_definitely_missing` 亦 false → `Unknown` → 保留 PID 并报无法确认）因此是实测行为，不再只有代码依据。

补测方法（2026-11-10）：`probe-restricted-token.rs` 用当前令牌 `CreateRestrictedToken`（deny-only Administrators，限制 SID 取 Everyone、Users、当前用户），再 `CreateProcessAsUser` 启动自身子进程；子进程对 `services.exe`、`lsass.exe` 调 `OpenProcess(PROCESS_SYNCHRONIZE)` 并写结果文件（受限子进程的 stdout 不接到父控制台）。依赖 `windows-sys` 的 `Win32_Security`、`Win32_Security_Authorization`、`Win32_System_Threading`；`cargo run -- 1680 1712` 即可复现。

### 5.1 失败层级归因（核对 CodeBuddy 的 5af3e8d 复核意见）

复核意见认为 `stop_daemon_process` 只经 `daemon_process_start_time` / `daemon_matches_config` / `observable_live_start_time`，不经过 `observe_process`，因此上一轮的失败应归因于进程快照的僵尸过滤，而不是句柄层。**本机实测与该判断相反**：

- 代码路径：`/shutdown/guarded` 被接受后，`stop_daemon_process` 调 `wait_for_exit(pid, start_time, GRACEFUL_STOP_WAIT)`，后者每 50 ms 调一次 `observe_process`；`daemon_process_start_time` 只是进入停止流程之前的前置校验。
- 突变实验（本地临时改动，未提交）：把 `process_handle_is_signaled(pid)` 从 `definitely_missing` 里移除、其余不动，单跑 `stop_daemon_process_waits_for_a_graceful_exit` → **FAILED**，8.70 s 后报 `daemon PID 38980 exit cannot be verified; PID file retained`，与上一轮逐字相同；改回即通过（0.83 s）。日志：`M1-mutation-no-handle.txt`。失败位置是 `daemon_stop_e2e.rs:183`（`stop_daemon_process` 的 `.expect`），不是 `:186` 的 `elapsed < 7s`，即「停止没有等到退出」，不是「等得太久」；同一次编译还打出 `warning: function process_handle_is_signaled is never used`，证明突变确实生效。
- 结论：决定这一层的是 `observe_process` 的 `definitely_missing → process_handle_is_signaled`，不是 `daemon_process_start_time` 的 zombie 过滤；§4.2 的 5 项转绿确实来自 c011b52 的句柄判据。

另：`5af3e8d` 新增的「stop 返回后再用 `try_wait` 检查保留的 Child」断言，在它与 `c011b52` 合并后的树上单跑通过（**1 passed / 0.83 s**），与「断言可合入」的判断一致。

清理两条过时注释（`process_handle_is_signaled` 的头部注释与用例里的 259 说明不再引用代码中没有的 `GetExitCodeProcess`）后，按同一组命令复跑：`daemon_control::tests` 两组各 13 passed、daemon 单元 25 passed、`daemon_stop_e2e` 单线程 **23 passed / 0 failed，35.46 s**（`D1`–`D4` 日志）。改动仅注释与文档，行为不变。

注意 `D4` 那次**没有配置 SSH fixture**：其中的 `shutdown_interrupts_an_active_ssh_request` 与 §4.2 一样按「跳过计通过」处理，日志本身不打印这件事；真实执行结果见 §4.3 的 `B2`（带 fixture，1 passed / 2.76 s）。单看 `D4` 的 23 passed 不能当作该用例在那一轮真跑过。

## 6. 补充：Windows 背压探针（非仓库用例）

按要求，参考报告里的探针结果属旧提交证据，本轮已**重新运行**：`probe-ws-backpressure.mjs` 在 c011b52 上 101 升级 → 首帧 `stdout`（8347 B）→ 对端停读 3 s → `/shutdown/guarded` 200 → daemon **2940 ms** 退出、`daemon.pid` 删除、`app.log` 有 `WebSocket drain deadline expired`、审计有中断记录 → **PASS**（`C1-ws-backpressure.txt`）。

仍须明确：这是**外部探针**，`stop_exits_when_exec_websocket_peer_stops_reading` 在 Windows 上依然不执行，CI 没有该场景的 Windows 回归覆盖。

## 7. 未验证 / 边界

- 连接建立、通道启动、阻塞写入阶段的关闭延迟；`/exec-multi`、本机代理执行、playbook、健康快照等路由（K15 剩余项）——本轮未覆盖。
- GUI（桌面端 Stop 按钮、退出时停 daemon）未跑；Windows 自带 OpenSSH 作服务端未跑；PTY 会话、多轮 keyboard-interactive 未跑。
- 权限不足分支已实测（见 §5，2026-11-10 补）：拒绝时判为「无法确认」并保留 PID，不会误判为已退出。实测用的是受限令牌下的拒绝，不等同于跨账户生产部署的全部情形。
- `process_is_alive` 在 Windows 上仍只依据 sysinfo 快照；对「已退出但句柄仍被持有」的进程返回 `false`。所以 `daemon_stop_e2e` 里的 `assert!(!process_is_alive(pid))` 在 Windows 上只能证明「快照里没有了」——它仍能捕获「daemon 还在跑」（那时 sysinfo 会列出），不是恒真断言，但不能当作句柄层已修复的证据；本轮的证据是 §4.2 的用例转绿与 §5.1 的突变实验。
- 非 PTY 远端命令本地断开后可能继续运行，「结果未知不得自动重试」的语义未因本轮通过而改变。

## 8. 真实用户数据

`~/.agent2ssh` 测试前后 SHA-256 完全一致（`audit.jsonl`、`approvals.jsonl`、`hosts.json`、`known_hosts.json`、`redact_rules.json`、`webhook.toml`、`app.log`），未写入审计/审批/密钥/token；报告中不含任何凭据。测试跑完后被跟踪文件无改动（`git status` 仅剩本报告与证据目录两个未跟踪新增项）。

## 9. 证据索引

`docs/reports/evidence/cnb-issue-2-retest-2026-11-09/`

| 文件 | 内容 |
|---|---|
| `A1-npm-build.txt` | 前端构建输出与退出码 |
| `A2-...txt` / `A3-...txt` | `daemon_control::tests` 两组（含 2 个 Windows 用例） |
| `A4-daemon_stop_e2e.txt` | 23 项全量单线程运行（5 项原失败用例转绿） |
| `B1-exec_fixture.txt` / `B2-shutdown-ssh-request.txt` | 真实 SSH fixture 与关闭路径 |
| `C1-ws-backpressure.txt` | 背压探针本轮结果 |
| `M1-mutation-no-handle.txt` | 突变实验：移除句柄判据后同一用例复现 `exit cannot be verified`（§5.1） |
| `D1`–`D4` | 注释清理后的复跑：`daemon_control::tests` ×2、daemon 单元、`daemon_stop_e2e` 单线程 |
| `probe-handle-signaled.rs` / `.txt` | 句柄 signaled / 保守分支探针与输出 |
| `../evidence/cnb-issue-2-denied-2026-11-10/probe-restricted-token.rs` / `P4-restricted-token.txt` | 受限令牌下的 `ACCESS_DENIED` 实测（2026-11-10 补） |

本轮未提交、未推送，未改动产品代码。原始日志同时保留在 `.workbuddy/issue2-retest/`（gitignored）。

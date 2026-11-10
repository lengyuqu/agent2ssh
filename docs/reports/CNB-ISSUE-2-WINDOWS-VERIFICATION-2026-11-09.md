# CNB issue #2 Windows 真机验证报告

日期：2026-11-09
仓库：`D:\vibercodeing\agent2ssh`（origin `https://git233.phynews.com:18443/workbuddy/agent2ssh`）
被测提交：`e4c25a1f6ef55237e334bb1ac2233068ab1f1bef`（`Preserve SSH shutdown outcomes`）
平台：Windows 11 专业版 26H2 / 10.0.26300.9818 / x86_64（本报告所有结果为 Windows 原生，无 WSL 结果）

## 1. 结论摘要

| 项 | 结果 |
|---|---|
| 基础验证 8 项 | 7 项通过，1 项失败（`daemon_stop_e2e`：18 passed / **5 failed**） |
| 真实 SSH fixture（`exec_fixture`） | 5 passed / 0 failed，真实 SSH 执行通过 |
| 活跃 SSH 请求关闭（仓库测试） | **未通过**：在 `stop_daemon_process` 处失败，未走到 503 断言 |
| 活跃 SSH 请求关闭（产品行为） | **通过**（独立原生探针）：HTTP 503、远端结果未知、审计保留 reason / side_effect / change_id |
| WebSocket 背压关闭 | 仓库测试在 Windows 上**不存在**（`cfg(unix)`）；用原生探针额外验证**通过**，但 CI 无该回归覆盖 |

一句话：issue #2 想修的关闭行为（`/exec` 返回 503 + 审计语义 + WS 背压有界排空）在 Windows 上**行为正确**；但本批新增的「停止前确认进程退出」在 Windows 上**误判为无法确认**，导致 5 个 `daemon_stop_e2e` 用例失败，也让仓库里那个关闭路径用例在 Windows 上跑到一半就中止。

## 2. 基线与仓库状态

| 项 | 值 |
|---|---|
| 分支 | `main` |
| HEAD 完整 SHA | `e4c25a1f6ef55237e334bb1ac2233068ab1f1bef` |
| 是否包含基线提交 | 是（HEAD 与基线同一提交） |
| `git fetch origin` | 成功，`main...origin/main` = `0 0`（无新提交） |
| 工作区 | 拉取前、全部测试后均 `git status --porcelain` 为空；未执行 reset / clean / stash |
| 已有未提交文件 | 无（实测为空） |

**CNB 远端不可读**：`git ls-remote https://cnb.cool/zdzso/agent2ssh` 返回 `could not read Username for 'https://cnb.cool'`（无凭据），`https://cnb.cool/zdzso/agent2ssh/-/issues/2` 返回 404。因此 **issue #2 的原文未能读取**，验证范围依据本次任务说明与 `CHANGELOG.md` 的 `[Unreleased]` 条目（SSH 关闭结果、有界拆除、daemon 退出等）确定。

## 3. 设备与工具版本

| 项 | 版本 |
|---|---|
| OS | Microsoft Windows 11 专业版，26H2，10.0.26300.9818，64 位 |
| 主机 | Machenike DT Computer，AMD Ryzen 9 7940H（8C/16T），63.2 GB RAM |
| Rust | rustc 1.99.1 (58cf8f9c3 2026-10-13)，cargo 1.99.1；`rust-toolchain.toml` 为 stable |
| Node / npm | v26.13.0 / 11.20.0 |
| Git | 2.55.0.windows.5 |
| SSH 客户端（用于校验 fixture） | OpenSSH_10.5p1, OpenSSL 3.5.7（MSYS2/Git Bash 的 `ssh`） |
| Windows 自带 SSH | OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2（未用于本轮测试） |
| 容器运行时 | Docker 29.9.2（linux engine，`docker info` 正常）；另装 wslc 3.0.5.0，本轮未使用 |

## 4. 配置隔离与真实数据保护

- 所有命令统一 `export AGENT2SSH_CONFIG_DIR=C:\Users\Administrator\AppData\Local\Temp\a2s-issue2-config`；`daemon_stop_e2e` 的 `RunningDaemon` 与 `exec_fixture` 各自再建一次性临时目录，测试结束即删除。
- 真实 `~/.agent2ssh` 在验证前后做了 SHA-256 与 mtime 对照，**未变**：

| 文件 | SHA-256（前后一致） |
|---|---|
| `audit.jsonl` | `ddf3015ebec91110dc6144b8d460a612d0daa5ebcdedb38289041615410595ee` |
| `approvals.jsonl` | `0926371db6dca8245e25cf301268f23bf4712a45c0f40f21dddd21a57cfa57ab` |
| `hosts.json` | `ef1a1282a343d3150d39fdbfe3d559420575fa5eb39212f1c0053fc2a36bc6cc` |
| `known_hosts.json` | `2a6fbc9e526228aa7351636c5f694614474cf5f7cb8e715181e79282d5f004a2` |
| `app.log` | mtime `2026-11-02 20:46`、32960 字节，未变 |

- 报告不含任何私钥、token、密码。审计日志中应用自身已把命令里的 `a` 串脱敏为 `<REDACTED:hex>`。

## 5. 基础验证逐项结果

命令均在仓库根目录、Windows 原生执行；日志见 `docs/reports/evidence/cnb-issue-2-2026-11-09/`。

| # | 命令 | 结果 | 耗时 | 日志 |
|---|---|---|---|---|
| 1 | `npm run build` | **PASS**（`tsc` + vite，exit 0） | 17.99 s（vite） | `A1-npm-build.txt` |
| 2 | `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib` | **PASS** 846 passed / 0 failed / 1 ignored | 28.16 s | `A2-lib-no-default.txt` |
| 3 | `cargo test --manifest-path src-tauri/Cargo.toml --lib` | **PASS** 863 passed / 0 failed / 1 ignored | 35.64 s | `A3-lib-default.txt` |
| 4 | `... --no-default-features --features daemon --bin agent2ssh-daemon` | **PASS** 25 passed / 0 failed | 1.10 s | `A4-daemon-bin.txt` |
| 5 | `... --no-default-features --features daemon --test daemon_stop_e2e` | **FAIL** 18 passed / **5 failed** / 0 ignored | 76.16 s | `A5-daemon_stop_e2e.txt` |
| 6 | `... --no-default-features --features daemon --test daemon_integration` | **PASS** 63 passed / 0 failed | 2.02 s | `A6-daemon_integration.txt` |
| 7 | `... --no-default-features --test connect_deadline` | **PASS** 2 passed / 0 failed | 4.07 s | `A7-connect_deadline.txt` |
| 8 | `... --no-default-features --test ssh_operation_deadline` | **PASS** 7 passed / 0 failed | 1.57 s | `A8-ssh_operation_deadline.txt` |

- 被忽略的 1 项两次都是 `transport::russh_impl::agent::tests::pageant_lists_identities`（需要本机运行 agent），与本次改动无关。
- 第 3 项（默认特性）**没有**设置 `TAURI_CONFIG`：当前 `build.rs` 在 `binaries/` 缺 sidecar 时自行注入空 `externalBin`，`dist/` 已由第 1 步生成，因此无需改环境变量；事后 `git status` 为空（`dist/` 与 `target/` 均已 gitignore）。
- 第 5 项用 `--test-threads=1` 复跑一次（`A5b-daemon_stop_e2e-single.txt`），结果完全相同（18/5），排除并发干扰。

## 6. 真实 SSH fixture 与关闭路径

### 6.1 fixture

| 项 | 值 |
|---|---|
| 来源 | 仓库自带 `scripts/sshd-fixture`（`alpine:3.20` + `apk add openssh`） |
| 镜像 | `a2s-sshd:issue2`（本轮构建） |
| 容器 | `a2s-sshd-issue2-1794228941`，`127.0.0.1:22222 -> 22`，本轮结束后删除 |
| 服务端版本 | **OpenSSH_9.7p1**, OpenSSL 3.3.7（`ssh -V` 于容器内） |
| 认证方式 | **ed25519 公钥**（临时 `ssh-keygen` 生成，注入容器 `/root/.ssh/authorized_keys`，权限 700/600）；镜像同时允许密码，本轮未使用 |
| 能力 | `/bin/sh` = busybox、`/tmp` 可写、`sleep` 可用（满足测试要求） |
| 隔离 | 端口独占、容器名带时间戳，未触碰任何现有容器或生产主机 |

环境变量（`AGENT2SSH_TEST_REQUIRE_SSH=1`，禁止静默跳过）：`AGENT2SSH_TEST_SSH_HOST=127.0.0.1`、`PORT=22222`、`USER=root`、`KEY=<临时私钥路径>`。

### 6.2 结果

| 命令 | 结果 |
|---|---|
| `... --no-default-features --test exec_fixture -- --test-threads=1` | **PASS** 5 passed / 0 failed（16.76 s），真实 SSH 执行路径可用 |
| `... --features daemon --test daemon_stop_e2e shutdown_interrupts_an_active_ssh_request -- --exact --nocapture` | **FAIL**：确认执行了 **1 项测试**（`running 1 test`，`22 filtered out`），但失败于 `daemon_stop_e2e.rs:436` 的 `stop_daemon_process`，未走到 503 断言 |

对照组：不设 fixture 时该用例打印 `skipping: SSH fixture not configured` 并以 `ok` 计（0.00 s）——正是 `AGENT2SSH_TEST_REQUIRE_SSH=1` 要防的假通过。基线表里的 `18 passed` 因此含这 1 项「跳过计通过」。

### 6.3 产品行为验证（独立 Windows 原生探针）

由于仓库用例卡在 stop 辅助函数，用探针 `probe-exec-shutdown.mjs` 直接驱动真实 `agent2ssh-daemon.exe`（同一 fixture、同一隔离配置目录）：

| 断言 | 实测 |
|---|---|
| 被中断的 `/exec` 响应 | **HTTP 503**，body 含 `remote command outcome unknown` |
| 审计 reason | `test change; SSH request interrupted during daemon shutdown; remote command outcome unknown` |
| 审计 side_effect | `test effect`（保留） |
| 审计 change_id | `test-42`（保留） |
| 审计 exit_code | `null`（未伪造退出码） |
| daemon 退出 | 212 ms，`daemon.pid` 已由 daemon 自行删除 |

结论：issue #2 的「关闭时区分未发送/结果未知、审计保留原因与副作用、`/exec` 返回 503」在 Windows 上**成立**。

## 7. 失败分析：`daemon_stop_e2e` 5 项

### 7.1 失败清单（错误文案完全一致）

| 用例 | 失败位置 |
|---|---|
| `stop_daemon_process_waits_for_a_graceful_exit` | `daemon_stop_e2e.rs:183` |
| `stop_closes_an_active_event_stream_and_exits` | `:231` |
| `stop_exits_with_an_idle_exec_websocket` | `:291` |
| `shutdown_cancels_a_request_waiting_for_approval` | `:491` |
| `exiting_old_daemon_preserves_replacement_pid_in_the_same_config` | `:568` |

统一错误：`daemon PID <n> exit cannot be verified; PID file retained`（`src-tauri/src/daemon_control.rs:746`）。

其中 `stop_daemon_process_waits_for_a_graceful_exit` 在基线 `9196f11` 就存在；另外 4 项是本批新增。另有 1 项（`shutdown_interrupts_an_active_ssh_request`）见 §6.2，同一根因。

### 7.2 根因

`stop_daemon_process` → `wait_for_exit` → `observe_process` → `classify_process_observation`：

```rust
let definitely_missing =
    snapshot.is_none() && (child_has_exited(pid) || pid_definitely_missing(pid));
```

- `child_has_exited` 在 Windows 上是 `#[cfg(not(unix))] fn child_has_exited(_: u32) -> bool { false }`——**没有** Windows 对应实现，而它正是「父进程从未 wait 过的已退出子进程」这一情形的确认手段。
- `pid_definitely_missing`（Windows）用 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`：只要还能打开句柄就返回 `false`。
- 于是：sysinfo 已经把退出的进程从列表里去掉（`snapshot = None`），而 `OpenProcess` 仍能打开那个**已终止**的进程对象 → `definitely_missing = false` → `Unknown` → 报「exit cannot be verified」。

独立探针（`probe-process-observation.rs`，同版本 sysinfo 0.39.3 + windows-sys，输出见同名 `.txt`）：

```
while the handle is held:
  sysinfo lists the process : false None
  OpenProcess succeeds       : yes (exit code 3 => terminated process object still openable)
after wait() reaped it (+3 s):
  sysinfo lists the process : false None
  OpenProcess succeeds       : yes (exit code 3 => terminated process object still openable)
```

即：**进程确实退出了（exit code 3）**，只是 Windows 上 `OpenProcess` 无法在观测窗口内证明它「已经不在了」。

### 7.3 影响范围（实证对照）

| 调用形态 | 结果 |
|---|---|
| 调用方全程持有 `std::process::Child`（5 个失败用例的 `RunningDaemon` 即此形态，且刻意不 wait） | 必然失败（两次运行、含单线程，5/5 复现） |
| 调用方不持有句柄：`start_daemon_background` 在函数结束即 drop `Child` | `background_start_verifies_and_stops_its_own_daemon` **通过** |
| 真实 CLI：`agent2ssh daemon start` → `status` → `stop` | **通过**：stop 用时 1201 ms，`daemon.pid` 删除，进程消失 |
| 桌面端 `daemon_stop` / `daemon_restart` | 走 `stop_daemon_process`，启动时已 drop 句柄（与上一条同形态），**未单独跑 GUI**；按代码路径与 CLI 同形态，风险为「若将来保留句柄则复现」 |

**分类**：产品在 Windows 上的缺口（缺少「已退出但未回收的子进程」判据），由测试的调用形态触发；不是环境问题（daemon 确实退出、pid 文件确实删除），也不只是测试写法问题（用例的断言本身合理）。**CI 影响**：`.github/workflows/ci.yml:324` 在 `windows-latest` 上运行该 target，因此该提交的 Windows job 会红。

### 7.4 建议（未改任何代码）

1. 给 Windows 补上 `child_has_exited` 的等价实现：对持有的子进程句柄 `WaitForSingleObject(h, 0) == WAIT_OBJECT_0` 即视为已退出；或在 `pid_definitely_missing` 中区分「能打开但已终止」——`OpenProcess` 成功后用 `GetExitCodeProcess` 判断，`!= STILL_ACTIVE` 视为已退出（探针已证明这条判据可用）。
2. 之后 5 个用例与 §6.2 的关闭用例应可全部转绿；`GRACEFUL_STOP_WAIT = 8s` 的等待会立刻结束，而不是每次都耗满 8 秒。
3. 若短期不修产品，则应把 `daemon_stop_e2e` 中「持有 Child」的用例在 Windows 上显式标注，避免把已知平台缺口算成环境噪声——但这会把 Windows 关闭路径的真实回归风险留在 CI 里，不推荐。

## 8. 验证边界（不得扩大结论）

- **`stop_exits_when_exec_websocket_peer_stops_reading` 在 Windows 上不存在**：`daemon_stop_e2e` 在 Windows 只有 23 项，`--list` 中无该名字（`#[cfg(unix)]` 排除）。Windows 上「0 项测试」不构成背压验证。
- **额外做了 Windows 原生背压验证**（探针 `probe-ws-backpressure.mjs`）：101 升级成功 → 首帧 `stdout`（8347 字节）→ 对端暂停读取 3000 ms → `/shutdown/guarded` 返回 200 → daemon **2938 ms** 退出、`daemon.pid` 删除 → `app.log` 出现 `WebSocket drain deadline expired` → `audit.jsonl` 出现 `daemon shutdown interrupted stream; remote outcome unknown`。**但这是外部探针，不是仓库用例**，CI 上仍无该场景的 Windows 回归。
- 与 `cfg(unix)` 用例的差异须记录：Node 没有 `SO_RCVBUF` 入口，探针是靠命令大量输出把套接字写满，而不是先缩小接收窗口；结论一致（有界排空 + 诊断 + 审计），机制细节不完全等价。
- **仍未验证**：连接建立阶段、通道启动、阻塞写入等其他阶段的关闭延迟（K15 明列的剩余路径）；`/exec-multi`、本机代理执行、playbook、健康快照等路由；GUI（桌面端 Stop 按钮、退出时停 daemon）；Windows 自带 OpenSSH 作为服务端；PTY 会话；多轮 keyboard-interactive。
- 非 PTY 远端命令在本地断开后**可能继续运行**：本轮 `sleep 30` 被中断后未验证远端是否终止，因此「结果未知」的正确处置仍是不自动重试。
- 本轮全部为 Windows 原生；**不含任何 WSL/Linux 结果**，也没有把容器内 Linux 结果当作 Windows 结论。

## 9. 复现步骤

```bash
cd /d/vibercodeing/agent2ssh
git rev-parse HEAD                     # e4c25a1f6ef55237e334bb1ac2233068ab1f1bef

# 隔离配置 + 基础套件
export AGENT2SSH_CONFIG_DIR="$(cygpath -w "$HOME/AppData/Local/Temp/a2s-issue2-config")"
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test connect_deadline
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test ssh_operation_deadline

# fixture
docker build -t a2s-sshd:issue2 scripts/sshd-fixture
docker run -d --name a2s-sshd-issue2 -p 127.0.0.1:22222:22 a2s-sshd:issue2
ssh-keygen -t ed25519 -N '' -f /tmp/a2s-key
docker exec -i a2s-sshd-issue2 sh -c 'mkdir -p /root/.ssh && chmod 700 /root/.ssh && cat > /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys' < /tmp/a2s-key.pub

export AGENT2SSH_TEST_SSH_HOST=127.0.0.1 AGENT2SSH_TEST_SSH_PORT=22222 \
       AGENT2SSH_TEST_SSH_USER=root AGENT2SSH_TEST_SSH_KEY="$(cygpath -w /tmp/a2s-key)" \
       AGENT2SSH_TEST_REQUIRE_SSH=1
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test exec_fixture -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_stop_e2e shutdown_interrupts_an_active_ssh_request -- --exact --nocapture
#   → FAILED at daemon_stop_e2e.rs:436: "exit cannot be verified; PID file retained"

# 产品行为（不经过 stop_daemon_process）
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh
node docs/reports/evidence/cnb-issue-2-2026-11-09/probe-exec-shutdown.mjs "<repo>/src-tauri"
node docs/reports/evidence/cnb-issue-2-2026-11-09/probe-ws-backpressure.mjs "<repo>/src-tauri"
```

## 10. 证据索引

`docs/reports/evidence/cnb-issue-2-2026-11-09/`

| 文件 | 内容 |
|---|---|
| `A1-npm-build.txt` … `A8-ssh_operation_deadline.txt` | 8 项基础命令完整输出（含 `test result` 行、失败 backtrace） |
| `A5b-daemon_stop_e2e-single.txt` | 单线程复跑，证明 5 项失败与并发无关 |
| `B1-exec_fixture.txt` / `B2-shutdown-ssh-request.txt` | 真实 SSH fixture 结果与关闭用例失败现场 |
| `C1-exec-shutdown-503.txt` / `C2-ws-backpressure.txt` | 两个原生探针输出（PASS） |
| `probe-exec-shutdown.mjs` / `probe-ws-backpressure.mjs` | 探针源码（可直接复跑） |
| `probe-process-observation.rs` / `.txt` | `OpenProcess` / sysinfo 观测实验与输出 |
| `SUMMARY.txt` | 逐条命令的退出码与计数 |

本轮未改动任何产品代码，也未触发任何真实数据写入（§4）。

证据日志以 `.txt` 存放：仓库 `.gitignore` 有全仓 `*.log` 规则，`git add` 默认不收录；改为 `.txt` 是为了让它们能随报告一起被审阅，而不是绕过忽略规则强推。


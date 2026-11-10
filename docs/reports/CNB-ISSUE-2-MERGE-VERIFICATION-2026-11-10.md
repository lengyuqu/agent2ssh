# 合并后 Windows 关闭路径复验（fbea044）

日期：2026-11-10
基线：`fbea044c2e98010138cb51f91511a36f199b4751`（`Merge the expect_pid stop protocol`）
平台：Windows 11 专业版 26H2 / 10.0.26300.9818 / x86_64，Rust 1.99.1，Node v26.13.0，Docker 29.9.2

## 1. 为什么重跑

合并（`fbea044`）把关闭协议从 `/shutdown/guarded` + `X-Agent2SSH-Expected-Pid` 换成 `/shutdown` 的 `expect_pid`。`docs/reports/evidence/` 里的旧探针仍打旧路由，对新 daemon 会 404；仓库里两个协议竞态用例是 `#[cfg(unix)]`，Windows 不执行。因此本轮用新探针在 Windows 原生复验三条路径。

## 2. 结果

| # | 探针 | 断言 | 结果 |
|---|---|---|---|
| P1 | `probe-exec-shutdown.mjs` | 活跃 `/exec` 被关闭打断 → HTTP 503 + `remote command outcome unknown`；审计保留 reason / side_effect / change_id；pid 文件删除 | **PASS**，214 ms 退出 |
| P2 | `probe-ws-backpressure.mjs` | `/exec/stream` 对端停读 3 s → 关闭仍有时限；`WebSocket drain deadline expired` + 中断审计 | **PASS**，2936 ms 退出 |
| P3 | `probe-expect-pid.mjs` | `expect_pid` 指向别的进程 → 409 且点名实际 PID；`expect_pid: null` → 400；daemon 存活；正确 PID → 200 并退出 | **PASS**，114 ms 退出 |

关键输出：

```
P1: shutdown HTTP 200 {"ok":true} / 503 / reason="test change; SSH request interrupted during daemon shutdown; remote command outcome unknown"
P2: first frame stdout 8347B / pause 3000ms / exit 2936ms / 1 drain-deadline record / 1 interrupted-stream audit
P3: wrong pid HTTP 409 {"error":"this daemon is pid 32684, not the pid 32685 the request named; refusing to shut down"}
    null field HTTP 400 {"error":"expect_pid must be a PID when present; omit the field to stop without a check"}
    right pid HTTP 200 {"ok":true} / exit 114ms / pid file removed
```

fixture：`scripts/sshd-fixture`（alpine 3.20）构建的镜像，`127.0.0.1:22222`，**OpenSSH_9.7p1**，ed25519 临时密钥，`AGENT2SSH_TEST_REQUIRE_SSH=1`；容器与密钥已删除，未触碰现有容器。

## 3. 覆盖与未覆盖

- 已覆盖（Windows 原生）：新协议下 `/exec` 中断语义、WebSocket 背压关闭、`expect_pid` 的拒绝/空值/接受三条分支（含 409 回读实际 PID）。
- 未覆盖：`a_stop_that_reached_the_wrong_daemon_stops_nothing`（真实 A/B + 代理换监听者）与 `a_daemon_that_ignores_the_expected_pid_stops_and_says_so`（旧 daemon 忽略字段）仍只在 unix 跑；Windows 侧由 P3 覆盖「拒绝」分支，但不等于覆盖「监听者被替换」那条竞态。
- 旧 daemon 忽略 `expect_pid` 时仍会关闭，客户端只记 warn —— 这是本次合并刻意接受的行为，不是回归。
- `config_dir` 比对：健康响应报出的目录与调用方不一致时停止会被拒绝（仓库用例 `a_daemon_serving_another_config_directory_is_not_stopped` 在 Windows 通过）。跨机器共享同一配置目录、但路径字符串不同的部署会被判为不一致而拒绝；该场景未实测。

## 4. 复跑

```bash
docker run -d --name a2s-sshd -p 127.0.0.1:22222:22 a2s-sshd:issue2
# 注入临时 ed25519 公钥到 /root/.ssh/authorized_keys（700/600）
export AGENT2SSH_TEST_SSH_HOST=127.0.0.1 AGENT2SSH_TEST_SSH_PORT=22222 \
       AGENT2SSH_TEST_SSH_USER=root AGENT2SSH_TEST_SSH_KEY=<临时私钥> \
       AGENT2SSH_TEST_REQUIRE_SSH=1
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
node docs/reports/evidence/cnb-issue-2-merge-2026-11-10/probe-exec-shutdown.mjs  src-tauri
node docs/reports/evidence/cnb-issue-2-merge-2026-11-10/probe-ws-backpressure.mjs src-tauri
node docs/reports/evidence/cnb-issue-2-merge-2026-11-10/probe-expect-pid.mjs      src-tauri   # 不需要 fixture
```

## 5. 证据

`docs/reports/evidence/cnb-issue-2-merge-2026-11-10/`

| 文件 | 内容 |
|---|---|
| `probe-exec-shutdown.mjs` / `P1-exec-shutdown.txt` | 新协议下的 503 与审计断言 |
| `probe-ws-backpressure.mjs` / `P2-ws-backpressure.txt` | 新协议下的背压关闭 |
| `probe-expect-pid.mjs` / `P3-expect-pid.txt` | `expect_pid` 的 409 / 400 / 200 三条分支 |

本报告与探针为新增文件；产品代码未改动。

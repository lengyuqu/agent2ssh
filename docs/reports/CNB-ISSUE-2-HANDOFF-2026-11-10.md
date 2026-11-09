# issue #2 交接（2026-11-10）

换设备继续时先看这一页。范围：CNB issue #2 在 Windows 侧的验证与停止协议收尾。

## 1. 当前状态

| 项 | 值 |
|---|---|
| main / origin / cnb | `26c77b423ff2982507ad6bb1c5120d139b7dd8ce`（三方一致） |
| issue #2 | open，54 条评论，最后一条是 2026-11-09T18:02:44Z 的收尾回复 |
| 工作区 | 干净；`main` 无未推送提交 |
| 产品代码 | 本轮未改动（只动文档、证据、探针） |

## 2. 已经完成的

| 时间 | 提交 | 内容 |
|---|---|---|
| 11-09 | `c011b52` | Windows 退出判据 `process_handle_is_signaled`（句柄 signaled 才判退出） |
| 11-09 | `0700b24` / `aee054e` | Windows 实机复测报告与证据；层级归因经突变实验纠正 |
| 11-09 | `fbea044` | 合并 `expect_pid` 停止协议（`/shutdown` + body，删 guarded 路由） |
| 11-09 | `aa49f12` | 合并后三条原生探针（503 / 背压 / expect_pid） |
| 11-10 | `9b4cadf` / `26c77b4` | `ACCESS_DENIED` 分支实测 + 实测范围收窄为「第一环」 |

关键结论：5 项原失败用例在 Windows 全绿（`daemon_stop_e2e` 22 passed 单线程），真实 SSH 关闭路径 1 passed / 2.73 s。

## 3. 仍未解决

- K14 的四项：pidfd 之类的进程身份绑定、PID 复用、共享令牌伪造、检查与发信号非原子。
- `a_stop_that_reached_the_wrong_daemon_stops_nothing`、`a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 是 `#[cfg(unix)]`，Windows 不执行（CI 的 windows job 也无此覆盖）。
- `signal_daemon` 在快照缺失时静默 `Ok(())`：调用点随后会报 `StillRunning`/`UncertainExit`，不会假成功，但签名分不清「发了」与「跳过」，要改需单独决策。
- `config_dir` 比对：健康响应报的目录与调用方不一致即拒绝停止；跨机器共享同一配置目录、路径字符串不同的部署会被误拒，未实测。
- 合并后的旧 daemon 行为：忽略 `expect_pid` 仍会关闭，只记 warn —— 这是刻意接受的降级。

## 4. 怎么继续

```bash
# 读最新评论（默认只给最旧 10 条，必须倒序）
cnb issues list-comments --repo zdzso/agent2ssh --number 2 --sort "-created" --page-size 5
# 回帖（body-file 必须是 Windows 路径，MSYS /tmp 不行）
cnb issues comment --repo zdzso/agent2ssh --number 2 --body-file "C:/path/to/reply.md"

# 推送 CNB（token 作密码，用户名固定 cnb.lengyuqu）
export CNB_GIT_TOKEN=$(python -c "import json,io,os;print(json.load(io.open(os.path.expanduser('~/.cnb/token'),encoding='utf-8'))['auths']['cnb.cool']['access_token'])")
export CNB_GIT_USER=cnb.lengyuqu GIT_ASKPASS=<应答脚本> GIT_TERMINAL_PROMPT=0
git push cnb main

# Windows 原生回归
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib daemon_control::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon_control::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1
```

真实 SSH fixture：`docker run -d --name a2s-sshd -p 127.0.0.1:22222:22 a2s-sshd:issue2`，注入临时 ed25519 公钥到 `/root/.ssh/authorized_keys`（700/600），再设 `AGENT2SSH_TEST_SSH_PORT=22222`、`AGENT2SSH_TEST_SSH_KEY`、`AGENT2SSH_TEST_REQUIRE_SSH=1`。

CodeBuddy 回帖延迟 60–90 秒；它是 `cnb/issue.comment@npc` 流水线，只对「@ 它」的评论回帖，不会自己开 issue。

## 5. 待决策

1. issue #2 是否关闭：K14 四项未动，建议保持 open。
2. 「持续 NPC 循环」是否落地：会话内循环（发帖→等 1–2 分钟→修→推，N 轮）还是定时自动化（每 N 小时一轮，带标签与轮次上限）。
3. `signal_daemon` 是否改成能区分「已发信号 / 已跳过」的返回值。

## 6. 证据位置

- `docs/reports/CNB-ISSUE-2-WINDOWS-VERIFICATION-2026-11-09.md`（首轮，5 项失败现场）
- `docs/reports/CNB-ISSUE-2-WINDOWS-RETEST-2026-11-09.md`（修复后复测，含层级归因与保守分支）
- `docs/reports/CNB-ISSUE-2-MERGE-VERIFICATION-2026-11-10.md`（合并后的三条探针）
- `docs/reports/evidence/` 下四个目录：`cnb-issue-2-2026-11-09/`、`cnb-issue-2-retest-2026-11-09/`、`cnb-issue-2-merge-2026-11-10/`、`cnb-issue-2-denied-2026-11-10/`

已归档的旧探针仍打 `/shutdown/guarded`，对新 daemon 会 404 —— 它们记录的是合并前那次运行，不要直接复用。

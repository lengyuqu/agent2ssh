# L2 待拍板清单（优化轮次）

`src-tauri/src/**` 的行为/签名改动、Windows 语义改动，按消费任务的分级规则**不进 main**，攒在这里等人工决策。每条都写清为什么不能自动落、以及落地时的验证要求。

---

## 2026-11-10 · 来自 issue #2 的消费（文档行为一致性）

### L2-1 诊断文案与它描述的控制流不符

`src-tauri/src/daemon_control.rs:1019-1024` 的 warn 文案是：

```
"daemon did not exit after a guarded shutdown; leaving it running"
```

但紧接着 `:1025` 就 `signal_daemon(pid, start_time, true)` 强制终止，并再 `wait_for_exit(2s)`。文案说「保持运行」，实际是「先杀一次，2 秒后还活着才放弃」。同一次消费已经修掉两处受此误导的用户文档（`docs/guides/configuration-guide.md:377`、`docs/KNOWN-ISSUES.md` K15），但日志本身还在说反话——查日志的人会得到与文档修正相反的信息。

**为什么归 L2**：改动落在 `src-tauri/src/**`。消费任务的自动验证口径只有 `--no-default-features` 的 lib 测试，而这条改动要完整验证需跑 default feature（依赖 `dist/` 与 `src-tauri/binaries/<name>-<triple>` 侧车），超出自动路径。

**建议改法**：文案改为描述实际三步，例如
`"daemon did not exit after a guarded shutdown; signalling it by force"`；
并把 force 之后的第二次 `wait_for_exit` 结果另记一条诊断（已退出 / 仍在运行），让「杀了但没杀掉」在日志里可区分，而不是只能从返回码反推。

**验证要求**：`npm run build` + `./scripts/prepare-sidecars.sh` 后跑 `cargo test --manifest-path src-tauri/Cargo.toml --lib`（default feature，唯一编译 `tauri_commands` 测试的目标）+ `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`。

### L2-2 `does not report its config directory` 分支无断言

`daemon_control.rs:970` 的拒绝分支（旧 daemon 的 `/health` 不报 `config_dir`）现在没有任何测试钉住：唯一覆盖它的 `legacy_daemon_without_pid_guard_is_not_automatically_stopped` 已由 `f373f2c` 删除，替代品全是「token 被拒」分支。语义上这条分支意味着旧 daemon 在升级前**停不了**，属真实限制，但只有代码可读。

**为什么归 L2**：要补的是 `tests/daemon_stop_e2e.rs` 里的断言。虽然该文件在 `tests/**` 而非 `src/**`，但它需要起真实 daemon 进程验证旧版健康响应，且断言内容涉及「旧 daemon 应被拒绝还是要兼容」的语义选择——那是产品决策，不是自动流程能拍的。

**建议改法**：加一条用例，构造一个 `/health` 响应不含 `config_dir` 的假 daemon（现有 fixture 支持子命令注入），断言 `stop_daemon_process` 返回 `UnverifiedIdentity` 且 message 含 `does not report its config directory`，且 PID 文件保留。

**验证要求**：`cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1`，并在 Windows 腿确认这条分支在 Windows 上同样可达（当前该文件在 GitHub Actions 的 `build` 矩阵里三平台都跑）。

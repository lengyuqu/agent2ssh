# CNB #25：macOS 对端 PID 与 kqueue 补充验证

基线 `17068ff`；macOS 27.2.1 (26B131)、arm64、uid 501、SIP enabled，普通非沙箱终端。运行三轮；只使用临时 Unix socket 和探针创建的子进程，不访问真实 `~/.agent2ssh`，对 pid 1 只调用 `kill(pid, 0)` 与事件注册，未发送终止信号。完整环境、逐轮 PID 和原始输出见 [output.txt](evidence/macos-identity-followup-2026-11-12/output.txt)，可复算源码见 [probe.c](evidence/macos-identity-followup-2026-11-12/probe.c)：

```sh
clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror -O1 \
  docs/reports/evidence/macos-identity-followup-2026-11-12/probe.c \
  -o /tmp/agent2ssh-issue25-probe
/tmp/agent2ssh-issue25-probe
```

## 结果

1. **kqueue 标志组合影响权限结果。** 对 pid 1，单独 `NOTE_EXIT` 或单独 `NOTE_EXITSTATUS` 均返回 `EV_ERROR/data=0`；两者同时设置时返回 `EV_ERROR/data=13`（`EACCES`）。同一目标 `kill(pid, 0)` 返回 `EPERM`。对自身与仍存活的直接子进程三种注册都成功；不存在的 `INT_MAX` PID 三种注册都返回 `ESRCH`。这三轮结果一致。`EV_RECEIPT` 在这些案例中通过事件 `data` 返回结果；这不排除其他情形下系统调用自身失败。
2. **客户端确实可以在已连接 Unix socket 上读取对端 PID，但它不是稳定的监听者身份。** 父进程作为客户端连接子进程服务端；服务端 accept 后读取到父进程 PID；客户端在 accept 后及服务端写回复后读取到服务端 PID。随后服务端 fork 子进程，子进程持有继承的已连接 fd 并发出回复、保持存活，客户端读取到的是该子进程 PID。探针故意让子进程一直存活到读取完成；这三轮均一致。
3. **方向错误的方案已更正。** `expect_pid` 是调用者希望停止的 daemon PID；服务端对端凭据对应的是客户端 PID，不能要求两者相等。客户端侧对服务端 PID 的观察可作为附加证据，但 fd 继承、传递、代理和 PID 复用仍会破坏“监听者即被验证进程”的假设，不能单独证明绑定。
4. **本地响应解析。** 新增 `local_health_response_parser_does_not_enforce_content_length`，通过真实 loopback TCP 调用 `direct_daemon_request`：只有响应头中带 JSON 时 body 为空；声明 `Content-Length: 0` 后追加 JSON 时，该 JSON 仍出现在 body 并可解析。测试经过停止路径共享的响应解析器，但没有调用 `stop_daemon_process`，不证明是否实际发送 `/shutdown`。
5. **回归基线。** `AGENT2SSH_CONFIG_DIR=$(mktemp -d) cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1`：34 passed / 0 failed / 34.82 s。包括真实 daemon A/B 监听切换、旧协议桩和 PID 文件相关回归；测试文件中的 SSH fixture 案例在未配置 fixture 时会跳过其真实 SSH 部分，不能据此声称本轮验证了真实 SSH 中断。

## 边界及后续

- 本机的 pid 1 案例表明双标志与单标志检查不同，**没有**证明任意 EPERM 目标都会返回 EACCES，也没有证明双标志成功后随时可以发送 SIGKILL：父子关系、权限变化、进程退出/PID 复用、sandbox 与 MACF 均未穷举。kqueue 引用不是可用于发信号的 pidfd；检查与 `kill(pid, ...)` 不原子。
- fd 继承是 fork 场景；先前 `SCM_RIGHTS` 实验是另一种传递方式。没有验证被继承 fd 的所有读写顺序、代理链、其他 macOS 版本或受限 entitlement，也没有实现新的 IPC 协议。
- `daemon_activity::request_at` 当前按第一个空行切分响应，未按 `Content-Length` 界定 body；上面的 H1/H2 分别已由实际共享解析器测试；**尚未以完整停止客户端验证是否发送 shutdown**。
- 进程起始时间改精度、自动强杀策略、版本兼容及是否采用 IPC 仍依赖 #22；旧秒值乘一百万不能恢复其丢失的微秒。本轮不改变生产停止行为，不代表 #25 的端到端验收完成。

## 提交前复核

再次编译并运行探针，检查 PID 对应关系与 pid 1 的各标志结果，与归档中的三轮一致。隔离配置并显式移除 SSH fixture 环境变量后，完整 `daemon_stop_e2e` 为 **35 passed / 0 failed / 34.98 s**，包括新增解析器测试；SSH 中断案例的真实 SSH 部分按代码提前跳过，因此 35 并非 35 个完整实机场景。修正了此前把 pid 1 的 `PROC_PIDTBSDINFO` 结果泛化到所有其他 uid、以及将启动／状态判定简化为只有 health JSON 的说法。

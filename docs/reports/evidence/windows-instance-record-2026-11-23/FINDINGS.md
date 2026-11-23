# Windows 实例记录 + 100 ns 绑定（CNB #24 阶段二）真机证据

- 日期：2026-11-23
- 机器：Windows 11 26300（x64），rustc/cargo 1.100.0
- 分支：`feat/windows-daemon-identity-phase2`（基于 main `d02c2d5`，v0.5.0）
- 范围：版本化 Windows 实例记录（原始 100 ns 创建时间）收敛同整秒 PID 复用边界。命名管道停止协议**不在本次范围**（v1 设计 §4.2：无可证明的本机控制端点，Windows 保持 `Unbound`；D4 禁止两条并存的自动停止协议）。

## 改动要点

1. `src-tauri/src/daemon_identity/windows.rs`（新增）：`Instance { version, instance_id, pid, config_id, creation_ticks }`，serde `deny_unknown_fields` + 校验；`publish` 在 `.daemon-pid.lock` 下先发 PID 再发记录，拒绝覆盖已有记录；`cleanup_locked` / `recover` / `remove_if_named` 都要求逐字节匹配才删，先删记录再删 PID。
2. `daemon_control.rs`：`ExpectedCreation::{Seconds, Ticks}` 把分辨率带进 `BoundProcess`，`terminate` 按同一精度复核；停止路径读记录、与观测 start_time 交叉核对（不一致 → 拒绝、保留文件），有记录按 ticks 绑定，无记录（旧 daemon）保持秒级；记录损坏/冲突 → `UnverifiedIdentity` 拒绝，不降级。
3. daemon 启动发布记录、退出条件清理；CLI `clear-stale --expect-instance` 放开到 Windows。
4. 记录是**发现证据，不是凭据**：不新增停止协议，`/shutdown` 与强杀的授权面不变。

## 关键行为验证（同整秒边界的显式翻转）

`a_same_second_daemon_image_reuse_is_refused_by_the_tick_record`（`daemon_control.rs` 单元测试）：重试构造同整秒、不同 ticks 的两个 daemon 命名进程对——

- `Ticks(first_ticks)` 绑定第二个进程 → 拒绝（阶段一按秒比较时这里会绑上，正是要收敛的洞）；
- `Ticks(second_ticks)` → 绑定成功；
- `Seconds(recorded_secs)` → 仍能绑上，如实记录旧 daemon（无记录）路径的秒级边界。

配套 `a_tick_binding_refuses_other_ticks_and_terminates_its_own_object`：ticks+1 拒绝；`terminate_for_stop` 只杀自己绑定的对象。

## 真机回归结果（全部通过）

| 套件 | 命令 | 结果 |
| --- | --- | --- |
| 库单元（无默认特性） | `cargo test --no-default-features --lib` | 903 通过，1 忽略 |
| 身份/绑定定向 | 同上，过滤 identity/binding 相关 | 25 通过 |
| 停止路径 e2e（真实 daemon 进程） | `cargo test --no-default-features --features daemon --test daemon_stop_e2e` | 34 通过，1 忽略（45.65 s） |
| daemon 二进制单元 | `cargo test --no-default-features --features daemon --bin agent2ssh-daemon` | 31 通过 |
| 库单元（默认 tauri 特性） | `cargo test --lib` | 921 通过，1 忽略（42.19 s） |
| CLI/MCP 冒烟 | `cargo test --no-default-features --test cli_smoke` | 41 通过 |
| daemon 集成（镜像路由） | `cargo test --no-default-features --features daemon --test daemon_integration` | 86 通过 |

e2e 新增三项（`tests/daemon_stop_e2e.rs`，均真实 daemon + 隔离配置目录）：

- `the_instance_record_names_the_daemon_and_a_normal_stop_removes_both_files`：记录字段逐一核对（version/pid/32 位 hex instance_id/config_id 文件一致/ticks 等于 `GetProcessTimes` 原始值），正常停止后两个文件都消失。
- `a_crashed_publication_refuses_a_new_daemon_until_explicit_recovery`：SIGKILL 残留 → 竞争 daemon 启动被拒（stderr 含 "instance record exists"）；无 `--expect-instance` 的 clear-stale 被拒；错误 instance id 被拒且文件保留；正确恢复后新 daemon 以新 instance_id 启动，优雅停止再清两文件。
- `a_corrupt_instance_record_refuses_the_stop_and_is_retained`：截断记录 / pid+1 记录都让停止拒绝（错误含 "cannot be verified" 与 "clear-stale"），daemon 存活、文件保留。

竞态测试 `a_stop_that_reached_the_wrong_daemon_stops_nothing` 在 Windows 分支补了显式恢复（镜像 macOS），顺带在 e2e 里再走一遍恢复路径。

## 环境备注

- 本机 cargo 需 `NO_PROXY="$NO_PROXY,192.168.77.6"` 前缀，否则沙箱代理劫持内部 crates 镜像返回 405。
- 修了一个被新测试暴露的既有竞态：`spawn_sleeper` 并行复制 ping.exe 到同一路径触发共享冲突（OS error 32），改为每次调用独立目录。
- FILETIME→Unix 秒换算（ticks/10^7 − 11 644 473 600）与 sysinfo `start_time` 在本机 24/24 一致。
# CNB issue #24 step 0：Windows 进程创建时间原语实测

日期：2026-11-12
仓库：`C:/Users/Administrator/WorkBuddy/Worktrees/agent2ssh/main-508a73ff`（`workbuddy/main-508a73ff`，HEAD `f98bd83`）
平台：Microsoft Windows 11 专业版 10.0.26300，AMD64，**Windows 原生**（无 WSL、无容器）
工具链：`cargo 1.99.1`、`rustc 1.99.1`、`sysinfo 0.39.3`、`windows-sys 0.48.0`
证据：`docs/reports/evidence/windows-identity-2026-11-12/`（`probe.rs`、`Cargo.toml`、`run1.txt`~`run4.txt`）

## 1. 这一轮回答的唯一问题

#24 的评论把这条列为「设计前置里最便宜、也最应该先做的一步」：`GetProcessTimes().ftCreationTime` 与 `sysinfo::Process::start_time()` 在 Windows 上**是不是同一个量**。

不成立的话，所有「句柄 + 创建时间」的组合在真机上都会退化成 `Unverified`；成立的话，才能判断秒级截断损失了多少判别力。

## 2. 方法

探针对一个进程同时用两条路径取创建时间：

| 路径 | 取值 |
|---|---|
| 内核 | `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 或 `GetCurrentProcess()` → `GetProcessTimes().ftCreationTime` |
| 库 | `sysinfo::System::refresh_processes` → `Process::start_time()` |

对照量：`ft_unix_secs = ft_raw_100ns / 10_000_000 - 11_644_473_600`（FILETIME 1601 纪元 → Unix 1970 纪元）。

四组观察：自身进程（1 个）、同一秒内连续 spawn 的 5 个子进程、退出后仍持有句柄的进程、以及「5 个子进程能否被 `start_time` 区分」。整套跑 4 轮。

## 3. 结论

### 3.1 换算关系成立（可重复）

4 轮 × 6 个进程（自身 + 5 子进程）共 24 个样本，`sysinfo start_time - ft_unix_secs` **全部为 0**：

```
distinct deltas      [0]      # run1
distinct deltas      [0]      # run2
distinct deltas      [0]      # run3
distinct deltas      [0]      # run4
sysinfo == ft_unix_secs (truncated): 5 match / 0 mismatch
```

即：

```
sysinfo::Process::start_time() == ftCreationTime / 10_000_000 - 11_644_473_600
```

**这条可直接用**：从持有句柄取 `GetProcessTimes`，结果与 `OwnerMarker.start_time`、`daemon.pid` 配套的秒级值同量纲，不需要任何单位换算表。

### 3.2 精度损失是真实的，不是理论担忧

`ftCreationTime` 的亚秒余数每轮都非零：

```
ft_subsec_ticks      4330353   # 自身进程，≈0.43 s
ft_frac  4701477 / 4846366 / 4883192 / 4907533 / 4931986   # 5 个子进程
any subsec ticks     true
```

内核给的是 100 ns 计数且**确有非零低位**；`sysinfo` 把它截断到整秒。所以「用 100 ns 做不变量」不是把秒乘以 10^7 就能得到的——低位信息只存在于 `GetProcessTimes` 一侧。

### 3.3 秒级判别力不足：实测构造出同秒碰撞

这是本轮的新事实，之前只有「概率低，但不是零」的推断。5 个连续 spawn 的子进程落在同一秒：

```
sysinfo_start 1794496248 -> 5 process(es): [36112, 7980, 24372, 23856, 740]
    <-- same second, indistinguishable by start_time alone
distinct ft_raw      5 of 5
distinct sysinfo     1 of 5
```

4 轮全部重现（每轮都出现 `same second`）。

推论：**只要 A 退出与 B 启动发生在同一秒内、且 PID 被复用，仅凭秒级 `start_time` 无法区分两者**。现状的 `classify_process_observation`（`daemon_control.rs:684-697`）以及 `signal_daemon` 的复核（`:766-772`）都以秒级相等为「同一进程」的充分条件，这个前提在真机上不成立。

- 对 #22：`Bound` 判定若用秒级时间戳，必须写明「同秒复用不可区分」；要真正关闭，`daemon.instance.json` 得存 100 ns（迁移设计归 #22）。
- 对 #24：句柄 + 100 ns 创建时间是唯一能自证的组合；秒级只能当辅助，不能当判据。

### 3.4 附带事实：进程退出后 `OpenProcess` 仍成功

第 4 组观察里，`cmd /C exit 7` 退出并被 `wait()` 回收后，**由外部持有的句柄让进程对象继续存在**：

```
pid                  15320 (exited)
held handle ticks    Some(134389698489263673)   (Some => handle outlived exit)
reopen by pid        Some(134389698489263673)   (与持有句柄同一创建时间)
sysinfo after exit   None
```

三点确认：

1. 「`OpenProcess` 成功」**不是存活证据**——与 `daemon_control.rs:700-706` 的注释一致，本轮第一次在真机上看到组合。
2. 「`sysinfo` 无条目」**也不是退出证据**——本例里进程确实已退出，但句柄仍可读创建时间。
3. 只要有人持有句柄，PID 就不会被释放复用；因此「句柄 signaled + 句柄创建时间」这一组合在持句柄期间是稳定的。

这也解释了 `pid_definitely_missing`（`:308-337`）只在 `ERROR_INVALID_PARAMETER` 时判「不存在」为何是必要的保守分支。

## 4. 对 #24 实施顺序的影响

原顺序（#24 评论 §6）不变，但第 0 步的结论把第 1 步收窄了：

| 步骤 | 原计划 | 本轮结论后的调整 |
|---|---|---|
| 0 | 先验证两条时钟同量 | ✅ 完成：同量，秒级截断确认 |
| 1 | 句柄生命周期：`OwnedDaemon { pid, handle }` | 仍然要做，但**句柄必须配 100 ns 创建时间自证**，否则「打开时已被复用」无从判断 |
| 5 | 秒级 `start_time` 是否升级 | 本轮给了实测反例，建议升级；但改文件格式，归 #22 迁移 |

### 4.1 同轮落地的改动（第 1 步的最小形态）

`src-tauri/src/daemon_control.rs`：

| 新增 | 作用 |
|---|---|
| `creation_time_secs(handle)` | 从句柄读 `GetProcessTimes`，换算到 Unix 秒；与 `start_time` 同量的依据即 §3.1 |
| `BoundProcess::open(pid, expected_start)` | `OpenProcess(SYNCHRONIZE｜QUERY_LIMITED_INFORMATION｜TERMINATE)` 一次，句柄自身创建时间必须等于记录值，否则关闭并返回 `None` |
| `BoundProcess::is_signaled` / `terminate` | 等待与终止都用同一个句柄对象，不再按 PID 重开 |
| `Drop for BoundProcess` | 句柄随作用域关闭，不泄漏 |
| `wait_for_bound_exit` / `terminate_for_stop` | 有绑定走句柄；`None`（拒绝访问/已消失/时间不符）回退原有 PID 路径，且**不把 `None` 当作退出** |

`stop_daemon_process` 在发出 `/shutdown` **之前**绑定句柄，因此 8 秒宽限期与随后的强杀都针对同一个对象。这也顺带减少了「检查与发信号非原子」（K14）在 Windows 上的一段窗口。

本轮**没有**引入全局注册表：CLI 是一次性进程，跨调用持有句柄没有意义；收益集中在单次 stop 内与常驻的桌面进程。

验证（本机 Windows 11 26300）：

```
cargo test --no-default-features --lib daemon_control::tests
  → 17 passed; 0 failed        （含 3 个新增：绑定拒绝错配创建时间、绑定句柄跨退出跟踪、终止只作用于绑定对象）

cargo test --no-default-features --features daemon --test daemon_stop_e2e -- --test-threads=1
  → 26 passed; 0 failed; 28.92 s    （停止路径端到端，全部真实 daemon 进程）
```

同轮观察到但**与本改动无关**的失败（环境原因，未修）：`embedded_ssh::remove_system_known_host_matching::*` 4 条在全量 `--lib` 里失败，原因是本机不存在 `C:\Users\Administrator\.ssh\known_hosts`，而该测试用 `HOME` 隔离、Windows 上 `dirs::home_dir()` 读的是别的环境变量。全量结果为 `859 passed; 4 failed; 1 ignored`。

## 5. 未验证范围（明确留白）

- **真实 PID 复用后的对象翻转**：本轮没有构造 A 退出→PID 复用→B 占用 的受控时序。§3.3 证明的是「秒级无法区分同秒进程」，不是「复用确实发生过」。
- **跨账户**（管理员 daemon + 普通用户 CLI，及反向）下 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 与 `TERMINATE` 的行为。
- **服务/作业对象终止**时的句柄语义、`TerminateProcess` 在受保护进程上的失败模式。
- 探针是独立 crate（`sysinfo 0.39.3` + `windows-sys 0.48.0`，与仓库版本一致），**不是**从 `agent2ssh` 内部调用，故未覆盖仓库自身的 feature 组合。

## 6. 复现

```bash
cd docs/reports/evidence/windows-identity-2026-11-12
cargo build --release --offline
./target/release/a2s-win-probe.exe
```

探针只读：只对自身、自己 spawn 的子进程调用 `OpenProcess(QUERY_LIMITED_INFORMATION)`，不触碰 `~/.agent2ssh`，不写任何配置。

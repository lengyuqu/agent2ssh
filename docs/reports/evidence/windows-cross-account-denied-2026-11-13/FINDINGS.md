# Windows 跨账户 ACCESS_DENIED 诊断 — 真机端到端验证（2026-11-13）

## 背景

`a3800f6`（issue #24）为 stop 路径的「无法观测」分支接入了 `unverified_stop_reason(pid, pid_access_error(pid))`：跨账户 daemon 给出独立诊断文案（"may be running under another account. Access denied is not evidence that it exited…"）。该改动在 macOS arm64 上完成并通过交叉 check 编译，**Windows 真机未运行**（见 issue #24 评论 2026-11-13T08:49 的待验证清单第 5 条）。

前一轮（`windows-denied-2026-11-13/`）的结论是：受限令牌下 `CreateProcessAsUser` 的孙进程全部 `0xC0000142`，**无法构造**「非特权观察者 + 活 daemon」的组合；sysinfo 进程表先于 `OpenProcess` 返回 Live。本轮换了一条免密路径把 denied 分支真实跑通了。

## 方法

`runas /trustlevel:0x20000`（Windows 基本用户上下文，UAC 过滤令牌，**无需密码**）运行探针与真实 CLI。该上下文是「同一管理员账户的非提升 token」：Administrators 组为 deny-only，对 SYSTEM 进程的 DACL 不构成允许。

1. **探针**（`probe.ps1`）：对 `winlogon` / `services` / `lsass`（均为 SYSTEM）调 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`，两个上下文各跑一次。
2. **端到端**：构造 `daemon.pid` = winlogon 的真实 PID（1480）+ 合成 `daemon.token`，在基本用户上下文执行 `agent2ssh daemon stop`。

## 结果

### 探针（admin-context.txt / basic-context.txt）

| 上下文 | is-elevated | winlogon | services | lsass |
|---|---|---|---|---|
| 直接（管理员） | True | OK | OK | OK |
| `runas /trustlevel:0x20000` | False | **DENIED err=5** | **DENIED err=5** | **DENIED err=5** |

`pid_access_error` 的 Windows 分支（`ERROR_ACCESS_DENIED` → `Some`）在真实非提升令牌下**确实可达**——此前「管理员对任何本机进程都能开 QUERY_LIMITED_INFORMATION」的结论仍然成立，但「denied 不可构造」只对**提升**上下文成立。

### 端到端（stop-basic-context.txt）

```
Error: the OS denied inspection of daemon PID 1480 (OpenProcess denied (error 5));
the daemon may be running under another account. Access denied is not evidence
that it exited: no signal was sent and the PID file is retained. Re-run as the
owning user, or inspect the process out of band before removing anything
```

红线全部守住：

- `daemon.pid` 仍存在且内容仍为 1480（记录保留）；
- winlogon 存活、启动时间不变（无信号、未退出）；
- 诊断包含全部关键短语（another account / not evidence that it exited / no signal / PID file retained / out of band）——与纯函数测试 `access_denied_is_reported_as_unknown_identity_not_exit` 的断言逐字一致。

## 边界

- 走到该分支的前提是 `daemon_process_start_time` 对该 PID 返回 `None`（名字白名单不过）。winlogon.exe 天然不在白名单，所以这构造的是「记录指向别的账户的进程」这一真实损伤场景，而不是「跨账户 daemon 被安全识别后仍拒绝」——后者仍不可构造（本机无第二交互账户）。
- 诊断路径（`observe_pid`）的 `denied` 观测仍依赖「sysinfo 无条目 + OpenProcess 拒绝」的组合，本轮没有构造（sysinfo 对 SYSTEM 进程可见，先返回 Live）；`windows-denied-2026-11-13/` 的该结论不变。
- 机器：WinR9（Windows 11，10.0.26300，管理员 Administrator，RDP 会话）。

## 复现

```powershell
# 探针
powershell -File probe.ps1 -OutFile admin.txt           # 管理员上下文
runas /trustlevel:0x20000 "powershell -File probe.ps1 -OutFile basic.txt"

# 端到端（先构建 CLI，写好 daemon.pid/daemon.token 指向 winlogon PID）
runas /trustlevel:0x20000 "cmd /c set AGENT2SSH_CONFIG_DIR=<dir>&& <repo>\src-tauri\target\debug\agent2ssh.exe daemon stop > out.txt 2>&1"
```

# 受限令牌能否构造「CLI 看到 ACCESS_DENIED」（2026-11-13）

目标：把 #21 的 `PidObservation::Denied` 与 #24 的「绑定失败不发信号」从推断变成实测。
结论先说：**没有构造成功**，但失败点比「没试」具体，且顺带排除了两个可能的原因。

## 环境

Windows 11 10.0.26300，当前会话已提升（`IsInRole(Administrator)=True`）。所有测试都在
`%TEMP%\a2s-denied-*` 下，stand-in 是复制改名的 `ping.exe`（`agent2ssh-daemon.exe -t`），
配置目录里只有合成的 `daemon.token` 与 `daemon.pid`，不触碰 `~/.agent2ssh`。

驱动脚本：`run_probe.py`；探针：`src/main.rs`（`cargo build --release`，依赖 windows-sys 0.60）。

## 做了什么

1. **改写 stand-in 的进程 DACL**：`OpenProcess(PROCESS_ALL_ACCESS)` →
   `SetEntriesInAclW`（只有 `S-1-5-18` SYSTEM 与 `S-1-5-32-544` Administrators）→
   `SetSecurityInfo(SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION)`。
   结果：**成功**（返回 0）。提升的管理员仍能打开它（`can_open=true`），符合预期——
   管理员 SID 在允许列表里。
2. **受限令牌 + `CreateProcessAsUser`**：`CreateRestrictedToken`（Administrators
   deny-only；restricted SIDs = Everyone / Users / INTERACTIVE / 当前用户），启动探针自身。
   探针自身能运行并写出结果，但**它启动的每一个子进程都立即退出**：

   | 子进程 | 退出码 |
   |---|---|
   | `whoami.exe`（对照） | `-1073741502` = `0xC0000142` `STATUS_DLL_INIT_FAILED` |
   | `agent2ssh.exe daemon status --json` | 同上 |
   | `agent2ssh.exe daemon stop` | 同上 |

3. **排除过的原因**（每次只改一处，重编重跑）：
   - `DISABLE_MAX_PRIVILEGE` → `0`（保留 `SeChangeNotifyPrivilege` 等）——仍失败；
   - 子进程加 `CREATE_NO_WINDOW`——仍失败；
   - `STARTUPINFOW.lpDesktop = "winsta0\\default"`——仍失败；
   - restricted SIDs 加 `INTERACTIVE`（`S-1-5-4`）——仍失败。

## 结论

- 这条路**没有**造出「CLI 在受限令牌下看到 `ACCESS_DENIED`」。不是「没构造」，而是受限令牌
  下连 `whoami.exe` 都起不来，卡在 0xC0000142。
- 即使子进程能起来，`observe_pid` 也会先问 sysinfo：受限令牌仍含当前用户 SID，对自己启动的
  进程可见 → 返回 `Live`，根本走不到 `pid_access_error`。要命中 `Denied` 必须是
  **「sysinfo 看不到 + OpenProcess 明确拒绝」** 的组合——与 K14 2026-11-10 那条探针的结论一致
  （那次也没造出这个组合）。
- 因此 #21 的 `denied` 分支与 #24 的「绑定失败报错」在本机仍属**未验证**；代码保留为防御性
  分支，不能因为「有这条分支」就宣称已覆盖。

## 下一步（若还要做）

- 另一账户（`net user` + `CreateProcessWithLogonW`）比受限令牌更接近真实场景，但需要设置密码
  与用户配置目录，且 `daemon.token` 的 owner 加固会挡住宿主以外的读取；
- 或让 sysinfo 看不到：把 stand-in 跑成 **SYSTEM**（计划任务 `schtasks /ru SYSTEM`），再用普通
  管理员令牌观察——这一步只需要管理员，不需要受限令牌。

本轮不继续，先记录边界。

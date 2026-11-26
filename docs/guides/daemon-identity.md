# Daemon 身份与安全停止

本指南只描述代码现在做什么（2026-11-26 核对 `fce514a`），不描述目标设计。代码与本文不符时以代码为准；文末列出每一节的代码位置，便于复查。

## 一句话

每个平台都留下一个 `daemon.pid`，macOS 与 Windows 还多一份版本化实例记录；但只有 macOS 的默认停止走本机私有的 Mach 通道。Windows 用实例记录把"强杀"绑定到一个打开过的进程句柄；Linux 仍按 PID 发信号。**没有任何平台达到完整 `Bound` 保证**，见最后一节。

## 配置目录里的身份文件

| 文件 | 平台 | 内容 | 何时清除 |
| --- | --- | --- | --- |
| `daemon.pid` | 全部 | 纯十进制 PID | daemon 正常退出时自清 |
| `daemon.instance.json` | macOS、Windows | 版本化实例记录，见下 | 同上 |
| `daemon.config-id` | macOS、Windows | 32 位十六进制，持久随机 | 不主动删除 |
| `.daemon-ipc/s` | macOS | Unix socket，只做无密钥发现 | daemon 退出时 |

`daemon.instance.json` 的共同字段：`version`（当前为 1）、`instance_id`（128 位随机十六进制）、`pid`、`config_id`。平台差异：

- **macOS** 追加 `endpoint{kind, path}`，指向同配置目录下的本地端点。
- **Windows** 追加 `creation_ticks`（`GetProcessTimes` 的原始 100 ns FILETIME 计数，**不是** Unix 秒），且**没有** `endpoint` 字段——Windows 目前没有可证明的本机控制端点，记录里不会写一个不存在的通道。

`daemon.config-id` 不是凭据：它只用于命名发现名与交叉核对，复制配置目录会一并复制它。实例记录不含 admin token。

## 三平台的实际停止路径

`daemon stop` / `daemon restart` / 桌面停止共用同一条入口（`daemon_control::stop_daemon_process`）。

### macOS：Mach 双端点

1. 锁内读取 PID、实例记录与 `config_id`，锁随即释放；只读取**已存在**的 admin token，不创建。
2. 用 `bootstrap_look_up("org.agent2ssh.<config_id>")` 找到发现端口，发送无密钥 `hello`：随机 nonce、实例 ID、期望 PID，附一个 send-once 回复权。
3. daemon 只接受同 euid、实例与 PID 都等于自身的 `hello`，登记会话（10 秒有效，最多 32 个），回复 task identity token 与**私有**端口的发送权。
4. 客户端核对内核 audit trailer（euid、PID）、回复里回显的 nonce / 实例 / PID，以及私有端口不等于发现端口。任一步不符即返回，此时 token 尚未发出。
5. 登记 `EVFILT_PROC/NOTE_EXIT`，登记前后各从同一 identity token 取一次 task name 端口并复核 audit token。
6. 以上成立后，才把 token 发到私有端口。daemon 要求发送者 audit token 与该 hello 会话一致，会话一次性作废；发现端口上的 stop 被丢弃。
7. 等 `NOTE_EXIT` 最多 8 秒。回执相符且观察到退出才报告已停止；否则为 `UncertainExit`——不重试、不回退 HTTP、不发信号、不删记录。

`.daemon-ipc/s` 上的 socket 现在只回答 `hello`，不能再触发关闭。

### Windows：句柄 + 100 ns 创建时间

1. 从进程表取该 PID 的秒级启动时间；取不到且 PID 状态不是 stale 时直接拒绝。
2. 锁内读实例记录。**无记录** = 旧版 daemon，保持秒级比较；**记录存在但无效、字段冲突或读不出** = 拒绝，不降级成秒级比较。
3. 记录里的 `creation_ticks` 换算成秒，必须与第 1 步的观察一致；不一致说明 PID 在这两次读取之间换了手，拒绝。
4. `OpenProcess` 后**保留句柄**，用同一个句柄读创建时间与镜像名：创建时间按记录精度相等、镜像名在 daemon 白名单内，才算绑定。绑定失败不是"已退出"。
5. 核对 `/health` 的 PID、它声明的 `config_dir`、以及本配置的 admin token，三项都过才发 `/shutdown`（带 `expect_pid`）。
6. 等 8 秒。超时后只对**第 4 步那个句柄**强杀；句柄没绑上就报错，不按 PID 重新打开。

### Linux：仍是 PID + HTTP

1. 与 Windows 的第 1、5、6 步相同，但没有实例记录，也没有句柄绑定（`BoundProcess::open` 在 Linux 恒返回 `None`）。
2. 8 秒后超时，按 PID 重新核对启动时间与镜像名，再发信号。**这一步不是原子的**：核对与信号之间 PID 仍可能换手。

### `/shutdown` 的默认值

| 平台 | 默认 | 打开方式 |
| --- | --- | --- |
| macOS | 403（空 body） | daemon 启动环境设 `AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN=1` |
| Windows、Linux | 接受，需 admin token | 无开关；空 body 表示不检查 PID |

`daemon stop --allow-unbound-graceful` **仅 macOS**：单次 HTTP，不等待、不强杀、不清记录，CLI 收到接受回执也以非零退出。

## 什么时候是"拒绝"，不是"失败"

拒绝的共同语义是：没有发送 token、没有发信号、PID 与实例记录原样保留。以下情况会拒绝，而不是猜一个结果：

- **macOS**：发现名查不到；实例 / PID / nonce / audit token 任一不符；私有端口等于发现端口；`NOTE_EXIT` 登记失败；实例记录缺失、损坏或与 PID 不一致。
- **Windows**：记录字段无效（版本不符、非十六进制 ID、`pid` 不符、`creation_ticks` 为 0、出现未知字段）；`config_id` 与 `daemon.config-id` 不符；记录的 ticks 与进程表观察的秒级启动时间冲突；`OpenProcess` 被拒或句柄的创建时间 / 镜像名不符；`/health` 的 PID、`config_dir`、admin token 任一不匹配。
- **通用**：进程由另一账号或受保护进程持有、无法观察 —— 权限被拒明确按"可能仍在运行"处理，不当成已退出；admin token 不存在时报 `NotAuthenticated`。

已发出但看不到退出的一律是"未知结果"，不自动重试。

## 恢复

| 场景 | 命令 |
| --- | --- |
| daemon 崩溃，PID 与实例记录残留 | `agent2ssh daemon clear-stale --expect-pid <N> --expect-instance <instance-id>` |
| 只有旧版 `daemon.pid` 残留 | `agent2ssh daemon clear-stale --expect-pid <N>` |
| macOS 显式尝试一次未绑定停止 | `agent2ssh daemon stop --allow-unbound-graceful` |

`clear-stale` 三个平台都可用，只清理记录、**不发任何信号**；它在 `.daemon-pid.lock` 内重读文件，内容与期望不符就保留。记录存在时 `daemon start` 会拒绝覆盖，必须显式恢复。

## 不声称的边界

以下限制是当前实现的真实状态，阅读本文件或其他文档时不要把未列出的能力当成已有保证：

1. **macOS 不读代码签名**。这条路径依赖同 euid 与内核 audit token，不防同 uid 主动替换程序。发现名由 daemon 自己用已弃用的 `bootstrap_register` 注册，不是 launchd 托管。
2. **Windows 命名管道不是身份证明**。实测表明服务端退出后管道仍报死 PID，句柄被 `DuplicateHandle` 委派给代理后 ticks 校验照样通过，因此 pipe 通道保持 `Unbound`——这正是实例记录不写 `endpoint` 的原因。
3. **Linux 没有进程对象绑定**。超时强杀按 PID，非原子；没有 `pidfd` 路径。
4. **代码里的 `DaemonStopOutcome::Bound` 是窄义**。它在 macOS 上只表示"Mach 路径观察到退出"，不等于协议 v1 定义的完整 `Bound`（拥有内核进程对象并证明连接与对象同源）。
5. **验收范围有限**：macOS arm64 本机 ad-hoc 构建、Windows 11 26300 真机；Linux 的停止路径未在新的对象绑定方案下实测。

## 代码索引

| 内容 | 位置 |
| --- | --- |
| 帧格式与 `hello` 校验（`VERSION=1`、`MAX_FRAME=16 KiB`） | `src-tauri/src/daemon_identity.rs` |
| macOS 实例记录（含 `endpoint`） | `src-tauri/src/daemon_identity/macos.rs` |
| Windows 实例记录（`creation_ticks`，无 `endpoint`） | `src-tauri/src/daemon_identity/windows.rs` |
| macOS Mach 通道（客户端侧，C 侧 8 秒 / 10 秒 / 32 会话） | `src-tauri/src/daemon_identity/macos_mach.rs`、`macos_mach_control.c` |
| macOS 默认停止分派 | `src-tauri/src/daemon_control.rs` `stop_daemon_process_detailed`（macOS 版） |
| 非 macOS 停止流程 | 同上（`#[cfg(not(target_os = "macos"))]` 版） |
| 句柄绑定判定 | 同上 `bound_identity_matches` |
| 强杀 | 同上 `terminate_for_stop` |
| 记录清理与恢复 | 同上 `clear_stale_daemon_pid`、`recover_instance_records` |
| macOS `/shutdown` 403 门 | `src-tauri/src/bin/agent2ssh-daemon.rs` `daemon_shutdown` |
| `--allow-unbound-graceful` 平台门 | `src-tauri/src/bin/agent2ssh.rs` |

# #22 macOS Mach 停止隔离实验

2026-11-13，macOS 27.2.1 (26B131)，arm64。该实验构建实际 daemon 的独立实验二进制，在临时目录用 ad-hoc hardened runtime 签名、固定合成 token 和随机 launchd 名称运行。它不是可发布的停止实现，也没有使用真实用户凭据。测试使用本机普通用户临时 sshd、临时密钥以及隔离的 HOME；实验代码、服务名和配置不能用于生产。

## 复现

```sh
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon,macos-mach-lab --bin agent2ssh-daemon-mach-lab
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh
python3 docs/reports/evidence/macos-mach-stop-lab-2026-11-13/verify.py --daemon src-tauri/target/debug/agent2ssh-daemon-mach-lab --archive
python3 docs/reports/evidence/macos-mach-stop-lab-2026-11-13/verify_launchd_daemon.py --daemon src-tauri/target/debug/agent2ssh-daemon-mach-lab --archive
```

脚本创建并清理其自有 daemon、临时 sshd 和 Mach 服务，结果在 `results.txt`。所有请求仅携带固定合成 token。实验二进制要求隔离目录标记、固定 token 和随机实验前缀，默认构建没有启用该服务。

## 已观察

- 三轮真实 daemon 停止：错误 CDHash、实例、token 均不停止；客户端在核对同一 Mach 端点、audit token、task identity、代码和运行时策略后，注册 PID 的 `EVFILT_PROC/NOTE_EXIT` 并重新核对 task identity，再请求停止。回执只表示已接受；客户端随后观察到匹配 PID 的退出事件和 task name 的 dead-name 通知，测试父进程另行观察到退出、IPC drain、活动本地进程树停止和实例记录清理。邻近的 exec 反例表明，单独的 dead-name 通知不能证明 PID 进程退出。
- 一次真实 SSH 请求在执行中停止：客户端收到 503，明确表示远端结果未知；审计保留相同结论和请求元数据。不据此推断远端命令一定终止。
- 不安全运行时或异常 entitlement 的五种临时签名副本在发送 token 前被拒绝；缺少标记或 token 不符的实验 daemon 不注册服务。
- 修复实验接收循环遇到超大 Mach 消息即退出的问题：三轮各发送一条超出接收缓冲的无秘密消息，观察到丢弃记录，随后正常握手、认证及停止均成功。这只覆盖该消息形态，不能证明对持续洪泛免疫。
- 32 个合成 hello 占满实验会话表后，额外 hello 未发送 token 即被拒绝；10 秒超时后新会话可用。该结果不排除同用户进程在容量占满期间造成拒绝服务。
- 发送后立即销毁回复端口的竞态中，客户端报告结果未知；本次观察到 daemon 仍在运行。另一个有同步屏障的实验在服务端完成授权后、发出回执前销毁回复端口：客户端同样报告未知且未重试，而测试父进程观察到 daemon 退出。两者说明未知状态不能推断停止是否已执行；并未覆盖所有丢失方式。
- 三轮 A/B 服务替换中，另一进程复用 nonce，以及原客户端在相同 PID 下 `exec` 后复用 nonce 的请求均被拒绝。客户端分别在注册进程事件前、注册后复核 identity 前被暂停；A 退出、同名 B 启动后恢复，客户端均保守拒绝且没有发送 token。另一个客户端保存的旧发送权没有向 B 投递请求，B 保持存活，重新认证 B 后可完成停止。这是不同 PID 的受控切换，不是真实 PID 复用。实验代码的 receive right 使用 `MPG_IMMOVABLE_RECEIVE`。
- 三轮临时 launchd 管理的真实 daemon：公开发现端口和实例自建的 guarded 私有端点分离；正常客户端先检查 audit、task identity 和实验固定 CDHash，再只在私有端点发送合成 token。错误代码要求或实例在发送前拒绝；不同进程复用另一客户端 nonce 的合成停止被拒绝。另行故意向公开端口发送合成停止，服务拒绝处理且保持运行；A 停止、B 重启后旧私有发送权返回目标无效，B 存活，用新身份可停止。两次正常停止的客户端均在发送前注册进程事件、再次核对原 task identity，并在接受后观察到目标 PID 的 `NOTE_EXIT`；测试守卫另行观察退出。每个临时 launchd job 均确认注销；详见 `launchd-daemon-results.txt`。
- 上述 launchd 第一轮还通过本机临时 sshd 和合成密钥启动真实 SSH 请求，在命令运行中执行私有端点停止：客户端得到 503 和远端结果未知，审计保留同一语义。不据此断言远端命令终止。
- 一次 launchd 管理的真实 daemon 崩溃恢复：活跃进程的 `clear-stale` 被拒绝；对该测试 job 定向发送 SIGKILL 后，launchd 拉起的候选实例拒绝覆盖 PID、实例记录和 socket inode。注销 job 后，真实 CLI 使用错误实例编号的清理请求被拒绝且三项记录不变；使用正确 PID 和实例执行 `clear-stale` 后，重新注册 job 启动了新实例并通过其私有端点停止。这不证明自动恢复、真实 PID 复用或非配合旧版本写入的安全性。

## 边界与剩余工作

实验的 CDHash 由测试 harness 获取，不是正式可信分发；客户端无法仅凭发送权独立查询 receive right 的 guard 状态，实验依赖所核对代码的实现。`bootstrap_register` 已弃用，此实验不证明受支持的生产部署和恢复方式。先注册进程事件再复核 task identity 是待审查候选：尚无真实 PID 复用、权限失败、内核支持差异或所有注册竞态的验证，且不提供安全发信号的进程句柄。尚须实现并审查正式客户端/daemon 接入、安装与签名信任根、逐消息对抗和资源耗尽测试、其他回复丢失与恢复场景及跨平台验收。用户当前没有 Apple Developer 身份，正式签名的包及其运行时策略未验证。不能据此关闭 #22。

相邻证据：`../macos-audit-code-identity-2026-11-13/` 中有 audit token、替换、注入和 receive right 转移实验；`../macos-mach-discovery-2026-11-13/` 中有临时服务发现和旧发送权实验。它们的结果只覆盖该机器及其测试配置。

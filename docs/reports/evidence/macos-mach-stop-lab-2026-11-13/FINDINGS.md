# #22 macOS Mach 停止隔离实验

2026-11-13，macOS 27.2.1 (26B131)，arm64。该实验构建实际 daemon 的独立实验二进制，在临时目录用 ad-hoc hardened runtime 签名、固定合成 token 和随机 launchd 名称运行。它不是可发布的停止实现，也没有使用真实用户凭据。测试使用本机普通用户临时 sshd、临时密钥以及隔离的 HOME；实验代码、服务名和配置不能用于生产。

## 复现

```sh
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon,macos-mach-lab --bin agent2ssh-daemon-mach-lab
python3 docs/reports/evidence/macos-mach-stop-lab-2026-11-13/verify.py --daemon src-tauri/target/debug/agent2ssh-daemon-mach-lab --archive
```

脚本创建并清理其自有 daemon、临时 sshd 和 Mach 服务，结果在 `results.txt`。所有请求仅携带固定合成 token。实验二进制要求隔离目录标记、固定 token 和随机实验前缀，默认构建没有启用该服务。

## 已观察

- 三轮真实 daemon 停止：错误 CDHash、实例、token 均不停止；客户端在核对同一 Mach 端点、audit token、task identity、代码和运行时策略后请求停止。回执只表示已接受；测试父进程另行观察到退出、IPC drain、活动本地进程树停止和实例记录清理。
- 一次真实 SSH 请求在执行中停止：客户端收到 503，明确表示远端结果未知；审计保留相同结论和请求元数据。不据此推断远端命令一定终止。
- 不安全运行时或异常 entitlement 的五种临时签名副本在发送 token 前被拒绝；缺少标记或 token 不符的实验 daemon 不注册服务。
- 发送后立即销毁回复端口的竞态中，客户端报告结果未知；本次观察到 daemon 仍在运行。该实验不能断言请求已被接受，也不能证明每种回执丢失时的结果。
- 三轮 A/B 服务替换中，另一进程复用 nonce 的请求被拒绝；A 死亡后保存的旧发送权没有向同名 B 投递请求，B 保持存活，重新认证 B 后可完成停止。实验代码的 receive right 使用 `MPG_IMMOVABLE_RECEIVE`。

## 边界与剩余工作

实验的 CDHash 由测试 harness 获取，不是正式可信分发；客户端无法仅凭发送权独立查询 receive right 的 guard 状态，实验依赖所核对代码的实现。`bootstrap_register` 已弃用，此实验不证明受支持的生产部署和恢复方式。尚须实现并审查正式客户端/daemon 接入、安装与签名信任根、逐消息对抗和资源耗尽测试、回复丢失与恢复、真实 PID 复用及跨平台验收。用户当前没有 Apple Developer 身份，正式签名的包及其运行时策略未验证。不能据此关闭 #22。

相邻证据：`../macos-audit-code-identity-2026-11-13/` 中有 audit token、替换、注入和 receive right 转移实验；`../macos-mach-discovery-2026-11-13/` 中有临时服务发现和旧发送权实验。它们的结果只覆盖该机器及其测试配置。

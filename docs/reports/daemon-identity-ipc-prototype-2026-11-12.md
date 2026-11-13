# Daemon 身份协议：macOS 无密钥握手原型

> 2026-11-13 后续：macOS daemon 已接入 IPC 服务端、实例记录与显式恢复；此报告仅描述之前的无密钥握手原型。完整进展及限制见配置指南。

这是 #22 的一部分，不是可用的 daemon 停止协议。`daemon_identity` 提供 v1 无密钥 hello 的 4 字节大端长度帧、16 KiB 上限、严格 JSON 结构及版本/标识/PID 检查，并把 hello 与期望实例、PID 和 macOS `LOCAL_PEERPID` 做否定性比对。帧解析器不输出原始数据；`authorize` 和 `stop` 在此解析器中不是合法消息。调用方必须另行实施 I/O 和总 deadline；单纯的 `Read` 没有总 deadline 保证。

2026-11-12 在 macOS 27.2.1 (26B131)、arm64 上，`daemon_identity_macos_ipc` 使用独立测试子进程和 Unix stream socket，核对 OS 返回的 peer PID、同连接 hello 及 PID 不一致拒绝；子进程确认连接关闭前没有后续字节。测试不是生产 daemon 的端点、认证或退出测试。库单元测试检查重复键、未知字段/版本、非 UTF-8、截断、超长和认证消息拒绝。

无密钥握手、记录和 `LOCAL_PEERPID` 均不是进程对象证明。此原型没有声明 `Bound`，没有发送管理员 token，也没有修改停止或清理行为。此前的 socket 句柄传递反例仍然适用，见 `macos-daemon-identity-followup-2026-11-12.md`。同 UID 主动对抗不在 v1 保证内。

尚未完成：daemon 端点安全发布及恢复、版本化实例记录与 PID 协调、协议整体期限、认证和一次性停止状态机、Linux/Windows 平台绑定原型及真实 daemon 端到端验收。macOS 在找到并验证可用的连接到稳定进程对象证明前必须保持 `Unbound`，不得发送授权令牌、自动停止或按裸 PID 发信号；不能用这个握手绕过 #25 的安全默认值。

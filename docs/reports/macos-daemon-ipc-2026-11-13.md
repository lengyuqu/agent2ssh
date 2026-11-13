# macOS daemon IPC 接入记录（#22）

本批实现 macOS daemon 的 IPC 服务端与恢复路径，不声称完整跨平台进程身份绑定。配置目录须由当前用户拥有且不可被其他用户写入；私有 `.daemon-ipc` 子目录为 0700，socket `s` 为 0600。目录与子目录的末级符号链接、不安全权限、任何路径目录上的 ACL 允许项、不可信上级目录、超长路径和已有 PID/实例/socket 均拒绝，不自动覆盖。ACL 拒绝项可以保留。持久配置 ID 和实例 ID 是发现信息，不是秘密或身份凭据。管理员 token 在 PID 锁内以 0600 创建，拒绝符号链接、硬链接和 ACL 授权，避免在可读取配置目录中出现宽权限的创建窗口。

发布在 PID 锁内以同目录独占临时文件、同步和不覆盖的 hard-link 顺序创建 PID 与实例记录；两文件不是事务，崩溃残留不能自动认领。锁等待最多 2 秒。daemon 正常退出时停止 IPC 接受并在锁内对比两份记录和 socket inode；有变化则保留。显式恢复需提供 PID 和完整实例 ID，比对记录、OS 返回 ESRCH 和 socket 非阻塞连接返回 ECONNREFUSED；这不是原实例退出的证明，不能排除旧程序忽略锁、描述符继承、PID 复用或不可靠文件系统。无实例记录、记录损坏及部分发布需独立检查，不能自动恢复。不得在锁内等待网络；本地 socket 的非阻塞探测不会等待队列。

服务端每条连接先检查 peer UID，再接收 16 KiB 上限的严格长度帧；总处理时间 5 秒，最多 16 条并发连接。仅接受 hello → admin token → 一次匹配本实例和 PID 的 stop；错误顺序、字段、版本、令牌和实例均关闭连接，认证内容不写日志。正确停止产生带请求 ID 的回执并进入现有 drain；回执不证明退出，响应丢失为未知结果，不能自动重试。IPC 客户端只有在证明连接与同一稳定进程对象绑定后才能发送 token；macOS 目前缺少此证明，现有客户端继续拒绝，测试中的手写客户端仅用合成令牌验证服务端状态机。`LOCAL_PEERPID` 和 hello 仅能发现矛盾，不能升级为 Bound。

macOS arm64、27.2.1 (26B131)：真实 daemon 进程测试覆盖正确停止、先 stop、错误令牌、错误实例、崩溃后的错误实例恢复和正确显式恢复、活进程与部分记录拒绝、可写目录、ACL 授权、socket 目录符号链接、被占用 socket、超长路径与不可信父目录拒绝、持锁启动超时；所有运行使用临时配置和合成令牌。旧 macOS 保守停止和真实 daemon 停止回归也需随本批运行，具体结果以交付记录为准。

未完成：具有平台证明的生产客户端绑定/授权状态机及对象等待，Linux peer pidfd 与 Windows 命名管道/进程对象、PID 复用和 IPC 描述符传递的完整对抗测试、多用户跨 UID 与签名发布包实机验收；不应据本批关闭 #22。其他平台仍沿用其现有停止行为。合并 #21 后 macOS `daemon status` 不再向未绑定端点发送管理员 token；后续修复使 `daemon_matches_config` 在 macOS 上不向 HTTP 端点发送 token：现有实例不再被自动复用，新子进程只使用无密钥记录及匿名健康响应检查，并明确标记未绑定。该限制针对启动、复用及诊断健康检查；其他业务 HTTP API 仍会按其各自调用方式发送 token，不能据此宣称所有 HTTP 操作都已完成服务端绑定。目录同步是尽力而为，未验证断电耐久性；服务端 token 只认证客户端，不认证服务端。

### 后续：启动和复用检查不发送 token

macOS `daemon_matches_config` 现在直接保留未验证状态，不读取 token 或访问认证端点。已存在 PID 的启动请求拒绝自动复用、保留记录。新启动只观察由本次调用创建的子进程、版本化实例/PID/socket 记录和匿名 `/health` 中一致的 PID/配置目录；这些只是就绪线索，CLI/桌面明确报告 HTTP 端点未绑定、未认证，不将它们升级为身份绑定。测试覆盖伪造端点自报一个活 PID、伪造子进程 PID 但不发布完整实例、真实 daemon 启动，以及真实 CLI 首次启动后再次启动；伪造服务的捕获记录验证没有 Authorization 头或合成 token。

端点仍可能代理、伪造或转移 socket；这些检查不证明 HTTP 服务端属于子进程。业务调用和桌面页面后续主动使用的 HTTP API 不属于本次健康检查修复，仍需按自己的信任边界处理。未配置 SSH fixture 时不声称验证了活跃 SSH 的关闭行为。

### 后续：macOS 生产客户端的无密钥发现

默认停止现在读取锁内版本化实例/PID/配置 ID/socket 快照；在一条非阻塞本机连接上读取 `LOCAL_PEERPID`、发送随机 nonce 的 hello，核对响应和第二次 peer PID，再锁内复核记录内容及文件/socket inode。只要记录不一致、路径不安全、peer/hello 不符、连接或读写超时，就拒绝。通信采用单一 1.5 秒 deadline；两次锁等待分别有界，文件系统调用本身没有证明全局期限。客户端不读取 token、发送授权/停止、发信号、删除记录或自动重连。检查全部一致也只报告 `Unbound`，不自动停止。

这是生产客户端的一部分，不是稳定对象绑定：记录与 inode 是发现线索，读取前后相同不证明中间没有替换；macOS 上 IPC 连接的 `LOCAL_PEERPID` 会受描述符继承/转移影响。Apple 的 [XPC 对端 PID 文档](https://developer.apple.com/documentation/xpc/xpc_connection_get_pid%28_%3A%29?language=objc) 同样提醒 PID 可回收；此来源是对 PID 时效性的说明，不证明 Unix socket 与进程对象关联。Mach task identity token 等机制还需要单独证明如何从实际 IPC 对端取得稳定对象、如何阻止描述符委派，以及无特权下是否可用，不能把 API 名称当成已验收方案。

显式 `--allow-unbound-graceful` 若观察到 IPC 记录，会先验证无密钥发现的一致性，并在读取 token 前再次比对原快照；发现 IPC 矛盾或不可用时在读取/发送管理员 token 之前拒绝。没有 IPC 记录的旧 daemon 仍走人工选择的旧式路径；IPC hello 与 HTTP 监听者之间没有绑定，显式例外仍可能将 token 发给错误端点，不能因为 IPC hello 一致就声称安全绑定。

2026-11-13 本机 macOS 27.2.1 (26B131)、arm64、隔离配置及合成 token：客户端原有测试 7 项通过（含一个仅供子进程调用的辅助测试），覆盖真实不同 PID 对端、匹配 hello 仍无授权、错误实例、记录和 socket 替换、慢速帧 deadline、没有自动重连；真实 daemon IPC 9 项通过，包括调用生产 `daemon stop` 后 daemon 仍存活；保守停止 9 项通过，包括 IPC 矛盾时显式 HTTP 例外没有发送 token；停止回归 33 项通过、2 项忽略，未配置 SSH fixture。无默认特性 Rust 库 873 通过/1 忽略，Tauri 库 891 通过/1 忽略。仍未测试真实 PID 重用、跨 UID、沙箱及签名发布包。

候选方向另见 Apple [Mach IPC 安全限制说明](https://developer.apple.com/documentation/xcode/conforming-to-mach-ipc-security-restrictions?changes=__8%2C__8)：task identity token 转换时仍需权限检查。该文讨论异常消息，不证明 Unix socket 可以安全获得同一对象，尚无相应生产绑定实现或验证。

提交前复核新增了 hello 后、兼容请求读取 token 前的快照复核；`observation_rejects_a_record_replaced_after_hello` 使用相同内容替换记录，确认 inode 变化被拒绝。此检查不能消除复核之后的替换竞态，也不绑定 HTTP 与 IPC 端点。

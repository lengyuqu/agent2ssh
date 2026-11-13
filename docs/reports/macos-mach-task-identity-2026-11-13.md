# #22：macOS Mach task identity token 与安全停止研究

## 结论

**Mach task identity token 在本机可以取得与其指向的 task 对应的 name 端口、audit token 和进程代次；它本身不能把现有 Unix socket 的后续接收者绑定到该 task。** 实验中 socket 的对端 audit token 可与 Mach 消息的发送者以及 token 对应 task 的 audit token 一致，但核对之后、发送数据之前，继承 socket 的另一进程仍能读走数据。因而不能据此让现有客户端发送管理员 token 或自动停止。

更有希望的方向是将**整个本地停止控制协议迁至经过身份验证的 Mach/XPC 通道**，并证明接收权不可被转移，使用 task name 端口和每条消息的内核身份做交叉检查。该方向有实测原语支持，但还没有满足完整交付前提；当前生产代码应继续按 `Unbound` 拒绝自动停止，不发送未经绑定的凭据，不按 PID 强杀。

## 环境和复现

- 2026-11-13；macOS 27.2.1 (26B131)、Darwin 27.2.0、xnu-13432.41.4~25、arm64，普通 uid 501，SIP enabled。
- 探针为本机 clang 生成的 arm64 ad-hoc 签名可执行文件，TeamIdentifier 未设置；不是 Tauri 签名发布包，无特别 entitlement。测试只创建自己的子进程、socket、Mach port 和合成单字节数据，不访问 `~/.agent2ssh`，不操作其他进程。
- 使用探针进程的 bootstrap special-port 槽向自己的子进程传递发送权，父进程立即恢复原槽；没有注册持久 launchd 服务。该引导方式只是实验夹具，不是可独立重连的生产方案。
- [证据目录](evidence/macos-mach-task-identity-2026-11-13/)包括源码和每轮原始输出；相关测试结束后没有残留探针进程。

```sh
clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror -O1 \
  docs/reports/evidence/macos-mach-task-identity-2026-11-13/probe.c \
  -lbsm -o /tmp/a2s-mach-identity-probe
/tmp/a2s-mach-identity-probe
/tmp/a2s-mach-identity-probe --exec-transition

clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror -O1 \
  docs/reports/evidence/macos-mach-task-identity-2026-11-13/fd_handoff.c \
  -lbsm -o /tmp/a2s-mach-fd-handoff
/tmp/a2s-mach-fd-handoff

clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror -O1 \
  docs/reports/evidence/macos-mach-task-identity-2026-11-13/guarded_receive.c \
  -o /tmp/a2s-mach-guarded
/tmp/a2s-mach-guarded
```

最后一个命令故意在自己的临时子进程中尝试转移受保护接收权；本机该子进程收到 SIGKILL。父探针验证对照组成功、受保护组未收到转移后退出。请勿把该实验用于真实 daemon 或其他用户进程。

也可运行 `python3 docs/reports/evidence/macos-mach-task-identity-2026-11-13/verify.py`，它在独立临时目录重新编译，并断言直接连接 3 轮、exec 3 轮、socket 交接 5 轮和受保护接收权 3 轮。该脚本同样会触发上述测试子进程的 SIGKILL；若系统或权限策略与本机不同，断言可能失败，应检查原始结果而不是直接推断平台语义。

## 本机结果

1. **token、Mach 发送者和 socket 在普通路径上的关联：3/3。** 子进程对自身 `task_create_identity_token` 成功，通过 Mach 端口描述符把 token 送到父进程。父进程请求内核附带的 Mach 消息 audit trailer；发送者 PID 和 pidversion 与 Unix socket 的 `LOCAL_PEERTOKEN` 一致。通过 token 转换 `TASK_FLAVOR_NAME` 成功；`pid_for_task` 和该 name 端口的 `task_info(TASK_AUDIT_TOKEN)` 与 Mach 发送者 audit token 一致。这比仅比较 PID 强，**但只证明本轮读取时的观察一致**，不证明之后谁持有 socket 或接收下一次写入。
2. **权限：3/3。** 在上述未授权普通用户/构建组合中，token 转换为 CONTROL、READ、INSPECT 均返回 `KERN_DENIED`（53）；NAME 成功。这不是所有签名、entitlement、用户或系统版本的结论。Apple 说明 token 转换仍进行权限检查，而获得 control 端口是高权限操作：[Mach IPC 安全限制](https://developer.apple.com/documentation/xcode/conforming-to-mach-ipc-security-restrictions)、[Endpoint Security task 事件](https://developer.apple.com/documentation/endpointsecurity/es_event_get_task_t)。
3. **正常退出与 token/name 端口：3/3。** 校准测试先对普通 Mach 端口确认 `MACH_NOTIFY_DEAD_NAME` 消息 ID 72 和端口名匹配。子进程存活期间的 50 ms 等待内 name 端口无通知；正常退出并 wait 后，name 端口收到同名 dead-name 通知并变成 dead name，原 identity token 在 300 ms 内没有对应通知，但再转换返回 `KERN_NOT_FOUND`（56）。不能从 token 本身未收到通知推出目标仍存活，也不能把本机时限扩展为系统保证。
4. **exec 反例：3/3。** 子进程持有同一 PID 执行 `exec` 到探针的另一个模式，随后明确响应父进程并由 `waitpid(WNOHANG)` 证明仍在运行。原 name 端口已发出匹配的 dead-name 通知，原 token 再转换返回 `KERN_NOT_FOUND`；socket 的 audit PID 相同但 pidversion 发生变化。因此 **name 端口死亡可表示旧 task 实例失效，不等于这个 PID 的进程已经退出**；不能据此清理 PID 或启动替代实例。
5. **描述符继承和凭据泄漏窗口：5/5。** A 发出 socket 字节后，客户端读取 `LOCAL_PEERTOKEN`/`LOCAL_PEERPID`，均指向 A；继承同一 socket 的 B 此时尚未调用该 socket。客户端完成检查后允许 B 开始读取，B 收到客户端发送的合成字节，之后客户端读取的 peer 凭据才变为 B。A、B 在交接检查时均保持存活。即使事后发现不一致，发送的秘密已经交给 B；两次查询、相同 PID 或匹配的 task token 无法关闭检查与发送之间的窗口。
6. **Mach 接收权控制：3/3。** 未保护的 receive right 可以作为 Mach 消息描述符转移；对照子进程正常退出。设置 `MPG_IMMOVABLE_RECEIVE` 后，该受控实验中的转移没有到达接收方，子进程被 SIGKILL；即使本探针此前成功把 guard 行为设为 `TASK_EXC_GUARD_NONE` 也如此。只持发送权的一方对 `mach_port_get_context`、`MACH_PORT_RECEIVE_STATUS` 和 `MACH_PORT_INFO_EXT` 查询均得到 `KERN_INVALID_RIGHT`（17），受保护与未保护两组相同。因此已测查询不能让客户端证明服务端真的启用了保护；不排除其他机制。Mach receive right 一般可以转移，见 Apple [Mach 边界说明](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/KernelProgramming/boundaries/boundaries.html)。

## 安全停止的候选方案

这是需要安全评审和后续平台验收的设计，不是已经证明可发布的保证。

1. **将握手、授权、停止和回执放在同一 Mach/XPC 控制通道**，不再把 Mach 探针结果用来为另一条 Unix socket 或 HTTP 连接授权。端点通过用户域的受控服务管理器发布，启动、用户会话、配置目录和实例生成号均明确隔离；无法取得可信服务或权限时保持 `Unbound`。不得从 IPC 失败自动回退到 HTTP。Apple 建议对可能受 Mach 消息拦截影响的场景优先采用 XPC：[Mach IPC 安全限制](https://developer.apple.com/documentation/xcode/conforming-to-mach-ipc-security-restrictions)。
2. **建立可审计的接收者不转移不变量。** 若采用原生 Mach，daemon 应在开放端点前设置并检查不可转移接收权，检查失败即不发布；所有 fork/exec、辅助进程、端口备份和服务重启路径都须验证。若采用 XPC，须先证明其端点、连接和消息身份在同样的交接、代理及重启案例下的语义。客户端不能仅根据服务端声称“已保护”作信任决定；需要可信签名/部署策略和完整代码路径审计。Apple 的 [XPC peer requirement](https://developer.apple.com/documentation/xpc/xpc_connection_set_peer_requirement) 可检查消息发送者的要求，但代码签名本身不证明同一个进程实例或接收权不可转移；具体 API 可用版本和打包方式还须验证。
3. **客户端持有对象并核对内核身份。** daemon 在可信控制通道内发送自己的 task identity token；客户端转换得到 name 端口，在同一任务会话内读取 `TASK_AUDIT_TOKEN`，与内核提供的消息发送者 audit token（包含 PID 和 pidversion）比较，并把该端口一直保留到停止结果处理结束。服务端也应按收到请求的内核身份和已有授权策略验证调用者。实例 ID 只作匹配和否定，不是信任根。若无法取得 token、name 端口、消息身份、签名/权限或不可转移不变量，全部拒绝，不能发送管理员 token。
4. **单次优雅停止和诚实结果。** 绑定条件经证明后，在同一受保护通道发送一次指定实例的停止，daemon 核对并进入现有 drain；回执只表示接受。断线、超时或回执丢失为 `UnknownOutcome`，不重连或自动重放。保留 name 端口的 dead-name 事件只能说明原 task/端口失效；因为 exec 反例，**不能单独证明进程退出，也不能单独清理 PID**。只有取得独立、可靠的原实例退出证据且锁内快照仍匹配时才条件清理；否则保留并报告未知。
5. **本版不自动强杀。** 本机 CONTROL 被拒绝，NAME 不提供终止权。即使另一环境给予 CONTROL，也必须另外证明该对象与同一通道绑定、权限和策略允许，并用同一对象操作；不能退回裸 `kill(pid)`。强制终止还会跳过 daemon drain，与正常关闭语义不同。

## 仍需证伪和验收

- Mach/XPC 实际产品服务的可信注册、签名/Team ID、ad-hoc 开发构建、launchd 重启、用户会话、sandbox/hardened runtime 和最低 macOS 版本；不同 UID 和无权限场景。
- 证明客户端如何在不依赖对端自报的情况下相信接收权不可转移；原生 Mach reply-port 劫持、port rights 转移、代理、继承和重连接时序。上面的 `MPG_IMMOVABLE_RECEIVE` 是一个受控本机观察，不是生产 XPC 实例的证明。
- 真实 PID 复用、task token 交叉替换、发送前后 exec/exit/服务替换、死名通知与回执竞争；不能将 `Unknown` 当 `Exited`。要构造只能改状态而不影响真实用户数据的真实进程测试。
- 用完整生产 daemon 和多个独立客户端做“错误目标不能收到 token/停止”“目标与替代进程均保持预期状态”“记录锁内条件清理”和崩溃恢复测试。观察远程任务是否结束仍是另外的问题。

目前没有修改生产协议、停止行为或 issue 状态；#22 应继续保持开放。

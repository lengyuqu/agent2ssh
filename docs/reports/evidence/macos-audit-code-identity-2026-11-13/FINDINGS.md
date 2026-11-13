# audit token、代码身份和 Mach 接收权实验

2026-11-13，macOS 27.2.1 (26B131)，arm64。运行 `python3 verify.py --archive`、`python3 verify_dyld.py --archive`，以及 `python3 verify_real_daemon_dyld.py --daemon <真实 daemon 构建路径> --archive`；选项以脚本 `--help` 为准。脚本创建临时 ad-hoc 签名副本、合成标记与独立进程，详细记录保存在同目录 `*results.txt`。

- `SecCodeCopyGuestWithAttributes(kSecGuestAttributeAudit)` 结合固定 identifier/CDHash，在本机测试中识别从 Mach 消息得到的 audit token。与 task identity token 得到的 task name port 的 audit 信息一致。相同 PID 的 `exec` 改变 pidversion，旧 audit token 和旧 SecCodeRef 不再有效；不同代码的同 identifier 替换未通过旧 CDHash。
- 持有原 task name port 并订阅 dead-name 后，同 PID 的 `exec` 会使原端口收到 dead-name、`task_info` 失败，原 identity token 也无法重新取得 task port，但 `waitpid(WNOHANG)` 仍显示该子进程存活。五轮同代码和三轮替换代码均复现。**因此不能仅凭原 task 的 dead-name 通知声称该 PID 的进程已经退出或据此删除记录**；它只能证明原 task port 不再有效。
- 在自有子进程上，先按 PID 注册 `EVFILT_PROC/NOTE_EXIT`，再从已持有的 identity token 取得 task port 并与原 audit token 对比：八轮匹配。随后 `exec` 未产生退出事件，真正退出后产生匹配 PID 的 `NOTE_EXIT`。这是同机受控实验，不证明真实 PID 复用、内核版本、权限拒绝或全部注册竞态，也不提供绑定信号接口。
- 上述身份一致仍不足以证明后续消息由原进程接收：五轮未保护 Mach receive right 转移后，已核验的 A 仍活着，实际是 B 接收合成数据。三轮 `MPG_IMMOVABLE_RECEIVE` 保护下转移失败，内核终止尝试转移的合成进程，B 未收到数据。发送权持有者不能因此独立查询 guard 状态。
- 对受保护端点补做同 PID `exec`：五轮执行原代码、三轮切换到不同 CDHash 的代码，原发送权均返回 `MACH_SEND_INVALID_DEST`，新映像未收到合成数据。新映像内相同数字端口名称偶尔对应另一个 receive right，因此端口数字或类型不能证明是原端点。该实验证明不了其他部署和 OS 版本的行为。
- 真实 daemon 的临时 ad-hoc 副本中，没有 hardened runtime 时无害 DYLD 标记库能运行，同时代码身份验证通过；启用 hardened runtime 且同时允许 DYLD 环境变量和关闭库验证时也观察到注入。其他所测组合没有观察到标记；这不是对全部注入路径的证明，也不能代表正式签名产物。

该结果说明正式方案还需要可信来源的代码要求、运行时和 entitlement 约束，以及可证明的接收权所有权约束。无法从实验中的 ad-hoc CDHash 推导正式信任根。

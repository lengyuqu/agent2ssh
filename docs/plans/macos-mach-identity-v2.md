# macOS daemon 身份绑定候选协议：评审稿

状态：**未实现、未完成正式验收**。这是 #22 在已有 v1 安全默认之上的 macOS 候选细化；生产客户端仍按 [v1](daemon-identity-v1.md) 拒绝未绑定停止。本稿不是 `macos-mach-lab` 的 C 结构体规范，也没有选定最终发布者身份。不得把实验特性、ad-hoc CDHash、环境变量或实例文件作为生产信任根。

## 对象与信任边界

- 公共发现端口 **D** 可由用户域服务管理器持有并在重启时继续接受消息；它只接收无秘密发现请求。A 死亡后旧 D 发送权可以投递给 B，即使 A 曾 guard D。见 [launchd 反例](../reports/evidence/macos-mach-discovery-2026-11-13/FINDINGS.md)。
- **P** 是 A 自行创建、启用不可转移 receive right 保护的实例私有端口；A 在无秘密回复中交付 P 的发送权。旧 P 发送权在本机受控 A/B 实验中不会指向 B，但客户端不能仅通过发送权查询 guard；保护是必须由可信发布代码保证并在各支持环境验收的部署不变量。
- **T** 是回复中提供的 task identity token，**K** 是通过 T 获取的 task name port，**A** 是由内核附加在该回复上的 audit token；**R** 是锁内读取的版本化发现记录，**Q** 是订阅进程退出事件的观察对象。R、服务名、PID、nonce 和 HTTP health 只能发现或否定矛盾，不能单独授权。
- 客户端必须从不可被普通配置或服务回复替换的可信发布策略取得可接受的代码要求，并校验 A 指向的动态代码、受限运行时及 entitlement。策略如何绑定正式 Apple 签名、Team ID、允许版本和签名轮换尚未定案；缺失或不匹配时拒绝，不能让 ad-hoc 测试 hash 自动成为生产信任根。持有发布签名密钥的对手、可修改客户端或内核的对手及同 UID 任意内存注入的完整防护不由本实验单独证明。

## 单次操作时序

1. **发现。** 在锁内读取并保存数字 PID、完整版本化 R 和文件身份；验证目录、版本与端点描述。只取得 D 的发送权，不读管理 token。记录丢失、损坏或不一致时保留原物并拒绝。
2. **无秘密 hello。** 客户端创建不被委派的 reply receive right，发送协议版本、预期 PID/实例、随机 nonce 和一次性 reply right 到 D。请求有严格长度、类型、descriptor 数量和 deadline；不包含 token。daemon 只接受与自身实例一致的 hello，记录内核提供的客户端 audit token、nonce 和有界有效期。
3. **校验回复。** daemon 的回复携带 P 的 send right、T、nonce 和实例信息。客户端先检查消息与 descriptor 类型、权利类型、数量和长度；再核对 A、K 的 task audit token、PID/实例/nonce、用户边界和可信代码要求。**失败时销毁接收的权利，不读取或发送 token。** 审计标识来自内核，不采用回复自报的 PID 代替。
4. **注册退出观察。** 若此操作需要观察退出，先注册 Q，再从原 T 重新获取并核对 task audit；失败则拒绝。`NOTE_EXIT`、PID 复用和权限语义在支持的正式运行环境经验证前不得自动升级为可靠退出证明；原 task 的 dead-name 不能单独等同进程退出。
5. **授权与停止。** 仅在 P 和相关证明均有效时，从安全存储读取 token，将 token、nonce、实例与单次停止请求发往**保留的 P 发送权**，绝不发往 D。daemon 对每条消息读取内核 audit trailer，要求客户端 audit、nonce、实例、时限和 token 与 hello 会话一致，且只处理一次。公开端口上的停止请求必须拒绝，不执行副作用。不得记录 token 或原始授权帧。
6. **结果与清理。** 回执也在受保护的 reply right 上，并核对消息的内核发送者、nonce、实例及结果；`Accepted` 仅代表接受。只有满足已验收的平台退出观察和锁内条件匹配才可认定目标退出或清理属于它的记录。发送失败、回复丢失、Q 不可观察、代码/端点变化或身份不一致时返回 `Unknown`/`Unbound`，不向 D 或 HTTP 自动回退、不自动重放、不按裸 PID 发信号。实际执行中用户显式发起的旧 HTTP 兼容例外仍遵循 v1 的单次风险提示和限制。

## 发布与迁移待办

- 现有 `daemon.pid` 保持数字格式；新增记录需明确 schema 版本、配置标识、D 的发现方式和所有者，版本未知或跨配置复制不视为绑定。D/P 的内核权利不能通过 JSON 记录持久化。新旧客户端、daemon 和显式 HTTP 兼容组合仍按 v1 降级表处理；未知版本不得猜测为新协议。
- launchd job 的注册、禁用、升级与卸载必须有明确所有权及隔离配置目录；崩溃后残留文件不被新实例自动覆盖。已有实验先注销测试 job，再显式按 PID 和实例核对恢复；它不构成自动恢复方案。
- daemon 必须以受限 entitlement 独立签名，外层 bundle、CLI 和更新包必须与实际发布信任策略一致。一次 ad-hoc 调试包重签通过仅说明本机可行；正式签名、bundle 验证、公证、更新和回滚均未验证。固定 hash 的更新/回滚与撤销策略必须先定案。
- 发布评审可在隔离、不会被并发修改的产物目录中运行 `python3 scripts/check_macos_daemon_signature.py <daemon-binary> --team-id <trusted-team> --identifier <trusted-identifier>`。该工具要求由发布方提供预期 Team ID 和标识，验证 Apple Developer ID Application 的代码要求，并逐架构检查 hardened runtime 和已知不允许的 entitlement；失败返回非零。它是**静态检查**，不为运行时客户端提供信任根，不检查公证、外层包、版本回滚或检查后的文件替换，也不能证明端点归属。Team ID、标识与可接受版本仍须通过可信发布流程固定；不要从待测 daemon 自报结果自动取得。证书要求的候选组合参照 Apple 的 [代码签名要求说明](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements/)；正式项目签名的成功侧尚未验证。
- 生产接入前还须审查 Mach 消息资源释放、发送失败与通知、队列和会话耗尽、fork/exec、跨用户权限及最低支持系统。实验中的直接 `bootstrap_register` 已弃用，不作为生产发布路径。

## 证据和剩余验收

本机 macOS 的合成签名真实 daemon 覆盖了私有端点 A/B 替换、不同客户端 nonce 复用、错误身份、公开端口拒绝、退出观察、SSH drain、丢失回执及显式崩溃恢复，详见 [记录和脚本](../reports/evidence/macos-mach-stop-lab-2026-11-13/FINDINGS.md)。这些测试没有证明正式签名信任根、真实 PID 复用、所有受限账户/系统版本或正式 app/CLI 的端到端行为。#23 Linux 与 #24 Windows 的对象绑定、退化及真实平台验收继续分别跟踪；不能将 macOS 的结果推及其他平台。

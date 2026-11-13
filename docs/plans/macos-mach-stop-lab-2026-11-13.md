# #22 macOS Mach 同端点停止实验设计（未验收）

目标是在不改变默认产品行为、不需要 Apple Developer 身份且只使用合成 token/临时配置的前提下，验证一个**真实 daemon 进程**如何由已核对的 Mach 接收权进入现有 drain。实验不等于正式安全停止：ad-hoc CDHash 的可信分发、生产签名、服务注册/迁移、同 UID 主动对抗、恢复和真实 PID 复用都还没有验证。

候选实验约束：

1. 仅显式构建 `macos-mach-lab` 实验二进制并提供临时配置标记和固定合成 token 时启动；正常编译和生产 daemon 不启用。动态服务名使用随机实验前缀；实验使用已弃用的 `bootstrap_register`，不能因此声称它适合发布。
2. daemon 在注册前创建并成功设置 `MPG_IMMOVABLE_RECEIVE`；任何错误均关闭实验。每条请求通过 Mach audit trailer 验证客户端 UID，不把 PID 数字当进程对象。
3. 客户端向取得的 service send right 发无秘密 hello，daemon 在 reply 上发送自身 task identity token 与指向**同一接收权**的 send right。客户端核对同一端口对象、nonce、实例和 PID；从内核 trailer 取得 audit token，与 identity token 转换出的 name port 的 `TASK_AUDIT_TOKEN` 对照。保留同一 name port 与 service send right。
4. 临时签名实验副本以 hardened runtime 且无动态加载例外的 ad-hoc 签名启动；测试客户端按自己从实验二进制计算并固定的 CDHash、identifier 对 audit token 查询运行中代码并验证，同时检查签名标志和异常 entitlement。该 hash 来源只在隔离实验中可信，不构成产品发布信任根。先前实测仅靠 CDHash 可以加载注入库，必须检查运行时与 entitlement；不能把本实验扩展成对所有注入方式的保证。
5. 通过上述测试后，在同一个保留的 send right 上发送**合成 token**与一次 stop。daemon 检查 token、实例、PID 和每条消息的用户身份，回复接受后触发现有 shutdown token/drain。客户端只报接受、进程退出未确认；测试父进程可以等待自己创建的真实 daemon 来验证退出及记录清理。绝不按数字 PID 强杀，异常/不一致不自动重试或回退 HTTP。
6. 需要对照：错误 CDHash、错误实例、错误 token、转移受保护接收权、持有旧 send right 的服务重启/替换、回执丢失及 daemon 存活，确认在前置证明失败时没有授权或停止。前述本机 DYLD 注入与 Mach receive handoff 反例必须保持可复现。

上述实验已在仅限合成配置的 feature-gated 真实 daemon 中实现；复现方式、观察结果和限制见 [实验记录](../reports/evidence/macos-mach-stop-lab-2026-11-13/FINDINGS.md)。它没有接入正式客户端或默认 daemon，也没有证明正式签名和真实运行环境的不可转移不变量；这些仍是 #22 的验收缺口。

**launchd 部署反例**：上述直接 `bootstrap_register` 的独立 receive right 不等同于 launchd 管理的 MachServices 端口。[三轮实测](../reports/evidence/macos-mach-discovery-2026-11-13/FINDINGS.md)中，服务给 receive right 加 guard 后退出，旧发送权仍可向重启的服务成功投递。正式方案不能将 token 发往该公共端口；若使用它发现服务，必须先建立并验证独立、受保护且不会被重用的实例端点，仍须实测部署与签名信任根。

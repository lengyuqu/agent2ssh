# #22：macOS 服务部署与逐消息身份验证补充

## 结论

**当前尚不能安全地将生产 daemon 的自动停止从 `Unbound` 升级为 `Bound`。** 本轮用临时用户域 launchd job 和 ad-hoc 签名 XPC 二进制验证了签名要求的部分行为，并用独立进程验证 Mach 每条消息的 audit trailer 与 task identity token 的对应关系；这不是可信签名发布包、完整 XPC 会话转移或生产 daemon Mach 控制协议的验收。普通生产停止仍保守拒绝，未启用 token 发送、PID 强杀或自动清理。

实验提示：**不能先给 XPC 服务发送管理 token，再依靠客户端 peer requirement 验证回复**。在本机不匹配签名要求的反例中，服务端 handler 已收到合成请求，客户端随后才收到 `Peer Forbidden`。只有先完成无秘密的认证、并解决请求目的接收权可转移与部署信任，才有继续研究安全停止的基础；不能把一次认证回复或相同代码签名等同于后续请求目标已绑定。

## 环境、隔离与复现

- 2026-11-13，macOS 27.2.1 (26B131)、Darwin 27.2.0、arm64、普通 uid 501。`security find-identity -v -p codesigning` 在当前钥匙串会话报告 `0 valid identities found`；这不说明其他钥匙串或其他机器没有可用身份。
- [证据目录](evidence/macos-xpc-deployment-2026-11-13/)包含 XPC C 程序、测试脚本、Mach 每消息探针及输出。构建均在 `/tmp`，两个不同的 server 二进制显式 ad-hoc 签名、同一指定 identifier、TeamIdentifier 未设置；测试仅发送合成字符串，不使用用户凭据。
- 每个 XPC 案例使用唯一的 `org.agent2ssh.test.*` 标签，以临时 plist `launchctl bootstrap gui/<uid>` 启动用户域 job，`finally` 使用该标签 bootout 并确认 `launchctl print` 明确报告不存在；清理状态未知时保留路径与标签供检查。没有安装持久 LaunchAgent。三轮共 24 个 job，均报告 bootout 成功且最终 absent。
- 生产 daemon 验证使用各自独立的临时配置、临时端口和合成 token；测试子进程由测试守卫回收。没有测试或停止用户现有 daemon。

```sh
python3 docs/reports/evidence/macos-xpc-deployment-2026-11-13/verify.py

# 验证上轮不可转移接收权和 exec/fd 交接原语；该脚本的 guarded
# 案例会故意让自己的受控子进程触发系统 SIGKILL。
python3 docs/reports/evidence/macos-mach-task-identity-2026-11-13/verify.py

AGENT2SSH_CONFIG_DIR=$(mktemp -d) \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  --features daemon --test daemon_identity_macos_e2e \
  --test daemon_macos_conservative_e2e --test daemon_identity_macos_client_e2e
```

## 结果与证据边界

1. **临时服务与客户端签名要求（3/3）。** 匹配 identifier 得到原始服务回复；错误 identifier 和 `anchor apple generic` 针对 ad-hoc 服务均拒绝回复。但这两种拒绝案例的服务端都记录到客户端预先发送的合成请求。固定同一个独立客户端时，另一个内容不同、由同一操作者 ad-hoc 签为相同 identifier 的替身也通过仅按 identifier 的要求并返回自己的 variant（3/3）；在 requirement 中固定原始代码的 CDHash 后，原始服务回复通过、替身回复被拒（3/3），但替身仍已收到合成请求。CDHash 只提供预先可信分发的精确代码匹配，仍需处理可信来源和升级轮换，不能证明同一进程实例。替身使用独立 job 名称，本测试不是对同一 launchd 服务名的抢占/替换演示。系统上的签名要求是对**收到的对端消息**的检查，不能据本结果把发送出去的秘密视为已保护。Apple 的 [XPC peer requirement](https://developer.apple.com/documentation/xpc/xpc_connection_set_peer_requirement) 和 [Mach IPC 限制](https://developer.apple.com/documentation/xcode/conforming-to-mach-ipc-security-restrictions) 给出 API 与平台边界。
2. **服务端对客户端的签名要求（3/3）。** 在每个收到的 peer connection 上设置仅接受指定 client identifier 的要求后，匹配客户端的合成请求到达 handler；不匹配客户端的请求未到 handler、客户端得到连接中断。这里只用两个独立连接，并未证明同一 XPC 会话发生端点转移或 sender 替换时的安全性。Apple 文档还说明 `xpc_listener_set_peer_requirement` 的 listener 要求不会自动继承到其创建的 peer session；新 listener/session API 与此处旧 connection API 不能混为一谈：[listener requirement](https://developer.apple.com/documentation/xpc/xpc_listener_set_peer_requirement)。
3. **原始 Mach 逐消息身份（3/3）。** 两个同时存活的独立子进程依次向同一 Mach 接收端发送各自的 task identity token；接收端每次取得内核 audit trailer，并比较 token 转换的 task name 端口 `TASK_AUDIT_TOKEN`。每条消息与对应发送者一致；测试程序固定第一条 task audit 后对第二个不同实例的比较结果为拒绝（模拟判定，第二条消息本身已到接收端）。该探针使用临时 special-port 引导，不是 launchd/XPC 生产会话；两进程不同 PID，并未验证 PID 复用、同 PID 的 exec 竞态或 token 委派。
4. **不可转移接收权：上轮实验复跑通过。** `MPG_IMMOVABLE_RECEIVE` 在受控子进程中阻止了 receive-right 转移，未保护对照可转移；发送端的 context、receive-status 和 info-ext 查询不能区分是否受保护。测试子进程尝试违规转移后被系统 SIGKILL；不证明 XPC 隐藏接收权使用了该保护，也不证明客户端可核实生产 daemon 的保护状态。
5. **真实 daemon 拒绝路径。** 新增 `replaced_socket_cannot_receive_stop_and_real_daemon_stays_alive`：启动真实 daemon，保留其 PID/实例/token 快照，替换其 pathname socket 为另一个进程的端点，调用生产 CLI 普通停止。CLI 因 peer PID 矛盾拒绝；替身收到零字节，真实 daemon 仍存活，三份记录不变；拒绝后同一 daemon 的测试 resident session 仍能执行合成命令并正常关闭。还复跑真实 daemon 的正常 IPC handler 测试和显式旧式 HTTP 遇 IPC 矛盾时不发送 token 的回归。handler 测试里用**合成 token 的手写未绑定客户端**请求关闭只是服务端状态机验证，不能当成已认证的安全停止成功。

## 候选方案与未完成项

- 需要可验证的用户域服务发布、可信签名根及客户端/服务端要求。`identifier`、ad-hoc 签名、launchd 服务名和一次成功回复都不足以证明期望实例；当前没有验证真实 Team ID、签名 app 内 helper、Service Management 注册/禁用、跨用户或受限进程。[SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice) 是 Apple 的持久 helper 管理入口，不是本轮临时 `launchctl bootstrap` 测试的同义词。
- 继续以无秘密的发现握手为第一步，查明如何在**同一** Mach/XPC 控制通道中可信地绑定服务接收者、task identity token、每条响应的内核身份和保留的 task name 端口。必须证明 receive right 的不可转移策略和服务发布不会仅依赖服务端自报；XPC 内部端口语义、端点交接、代理、fork/exec、恶意同 UID 的可信根和 reply-port 保护均未完成。
- 收到同一实例的单次接受回执也不证明退出；name 端口因 exec 可以死亡而进程仍存活。响应丢失/超时不自动重试，进程与记录状态不能证明时保留记录、报告 Unknown；不得按裸 PID 自动强杀。
- 在证明前继续默认拒绝。完整成功路径需在真实签名部署的 daemon 上覆盖被替换服务不能收到 token/stop、客户端与 daemon 所持对象一致、活跃 SSH/本地任务 drain、exec/exit/PID 复用及回执丢失、重启/恢复等。当前测试不应将「手写未绑定客户端能关闭测试 daemon」描述为安全停止已完成。

## 本轮回归结果

- `verify.py`：原始 Mach 每消息测试 3 轮、XPC 8 个案例各 3 轮通过，全部 24 个临时 job 确认 absent；归档输出也核对了各案例轮数。可信 Team ID 仍未实测。
- 上轮 Mach 原语 `verify.py`：直接身份 3 轮、exec 3 轮、socket 交接 5 轮和受保护端口 3 轮通过。
- 真实 daemon IPC 套件 10 项、IPC 客户端套件 8 项、保守停止套件 9 项通过；新 socket 替换测试还验证受影响 daemon 的测试 resident session 在拒绝后可继续执行和正常关闭。活跃 SSH 和**绑定后的自动停止成功**仍未实测。
- 无默认特性 Rust 库 873 通过、1 忽略；Tauri 库 891 通过、1 忽略；前端生产构建通过。`cargo fmt --check`、`git diff --check`、报告本地链接和证据计数检查通过。

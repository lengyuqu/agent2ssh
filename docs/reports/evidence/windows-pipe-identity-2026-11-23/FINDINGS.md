# Windows 命名管道身份原语探针（CNB #24 / v1 §4.2、§7.4）真机测量

- 日期：2026-11-23
- 机器：MACHENIKER9，Windows 11 10.0.26300（x64），Administrator，rustc/cargo 1.100.0
- 分支：`feat/windows-daemon-identity-phase2`
- 探针：`src-tauri/tests/pipe_identity_probe.rs`（Windows-only，全部 `#[ignore]`，仿 `container_terminal_manual`）
- 原始输出：本目录 `probe-run.log`
- 复现：
  ```text
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
      --test pipe_identity_probe -- --ignored --nocapture --test-threads=1
  ```
  结果：**9 通过，0 失败**（13.99 s）。子进程角色（server/proxy）通过 `A2S_PIPE_PROBE_ROLE` 重执行同一测试二进制，无外部 stand-in；管道名按测试唯一，不触碰真实 daemon 或 `~/.agent2ssh`。

## 0. 结论先说

**Windows 保持 `Unbound`。** 命名管道原语无法把控制连接 C 证明到 daemon 进程对象 O：

1. `GetNamedPipeServerProcessId` 给的是**数字 PID（创建者）**，仍需 `OpenProcess` 才变成对象——复用窗口存在（m4 实测服务端退出后该 API 仍返回死 PID）。
2. **委派不可见**：持有复制/继承服务端句柄的代理进程应答时，API 仍报创建者 PID，阶段二的 ticks 校验也照样通过（m5/m6）。所以 ticks+记录能收敛停止路径的复用*检测*窗口，却**不能**把管道连接升级为 `Bound`——委派在 PID 复用之外独立地破坏 C↔O。
3. DACL / 首实例 / 远程拒绝确实提供真实保护（m1/m2/m3），但它们保护的是**管道名字**，不是**应答者身份**：同用户进程仍可用 `PROCESS_DUP_HANDLE` 把句柄委派出去（m6c）。

这与 v1 §4.2 的预期一致（"没有经 Windows 真机验证的证明前仍 `Unbound`……不预先宣布成功"）。本轮**不改生产代码**，D2/D3 的 Windows 默认值（拒绝未绑定停止、默认关 HTTP `/shutdown`）仍待一个可证明的绑定方案，不在本轮范围。

## 1. 逐项实测

| 项 | 实测 | 错误码/数值 |
|---|---|---|
| **m1 首实例** | 带 `FILE_FLAG_FIRST_PIPE_INSTANCE` 的第二次 `CreateNamedPipeW` 同名失败 | `ERROR_PIPE_BUSY` (231) |
| **m1 多实例** | 不带 FIRST 标志时第二实例允许 | 成功 |
| **m2 DACL 拒绝建实例** | 限制性 DACL（授予 `0x12019b`，即 RW 去掉 `FILE_CREATE_PIPE_INSTANCE` 0x4）下，第二次 `CreateNamedPipeW` 被拒 | `ERROR_ACCESS_DENIED` (5) |
| **m2 连接检查忽略 0x4** | 客户端用完整 `FILE_GENERIC_READ\|WRITE`（`0x12019f`，**含** 0x4）`CreateFileW` 仍能连 | 成功（0x4 只在 `CreateNamedPipe` 强制） |
| **m2 对照** | 全权限 DACL（`0x1f01ff`）下第二实例允许 | 成功 |
| **m3 远程拒绝** | `PIPE_REJECT_REMOTE_CLIENTS`（pipe mode 位 0x8）本地连接成功；经 `\\<主机名>\pipe` 与 `\\127.0.0.1\pipe` 的连接尝试失败 | 231（见 §2 未验证） |
| **m4 客户端 PID 对称** | `GetNamedPipeClientProcessId` 报连接客户端 PID == 本进程 | 相等 |
| **m4 服务端 PID 采样** | `GetNamedPipeServerProcessId` 报创建者 PID == 实际 server 子进程 | 相等 |
| **m4 断管后仍报死 PID** | server 被杀后，客户端句柄上 `GetNamedPipeServerProcessId` 仍 `Ok(死PID)`；`ReadFile` 失败 | `ERROR_BROKEN_PIPE` (109) |
| **m5 委派不可见** | 创建者把非继承服务端句柄 `DuplicateHandle` 进活代理；代理应答。API 仍报创建者 PID，`roundtrip` 应答者 == 代理 PID，`creation_ticks(创建者)` 存在 | ticks 校验**通过**而应答者是代理 |
| **m6a 继承泄漏** | 可继承句柄 + `bInheritHandles=TRUE`：子进程继承并应答 | 应答者 == 子进程 |
| **m6b 非继承阻断** | `bInheritHandles=FALSE`：子进程拿不到句柄，退出 | 子进程自行退出 |
| **m6c 同用户 DUP_HANDLE** | 同用户 `OpenProcess(self, PROCESS_DUP_HANDLE)` 成功，`DuplicateHandle` 把非继承句柄 dup 进新代理并应答 | 成功；非继承 ≠ 不可委派 |
| **m7 复用窗口** | server 退出后 500 次 spawn 未命中 PID 复用（Windows 顺序分配）；窗口由 API 形状（数字 PID→`OpenProcess`）记录 | 未强制复用（见 §2） |

## 2. 未验证 / 推断边界（如实保留）

- **m3 远程拒绝的"远程"语义**：本机经 `\\<主机名>\pipe` / `\\127.0.0.1\pipe` 的连接尝试返回 231（`PIPE_BUSY`），而非明确的远程拒绝错误。单实例已被本地客户端占用可能是 231 的原因；**没有第二台机器**，真正的远程客户端拒绝未实测。`PIPE_REJECT_REMOTE_CLIENTS` 被接受且本地连接正常是实测；远程拒绝效果是推断。
- **m7 PID 复用**：500 次 spawn 未触发复用，复用窗口由 m4（断管仍报死 PID）+ API 形状佐证，未在本机强制构造一个"同 PID 不同进程"的 impostor 并测 ticks 区分。ticks 区分 impostor 的能力已在阶段二 `a_same_second_daemon_image_reuse_is_refused_by_the_tick_record` 用同整秒进程对证明，此处不重复。
- **`PROCESS_DUP_HANDLE` 的跨账户边界**：m6c 是同用户。跨账户能否打开 daemon 进程 dup 句柄取决于进程 DACL，未实测（与 #24 已记录的跨账户 `runas /trustlevel` 拒绝观测一致：拒绝访问是未知，不是已退出）。
- **预建不可委派管道候选**：v1 §4.2 提到"启动时预建不可委派管道与句柄"。m6 表明只要同用户持有 `PROCESS_DUP_HANDLE`，句柄就可被 dup 委派；除非 daemon 进程对象 DACL 拒绝 `PROCESS_DUP_HANDLE`（会同时影响停止路径的 `OpenProcess`），否则"不可委派"不成立。本轮未实现该候选，仅测量其前提。

## 3. 对设计的影响

- v1 §4.2 Windows 段与 §7.4 的"证明不了即保持 `Unbound`"由本轮真机测量支撑：管道名字可保护（首实例 + DACL + 远程拒绝），但应答者身份不可证明（数字 PID + 可见委派）。
- 阶段二的 ticks 记录定位不变：它是**停止路径**（直接 `OpenProcess` 记录的 PID）的复用检测收敛，不是管道连接的绑定凭据。记录仍无 endpoint 字段，Windows 仍 `Unbound`。
- 不新增第二条停止协议（D4），不改 D2/D3 现状。

## 4. 环境备注

- cargo 需 `NO_PROXY="$NO_PROXY,192.168.77.6"` 前缀（沙箱代理劫持内部 crates 镜像）。
- 探针为 `windows-sys` 0.48 新增特性 `Win32_System_Pipes`、`Win32_Security`、`Win32_Security_Authorization`、`Win32_Storage_FileSystem`、`Win32_System_IO`——纯 FFI 声明， shipped 二进制不调用。
- 探针编写中纠正的三处 Windows 语义（已固化进测试注释）：`PIPE_REJECT_REMOTE_CLIENTS` 属 pipe mode 而非 open mode；Unicode 环境块需 `CREATE_UNICODE_ENVIRONMENT`；`nMaxInstances` 是 `CreateNamedPipe` 次数上限而非自动实例数（单实例服务单客户端，第二客户端 `PIPE_BUSY`）。
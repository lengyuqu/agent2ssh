# macOS daemon 静态签名检查

## 范围

`scripts/check_macos_daemon_signature.py` 是单独运行的发布评审工具，不是运行时身份绑定或已启用的发布门禁。调用者必须从可信发布配置提供预期的 Team ID 和 daemon 代码标识；工具不会从待测二进制选择信任根。它要求 Apple Developer ID Application 证书链、匹配的 Team ID 和标识，并对通用二进制逐架构检查 hardened runtime 和一组不允许的 entitlement。签名要求依据 Apple 的 [TN3127](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements/)；其中 Apple anchor 与 Developer ID 证书扩展检查共同使用。

```sh
python3 scripts/check_macos_daemon_signature.py /path/to/daemon \
  --team-id TRUSTEDTEAMID --identifier trusted.daemon.identifier
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -v scripts/tests/test_macos_daemon_signature.py
```

示例中的 `TRUSTEDTEAMID` 是占位符，必须换成实际的十位预期 Team ID。检查必须针对发布方控制、检查期间不会被其他进程替换的独立 daemon 文件运行；通过后仍须保护后续打包的同一产物。

## 本机验证

2026-11-13，macOS 27.2.1 (26B131)，arm64：

- 使用临时编译并 ad-hoc 签名的合成二进制，Developer ID 要求拒绝它；无 hardened runtime、JIT entitlement、格式错误的预期身份均拒绝。
- 自建 arm64/x86_64 通用二进制中，两切片策略均符合时逐切片的运行时检查通过；仅 x86_64 切片开启 JIT 时拒绝。
- 对本机已安装的第三方 Developer ID 签名应用做只读对照：正确 Team ID 与标识的签名要求通过，错误 Team ID 或标识拒绝。另一个已签名的受限独立可执行文件通过完整静态检查；将两个各自验证通过、代码标识不同的第三方切片拼成临时通用二进制后，身份检查拒绝。没有启动、修改或重新签名这些第三方应用。
- 上述四类条件由 `scripts/tests/test_macos_daemon_signature.py` 覆盖。本次提供本机第三方只读 fixture，7 项通过；不提供 fixture 时相关测试跳过。第三方发行者与 Agent2SSH 没有信任关系，只用于验证系统检查的正反例。
- GitHub 的 macOS build 矩阵运行无外部 fixture 的合成测试；这不表示 CNB 的 Linux 流水线验证了 macOS，也不代替正式签名包测试。本地已检查工作流 YAML，尚未确认远端工作流运行结果。
- CNB 对提交 `1b17616` 的构建 [cnb-e77-1k7a7ckc4](https://cnb.cool/zdzso/agent2ssh/-/build/logs/cnb-e77-1k7a7ckc4) 两条流水线均在 Prepare 阶段因根组织 CPU 核时配额不足失败，测试阶段跳过；这不是代码测试通过或失败的证据。

本机仍无可用的**项目** Developer ID 身份，不能证明 Agent2SSH 的正式签名、Team ID、外层 app、Notarization、更新或回滚。静态工具还不能验证运行中进程的签名状态、Mach 私有端点归属或检查后文件未被替换；这些仍是 [生产验收条件](../plans/macos-mach-identity-v2.md)和 #22 的未完成部分。

## 运行时代码签名原语

macOS 构建现在包含 `daemon_identity/macos_codesign`，它通过内核 audit token 获取运行中代码，核对调用方提供的代码要求、动态有效状态、hardened runtime 和受限 entitlement。生产侧的项目发行要求尚未配置，`verify_release_code` 因此返回 `TrustRootUnavailable`；它也没有接入普通停止路径。该原语只提供代码签名证据，调用方仍须独立证明 audit token 来源、私有端点绑定、存活和授权，不得以其结果单独发送 token 或清理记录。

本机合成进程单元测试从 Unix socket 的 `LOCAL_PEERTOKEN` 获取实际 audit token，验证测试固定 CDHash 的 ad-hoc hardened 进程能通过底层检查；错误标识、Apple anchor、无 hardened runtime、JIT entitlement 和缺少项目发行信任根均被拒绝。实验 Mach 客户端也改用相同 C 校验原语，直接注册与临时 launchd 的真实 daemon 回归均通过。ad-hoc 的正向检查只用于测试原语，并不满足生产发布策略；项目正式 Developer ID 签名的成功路径仍未验证。

补充运行中真实 Developer ID 对照：在用户显式提供可选 fixture 时，测试启动一个第三方签名的 `rg`，只通过 stdin 提供一行合成文本，并从其 stdout 所连接的 Unix socket 获取内核 audit token；没有扫描用户文件或读取应用配置。Apple Developer ID 证书要求、该 fixture 的 Team ID 和标识均匹配时，底层运行时代码检查通过；分别更换 Team ID 或标识时拒绝，项目发行信任根未配置时也拒绝。本机使用 ChatGPT 随附的签名 `rg` 运行通过；该第三方发行者不是 Agent2SSH 信任根，也不能替代项目签名 daemon、私有 Mach 端点或完整发布包验收。该 fixture 仅在显式设置 `A2S_TEST_DEVELOPER_BINARY`、`A2S_TEST_DEVELOPER_TEAM` 和 `A2S_TEST_DEVELOPER_IDENTIFIER` 时运行，常规测试不依赖第三方应用。

后续将生产候选的代码要求收紧为结构化组合：Apple Developer ID Application 证书链、预期 Team ID、代码标识和单个精确 CDHash 全部匹配，且拒绝无法安全嵌入要求语法的输入。使用上述第三方运行进程，正确组合通过，单独更换 Team ID、标识或 CDHash 均拒绝；实际项目策略仍为 `None`。单 hash 如何覆盖多架构、升级、回滚和撤销尚未决定，不能因为此候选通过而启用生产停止。

同一 macOS 原语还提供 audit token 与持有的 task name port 的比较：检查 UID/PID、`TASK_AUDIT_TOKEN` 及返回长度，完整 token 不一致时拒绝。它不接管端口，也不证明端点归属。单元测试覆盖真实 self task、篡改 token、错误 PID 和无效端口；真实 daemon 的直接注册与临时 launchd 实验客户端复用该比较并通过回归。生产停止路径仍未接入，Unix socket 的 peer audit 快照不能借此升级为 `Bound`。

候选退出观察器现已抽出为共享原语：先核对 task identity，再注册 `NOTE_EXIT`，注册后重新由原 token 获取 task port 并复核 audit；观察器拥有并释放自己的 queue 与 task name port。超时、错误和事件分别返回，事件仅是观察线索。隔离单测覆盖当前 task 的超时、错误 token/端口/PID；实验客户端复用该实现并在真实 daemon 停止中观察事件。真实 PID 复用、受限权限、fd 继承与正式生产绑定仍未验收；它不能单独授权信号或清理记录，也未接入普通停止路径。

构建和回归：`npm run build` 通过；macOS Rust 无默认特性库 877 通过、1 忽略，Tauri 库 895 通过、1 忽略，新增的运行时单测在两种特性下单独复跑通过；普通 CLI 与 daemon 构建通过。`cargo check --locked --no-default-features --target x86_64-pc-windows-gnu --bin agent2ssh` 在 macOS 交叉编译通过，不能代替 Windows 运行验证。新增本地原语没有接入停止路径，CNB 配额不足的限制仍适用。

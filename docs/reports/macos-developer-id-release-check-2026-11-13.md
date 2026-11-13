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

本机仍无可用的**项目** Developer ID 身份，不能证明 Agent2SSH 的正式签名、Team ID、外层 app、Notarization、更新或回滚。静态工具还不能验证运行中进程的签名状态、Mach 私有端点归属或检查后文件未被替换；这些仍是 [生产验收条件](../plans/macos-mach-identity-v2.md)和 #22 的未完成部分。

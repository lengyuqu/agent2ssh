# Pageant 真机验收 — 2026-10-23

结论：**本轮验收通过**。Windows 上的真实 PuTTY Pageant 可以为 Agent2SSH 完成 Ed25519 和 RSA-3072 认证；第二把身份匹配、远程执行、SFTP 往返和正反对照均通过。本轮没有修改传输实现。

原始结果见 [验收证据 JSON](evidence/pageant-acceptance-2026-10-23.json)，包含进程退出码、超时状态、耗时、标准输出、Plink 协议日志及应用认证日志。日期使用 Asia/Shanghai；原始应用日志保留 UTC 时间。

## 环境与隔离

- Windows 11 Pro，版本 `10.0.26300`；PuTTY / Pageant / Plink `0.85`。
- 源码基线 `a56d2d9c0aaf7bf60636eb073c6ad6e9eeb675d9`，重新构建 `--no-default-features --bin agent2ssh`。开始时工作区已有 `agent.rs` 的 ignored 身份探针，本轮保留该改动。
- 服务端为 Docker 镜像 `lscr.io/linuxserver/openssh-server:version-10.3_p1-r1`，OpenSSH 10.3，只将容器端口发布到 `127.0.0.1:22197`，账户 `tester`。
- 使用独立临时 `AGENT2SSH_CONFIG_DIR`。主机 `pageant-test` 的 `key_path`、`password`、`passphrase` 均为空；仅将本轮生成的公钥放入容器 `authorized_keys`，避免默认 `~/.ssh/id_*` 文件造成假阳性。
- Windows OpenSSH `ssh-agent` 全程保持 `Stopped / Disabled`，并实际确认 `\\.\pipe\openssh-ssh-agent` 无法打开。因此本轮成功走的是 Pageant 分支。
- 三把临时密钥由本次构建的 `agent2ssh keys generate` 生成，再由临时 Alpine 容器的 `putty` 包所提供的 **PuTTYgen** 转为 PPK。未手工拼装 PPK。
- 每个探针、CLI 和 Plink 子进程均受外层 **25 秒 watchdog** 约束，所有用例自然退出，没有触发 watchdog。

## 验收结果

耗时是本次单次运行的进程墙钟时间，不是性能基准。

| 用例 | 结果 | 证据 / 耗时 |
|---|---|---|
| 未运行 Pageant | 通过负对照 | CLI 退出 1，明确 `SSH authentication failed`；888 ms |
| 空 Pageant 列举身份 | 通过 | `identities: 0`；561 ms |
| 空 Pageant 执行 | 通过负对照 | CLI 退出 1，认证失败；1994 ms |
| Pageant 只有未授权 Ed25519 | 通过负对照 | 能列出 1 个身份，CLI 认证失败；692 ms |
| Pageant 加载两把 Ed25519 | 通过 | 探针列出 2 个身份；112 ms |
| Agent2SSH 使用已授权 Ed25519 | 通过 | `pageant-ok`，远端退出 0；1781 ms |
| 独立 Plink 对照 | 通过 | `Pageant has 2 SSH-2 keys`、`Sending Pageant's response`、`Access granted`；670 ms |
| 仅授权列表中的第二把 Ed25519 | 通过 | Agent2SSH 返回 `pageant-second-key-ok`；1250 ms。Plink 日志先拒绝 `key #0`，随后接受 `key #1` |
| 通过 Pageant 做 1 MiB SFTP 上传 / 下载 | 通过 | 两端均为 1,048,576 字节，SHA-256 相同；1299 / 1262 ms |
| 仅授权 RSA-3072 | 通过 | Agent2SSH 返回 `pageant-rsa-ok`；2189 ms。Plink 同样成功 |
| 停止本次 Pageant 后重跑 RSA 用例 | 通过负对照 | 同一主机、同一命令退出 1，认证失败；653 ms |
| 重新启动 Pageant 并加载 RSA | 通过恢复对照 | `pageant-restored-ok`，远端退出 0；2289 ms |

第二把身份用例没有依赖加载顺序：先从探针确认 Pageant 实际返回的列表顺序，再把容器的 `authorized_keys` 改为只包含列表中的第二把公钥。该钥匙仍沿用最初负对照阶段的 `pageant-denied-20261023` 注释。

六次成功的 Agent2SSH 连接（Ed25519 exec、第二把身份 exec、SFTP 上传、SFTP 下载、RSA exec、恢复 exec）均有实际认证日志：

```json
{"message":"ssh connection authenticated","fields":{"host":"pageant-test","address":"127.0.0.1:22197","auth_method":"agent","username":"tester"}}
```

RSA 用例的有效服务端 `PubkeyAcceptedAlgorithms` 包含 `rsa-sha2-512` / `rsa-sha2-256`，不包含裸 `ssh-rsa`，所以此次成功要求 RSA SHA-2 签名；本轮日志没有进一步区分 SHA-256 与 SHA-512。

SFTP 往返 SHA-256：

```text
D6BC953819DF8CE55873697D9D23630B54601E14270D191152E7403F3A28467D
```

## 复测要点

1. 建立独立配置目录、生成新测试密钥，仅授权给回环地址上的测试 sshd。
2. 用 PuTTYgen 转换 PPK，再启动 `pageant.exe <test.ppk>`。Pageant 要求 PPK，并支持从命令行加载密钥，见 [PuTTY 0.85 官方手册](https://the.earth.li/~sgtatham/putty/0.85/htmldoc/Chapter9.html#pageant-cmdline-loadkey)。本轮在实际 Pageant Key List 中核对了两把 Ed25519 密钥。
3. 确认 OpenSSH 固定命名管道不可用；不提供 `--key`、密码或口令，运行以下命令。

```powershell
# 先把 AGENT2SSH_CONFIG_DIR 指向独立测试目录。
agent2ssh host add pageant-test --host 127.0.0.1 --port 22197 --user tester
agent2ssh exec pageant-test "echo pageant-ok" --json --timeout-secs 10
agent2ssh sftp put pageant-test <test-file> /tmp/pageant-roundtrip.bin --json
agent2ssh sftp get pageant-test /tmp/pageant-roundtrip.bin <returned-file> --json
```

4. 核对配置目录内 `app.log` 的实际 `auth_method`，并用 `plink -batch -agent` 独立对照；Plink 的 `-hostkey` 应使用从测试容器读取并核对的主机指纹。
5. 只停止本次启动的 Pageant，保持主机配置不变，确认认证失败；重新加载后应恢复成功。

`pageant_lists_identities` 只是身份探针，返回 0 个身份也会通过，不能代替认证验收。现有探针内部的 Tokio timeout 也不能代替外层进程 watchdog：依赖的 Windows 消息传输可能同步阻塞。

## 回归、边界与清理

- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`：**744 passed / 0 failed / 1 ignored**，75.47 秒；ignored 为上述实机探针，本轮已单独执行。仅有既有的 `tempfile_dir` 未使用警告。
- 上一轮“加载后停在 Requesting keys”的现象本轮没有复现。正规 PuTTYgen 转换的 PPK 可以加载并签名；这关闭了运行时验证缺口，**不证明已查明上一轮卡死的具体根因**。
- 当前实现仍优先选择 Windows OpenSSH 固定管道。若管道能打开但其中没有可用身份，不会继续尝试 Pageant。本轮验证的是 OpenSSH 管道不可用时的 Pageant 路径。
- 本次未验收加密 PPK 的交互解锁、Pageant 用户证书或硬件安全密钥，也未逐一驱动桌面 / daemon / MCP 的入口。
- 验收结束后关闭本次 Pageant、删除测试容器及临时私钥 / PPK；保留去除临时目录路径的证据 JSON。未改动原有 SSH agent 服务设置或真实 Agent2SSH 配置。

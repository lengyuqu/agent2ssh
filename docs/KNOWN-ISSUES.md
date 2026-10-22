# 已知问题与未覆盖范围

日期：2026-10-23（K10、K11 均已修复；K12 为上游限制）。基线：macOS 开发机 + WinR9，Docker / wslc / Apple container；GitHub Actions。

本文只记录 **已经知道、尚未解决** 的事项，以及 **没有验证过所以不能下结论** 的范围，按处理价值排序。已修复的问题在 `CHANGELOG.md`；历史缺陷快照在 `DEFECTS.md`（0.2.1 时代，已归档）；Windows 相关的细节在 [reports/WINDOWS-HANDOVER.md](reports/WINDOWS-HANDOVER.md)，此处只列条目。

状态说明：**待修**＝行为不对，有明确方向；**限制**＝按当前设计成立，使用者需要知道；**未验证**＝没有证据说好或不好。

## 1. 行为问题

| # | 状态 | 问题 | 影响 | 位置 / 下一步 |
|---|------|------|------|---------------|
| K1 | 待修 | 已知主机指纹按 `host:port` 只存一个条目（一种密钥类型） | 服务器同时有多种主机密钥时，一旦主机密钥算法偏好变了（`algo set --hostkey`）或服务器新增了更优先的密钥类型，会误报 `SSH host fingerprint changed`，必须手动重新信任。方向是安全的（拒绝连接），但会误伤正常使用 | `embedded_ssh.rs` `trust_or_verify_host_fingerprint`、`known_hosts.json`。要改成按密钥类型存储（类似 OpenSSH `known_hosts`），会牵动存储格式、`HostFingerprintStatus` 以及 daemon / MCP / 桌面三处调用 |
| K2 | 待修 | Dropbear 上传的规避靠 banner 里的 "dropbear" 字样 | 其它在 SFTP 上限制写入大小的服务器不会被识别：写入返回 0 字节时能自动减半重试，但像 Dropbear 在 30000 字节写入下"卡死在 128 KiB"那种失败是**挂起**而不是错误，无法兜底。libssh2 为什么会停住没有查明，只测得 16 KiB 写入可行 | `sftp_transfer.rs` `upload_chunk_for_server`。若遇到新的卡死服务器，需要找根因（libssh2 SFTP 写入流水线与小窗口的交互）或给上传加进度超时 |
| K3 | 待修 | 证书认证被拒时，错误信息不区分原因 | 证书过期、principal 不匹配、CA 不被信任，客户端都只看到 `the server rejected these credentials`，用户无法判断是证书问题。矩阵只断言了"被拒绝"，没有核对文案 | `transport/russh_impl/mod.rs` `userauth_pubkey_file`（证书分支）。可以在失败时读取证书有效期 / principal，给出提示 |
| K4 | 待修 | RSA 的 `ssh-rsa`（SHA-1）回退没有任何记录 | 回退到 SHA-1 签名发生时，`app.log` 和审计里都看不出来，事后无法知道哪些主机在用旧签名。迁到 russh 后回退逻辑在传输层内部（`userauth_pubkey_file` 的第二次尝试），同样不打日志 | `transport/russh_impl/mod.rs`：成功时应写一条诊断日志；也可以考虑提供关闭它的开关 |
| K5 | 待修 | 审批文件只有 daemon 会压缩，且过期条目每次加载都重新过滤 | 只用一次性 CLI、从不启动 daemon 的用户，`approvals.jsonl` 里的过期记录永远不会被清理，文件会一直增长 | `approval.rs`：加载刻意改成只读（避免 CLI 命令截断共享存储），压缩只在 `cleanup_expired_approvals`（daemon 启动后 1 分钟起，每小时一次）。可以在 CLI 提供显式清理命令 |
| K6 | 待修 | `containers start`、`service_running` 没有接到所有入口 | 只有 CLI 有 `containers start`；MCP、daemon、桌面没有。`container_runtimes` 返回了 `service_running` / `start_command`，但桌面 `ContainersPanel` 没有展示 | 违反"四个入口共用一个核心"的约定。需要补 MCP 工具（同时更新 `docs/skills.md` 和"71 个工具"的计数断言）、daemon 路由（同时更新 `docs/api.yaml`）与前端展示 |
| K7 | 限制 | 证书认证只认私钥旁边的 `<key>-cert.pub` | 没有 `cert_path` 之类的主机字段，桌面界面也没有入口；证书不在私钥旁边时无法使用。主机证书（CA 签发的服务器主机密钥）没有做校验 | `embedded_ssh.rs` `user_certificate_for` |
| K8 | 限制 | `host add --passphrase` 的值会出现在进程参数里 | 与已有的 `--password` 一样，同机其他用户可通过进程列表看到。MCP / daemon 设置口令的路径没有逐个核对 | `bin/agent2ssh.rs`。可以补从环境变量或标准输入读取 |
| K9 | 限制 | 内置算法默认值刻意只含较新的算法 | 只提供 SHA-1 密钥交换、CBC 加密或 HMAC-SHA1 的旧服务器默认连不上，必须用 `agent2ssh algo set` 显式放开（现在错误信息会指路） | `ssh_algo.rs` `safe_defaults`，这是安全取舍，不是缺陷 |
| K10 | 已解决 | RSA 用户证书曾被签成 legacy `ssh-rsa-cert-v01@openssh.com` | OpenSSH 8.8+ 不提供该算法，RSA 证书一律被拒。2026-10-22 修复：传输层升级到 `russh 0.64.1`（其 `client_make_to_sign` 现在把哈希带进证书类型名），证书改用 `authenticate_certificate_with` + 本仓库的 `LocalKeySigner`（按调用方给的哈希签名，RSA 证书用 SHA-2）。矩阵里 5 个 OpenSSH 版本的 RSA 证书用例均通过，服务端日志为 `Accepted … RSA-CERT … CA` | `transport/russh_impl/mod.rs` `LocalKeySigner` / 证书分支。回归由 `scripts/e2e-ssh-matrix.sh` 的 `rsa* user certificate` 用例覆盖 |
| K11 | 已解决 | 加密的传统 PEM EC 私钥曾无法读取 | `ssh-keygen -m PEM -N <pass>` 的 ECDSA 密钥解密后仍是 SEC1，而传输层的加载器把解密结果一律按 PKCS#1 解析，报 `Pkcs1: PKCS#1 ASN.1 error: expected INTEGER, got OCTET STRING`。2026-10-23 修复：新增 `legacy_pem`，自己按 OpenSSL 的 `DEK-Info` 方案解密（AES-128/192/256-CBC + MD5 `EVP_BytesToKey`），再把明文 PEM 交回 `russh::keys::decode_secret_key`——不写临时文件，也不引入第二个密钥解析器。错误口令现在会明确说「padding 不合法」而不是 ASN.1 错误 | `src/legacy_pem.rs`、`transport/russh_impl/mod.rs` 的 `load_private_key`。矩阵里 `ecdsa256-pem key, encrypted` 由 SKIP 变为 PASS |
| K12 | 限制 | 不再支持 SHA-1 密钥交换，只提供 `diffie-hellman-group14-sha1` 的服务器连不上 | 传输层（russh）没有实现任何 SHA-1 KEX，`algo set` 会把该值存下来，连接时才报 `unsupported SSH algorithm 'diffie-hellman-group14-sha1' in preference list (supported: ...)`。受影响的是 OpenSSH 6.5 以前默认配置的设备 | `transport/russh_impl/mod.rs` `apply_prefs` / `russh::kex`。libssh2 时代可用。没有本地修复路径，只能等上游或换后端；错误信息已明确指出 |

## 2. 环境与工程问题

| # | 状态 | 问题 | 说明 |
|---|------|------|------|
| E1 | 待修 | GitHub Actions 的任务没有 `timeout-minutes` | Windows 任务出现过两次长时间无输出（单元测试一次，`cli_contract` 一次），默认会跑数小时才超时，取消后日志也取不到。加上超时会让挂起变成带日志的失败（详见 Windows 交接文档第 1 节） |
| E2 | 待修 | `scripts/e2e-container.sh` 的默认镜像在 Apple container 上拉不下来 | 默认 `lscr.io/linuxserver/openssh-server`，Apple container 会拒绝其跨注册表的令牌交换（`authorization server ghcr.io is not in the same registrable domain as registry lscr.io`）。矩阵脚本已自动换成 `ghcr.io`，`e2e-container.sh` 没有，需要手动设 `AGENT2SSH_E2E_IMAGE` |
| E3 | 限制 | Apple container 拉取镜像又慢又不稳 | 实测首次拉取约 8 分钟（几十 KB/s），中途出现过一次 `HTTP2 StreamClosed`，重试后成功。属于网络环境 |
| E4 | 已解决 | 默认特性（桌面应用）在干净克隆上 `cargo check` 失败 | 曾因缺 `binaries/<name>-<triple>` 由 `tauri_build` 直接 abort。2026-10-22 起 `build.rs` 在找不到本 target 的 sidecar 时自动注入 `bundle.externalBin: []`，并打印一行指向 `scripts/prepare-sidecars.sh`；打包任务先构建 sidecar，行为不变 |
| E5 | 待修 | `cargo clippy --all-targets --features daemon` 报 `items_after_test_module` | `agent2ssh-daemon.rs` 的 `mod tests` 后面还有 `main`。合并前两边都是这样，只影响这种检查方式（带 `--bins` 的检查通过） |
| E6 | 限制 | `container_discovery` 的库测试依赖本机服务状态 | Apple `container` 已安装但服务没启动时，已安装的运行时会被报告为"未运行"而不是列出容器，测试对这种情况已放宽；设置了 `AGENT2SSH_CONTAINER_AUTOSTART=1` 的环境里行为会不同 |
| E7 | 已发生 | 之前几次集成测试没有隔离配置目录，写入过真实的 `~/.agent2ssh` | 一次运行清空了真实的 `approvals.jsonl`（无备份，无法恢复）；`resident_session` 测试在隔离前向真实的 `audit.jsonl` 追加过 `source: "test"` 的记录（`cd /tmp`、`pwd` 等，host 为 `local`），这些行仍留在文件里。现在所有集成测试都经 `tests/common` 隔离并拒绝真实目录；残留记录需要手动按 `source":"test"` 过滤 |
| E8 | 限制 | 真实 Apple container 服务"从停止到启动"的路径没有实测 | `containers start` 与 `AGENT2SSH_CONTAINER_AUTOSTART` 只用假的 `container` 脚本测过，因为实测需要先停掉正在使用的服务 |

## 3. 未验证的范围（不能说"支持得好"）

已验证的是：OpenSSH 8.3 / 8.8 / 9.3 / 9.9 / 10.3 与 Dropbear，在 Docker 与（部分）Apple container 上，ed25519、RSA-3072、ECDSA P-256/384/521（OpenSSH 与 PEM 容器）的明文 / 带口令密钥与证书，密码，ssh-agent，1 MiB SFTP 往返，以及一台限制成旧算法的 OpenSSH 8.3。矩阵脚本是 `scripts/e2e-ssh-matrix.sh`。

没有验证的：

| 范围 | 说明 |
|------|------|
| 多轮 keyboard-interactive（二次验证） | 代码里有多轮处理，但没有真实服务端测过 |
| FIDO / 安全密钥（`*_sk`） | 默认密钥列表里有，没有测过 |
| 跳板机 / ProxyJump 的矩阵组合 | 只有较早的单独测试，没有按服务端版本和认证方式展开 |
| 端口转发、PTY 会话按服务端版本 | 矩阵只覆盖连接、exec、SFTP |
| OpenSSH 7.x 之前的真实旧版本 | 目前用"限制了算法的 8.3"模拟，真实老系统的握手细节可能不同 |
| OpenSSH 与 Dropbear 之外的实现 | Windows 自带 OpenSSH、各类网络设备、商业 SSH 服务器都没碰过 |
| 其它密钥类型的反向用例 | "未授权密钥 / 错误口令 / 错误 principal"的拒绝用例只对 ed25519 做过，RSA 与 ECDSA 只测了成功路径 |
| Apple container 上的完整矩阵 | 只跑了 OpenSSH 10.3 与 Dropbear；8.x / 9.x 和旧算法服务端只在 Docker 上跑过 |
| Windows（包含矩阵脚本、证书、`ssh-rsa` 重试、密钥格式探测） | 见 [reports/WINDOWS-HANDOVER.md](reports/WINDOWS-HANDOVER.md)；CI 上 Windows 任务在 `4d24c11` 全部通过，但那不等于真机验证 |
| 懒加载审批存储在桌面 / daemon 里的实际表现 | 单测与 daemon 集成测试通过，没有在真实桌面应用里手动试过 |

### 3.1 Pageant 已完成真实验收（2026-10-23）

Windows 11 上的 PuTTY / Pageant / plink 0.85 对本地 Docker 中的 OpenSSH 10.3 完成认证和传输验收，详情见 [Pageant 验收报告](reports/PAGEANT-ACCEPTANCE-2026-10-23.md)。本轮使用全新的 `AGENT2SSH_CONFIG_DIR`，host 不配置密钥文件或密码；Windows OpenSSH `ssh-agent` 服务保持 `Stopped` / `Disabled`，实际打开其命名管道也超时。应用生成的 Ed25519 和 RSA-3072 测试密钥通过临时 Alpine 容器的 `putty` 包所提供的 `puttygen` 转为 PPK 后，Pageant 正常列举密钥并完成签名；应用诊断日志确认实际认证方式为 `agent`。

| 验收项 | 结果 |
|------|------|
| Ed25519 认证和命令执行 | 通过 |
| 多身份，仅授权第二把密钥 | 通过；探针核实了身份顺序，plink 明确先拒绝 key #0，再接受 key #1 |
| 1 MiB SFTP 上传、下载 | 通过；往返 SHA-256 相同 |
| RSA-3072 认证 | 通过；服务端有效配置对 RSA 签名仅接受 `rsa-sha2-256` / `rsa-sha2-512`，不包含 `ssh-rsa` |
| 空 Pageant、未授权密钥、关闭 Pageant | 同一隔离 host 均按预期认证失败 |
| 重新加载密钥 | 恢复成功 |
| 无默认 feature 的 Rust library tests | 744 passed / 0 failed / 1 ignored；实机探针按预期忽略 |

此前“加载密钥后停在 `Requesting keys`”的现象本轮没有复现，旧根因尚未查明，不能据此断言是模态对话框或排除某一层。本轮没有修改 SSH 传输实现。

**共存边界仍然存在**：Windows 上优先打开 OpenSSH agent 命名管道，仅在打开失败时尝试 Pageant；如果该管道能打开，但 agent 为空或没有匹配密钥，当前实现不会再尝试 Pageant。本轮验证的是 OpenSSH agent 不可用时的 Pageant 路径。

## 4. 维护约定

- 解决一项就把它从这里删掉，并在 `CHANGELOG.md` 记一笔；新发现的问题先在这里登记。
- 编号只增不改，被删除的编号不再复用。

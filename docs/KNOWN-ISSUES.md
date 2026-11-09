# 已知问题与未覆盖范围

日期：2026-11-04（K1、K3、K4、K5、K6 已修复；K10、K11 均已修复；K12 为上游限制；K13 为设计限制；E1、E2、E5、E7 已解决并按维护约定移入 `CHANGELOG.md`）。基线：macOS 开发机 + WinR9，Docker / wslc / Apple container；GitHub Actions。Windows 真机验证已于同日完成，结果见 [reports/WINDOWS-HANDOVER.md](reports/WINDOWS-HANDOVER.md) §7。

本文只记录 **已经知道、尚未解决** 的事项，以及 **没有验证过所以不能下结论** 的范围，按处理价值排序。已修复的问题在 `CHANGELOG.md`；历史缺陷快照在 `DEFECTS.md`（0.2.1 时代，已归档）；Windows 相关的细节在 [reports/WINDOWS-HANDOVER.md](reports/WINDOWS-HANDOVER.md)，此处只列条目。

状态说明：**待修**＝行为不对，有明确方向；**限制**＝按当前设计成立，使用者需要知道；**未验证**＝没有证据说好或不好。

## 1. 行为问题

| # | 状态 | 问题 | 影响 | 位置 / 下一步 |
|---|------|------|------|---------------|
| K2 | 已修（待复测旧版本） | SFTP 上传在特定服务器上「挂起」而没有错误 | 旧记录描述 libssh2 在 Dropbear 约 30000 字节写入下卡死在 128 KiB，靠 banner 里的 "dropbear" 字样降到 16 KiB 规避。2026-11-03 在 Docker 上重测：Dropbear 2024.85 用**默认 64 KiB** 与 16 KiB 分块各传 8 MiB 均成功（485 ms / 1 s，MD5 一致），OpenSSH 8.8、9.3、9.9、10.3 四个版本上传下载全部逐字节一致——旧症状在 russh 后端不复现，banner 启发式已无实测依据。真正的缺陷是另一件事：所有自愈逻辑都依赖 `Ok(0)`，而挂起意味着根本不返回，因此新增了进度上限，停滞时报告「写到多少字节、用的多大写入」而不是无限等待 | `sftp_transfer.rs` `upload_chunk_for_server`、`StallGuard`。若仍遇到卡死，先确认该服务器是否根本没有 sftp-server（本次复现的第一版就是这个原因），再看 `STALL_TIMEOUT` 的 600 s 是否对该链路过长 |
| K7 | 限制 | 证书认证只认私钥旁边的 `<key>-cert.pub` | 没有 `cert_path` 之类的主机字段，桌面界面也没有入口；证书不在私钥旁边时无法使用。主机证书（CA 签发的服务器主机密钥）没有做校验 | `embedded_ssh.rs` `user_certificate_for` |
| K8 | 限制 | 旧版主机凭据参数可能暴露 | `host add --password` 和 `--passphrase` 的值可能出现在进程参数中。新增 `--password-stdin` 和 `--passphrase-stdin` 可从重定向标准输入读取，且保存失败时拒绝新密码明文回退；旧参数仍保留，旧密码参数的锁库回退行为不变。配置临时文件打开失败不会修改凭据，但后续写入或重命名失败及崩溃仍没有跨文件事务回滚。Windows 真机与其他凭据入口未全面验证。 | `bin/agent2ssh.rs`；建议脚本和交互式 shell 使用标准输入方式。 |
| K9 | 限制 | 内置算法默认值刻意只含较新的算法 | 只提供 SHA-1 密钥交换、CBC 加密或 HMAC-SHA1 的旧服务器默认连不上，必须用 `agent2ssh algo set` 显式放开（现在错误信息会指路） | `ssh_algo.rs` `safe_defaults`，这是安全取舍，不是缺陷 |
| K10 | 已解决 | RSA 用户证书曾被签成 legacy `ssh-rsa-cert-v01@openssh.com` | OpenSSH 8.8+ 不提供该算法，RSA 证书一律被拒。2026-10-22 修复：传输层升级到 `russh 0.64.1`（其 `client_make_to_sign` 现在把哈希带进证书类型名），证书改用 `authenticate_certificate_with` + 本仓库的 `LocalKeySigner`（按调用方给的哈希签名，RSA 证书用 SHA-2）。矩阵里 5 个 OpenSSH 版本的 RSA 证书用例均通过，服务端日志为 `Accepted … RSA-CERT … CA` | `transport/russh_impl/mod.rs` `LocalKeySigner` / 证书分支。回归由 `scripts/e2e-ssh-matrix.sh` 的 `rsa* user certificate` 用例覆盖 |
| K11 | 已解决 | 加密的传统 PEM EC 私钥曾无法读取 | `ssh-keygen -m PEM -N <pass>` 的 ECDSA 密钥解密后仍是 SEC1，而传输层的加载器把解密结果一律按 PKCS#1 解析，报 `Pkcs1: PKCS#1 ASN.1 error: expected INTEGER, got OCTET STRING`。2026-10-23 修复：新增 `legacy_pem`，自己按 OpenSSL 的 `DEK-Info` 方案解密（AES-128/192/256-CBC + MD5 `EVP_BytesToKey`），再把明文 PEM 交回 `russh::keys::decode_secret_key`——不写临时文件，也不引入第二个密钥解析器。错误口令现在会明确说「padding 不合法」而不是 ASN.1 错误 | `src/legacy_pem.rs`、`transport/russh_impl/mod.rs` 的 `load_private_key`。矩阵里 `ecdsa256-pem key, encrypted` 由 SKIP 变为 PASS |
| K12 | 限制 | 不再支持 SHA-1 密钥交换，只提供 `diffie-hellman-group14-sha1` 的服务器连不上 | 传输层（russh）没有实现任何 SHA-1 KEX，`algo set` 会把该值存下来，连接时才报 `unsupported SSH algorithm 'diffie-hellman-group14-sha1' in preference list (supported: ...)`。受影响的是 OpenSSH 6.5 以前默认配置的设备 | `transport/russh_impl/mod.rs` `apply_prefs` / `russh::kex`。上一代 libssh2 后端可用。没有本地修复路径，只能等上游或换后端；错误信息已明确指出 |
| K13 | 限制 | `ai_allowed`（主机“仅人工”）只约束 `hosts.json` 里的 SSH 主机，`local:*` 永远放行 | 这是刻意的，不是遗漏。主机标为仅人工，限制的是一项 agent 本来必须被**授予**的能力：凭据、网络可达性、跳板机。本地目标什么也限制不了——agent 进程本来就跑在这台机器上，不需要本服务也能起一个 shell（`sh -c`、`Command::new`、`subprocess`），所以拒绝 `local:gitbash` 不会让命令不发生，只会让它换一条路走，同时多出一个看起来像防护、实际不是的设置。本地路径真正提供的是捕获、超时、进程树回收和审计，这些与发起者是谁无关。**两个真正的边界不在这个开关的职责内**：① `local:docker:<id>` 之类的容器目标进入的是另一个隔离域，agent 没有本服务就得用它自己的运行时凭据才能到达；② `local:*` 被转发到**远程 daemon** 时命令跑在那台机器上，而不是调用方机器上——正常使用不会出现，除非是刻意要绕过的 agent。 | `execution_control.rs` `host_allows_source` 的文档注释写了理由，`a_local_target_is_not_gated_by_the_human_only_flag` 钉住行为。若将来要管 ①②，需要单独设计，不是在这里补一个判断 |

| K14 | 限制 | daemon 停止仍依赖 PID 快照 | `daemon.pid` 只存 PID，没有最初启动时间；进程名和 argv 可伪造。停止前需健康响应 PID 匹配、**健康响应报出的配置目录与本进程一致**、且管理员令牌获准关闭，信号前再核对启动时间；共享或被复制的令牌不能证明配置目录的唯一身份，检查与发信号也不是原子操作，秒级启动时间可能碰撞。无法识别或健康检查不可用时会保留 PID 并拒绝停止，需人工检查。重命名或新平台后缀的 daemon 也可能不在名称白名单。`/health` 的 `config_dir` 缺失（旧 daemon）同样按"无法识别"拒绝，并提示先 `agent2ssh daemon restart` 升级后重试。**2026-11-05 收窄**（不是修复）：`/shutdown` 接受 `expect_pid`，服务端在完成 admin 认证后核对请求指定的 PID——"健康检查命中 A、监听者已换成同 token 的 B、旧客户端的 shutdown 落到 B"这条竞态现在由 B 以 409 拒绝，A 的停止请求不会再被 B 顶替执行。旧 daemon 会忽略该字段并照旧关闭，`stop_daemon_process` 对此写一条 warn 并在日志里说明"检查未执行"，不把它当作透明兼容；显式传 `expect_pid: null` 返回 400。这次改动**取代**了同日更早的 `/shutdown/guarded` + `X-Agent2SSH-Expected-Pid` 请求头方案：那条路由和那个头都删了，同一个保证改由 `/shutdown` 的 `expect_pid` 承担——不为「已验证可用」留第二套并行停止协议，否则旧的「不兼容就安全拒绝」会被两套实现各自解释。仍未解决：pidfd、PID 复用、共享令牌伪造、跨平台原子性。Windows 侧退出判据 `process_handle_is_signaled` 在 `OpenProcess` 被拒绝时只返回 false，停止按"无法确认"保守处理。该拒绝分支的第一环已实测（2026-11-10）：受限令牌（deny-only Administrators + Everyone/Users 限制 SID）下对 `services.exe`、`lsass.exe` 调 `OpenProcess(PROCESS_SYNCHRONIZE)` 稳定返回 `ACCESS_DENIED(5)`，函数即为 false，三轮一致；同一子进程对自身仍能打开，说明失败来自目标进程的 DACL 而非令牌失效（后续 `pid_definitely_missing` 与 `Unknown` 归类仍是代码路径，探针没有造出「sysinfo 无条目 + 拒绝」的组合）（`reports/evidence/cnb-issue-2-denied-2026-11-10/`）。这只证明"拒绝时不会误判为退出"，不改变 PID 绑定本身仍未解决。 | `daemon_control.rs`、`daemon_activity.rs`、daemon 的 `daemon_shutdown`；回归由 `tests/daemon_stop_e2e.rs` 的 `a_daemon_refuses_a_shutdown_that_names_another_process` 钉住；`a_stop_that_reached_the_wrong_daemon_stops_nothing`（真实 A/B 进程 + 代理换监听者）与 `a_daemon_that_ignores_the_expected_pid_stops_and_says_so` 是 `#[cfg(unix)]`，Windows 上不执行。后续仍需可绑定进程身份的跨平台停止协议，并考虑旧 PID 文件兼容。 |


| K15 | 限制 | 部分请求仍可能延迟 daemon 停止 | 关闭时会取消等待中的审批、session 读取和本地一次性命令；`/exec` 已建立的 SSH 通道会结束本地等待并中断连接，等待通道关闭时也可响应取消。但无 PTY 的远端命令可能继续运行，结果记为未知。SSH 连接建立、通道启动和阻塞写入阶段，以及 `/exec-multi`、本机代理执行、playbook 和健康快照等其他路径仍可能超过停止等待期限；客户端会先等宽限期过去，再向该 PID 发送一次强制终止信号并再等 2 秒，仍未退出才报错并保留 PID 文件。`/exec/stream` 的网络背压已在 macOS + Docker SSH fixture 验证；终端 WebSocket 背压和 Windows 实机尚未验证。 | 后续需在确保远端资源与审计语义明确的前提下处理其他阶段及请求的协作式取消。 |

## 2. 环境与工程问题

| # | 状态 | 问题 | 说明 |
|---|------|------|------|
| E3 | 限制 | Apple container 拉取镜像又慢又不稳 | 实测首次拉取约 8 分钟（几十 KB/s），中途出现过一次 `HTTP2 StreamClosed`，重试后成功。属于网络环境 |
| E6 | 限制 | `container_discovery` 的库测试依赖本机服务状态 | Apple `container` 已安装但服务没启动时，已安装的运行时会被报告为"未运行"而不是列出容器，测试对这种情况已放宽；设置了 `AGENT2SSH_CONTAINER_AUTOSTART=1` 的环境里行为会不同 |
| E8 | 限制 | 真实 Apple container 服务"从停止到启动"的路径没有实测 | `containers start` 与 `AGENT2SSH_CONTAINER_AUTOSTART` 只用假的 `container` 脚本测过，因为实测需要先停掉正在使用的服务 |

## 3. 未验证的范围（不能说"支持得好"）

已验证的是：OpenSSH 8.3 / 8.8 / 9.3 / 9.9 / 10.3 与 Dropbear，在 Docker、（部分）Apple container 与 Windows 11 真机（2026-11-04，Docker 29.9.2，矩阵 190 passed / 0 failed）上，ed25519、RSA-3072、ECDSA P-256/384/521（OpenSSH 与 PEM 容器）的明文 / 带口令密钥与证书，密码，ssh-agent（macOS/Linux；Windows 的 Pageant 见 §3.1），1 MiB SFTP 往返，以及一台限制成旧算法的 OpenSSH 8.3。矩阵脚本是 `scripts/e2e-ssh-matrix.sh`。

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

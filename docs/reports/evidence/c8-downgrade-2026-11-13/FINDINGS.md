# C8：旧版本二进制读到带 tag 的引用会做什么（实测）

CNB #26 的崩溃/回滚矩阵里，C8 原本只有「源码推断 + 构造回归」：阶段 2 A/B 把引用从
裸 `$agent2ssh-secret$` 改成 `$agent2ssh-secret$:<tag>`，旧版本的引用判定是**精确相等**，
于是会把带 tag 的字符串当成明文口令重新密封。这份证据用两个真实二进制把它测了出来，
并顺带发现一处比设计稿更严重的时机差异。

## 结论

1. **锁定态安全**：库未解锁时，旧版本只读命令不改动 `hosts.json` / `secrets.enc`（逐字节相同）。
2. **解锁态即损坏，不需要写命令**：旧版本 CLI 在启动时对除 `completions` / `webdav decrypt` /
   `events` 之外的每条命令都跑 `migrate_plaintext_secrets()`。库已解锁时，**一次 `host list`
   就足以**把 `$agent2ssh-secret$:<tag>` 当成明文口令密封：引用退化成裸 marker，
   `secrets.enc` 丢掉 `tag` 字段并退回 v2 形状，代次 +1，另留下一个 `hosts.json.bak`。
3. **新版本读回该状态不会静默用错凭据**：引用会被还原成当初被封存的那串 41 字符文本，
   与库中无 tag 的条目对不上，`host list` 打印明确 warning 且**不使用**该凭据。
4. **可恢复**：`agent2ssh config-repair --restore-pair 0 --confirm` 把两个文件逐字节恢复到
   降级前（hash 与降级前副本相同），之后的保存保持 tag、不再告警。

## 环境与二进制

| 项 | 值 |
|---|---|
| 主机 | macOS 27.2.1（26B131），arm64，uid 501，SIP enabled |
| 旧版本 | commit `48ac012`（阶段 A/B 之前），`cargo build --no-default-features --bin agent2ssh`，sha256 `88751960…33433c3` |
| 新版本 | commit `e31e561` 工作树，sha256 `46b2b22b…f1a13a3` |
| 两者 `--version` | 都是 `agent2ssh 0.4.5`（**版本号无法区分新旧**，见下文） |

`0.4.5` 在两边一样，所以脚本不靠版本号判断，而是靠可观测行为：新版本写入的引用带 tag，
旧版本写入的是裸 marker 且 `secrets.enc` 里没有 `tag` 字段。

## 用例与结果

| # | 操作 | 结果 |
|---|---|---|
| 1 | 新版本 `secrets set-password` + `host add probe-a --password-stdin` | 引用 `$agent2ssh-secret$:7uWsVY94…`；条目 tag 相同；`hosts.json` 代次 1 |
| 2 | **锁定态**旧版本 `host list`（不提供主密码、stdin 关闭） | 列出主机；两个文件 sha256 **未变** |
| 3 | **解锁态**旧版本 `host list` | 引用变成裸 `$agent2ssh-secret$`；条目 `tag` 变 `None`；代次 1→2；出现 `hosts.json.bak` |
| 4 | 新版本再 `host add probe-c` | `probe-a` 的引用**回到** `$agent2ssh-secret$:7uWsVY94…`（当初被封存的那串文本）；条目 `tag` 为 `""`；随后 `host list` 打出下面的 warning |
| 5 | 旧版本再 `host add probe-d` | 所有引用（含 `probe-c`）又变裸 marker；`host:probe-a` / `host:probe-c` 的密文长度 76 |
| 6 | 新版本 `config-repair --restore-pair 0 --confirm` | 预览显示「paired copies present… hashes match record: true」；恢复后两个文件与降级前副本 **`cmp` 相同**；后续保存 tag 不变、无 warning |

第 4 步的 warning 原文：

```
warning: credential reference for host 'probe-a' password was written against content
tag 7uWsVY94ELgJ9Rcj5XifWA, but secrets.enc now has no matching entry; the stored
secret is NOT used. Re-enter the credential or restore a paired hosts.json/secrets.enc
snapshot.
```

## 三处值得单独说

**一、「保存时才坏」是低估。** 设计稿与 CHANGELOG 原本的措辞是「旧程序会把带 tag 引用当明文
重新密封」，容易被读成「只要不保存就没事」。实测是：旧版本 CLI 的启动迁移
（`src-tauri/src/bin/agent2ssh.rs:1884` 的 `migrate_plaintext_secrets()`）对几乎所有子命令都生效，
解锁态下**一次只读命令**就完成同样的破坏。锁定态是安全的，因为迁移在未解锁时是 no-op。

**二、旧版本连 `tag` 字段都静默丢弃。** 旧结构的 `EncryptedEntry` 只有 `nonce` / `ciphertext`，
没有 `deny_unknown_fields`，所以能读 v3 文件，但重写时把 `tag` 整个丢掉、把文件降回 v2 形状。
这不是报错，是静默降级——文件看起来仍然「正常」。

**三、密文长度的旁证。** `host:probe-a` 的密文从 48 个 base64 字符变成 76 个：41 字符的引用文本
加 16 字节 AEAD tag 是 57 字节，base64 恰好 76。这与「被密封的内容就是那串 41 字符引用」一致，
也与第 4 步「新版本原样还原出同一串文本」互为印证。

## 证据分级

- **实测**：第 2–6 步的全部文件级变化、warning 文本、逐字节恢复、恢复后 tag 保持。
- **实测（间接）**：被封存的明文就是那串 41 字符引用（密文长度 + 新版本原样还原两个独立观察）。
- **推断**：旧版本随后会把该 41 字符文本当作 SSH 口令发送出去——`48ac012` 的
  `internalize_secrets` 只识别裸 marker（`src-tauri/src/store.rs:565`），tagged 形式会原样留在
  `password` 字段上。**没有起真实 SSH 服务观察认证报文**，所以不写成实测。
- **未验证**：交互式解锁（stdin 提示）路径是否同样触发迁移；daemon / 桌面端二进制
  （`agent2ssh-daemon.rs` 启动时也调用同一函数，未单独测）；Windows。

## 复现

```sh
git worktree add /tmp/a2s_old 48ac012
cd /tmp/a2s_old/src-tauri && CARGO_TARGET_DIR=/tmp/a2s_old_target \
    cargo build --no-default-features --bin agent2ssh
cd docs/reports/evidence/c8-downgrade-2026-11-13
python3 run_c8_downgrade.py --old-binary /tmp/a2s_old_target/debug/agent2ssh \
    --old-commit 48ac012 > raw-c8.txt 2>&1
```

脚本使用一次性配置目录、一次性主密码与合成口令，从不触碰 `~/.agent2ssh`；
本次运行的完整输出在 `raw-c8.txt`（20 项检查全部通过）。

## 对文档的影响

- `docs/KNOWN-ISSUES.md` K8：C8 从「未用旧版本二进制复现」改为实测，并写明「解锁态任意命令即触发」。
- `docs/guides/configuration-guide.md`：降级一节补上实测症状与 `config-repair --restore-pair` 的恢复能力。
- `docs/reports/issue26-acceptance-matrix.md`：C8 行升级为实测，未闭合边界里删掉该条。

# C8：旧版本二进制读到带 tag 的引用会做什么（实测）

CNB #26 的崩溃/回滚矩阵里，C8 原本只有「源码推断 + 构造回归」：阶段 2 A/B 把引用从
裸 `$agent2ssh-secret$` 改成 `$agent2ssh-secret$:<tag>`，旧版本的引用判定是**精确相等**，
于是会把带 tag 的字符串当成明文口令重新密封。这份证据用四个真实二进制（旧 CLI、旧 daemon、
由旧桌面命令体编出的探针、当前 CLI/daemon 作对照）把它测了出来，
并顺带发现一处比设计稿更严重的时机差异。

## 结论

1. **锁定态安全**：库未解锁时，旧版本只读命令不改动 `hosts.json` / `secrets.enc`（逐字节相同）。
2. **解锁态即损坏，不需要写命令**：旧版本 CLI 在启动时对除 `completions` 与 `webdav decrypt`
   之外的每条命令都跑 `migrate_plaintext_secrets()`（`events` 只是跳过事件订阅者，迁移照跑）。
   库已解锁时，**一次 `host list` 就足以**把 `$agent2ssh-secret$:<tag>` 当成明文口令密封：
   引用退化成裸 marker，`secrets.enc` 丢掉 `tag` 字段并退回 v2 形状，代次 +1，
   另留下一个 `hosts.json.bak`。
3. **新版本读回该状态不会静默用错凭据**：引用会被还原成当初被封存的那串 41 字符文本，
   与库中无 tag 的条目对不上，`host list` 打印明确 warning 且**不使用**该凭据。
4. **可恢复**：`agent2ssh config-repair --restore-pair 0 --confirm` 把两个文件逐字节恢复到
   降级前（hash 与降级前副本相同），之后的保存保持 tag、不再告警。
5. **另外两条入口也一样**（补测）：旧 CLI 没有「提示输入当前主密码」的命令，所以 CLI 侧不存在
   这条风险；真正会解锁的桌面端 `secrets_unlock` 会吃掉 tag，旧 daemon 在拿到
   `AGENT2SSH_MASTER_PASSWORD` 时**在启动瞬间**、任何客户端请求之前就完成同样的降级。


## 环境与二进制

| 项 | 值 |
|---|---|
| 主机 | macOS 27.2.1（26B131），arm64，uid 501，SIP enabled |
| 旧 CLI（主测） | commit `48ac012`（阶段 A/B 之前），`cargo build --no-default-features --bin agent2ssh`，检出 `/private/tmp/a2s_old_48ac012`，sha256 `88751960…33433c3` |
| 旧 CLI（补测） | 同 commit 的另一检出重编，sha256 `766a8b50…8aa9516` |
| 旧 daemon | 同 commit，`--features daemon --bin agent2ssh-daemon`，sha256 `cdae0244…751b2fa` |
| 旧桌面解锁探针 | 同 commit + `desktop-unlock-probe.rs`，`--features tauri`，sha256 `1f79a383…ba472ec` |
| 新 CLI / daemon | commit `e31e561` 工作树（此后只前进过 docs-only 提交，二进制未变），`46b2b22b…f1a13a3` / `c4136c4a…87cb704` |
| 全部 `--version` | 都是 `agent2ssh 0.4.5`（**版本号无法区分新旧**，见下文） |

`0.4.5` 在两边一样，所以脚本不靠版本号判断，而是靠可观测行为：新版本写入的引用带 tag，
旧版本写入的是裸 marker 且 `secrets.enc` 里没有 `tag` 字段。

**重编同一 commit 不等于同一 sha256。** 同一 commit `48ac012` 在本轮先后编出三个不同的
sha256（`88751960…` / `766a8b50…` / `25b24b05…`）：debug 产物内嵌源路径与启用的 feature 集，
换个检出目录、或同时编 daemon / tauri，都会变。所以上面这些 sha256 只用来钉住
「本次跑的是哪个文件」，不能当版本指纹；判断新旧只能看行为。


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
（`src-tauri/src/bin/agent2ssh.rs:1884` 的 `migrate_plaintext_secrets()`）在除 `completions` 与
`webdav decrypt` 之外的每条命令上都会执行，解锁态下**一次只读命令**就完成同样的破坏。
锁定态是安全的，因为迁移在未解锁时是 no-op。

**二、旧版本连 `tag` 字段都静默丢弃。** 旧结构的 `EncryptedEntry` 只有 `nonce` / `ciphertext`，
没有 `deny_unknown_fields`，所以能读 v3 文件，但重写时把 `tag` 整个丢掉、把文件降回 v2 形状。
这不是报错，是静默降级——文件看起来仍然「正常」。

**三、密文长度的旁证。** `host:probe-a` 的密文从 48 个 base64 字符变成 76 个：41 字符的引用文本
加 16 字节 AEAD tag 是 57 字节，base64 恰好 76。这与「被密封的内容就是那串 41 字符引用」一致，
也与第 4 步「新版本原样还原出同一串文本」互为印证。

## 补测：交互式解锁与 daemon

上面把两条路径留在「未验证」：交互式解锁、daemon / 桌面端二进制。两条都补测了，
原始输出在 `raw-c8-gaps.txt`（5 组、26 项检查全部通过，驱动脚本 `run_c8_gaps.py`）。

### 一、旧 CLI 里根本没有「提示输入主密码来解锁」这条路径

先校正我自己的说法：缺口原本写成「交互式解锁（stdin 提示）路径是否同样触发迁移」。翻
`48ac012` 的 CLI 源码，全部子命令里唯一的密码提示是 `secrets set-password` 的
`New master password:` / `Confirm master password:`（`src-tauri/src/bin/agent2ssh.rs:2888`），
而且只在库**尚未初始化**时走；库一旦存在，连改主密码都要求 `AGENT2SSH_MASTER_PASSWORD`
提供当前密码（同文件 2907–2916）。即**旧 CLI 没有任何命令会提示输入当前主密码来解锁**，
`AGENT2SSH_MASTER_PASSWORD` 是 CLI 唯一的解锁入口。

实测与之相符：锁定态下把主密码文本从 stdin 喂给 `host add --password-stdin`，命令以
`credential store is locked` 退出（exit=1），`hosts.json` / `secrets.enc` 逐字节未变，
也没有多出主机。所以「CLI 会顺手拿 stdin 上的密码解锁」不成立——CLI 侧的风险其实是零，
因为它压根没有这条通路。

### 二、真正会解锁的是桌面端命令，它会把 tag 吃掉

用户在界面里输入主密码、点解锁，走的是 Tauri 命令 `secrets_unlock`。GUI 无法无头启动，
所以把 `48ac012` 的这段命令体原样编进一个小 harness（源码见 `desktop-unlock-probe.rs`，
以 `--features tauri` 编译成 `c8_desktop_unlock_probe`），让它对一份带 tag 的配置调用：

| 观察点 | 解锁前 | 解锁后 |
|---|---|---|
| 引用 | `$agent2ssh-secret$:JAsjzuvor_…` | `$agent2ssh-secret$` |
| `host:probe-a` 的 `tag` | `JAsjzuvor_…` | `None` |
| 该条目密文长度 | 48 | 76 |
| `hosts.json` 代次 | 1 | 2 |
| `hosts.json.bak` | 无 | 出现 |

命令本身返回成功（`secrets_unlock: ok`），封存进去的正是那串 41 字符引用
（`the sealed text is '$agent2ssh-secret$:JAsjzuvor_…'`）。也就是说用户看到的是「解锁成功」，
代价是 tag 没了。对应实现是 `48ac012:src-tauri/src/tauri_commands.rs:3278`：
`unlock_or_init` 之后**无条件**调用 `store::migrate_plaintext_secrets()`。

### 三、daemon：锁定态安全，解锁态在启动瞬间就损坏

| 场景 | 结果 |
|---|---|
| 旧 daemon + 锁定库（不设 `AGENT2SSH_MASTER_PASSWORD`） | `/health` 正常应答；两个文件逐字节未变，引用与 tag 都保留 |
| 旧 daemon + `AGENT2SSH_MASTER_PASSWORD` | 启动后引用变裸 marker、条目 `tag=None`、密文 76、代次 1→2、留下 `.bak`；**这一切发生在任何客户端请求之前** |
| 新 daemon + 同一份带 tag 配置 | 引用、tag、代次全不变；随后 `host save` 无 tag warning，凭据仍带 tag |

调用点在 `48ac012:src-tauri/src/bin/agent2ssh-daemon.rs:5675`——`main()` 里、HTTP 监听
起来之前，无条件调用（唯一保护就是「库锁着」时早退）。所以只要有人用环境变量给 daemon
供密码，重启一次就已经降级完了，与他是否连过任何主机无关。

同一脚本跑了两轮，26 项检查与全部结构性观察**逐字相同**，差异只有时间戳、临时端口、随机 tag，
以及头部取版本号那处修正（探测程序会把 `--version` 当主密码，所以只对真正的 CLI 取版本）。
也就是说这些结果不依赖某一次的随机值。

## 证据分级

- **实测**：第 2–6 步的全部文件级变化、warning 文本、逐字节恢复、恢复后 tag 保持；
  旧 CLI 锁定态下把主密码喂给 stdin 的失败与文件不变；桌面 `secrets_unlock` 命令体的损坏；
  daemon 锁定态不变、解锁态启动即损坏、新 daemon 不变。
- **实测（间接）**：被封存的明文就是那串 41 字符引用（密文长度 + 新版本原样还原 + 桌面解锁路径
  打印的 `the sealed text is …` 三个独立观察）。
- **推断**：旧版本随后会把该 41 字符文本当作 SSH 口令发送出去——`48ac012` 的
  `internalize_secrets` 只识别裸 marker（`src-tauri/src/store.rs:565`），tagged 形式会原样留在
  `password` 字段上。**没有起真实 SSH 服务观察认证报文**，所以不写成实测。
- **未验证**：旧桌面端真正的界面交互（只测到命令体，没点过 GUI 的解锁按钮）；
  旧 daemon 在已经损坏之后继续接受请求时的行为；Windows。


## 复现

```sh
git worktree add /tmp/a2s_old 48ac012
cd /tmp/a2s_old/src-tauri && CARGO_TARGET_DIR=/tmp/a2s_old_target \
    cargo build --no-default-features --bin agent2ssh
cd docs/reports/evidence/c8-downgrade-2026-11-13
python3 run_c8_downgrade.py --old-binary /tmp/a2s_old_target/debug/agent2ssh \
    --old-commit 48ac012 > raw-c8.txt 2>&1
```

补测要多编两个二进制：daemon（同一 target 目录；**它忽略 argv，`--version` 会直接起服务**）
和桌面解锁探针（拷进旧工作树的 `src/bin/`，需要 `--features tauri`）：

```sh
cd /tmp/a2s_old/src-tauri && CARGO_TARGET_DIR=/tmp/a2s_old_target \
    cargo build --features daemon --bin agent2ssh-daemon
cp <repo>/docs/reports/evidence/c8-downgrade-2026-11-13/desktop-unlock-probe.rs \
    /tmp/a2s_old/src-tauri/src/bin/c8_desktop_unlock_probe.rs
cd /tmp/a2s_old/src-tauri && CARGO_TARGET_DIR=/tmp/a2s_old_target \
    cargo build --features tauri --bin c8_desktop_unlock_probe

cd <repo>/docs/reports/evidence/c8-downgrade-2026-11-13
python3 run_c8_gaps.py --old-binary /tmp/a2s_old_target/debug/agent2ssh \
    --old-daemon /tmp/a2s_old_target/debug/agent2ssh-daemon \
    --desktop-probe /tmp/a2s_old_target/debug/c8_desktop_unlock_probe \
    --old-commit 48ac012 > raw-c8-gaps.txt 2>&1
```

两个脚本都使用一次性配置目录、一次性主密码与合成口令，从不触碰 `~/.agent2ssh`；
本次运行的完整输出在 `raw-c8.txt`（20 项检查）与 `raw-c8-gaps.txt`（26 项检查），全部通过。

## 对文档的影响

- `docs/KNOWN-ISSUES.md` K8：C8 从「未用旧版本二进制复现」改为实测，并写明「解锁态任意命令即触发」，
  以及桌面解锁与 daemon 启动两条同样会损坏的入口。
- `docs/guides/configuration-guide.md`：降级一节补上实测症状与 `config-repair --restore-pair` 的恢复能力。
- `docs/reports/issue26-acceptance-matrix.md`：C8 行升级为实测，未闭合边界里删掉该条。


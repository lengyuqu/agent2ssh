# 跨文件事务阶段 2：引用自证与成对快照（设计评审稿）

**状态：本文件是 2026-11-12 的设计评审稿，保留原样供追溯。** 阶段 A/B/C/D 已于 2026-11-12
落地（`f98a4b9`），实现与本文的偏差、以及 C1–C8 与 P1–P6 的逐项证据级别见
`docs/reports/issue26-acceptance-matrix.md`；决策取舍见
`docs/plans/cross-file-transaction-phase2-decisions.md`。对应
[CNB #26](https://cnb.cool/zdzso/agent2ssh/-/issues/26)，评审时基线 `cnb/main` = `f98bd83`。
本文写作时**不修改 `src-tauri/src/`、不改 CLI 输入方式、不重做阶段 0/1**；所有「实测」都有
证据路径，其余一律标注为源码事实、推断或未验证。

本文的决策编号用 **P1–P6**，避免与 #22 身份协议 v1 的 D1–D6 混淆。

---

## 0. 现状：源码事实与可复现证据

### 0.1 引用与凭据库

| 事实 | 位置 |
|---|---|
| `SECRET_REF = "$agent2ssh-secret$"`，裸 marker、不带密文身份 | `src-tauri/src/secrets.rs:49` |
| `LEGACY_KEYRING_REF = "$agent2ssh-keyring$"`，指向已移除的 OS keyring 路径 | `secrets.rs:51-53` |
| `is_secret_ref` / `is_current_secret_ref` / `is_legacy_keyring_ref` **全部是精确相等比较** | `secrets.rs:57-70` |
| `EncryptedEntry { nonce, ciphertext }`；account 名作为 AEAD AAD，密文绑定到 account | `secrets.rs:223-233` |
| `EncryptedStoreV2 { version, kdf, salt, generation, entries }`，`generation` 缺省读 0 | `secrets.rs:241-255` |
| `save_map` 整库读-改-写，每次**全量重加密**，`generation = old + 1` | `secrets.rs:683-720`（`:711`） |
| `change_master_password` 重加密全库但 **generation 保持不变** | `secrets.rs:932-975`（`:965-968`） |
| `unlock_or_init` 兼容 v1/v2，用 `create_new` 处理并发创建，初始化 `generation: 0` | `secrets.rs:823-928` |
| `load_map` 按 `is_v1_to_v2_migration_resolved()` 决定跳过版本探测，v1 在下一次 `save_map` 迁移 | `secrets.rs:620-677` |
| account 名稳定：`host:<name>`、`host-key-passphrase:<name>`、`proxy:<id>` | `secrets.rs:119-131` |

**由源码直接推出的两个设计约束**（不是实测）：

1. **新引用格式会让现有判定全部失效。** `externalize_secrets` 的过滤条件是
   `!pw.is_empty() && !is_secret_ref(pw)`（`store.rs:620`），而 `is_secret_ref` 只认精确裸
   marker。写入 `$agent2ssh-secret$:<tag>` 后，它会被当成**真实明文密码**再次
   `store_secret`，造成重复密封、`generation` 递增、引用被覆写；`internalize_secrets`
   （`store.rs:565/578/591`）同样只认裸 marker，解析不到就可能在已解锁时把 password 置
   `None`（`:570-573`）。因此阶段 2 不是一个「新增字段」的改动，而是**所有引用判定点的
   同步迁移**，附带的回归必须覆盖每个调用点。
2. **tag 不能绑定密文字节或密钥。** `change_master_password` 用新 salt 重加密全部条目却
   刻意不动 `generation`（`secrets.rs:965-968`），所以任何基于 `ciphertext`/`nonce`/
   `derived key` 计算的 tag 都会在改主密码后**集体失配**，把一次正常的口令变更变成全库
   引用不可用。tag 必须在重加密后保持不变。

### 0.2 覆盖 `hosts.json` 但不碰 `secrets.enc` 的路径

`SYNCABLE_FILES` 只有 9 个文件，`secrets.enc` 被**显式排除**并列为
`LEGACY_UNSYNCABLE_REMOTE_FILES`（`webdav_sync.rs:35-60`，注释写明是产品决策）：

| 路径 | 位置 | 行为 |
|---|---|---|
| `restore_config_snapshot` → `_unlocked` | `webdav_sync.rs:866-938` | scope = `SYNCABLE_FILES` + `sync_version.json`；先做 `pre-restore` 备份，再用 `write_private_bytes` 直接覆盖，删除快照中没有的可同步文件（`:923-935`） |
| `sync_pull` 写回段 | `webdav_sync.rs:1242-1270` | 持 `.hosts.lock` → `create_sync_backup` → 直接写远端文件 → 删除不在远端集合中的 `SYNCABLE_FILES`（`:1252-1268`） |
| `apply_config_template` | `webdav_sync.rs:951+` | 模板可含 `hosts.json`，同样直接写 |
| 桌面云恢复 `Snapshot::restore` | `desktop_sync.rs:86-140` | 加密 JSON blob 解密校验后持 `.hosts.lock`，`restore_with` 用 `write_private_bytes` 写 `SYNCABLE_FILES`（`:94-130`）；写入失败时调 `restore_config_snapshot_unlocked` 回滚（`:133`） |

这四条都**不经 `save_config`**，因此既不更新 `secrets.enc`，也不校验引用或代次。
#26 正文提到的 `Snapshot::restore` 在当前树确实存在（`desktop_sync.rs:86`，结构在 `:13-18`），
是桌面加密快照的云恢复入口；`restore_config_snapshot`（`webdav_sync.rs:866`）是本地快照恢复，
前者失败时以后者回滚。正文的 `sync_pull` 与上表一致。

### 0.3 备份与提交日志

- 常规保存顺序（`store.rs:786-873`）：`externalize_secrets`（密封并 bump generation）
  → 写/同步 tmp → `store_commit::begin` 写 `.commit.json`（`:823-833`）→
  `fs::copy(hosts.json, hosts.json.bak)`（`:841-851`，**失败非致命**）→
  `replace_file_atomic(tmp, hosts.json)` → `finish()`。
- `.commit.json` 只含 `hosts_sha256` / `secrets_sha256` / `secrets_generation` /
  `tmp_name` / `version`（`store_commit.rs:42-52`）；`recover()` 三分支见 `:147-181`。
- 因此 `hosts.json.bak` 是**上一次 `hosts.json` 的普通副本**，既不含 `secrets.enc` 的
  配对副本，也不与某条 commit log 关联；它保护的是「坏内容」，不是「两文件一致性」。

### 0.4 #28 的实测输入（不是推断）

`docs/reports/evidence/macos-exposure-crash-2026-11-12/FINDINGS.md` 与
`raw-crash.txt`：在真实 CLI 进程上注入两次外部 `SIGKILL`，`lost-tmp` case 的磁盘状态是
`hosts.secrets_generation=1`、`secrets.enc.generation=3`，且 `entries` 已含
`host:crash-target` 密文，而 `hosts.json` 仍是只有 `seed` 的旧版本、密码字段为裸 marker。
`recover()` 在该状态下 **fail-closed**（拒绝写入、保留 log），没有静默用错密文；但它证明
「裸 marker + 代次」不足以让引用自证内容身份。本设计把这份状态作为阶段 2 的**实测输入**，
不把它写成「已复现错配登录」——错配发生在恢复旧 `hosts.json` 的路径，那是源码推断，
尚无真机复现。

### 0.5 与 #22 v1 的 D6 口径关系（不要混淆）

#22 在 2026-11-12 07:13Z 的收尾写过「D6 与 #26 的阶段 2 共用一份口径」，那是**旧草案**
`docs/plans/daemon-identity-protocol.md` 的编号。v1
（`docs/plans/daemon-identity-v1.md:14`）收敛后，**D6 已变成「保留数字 `daemon.pid`，
新增版本化 `daemon.instance.json`；锁内发布、两文件分步不原子、条件清理和降级」**，
是 daemon 记录发布，不是凭据引用/快照。

两者**同构但不等价**：v1 的「分步发布不原子、不一致时保留记录、拒绝自动覆盖、旧程序不
遵守锁时无保证」是本设计应遵循的原则；但 v1 没有替 #26 决定「旧格式兼容期」「tag 不匹配
是否阻断登录」「快照是否参与同步」。本文 P1–P6 独立编号，不引用 D6 作为已决事项。

---

## 1. 目标、非目标与失败模型

**目标**：让 `hosts.json` 中每个引用都能证明「它指向的密文就是当初写入的那一份」，
并且恢复一份旧 `hosts.json` 时，引用与当前 `secrets.enc` 的组合是可判定的——要么匹配，
要么**明确报错**，绝不静默用错密文。

**非目标**（本单不做）：阶段 0/1 的锁与 redo log 重做；CLI 输入方式（#6 已交付）；
daemon 身份绑定（#22–#25）；Windows 实机验证（#27，已交付，结论见 §3.3）；WebDAV 传输协议本身。

**失败模型**（源码推断，除标注外无真机复现）：

| 编号 | 场景 | 当前后果 |
|---|---|---|
| F1 | 恢复旧 `hosts.json` 快照 / pull 到旧版本 / 应用旧模板，同名 host 的口令在这期间被改过 | 旧裸 marker 按 `host:<name>` 解析到**新密文**，登录用错口令且无任何提示 |
| F2 | `hosts.json.bak` 被手工回卷，但 `secrets.enc` 未回卷 | 同 F1，且没有记录说明两份文件不属于同一时点 |
| F3 | 崩溃后 `secrets.enc` 已推进、`hosts.json` 未推进（#28 实测状态） | `recover()` 按 tmp 是否存在决定重放或拒绝；拒绝路径保留代次差，用户只能人工检查 |
| F4 | 改主密码后密文全变 | 现有 `generation` 不变是正确的（内容没变）；任何基于密文的 tag 会误报 F1 |
| F5 | 旧程序（不懂新引用格式）读/写新配置 | 把 `$agent2ssh-secret$:<tag>` 当明文重新密封，或解析失败 |

---

## 2. 方案 A：`secrets.enc` v3 与内容 tag

### 2.1 tag 的定义（必须满足三个条件）

| 条件 | 说明 |
|---|---|
| 稳定 | `change_master_password` 重加密后不变；`save_map` 整库重写时未被修改的条目不变 |
| 可判定 | 引用携带的值能与条目里的值做**常数时间比较**，不匹配可明确报错 |
| 不泄露 | tag 不能是口令或其简单哈希的明文函数，否则 `hosts.json`（可同步）会成为离线爆破弱口令的靶子 |

因此**排除**：`SHA256(ciphertext)`、`HMAC(derived_key, plaintext)`（换密钥即失效）、
`SHA256(plaintext)`（泄露可验证的口令哈希）。

**推荐**：tag 是每次写入该 account 时生成的**随机 128-bit 内容版本 id**（`base64url`），
与条目一起保存；引用携带它。它不携带口令信息，换密钥时随条目一起被重加密/搬运而保持
不变，且每次真正改口令都会换新值——正好把「同名 account 的旧/新密文」区分开。

三个放置位置（**P3 的子项，需拍板**）：

| 位置 | 优点 | 代价 |
|---|---|---|
| a. `EncryptedEntry` 新增明文字段 | 不解密即可比对；`load_map` 可先校验再解密 | tag 本身可见（随机值，无信息量）；`change_master_password` 重建 entries 时必须显式保留（`:947-960` 目前会丢） |
| b. 作为 AAD 的一部分 | 密文与 tag 密码学绑定 | 每次比对都要先解密，无法在锁定态给出精确诊断；改动 `encrypt_entry`/`decrypt_entry` 全部调用点 |
| c. 放进加密 payload | 不额外暴露元数据 | 同样需先解密；`load_map` 的 `String` 返回值要改造成携带 tag 的结构，改动面最大 |

**倾向 (a)**：它让「锁定态也能判断引用是否错配」成为可能，且改动集中在
`EncryptedEntry`、`save_map`、`change_master_password`、`load_map` 四处。代价是
`change_master_password` 与 v1→v2 迁移必须把 tag 一起带走——这一点要写进回归。

### 2.2 引用格式与兼容读法矩阵

新格式：`$agent2ssh-secret$:<tag>`（`:` 后为 base64url 的 128-bit 值）。

| 磁盘值 | 读法 | 写入策略 |
|---|---|---|
| `$agent2ssh-secret$` 裸 marker | 旧格式：按 account 解析，**无法自证内容** | 默认保留；解锁且能算出 tag 时可惰性升级（P1） |
| `$agent2ssh-keyring$` | 旧 keyring 路径：**不可解析**，保留迁移信号（`store.rs:542-545`） | 永不当作 SSH 口令、不自动删除 |
| `$agent2ssh-secret$:<tag>` | 先按 account 取条目，再比对 tag；不匹配 → 明确错误 | 保持 |
| `$agent2ssh-secret$:<非法/空>` | 格式错，**报错并保留原值**，不降级到裸 marker | 拒绝保存 |
| 其他字符串 | 明文（现状），按 `externalize_secrets` 密封 | 不变 |

**实现前提**：所有 `is_secret_ref` / `is_current_secret_ref` / `is_legacy_keyring_ref`
（`secrets.rs:57-70`）改为前缀/结构判定，并逐一核对 `store.rs:565/578/591/620/635/653/692/698/704`
与测试里的 `raw.contains(SECRET_REF)` 断言（`tests/store_transaction.rs:2078/2118/2160`）。
漏掉任何一处都会触发 F5。

### 2.3 tag 不匹配的策略对比（P2）

| 选项 | 行为 | 优点 | 代价 / 失败模式 |
|---|---|---|---|
| ① 报错并阻断登录（**倾向**） | 该字段保留引用、标记 `Unresolved`；连接时给出「凭据内容已变，请重新输入或恢复成对快照」 | 满足「不得静默用错密文」；用户有明确修复路径 | 旧配置在升级后可能大面积报错，需要 P1 的惰性升级与迁移工具 |
| ② 告警 + 仍使用解密值 | 记 warning，照常登录 | 兼容最平滑 | **直接违反 #26 验收**；等于把 F1 合法化，不可选 |
| ③ 降级为无口令 | 把字段置 `None` | 不会用错 | 与 `store.rs:570-573` 现有行为同形，下一次保存会**抹掉唯一引用**，属静默数据丢失，不可选 |
| ④ 报错但仅在审计里记 | 不阻断，事后可查 | — | 同样是静默使用，不可选 |

**推荐 ①**，并把「保留引用、不置 `None`」写成显式断言：这与 `internalize_secrets`
现有「已解锁且条目缺失就置 `None`」的分支（`:570-573`）不同，实现时必须区分
「条目不存在」与「条目存在但 tag 不匹配」两种状态，不能复用同一个 else。

---

## 3. 方案 B：成对快照

### 3.1 两个候选

| | 方案 B1：扩展 commit log 产出 | 方案 B2：独立版本化索引 |
|---|---|---|
| 形态 | 保存时在 `.commit.json` 增加 secrets 快照文件名/哈希，落一份 `secrets.enc.snap.<gen>`，与 `hosts.json.bak` 配对 | 新增 `config_pair_index.json`：`{schema_version, pairs:[{hosts_sha256, secrets_sha256, generation, tag_digest, hosts_bak, secrets_bak, created_at}]}`；同时落 `secrets.enc.bak` |
| 与 `recover()` 的关系 | 复用现有 redo log，崩溃后同一分支处理 | `recover()` 需新增「索引与磁盘不一致」分支；现有三分支不动 |
| 崩溃窗口 | log 是短生命周期文件，承载长期快照语义后，`finish()` 的清理条件变复杂；误删 log 会让快照失去配对依据 | 索引自身是第三个文件，索引写与两份 `.bak` 写同样**不是原子事务**，需要自己的 redo/自校验 |
| 磁盘 / 兼容性 | 不需要新索引文件，但 `.commit.json` 格式 v1→v2 且旧程序不认识 | 新文件不影响旧程序读 `hosts.json`；索引缺失时按「无可信配对信息」保守处理 |
| 回滚 | 回卷 hosts 时必须连同 secrets 快照一起，否则拒绝 | 索引里按 pair 选择，可显式列出「将回卷的两份文件与代次」 |
| 改动面 | `store_commit.rs` + `store.rs` 保存路径 + `recover()` | 新模块 + 保存路径 + 恢复路径 + 快照恢复的三条路径 |

**倾向 B2**：`.commit.json` 的语义是「一次未完成的写入」，把它变成长期快照索引会混淆
两种生命周期；独立索引可版本化、可被 `recover()` 只读校验，且索引缺失时天然 fail-closed。
但 B2 引入第三个文件，其自身的崩溃一致性必须有可复现断言——不能只写「索引是自校验的」。
**这是 P3 的主项，需拍板。**

### 3.2 恢复旧快照时的组合判定

无论选 B1/B2，恢复 `hosts.json` 前应在 `.hosts.lock` 下做一次判定：

| 组合 | 动作 |
|---|---|
| 引用全为裸 marker，且无配对信息 | 允许恢复，但**明确告警**「无法验证引用与当前凭据库的内容一致性」（P1 决定兼容期） |
| 引用带 tag，全部能在当前 `secrets.enc` 找到匹配 tag | 允许恢复 |
| 引用带 tag，任一 tag 不匹配或条目缺失 | **拒绝恢复**并点名 account；若配对信息里存在对应的 `secrets.enc` 备份，提示「恢复成对快照」这一条显式路径 |
| 配对索引缺失/损坏/版本未知 | 拒绝自动恢复，保留现状并给出诊断；不猜测 |
| 用户显式选择「恢复配对快照」 | 同时回卷 `hosts.json` 与 `secrets.enc`，在锁内、原子替换、记录审计；失败则整体回滚 |

`secrets.enc` 按设计**不参与 WebDAV 同步**（`webdav_sync.rs:28-30, 35-45`）。若 P4 决定把
成对快照纳入同步范围，等于把加密凭据库放到远端，与既有产品决策冲突，必须重写同步白名单、
远端 marker 校验、备份加密边界与「旧远端 marker 跳过 `secrets.enc`」的兼容逻辑
（`:47-60, 1601-1639` 已有对应回归）。**本设计默认不纳入**，P4 留给人决定。

### 3.3 Windows 语义（2026-11-12 真机实测，#27 `15e2ac5`）

`replace_file_atomic` 在 Windows 上清理 `FILE_ATTRIBUTE_READONLY` 并对 `fs::rename` 做最多
6 次约 630 ms 退避重试（`store.rs:260-302`）；`write_private_bytes`（`:316-361`）与快照
恢复路径（用同一个 `write_private_bytes`）共用它。2026-11-12 在 Windows 11（10.0.26300）
实测（证据 `docs/reports/evidence/cnb-issue-27-2026-11-12/`，回归
`tests/host_secret_stdin.rs` 的 `windows_shells`）：

| 情形 | 实测结果 | 对阶段 2 的含义 |
|---|---|---|
| 目标带 `FILE_ATTRIBUTE_READONLY` | 只读位被清，替换成功 | 恢复覆盖只读 `hosts.json` 不需要额外步骤 |
| 目标被另一进程以 `FILE_SHARE_READ\|WRITE`（无 DELETE）打开 | 6 次退避后失败，报 `after 6 retries … (os error 5)`，目标字节不变；实测总时长减基线约 650 ms，与 630 ms 退避吻合 | 恢复路径必须把「替换失败」当成可重试的用户可见错误，不能假定一次成功 |
| 同上，但句柄 300 ms 后释放 | 成功（总时长约 674 ms） | 630 ms 窗口够覆盖短命读者，不够覆盖长期持有者 |
| 目标被带 `FILE_SHARE_DELETE` 的句柄打开（Rust `File::open` 的形态） | 成功 | 「另一个 CLI 正在读」不阻塞恢复；阻塞只来自无删除共享的持有者（备份/杀软/编辑器） |
| 目录里有另一个 pid 的残留 `hosts.json.tmp.<pid>` | 不影响保存，残留文件未被误删 | 阶段 2 的成对临时文件可按 pid + 随机名共存；清理只能针对自己创建的 |
| `icacls` 失败 | **未验证**：三种构造手法实测排除（见 FINDINGS §3 D6） | 不能假定加固失败一定发生在 rename 前 |

仍未验证：SMB/网络文件系统上的 rename 与共享删除语义、其他 Windows 版本、真实断电与目录
`fsync` 的持久性。阶段 2 的 `store_transaction.rs` 不应把上述任一条写成跨平台保证。

---

## 4. 阶段 3 评估：append-only journal + compaction（本单只评估）

现状 `save_map` 每次全量重加密整库（`secrets.rs:683-720`），`.secrets.lock` 只把 RMW
串行化，没有消除 RMW。改 journal 的兼容矩阵：

| 接口 | 现状 | 改 journal 后必须保持 |
|---|---|---|
| `unlock_or_init` | 读 `version` 字段，v1 blob / v2 entries；`create_new` 防并发创建（`:823-928`） | 仍能识别 v1/v2/v3；首次创建写一条 init 记录；并发创建时不能两个进程各写一份 journal |
| `change_master_password` | 新 salt 重加密全库，`generation` 不变（`:932-975`） | 要么把 journal 整体重加密（等价于一次 compaction），要么保留旧记录并用新密钥链式解密；**tag 必须不变** |
| v1→v2 迁移 | `load_map` 依迁移标记跳过版本探测，下次 `save_map` 迁移（`:620-677`, `:723-727`） | 迁移标记不能因 journal 而失效；v1 blob 转 entries 时要为每个 account 生成 tag |
| `store_secret` / `delete_secret` | 每次改一个条目触发整库重写 | 追加一条 put/del；compaction 需持 `.secrets.lock` 且崩溃可重放 |
| `store_generation` | 读 `generation` 字段（`:277-293`） | 需从 journal 尾部或头部元数据读，不能要求重放全部记录 |
| `hosts.json.secrets_generation` | 与 `secrets.enc.generation` 对齐（`store.rs:775-800`） | generation 语义不变，否则阶段 0/1 的守卫全部失效 |

**回滚表**：v3 → v2 需要能把 journal 压实回单个 `EncryptedStoreV2`（含 tag 字段的降级处理）；
若 P5 决定阶段 3 不在本单落地，则本单只保留上表作为接口约束，不实施。
**倾向 P5 = 不在本单落地**：阶段 2 的 tag 与成对快照已经改动所有引用判定点和恢复路径，
再叠加存储格式替换会让回滚单元过大；且 #26 验收项针对的是阶段 2 的语义。

---

## 5. 崩溃 / 回滚矩阵（阶段 2 需要逐项补断言）

沿用 #28 已验证的两个真实注入点（`store_crash_injection.rs` 的 `before-rename` /
`before-finish`），扩展到新文件后的状态：

| # | 注入点 / 状态 | 期望 | 现有基础 |
|---|---|---|---|
| C1 | `begin` 后、hosts rename 前（#28 已实测） | `recover()` 重放 tmp；tag 集与 secrets 配对一致 | `store_crash_injection.rs` |
| C2 | hosts rename 后、`finish` 前（#28 已实测） | Case 1 只清 log | 同上 |
| C3 | 可重放但 tmp 丢失（#28 已实测） | 拒绝写入、保留 log、hosts 字节不变 | 同上 |
| C4 | 新增：secrets 快照/索引已写、hosts `.bak` 未写 | 拒绝把「半对」当成有效配对；保留诊断，不自动补写 | 待测 |
| C5 | 新增：两份 `.bak` 已写、索引 rename 前崩溃 | 索引缺失 → 保守拒绝恢复（不猜测配对） | 待测 |
| C6 | 新增：索引存在但 `tag_digest` 与当前 `secrets.enc` 不符 | 明确点名，不允许用旧 hosts 覆盖 | 待测 |
| C7 | 新增：回卷过程中第二次崩溃 | 整体回滚到回卷前状态，两份文件都不处于中间态 | 待测 |
| C8 | 新增：旧程序写新格式（F5） | 新程序能检测出「带 tag 引用被当明文密封」并给出迁移指引，不静默吞掉 | **已实测**（阶段 A/B 之前的二进制 `48ac012`）：解锁态下任意命令即触发重新密封，新程序拒绝该凭据并给出指引；`docs/reports/evidence/c8-downgrade-2026-11-13/` |

每项必须区分「真机 `SIGKILL` 实测」「构造状态回归」「源码推断」三类，不得混写。
C4–C8 在**设计轮**未实现、未实测；落地后的分级见 `docs/reports/issue26-acceptance-matrix.md`
（C1–C3/C5/C7 真机 `SIGKILL`、C8 旧版本二进制实测，C4/C6 构造回归）。

---

## 6. 需人拍板的决策（P1–P6）

| 编号 | 决策 | 选项 | 倾向与主要代价 |
|---|---|---|---|
| **P1** | 旧格式引用（裸 marker / keyring）的兼容期 | (a) 长期兼容、只读不升级；(b) 惰性升级 + 明确弃用时间表；(c) 某版本起拒绝 | **(b)**：(a) 让 F1 永久存在，(c) 会打断旧脚本与历史配置；需定弃用版本与迁移命令 |
| **P2** | tag 不匹配是否阻断登录 | (a) 阻断 + 保留引用 + 明确指引；(b) 告警仍使用；(c) 降级为无口令 | **(a)**；(b)(c) 违反「不得静默用错密文」，(c) 还会抹掉引用 |
| **P3** | 成对快照形态与 tag 放置 | B1 / B2；tag 位置 a / b / c | **B2 + a**；代价是第三个文件的一致性断言与 `change_master_password` 保留 tag |
| **P4** | 成对快照是否纳入 WebDAV 同步 | (a) 不纳入；(b) 纳入并改同步白名单与远端加密边界 | **(a)**：(b) 与「`secrets.enc` 不参与同步」的既有产品决策冲突，改动面跨 `webdav_sync.rs` 与桌面 |
| **P5** | 阶段 3 是否本单落地 | (a) 只评估；(b) 同单实施 journal | **(a)**，理由见 §4 |
| **P6** | 显式修复命令 | (a) 与 #28 的 `repair-commit` 合并设计；(b) 本单单独做；(c) 暂不提供 | **(a)**：#28 已发现不可重放 log 无显式出口，两份口径应合并评审，避免两套恢复命令 |

P1–P6 均未拍板；本设计不替人决定，也不把「倾向」写成已选。

---

## 7. 验收映射与测试计划（本单不实施）

| #26 验收项 | 对应测试 | 现有基础设施 |
|---|---|---|
| 多进程 `host add` × `config restore` 并发后每个引用解析到内容匹配的密文，无 `None` / 错配 | 真实 CLI 多进程并发 + 解密断言；覆盖 §0.2 的四条恢复路径 | `tests/store_transaction.rs::concurrent_writers_keep_every_sealed_secret`；`tests/store_crash_injection.rs` 的隔离目录与解密校验 |
| 恢复旧 `hosts.json` 快照时 tag 不匹配给出明确错误；旧格式引用有真实 CLI 迁移回归 | 构造旧快照 / 裸 marker / keyring 三种输入，断言错误文案与「引用未被抹除」 | `tests/store_transaction.rs` 的 `seed` / `write_log` 夹具 |
| 崩溃/回滚矩阵逐项可复现并区分实测与推断 | §5 的 C1–C8，C1–C3 已有真机注入 | `run_crash_injection.py`、`store_crash_injection.rs` |
| 更新 K8 / `CHANGELOG.md` / 配置指南中 `hosts.json.bak` 说明 | 文档检查 | `docs/guides/configuration-guide.md` §本地配置快照（当前**没有** `.bak` 说明） |

**实施时的改动面清单**（供评审估算，本轮零改动）：
`secrets.rs`（引用判定、v3 结构、save/load、改主密码保留 tag、v1→v2 迁移）、
`store.rs`（`internalize_secrets` / `externalize_secrets` 全部判定点、保存顺序、`.bak`）、
`store_commit.rs`（配对索引的只读校验）、`webdav_sync.rs` 与 `desktop_sync.rs`（恢复前判定）、
`tests/store_transaction.rs` / 新增 `tests/store_phase2_*.rs`、`docs/KNOWN-ISSUES.md` K8、
`docs/guides/configuration-guide.md`、`CHANGELOG.md`。

---

## 8. 与相邻 issue 的边界

- **#27**（已交付 `15e2ac5`）：§3.3 已由源码推断改为真机实测表；`icacls` 失败路径仍是
  未验证（三种构造手法实测排除），SMB、其他 Windows 版本、断电持久性也仍未验证。本设计
  在阶段 2 的验收里不得把 §3.3 的任一实测项扩展成跨平台保证。
- **#28**（已交付 `f98bd83`）：提供 C1–C3 真机注入与 `lost-tmp` 代次差实测；#26 正文原写
  「本单阶段 2 设计是 #27/#28 的前置输入」，实际 #28 先行完成，本设计把它作为**输入**而非前置。
- **#29**（已关闭）：锁库策略统一 fail-closed 的结论已落地；本设计的 tag 不匹配策略与之
  同向（不静默降级），不需要重新评审锁库策略。
- **#22 v1**：仅继承「分步发布不原子、不一致时保留记录」的原则，见 §0.5。

---

## 9. 状态

**本文是设计评审稿（阶段 2 的方案与决策依据）。** 2026-11-12 按
[decisions](cross-file-transaction-phase2-decisions.md) 的推荐默认完成了**阶段 A/B**：
结构化引用解析、`secrets.enc` 内容 tag、`$agent2ssh-secret$:<tag>` 写入与校验、
旧裸 marker 惰性升级、改主密码 tag 稳定、P2 的保守拒绝与明确指引；代码见
`secrets.rs` / `store.rs` / `embedded_ssh.rs`，回归见 `tests/store_phase2.rs`（5 项真实 CLI）。
**阶段 C 已按 B2 方案实施**（`config_pair_index.json` + 成对 `.pair` 副本 + 四条恢复路径
统一前置判定 + `recover()` 索引只读诊断），见 `tests/store_phase2_c.rs`；实际实现与 §3 的
设计有一处务实偏差：索引只作诊断与后续 D 阶段的证据，**不阻断常规写入**，判定以内容 tag
本身为准（避免第三个文件成为单点故障）。**阶段 D 已实施**（`config-repair --list` / `--discard-pending` / `--restore-pair`，带确认与审计，回归 `tests/config_repair.rs`）；**阶段 3 仍未实施**；
C1–C3/C5/C7 已有真机 SIGKILL 实测（`tests/store_phase2_cd_crash.rs`），C4/C6 为构造状态回归，**C8 已用阶段 A/B 之前的真实二进制实测**（`docs/reports/evidence/c8-downgrade-2026-11-13/`，解锁态下任意命令即触发重新密封）；逐项证据级别与收口判定见 `docs/reports/issue26-acceptance-matrix.md`；
Windows 实机结论以 #27（`15e2ac5`）为准，Linux 侧边界未验证。#26 保持 open。

P1–P6 的可勾选版本、推荐默认作为实施轮工作假设的说明、分阶段实施顺序与逐文件改动清单，
见 [cross-file-transaction-phase2-decisions.md](cross-file-transaction-phase2-decisions.md)。
该预案同样**未改动生产代码**，不把「推荐」当成已决事项。

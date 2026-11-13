# 阶段 2 决策卡与实施轮预案

配套评审稿：[cross-file-transaction-phase2.md](cross-file-transaction-phase2.md)。
基线 `cnb/main` = `7b624a5`（2026-11-12）。**本文件不改动生产代码，也没有改变评审稿的
「设计待评审」状态**；它把 P1–P6 收敛成可勾选的六行，并给出拍板后立即能执行的顺序。

---

## 1. 六行决策卡

勾选一列即可；「推荐」列的组合是经过评审稿论证、彼此自洽的一套默认值。

| 编号 | 决策 | 选项 | 推荐 | 一句话理由 | 否决后的改动面 |
|---|---|---|---|---|---|
| **P1** | 旧格式引用兼容期 | (a) 长期只读不升级 / **(b) 惰性升级 + 弃用时间表** / (c) 某版本起拒绝 | **b** | (a) 让 F1 永久存在；(c) 会打断历史配置与旧脚本 | (a) 删迁移路径与弃用告警；(c) 加版本闸门与一次性迁移命令 |
| **P2** | tag 不匹配是否阻断登录 | **(a) 阻断 + 保留引用 + 明确指引** / (b) 告警仍使用 / (c) 降级为无口令 | **a** | (b)(c) 分别违反「不得静默用错密文」与造成静默数据丢失 | (b)(c) **不建议**：需重写 #26 验收并说明为何接受 F1 |
| **P3** | 成对快照形态 / tag 位置 | B1 扩展 commit log / **B2 独立版本化索引**；tag 位置 a 明文 / b AAD / c payload | **B2 + a** | `.commit.json` 是「未完成的一次写入」，不宜承载长期快照；明文字段可在锁定态判错配 | B1 改 `store_commit.rs` 生命周期；b/c 改 `encrypt_entry`/`load_map` 全部调用点 |
| **P4** | 成对快照是否纳入 WebDAV 同步 | **(a) 不纳入** / (b) 纳入 | **a** | (b) 与「`secrets.enc` 不参与同步」的既有产品决策冲突 | (b) 重写 `SYNCABLE_FILES`、旧远端 marker 兼容、桌面备份加密边界 |
| **P5** | 阶段 3 journal 是否本单落地 | **(a) 只评估** / (b) 同单实施 | **a** | 阶段 2 已改所有引用判定点与恢复路径，再叠加存储格式会让回滚单元过大 | (b) 追加 `secrets.rs` journal/compaction 与 v3→v2 压实降级 |
| **P6** | 显式修复命令 | **(a) 与 #28 `repair-commit` 合并设计** / (b) 本单单独做 / (c) 暂不提供 | **a** | #28 已发现不可重放 log 无显式出口，两份口径应合并，避免两套恢复命令 | (b) 单独加 CLI 子命令与审计；(c) 保留人工删 `.commit.json` |

### 1.1 关于「推荐默认」的适用范围

用户在 2026-11-12 以「按建议继续」授权继续推进，因此本文件把上表推荐列固化为
**实施轮的工作假设（working assumption）**，目的是让拍板后不必重新设计。

- 它**不是产品终裁**：P2 与 P4 直接改变用户可见行为（登录被阻断 / 凭据库是否上云），
  若与意图不符，回一句否决即可；此时只有本文档与实施顺序需要改，生产代码尚未动。
- 评审稿 §6 仍写「P1–P6 未拍板」；本文件与它不矛盾——一个是待决事项，一个是
  「按推荐执行时怎么走」的预案。
- 每一项否决后要改哪里已列在最后一列，不需要重新做威胁模型。

---

## 2. 实施顺序（按依赖分阶段，每阶段可独立回滚）

| 阶段 | 内容 | 前置 | 回滚点 |
|---|---|---|---|
| **A. 引用判定迁移** | 新增 `parse_secret_ref` 结构化解析（`Bare` / `Tagged(tag)` / `Keyring` / `Malformed` / `Plaintext`），同步替换 `secrets.rs:57-70` 与 `store.rs:565/578/591/620/635/653/692/698/704` 全部判定点；旧磁盘格式行为**不变** | 无 | 单笔 revert；磁盘格式未变，旧程序仍可读 |
| **B. v3 内容 tag** | `EncryptedEntry` 加 `tag`（`#[serde(default)]` 保证 v2 可读）；`save_map` / `load_map` / `change_master_password` / v1→v2 迁移保留 tag；写入端产出 `$agent2ssh-secret$:<tag>`；解析端区分「条目不存在」与「tag 不匹配」 | A | 单笔 revert + 数据降级脚本（把 Tagged 引用改回裸 marker）；**见 §4 不可逆警告** |
| **C. 成对快照与恢复判定** | B2 索引 + `secrets.enc.bak`；四条恢复路径统一前置判定（`restore_config_snapshot_unlocked` / `sync_pull` / `apply_config_template` / `Snapshot::restore`）；`recover()` 增加索引只读校验；崩溃矩阵 C4–C8 | B | 单笔 revert；索引缺失时按「无可信配对信息」保守拒绝，不删用户文件 |
| **D. 显式修复命令（P6=a）** | 与 #28 的 `repair-commit` 合并：带确认与审计，打印将保留的 hosts 版本、代次差与孤儿条目 | C | 单笔 revert；命令本身不改数据，除非用户显式确认 |
| **E. 阶段 3（仅当 P5=b）** | append-only journal + compaction | C | 需 v3→v2 压实降级路径，回滚单元最大 |

**每阶段验收门**：`cargo fmt --check` + `cargo test --no-default-features --lib` +
`--test store_transaction` + 新增 `--test store_phase2_*`；跨平台结论仍按平台分别标注，
Windows 实机结论以 #27（`15e2ac5`）为准，未覆盖的 SMB/其他版本/断电持久性仍需单独标注。

---

## 3. 逐文件改动清单（实施轮）

| 文件 | 阶段 | 改动 |
|---|---|---|
| `src-tauri/src/secrets.rs` | A, B | 引用解析函数；`EncryptedEntry.tag`；`save_map` / `load_map` / `change_master_password` / v1→v2 迁移保留 tag；`store_generation` 语义不变 |
| `src-tauri/src/store.rs` | A, B | `internalize_secrets` / `externalize_secrets` 全部判定点；保存顺序里 `.bak` 与配对索引的发布顺序；generation 守卫不动 |
| `src-tauri/src/store_commit.rs` | C | 索引只读校验加入 `recover()`；现有三分支（`:147-181`）不改 |
| `src-tauri/src/webdav_sync.rs` | C | `restore_config_snapshot_unlocked`（`:876-938`）、`sync_pull` 写回段（`:1242-1270`）、`apply_config_template`（`:951+`）统一前置判定 |
| `src-tauri/src/desktop_sync.rs` | C | `Snapshot::restore`（`:86-140`）前置判定；失败回滚路径仍走本地快照恢复 |
| `src-tauri/src/bin/agent2ssh.rs` | D | 修复命令的 CLI 入口（若 P6=a 与 #28 合并，则与 #28 共用同一子命令） |
| `src-tauri/tests/store_transaction.rs` | A–C | 旧断言 `raw.contains(SECRET_REF)`（`:2078/2118/2160`）改为结构化判定；补并发与迁移回归 |
| 新增 `src-tauri/tests/store_phase2_*.rs` | A–C | 判定单元测试、tag 稳定性、错配拒绝、C4–C8 崩溃矩阵 |
| `docs/KNOWN-ISSUES.md` K8 / `CHANGELOG.md` / `docs/guides/configuration-guide.md` | A–C | 从「设计已交付」改为「已落地」时再改；本轮已写现状说明 |
| `docs/plans/cross-file-transaction-phase2.md` | 全部 | 实施完成后把状态从「设计待评审」改为「已实现」，并回填实测结果 |

---

## 4. 升级不可逆警告（实施前必须写进发布说明）

旧程序不认识 `$agent2ssh-secret$:<tag>`：`is_secret_ref` 是精确相等比较
（`secrets.rs:57-70`），旧构建会把它当**明文口令**重新密封（评审稿 F5）。因此：

- 阶段 B 落地后，配置目录**不能**直接被旧版本程序读写；降级前必须先把 Tagged 引用
  批量改回裸 marker，并备份 `secrets.enc`。
- 多设备共享同一配置目录（含网络文件系统、WebDAV 只同步 `hosts.json`）时，
  需要「所有端同时升级」的窗口；P4=a 决定 `secrets.enc` 不上云，所以远端拿到的是
  **带 tag 的 `hosts.json` 但没有对应凭据库**——这必须在同步文档里写明，否则用户会
  在另一台设备上看到一堆无法解析的引用。
- 这条不是推断出来的edge case，而是由现有精确相等判定直接推出的必然结果；
  实施轮要有一条真实 CLI 回归覆盖「旧程序读新配置」的检测与提示。

---

## 5. 测试矩阵（对应 #26 四条验收）

| 验收 | 测试 | 手段 | 现状 |
|---|---|---|---|
| ① 多进程 `host add` × `config restore` 并发后引用内容匹配，无 `None` / 错配 | 8+ 真实 CLI 进程并发，解密断言 + generation 对齐 + tag 比对 | 复用 `store_transaction.rs::concurrent_writers_keep_every_sealed_secret` 与 `store_crash_injection.rs` 的隔离目录 | 待实施 |
| ② 恢复旧 `hosts.json` 快照时 tag 不匹配明确报错；旧格式迁移有真实 CLI 回归 | 构造旧快照 / 裸 marker / keyring 三种输入；断言错误文案与「引用未被抹除」 | 复用 `seed` / `write_log` 夹具 | 待实施 |
| ③ 崩溃/回滚矩阵逐项可复现，区分实测与推断 | C1–C8；C1–C3 用真机 `SIGKILL`，C4–C8 用注入脚本 + 构造状态 | `run_crash_injection.py`、`store_crash_injection.rs` | C1–C3 **已实测**（#28）；C4–C8 待测 |
| ④ 更新 K8 / `CHANGELOG.md` / 配置指南 `hosts.json.bak` 说明 | 文档检查 | — | **本轮已完成现状部分**；「已落地」措辞待实施后替换 |

每条结果必须标注「真机实测 / 构造状态回归 / 源码推断」，不得混写。

---

## 6. 状态

**阶段 A/B 已实施**（P1–P6 按推荐默认锁定后的实施轮）：结构化引用解析、`secrets.enc` 内容 tag、
`$agent2ssh-secret$:<tag>` 写入与校验、引用自证与 P2 的保守拒绝、旧裸 marker 惰性升级
（**限定条件**：只有条目本身已带合法 tag 时才升级——`entry_tags()` 会丢掉空 tag，实测
2026-11-13 一对「裸引用 + 空 tag」的全遗留配置经过普通保存后仍是裸引用，必须重写该凭据才会打上 tag）、
改主密码 tag 稳定；`tests/store_phase2.rs` 5 项真实 CLI 回归 + 1 项解析单元测试全绿。
**阶段 C 已实施**（B2 索引 + 成对 `.pair` 副本 + 四条恢复路径统一前置判定 +
`recover()` 索引只读诊断，`tests/store_phase2_c.rs` 6 项回归）。实现按 §3 的 B2 方案，
但索引只作诊断、不阻断常规写入；判定以内容 tag 为准。**阶段 D 已实施**（`config-repair --discard-pending` / `--restore-pair`，带确认与审计，
`tests/config_repair.rs` 6 项真实 CLI 回归）；**E（阶段 3）未实施**；C1–C3/C5/C7 已有真机 SIGKILL 实测，C4/C6 为构造状态回归，**C8 已用阶段 A/B 之前的真实二进制实测**（`docs/reports/evidence/c8-downgrade-2026-11-13/`），逐项见 `docs/reports/issue26-acceptance-matrix.md`，
Linux 侧边界未验证。§2 的阶段表仍是后续实施顺序，§4 的升级不可逆警告已在配置指南落地。
Windows 侧由 #27（`15e2ac5`）补上真机结论（rename 重试 / 只读属性 / 共享删除 / 残留临时文件），
`icacls` 失败路径仍是未验证，SMB、其他 Windows 版本与断电持久性同样未验证。
#26 保持 open。

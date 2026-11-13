# CNB #26 收口判定表

**状态：实施完成，等待人决定是否关闭 issue。** 阶段 A/B/C/D 的实施落在
`f98a4b9`；本文记录此后的真机崩溃注入证据（C5/C7）。`cnb/main` 在实施期间
仍被并行会话推进（daemon / 诊断面板等无关改动），所以本文不引用会移动的
`main` 提交号。本文逐项标注证据级别，不把「构造状态回归」写成「真机实测」，
也不把「源码推断」写成「已验证」。

证据级别：**实测** = 真实进程 / 真机 `SIGKILL`；**构造** = 可复现的构造状态回归；
**推断** = 源码语义推导，无真机复现；**未验证** = 本仓库范围内没有证据。

---

## 1. #26 正文四条验收

| # | 验收项 | 判定 | 证据 |
|---|---|---|---|
| ① | 多进程 `host add` × `config restore` 并发后每个引用解析到**内容匹配**的密文，无 `None` / 错配 | **已闭合（构造 + 实测）** | `tests/store_transaction.rs::concurrent_writers_keep_every_sealed_secret`（8 真实 CLI 并发、解密断言）；阶段 A/B 的 `tests/store_phase2.rs` 5 项证实 tag 与明文内容绑定、改主密码后 tag 稳定；阶段 C 的 `tests/store_phase2_c.rs` 证实旧 `hosts.json` 经真实 `apply_config_template` 路径因 tag 错配被拒、两文件字节均未改 |
| ② | 恢复旧 `hosts.json` 快照时 tag 不匹配给出明确错误，不静默使用同名新密文；旧格式引用迁移有真实 CLI 回归 | **已闭合（构造 + 实测）** | `guard_restore` 的四条恢复路径前置判定（`webdav_sync.rs` / `desktop_sync.rs`）；`tests/store_phase2_c.rs::restoring_hosts_json_with_a_mismatched_tag_is_refused_before_any_write`；裸 marker 惰性升级、malformed 不重密封见 `tests/store_phase2.rs`。**边界**：裸 marker / keyring / 非 JSON 内容按 `LegacyOnly` 告警放行，无法自证内容 |
| ③ | 崩溃 / 回滚矩阵（rename 前后、log 残留、备份回卷）逐项有可复现断言，并区分实测与推断 | **已闭合（逐项分级见 §2）** | C1–C3 真机 `SIGKILL`（#28），C5 / C7 本轮真机 `SIGKILL`，C4 / C6 / C8 构造状态回归；每项在下表标级 |
| ④ | 更新 K8 与 `CHANGELOG.md`，同步配置指南中 `hosts.json.bak` 说明 | **已闭合** | `docs/KNOWN-ISSUES.md` K8、`CHANGELOG.md` 各阶段条目、`docs/guides/configuration-guide.md` 的 `.bak` 局限与 `config-repair` 用法 |

## 2. 崩溃 / 回滚矩阵 C1–C8

| # | 场景 | 级别 | 结论 | 证据 |
|---|---|---|---|---|
| C1 | `begin` 后、hosts rename 前 | **真机实测** | 重放 tmp，hosts / secrets 一致，引用可解密 | `tests/store_crash_injection.rs`（#28 `f98a4b9`） |
| C2 | hosts rename 后、`finish` 前 | **真机实测** | `recover()` Case 1 只清 log，无双重重放 | 同上 |
| C3 | 可重放但 tmp 丢失 | **真机实测** | 拒绝写入、保留 log、hosts 字节不变；`config-repair --discard-pending` 提供显式出口 | 同上 + `tests/config_repair.rs` |
| C4 | 成对副本写了一半、索引未写 | **构造状态回归** | 索引缺失/旧 → 只做诊断，tag 判定仍是权威，不阻断写入 | `tests/store_phase2_c.rs::c4_to_c6_states_keep_tag_verification_authoritative` |
| C5 | 索引 rename 前崩溃（副本已写、索引未更新） | **真机实测（本轮）** | 崩溃瞬间 `.commit.json` 仍在、索引为 Stale；重启后 `recover()` 清 log、普通写入恢复、引用仍解析到正确密文 | `tests/store_phase2_cd_crash.rs::c5_sigkill_before_pair_index_leaves_writes_recoverable` |
| C6 | 索引存在但 `tag_digest` 与当前库不符 | **构造状态回归** | `index_status()` = Stale；恢复判定不受索引影响，写入不阻断 | `tests/store_phase2_c.rs` |
| C7 | 成对回卷中途崩溃（hosts 已回卷、secrets 未回卷） | **真机实测（本轮）** | 半回卷状态**保持 fail-closed**：`verify_hosts_restore` = `Refused`，不会静默用错密文；重启后重复同一 `--restore-pair` 可完成修复并解密回原值 | `tests/store_phase2_cd_crash.rs::c7_sigkill_during_paired_restore_stays_fail_closed_and_repairable` |
| C8 | 旧程序把带 tag 引用当明文重新密封 | **推断 + 构造回归** | 由 `is_secret_ref` 原为精确相等推出；新程序对 malformed/tagged 不重密封已有回归。**未用旧版本二进制真机复现**，降级步骤已写进配置指南 | `tests/store_phase2.rs::malformed_reference_is_never_re_sealed_as_plaintext`；配置指南「升级与降级」 |

注入手法：`src-tauri/tests/fixtures/crash_hook.c` 经 `DYLD_INSERT_LIBRARIES` 注入**真实 CLI 进程**，
在 `rename` / `unlink` 前冻结并由测试进程从外部发 `SIGKILL`（`returncode == -9`）；
不改生产代码。四个 stage：`before-rename` / `before-finish` / `before-pair-index` /
`before-restore-secrets`。Windows 侧该机制不适用，由 #27 真机证据覆盖。

## 3. 决策 P1–P6（已按推荐默认锁定）

| 编号 | 决定 | 落地 |
|---|---|---|
| P1 | 惰性升级 + 明确弃用口径 | 裸 marker 下次保存升级为 `$agent2ssh-secret$:<tag>`；锁定态保留 |
| P2 | tag 不匹配阻断登录 + 保留引用 | `internalize_secrets` 保留引用并告警，`embedded_ssh` 的 marker 过滤阻断认证；不置 `None` |
| P3 | B2 独立索引 + tag 作为 `EncryptedEntry` 明文字段 | `config_pair_index.json` + `.pair` 副本；索引只作诊断，不阻断写入 |
| P4 | 成对快照不纳入 WebDAV 同步 | `SYNCABLE_FILES` 不变；**多设备同步可用性代价仍需产品确认** |
| P5 | 阶段 3 只评估、不实施 | 兼容/回滚表在设计稿 §4；append-only journal 未落地 |
| P6 | 与 #28 的 `repair-commit` 合并为一条命令 | `agent2ssh config-repair`，带 `--confirm` 与 `app.log` 审计 |

## 4. 阶段 A–E

| 阶段 | 内容 | 状态 |
|---|---|---|
| A | 结构化引用解析 | 已实施 |
| B | v3 内容 tag + 引用自证 | 已实施 |
| C | 成对快照 + 四条恢复路径判定 | 已实施（C5/C7 已有真机证据） |
| D | 显式成对恢复 / 修复命令 | 已实施 |
| E | append-only journal + compaction | **未实施，且按 P5=a 不属于本单** |

## 5. 平台与持久性边界（未闭合项）

| 项 | 级别 | 归属 |
|---|---|---|
| Windows rename / 只读属性 / 共享删除 / 残留 tmp | **真机实测** | #27（`15e2ac5`） |
| Windows `icacls` 失败路径、SMB、其他版本、断电与目录 `fsync` | **未验证** | #27 遗留 |
| Linux `hidepid=1/2`、`ptrace_scope`、跨 uid 矩阵 | **未验证** | #28 遗留（本机无 Linux 环境） |
| Terminal.app 人工肉眼确认 | **未验证** | #28 遗留（本轮为 `openpty()` 伪终端） |
| 真实断电 / 内核崩溃 / `fsync` 持久性 | **未验证** | 本轮只覆盖进程级 `SIGKILL` |
| C8 旧版本二进制真机复现 | **未验证** | 需旧构建 |

## 6. 结论与关闭建议

- **本单实施与四条验收已闭合**：A/B/C/D 全部落地，C1–C3/C5/C7 真机实测，
  C4/C6/C8 有构造回归并如实标注，文档与配置指南同步完成。
- **仍未闭合的是边界而非功能**：阶段 3（按 P5=a 明确不在本单）、C8 旧版本真机复现、
  Linux/Windows 遗留平台矩阵、真实断电持久性。这些分别归属 #27、#28 或需要专门环境。
- **建议**：若按 `P5=a` 与既有 issue 边界，#26 可以收口；若要求「所有平台/断电边界
  实测完毕」才关闭，则继续等 #27 / #28 补测。
- **本文件不关闭 issue**：关闭是需人明确确认的操作，本文只提供判定依据。
  P4 的多设备同步可用性代价也仍待产品决定，可能需要复议 P4 或为同步场景单独设计。

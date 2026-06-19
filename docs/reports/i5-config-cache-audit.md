# I5 · 配置热加载一致性审计

目标：盘点 `~/.agent2ssh/` 下各配置文件的读取频率与失效语义，给出「哪些值得纳入统一 `ConfigCache`、哪些应保持每次读盘」的结论，并落地至少一处确认收益项。

## 背景

`config_cache::ConfigCache<T>` 是单槽、`(mtime, len)` 签名失效的解析缓存：

- 命中时返回克隆值，不读盘、不解析；
- 文件 `mtime`/`len` 变化时自动 reload，因此**跨进程**外部编辑（CLI/桌面改了文件）会被另一个进程（daemon）自动感知；
- 同进程写入后调用 `invalidate()` 立即失效，规避文件系统 mtime 粒度问题。

适用判据：**读多写少 + 容忍亚秒级跨进程延迟**。不适用：需要严格实时一致的状态。

## 现状盘点

| 配置文件 | 加载函数 | 读取热度 | 当前状态 | 写入方 |
|----------|----------|----------|----------|--------|
| `anomaly.toml` | `load_anomaly_config` | 每次诊断 error 聚合 | ✅ 已缓存（O2-3） | 用户编辑 |
| `execution_limits.toml` | `load_execution_limits` | 每次 exec/session | ✅ 已缓存（O2-3） | 用户编辑 |
| `daemon_tokens.toml` | `load_scoped_daemon_tokens` | 每个鉴权请求 | ✅ 已缓存（O2-3） | 用户编辑 |
| `webhook.toml` | `load_webhook_config` | 每个事件/告警 | ✅ 已缓存（O2-3，`save_webhook_config` invalidate） | `save_webhook_config` |
| `hosts.json` | `load_config` | **每次 host 查找**（exec/list/SFTP/session） | ✅ 已缓存（**I5 本次**，`save_config_unlocked` invalidate） | `save_config_unlocked`（全部写入唯一漏斗） |
| `execution_gate.toml` | `load_execution_gate` | **每次 mutating op** | ⏳ 未缓存（建议保持，见下） | `pause`/`resume` |
| `policy.toml` | `load_policy_file` | **每次风险分类/授权** | ⏳ 未缓存（**建议纳入**） | `save_policy_approval_policies` |
| `risk_rules.toml` | `load_risk_rules`（async） | 每次分类（legacy 兼容） | ⏳ 未缓存（低优先，见下） | 用户编辑 |
| `playbooks.toml` | `load_playbooks` | playbook 运行/列举（非 per-exec） | ⏳ 未缓存（暂保持） | 用户编辑 |
| `approval_policies.toml` | `load_approval_policies` | 审批决策（warm） | ⏳ 未缓存（暂保持） | 用户编辑 |
| `remotes.toml` | `load_remotes` | 列举/路由/scope 检查 | ⏳ 未缓存（暂保持） | 用户编辑 |

## 结论

### 本次落地

- **`hosts.json` 纳入 `ConfigCache`。** 它是最热的读路径（几乎每个操作都要解析 host），但仅在显式增删改 host 时变化。所有写入都经由唯一漏斗 `save_config_unlocked`，在其成功分支统一 `invalidate()`，覆盖全部 15 处 `save_config` 调用点，无遗漏风险。新增 `load_config_reflects_saved_hosts_via_cache` 单测验证写后不返回陈旧值。

### 建议后续纳入（高收益、低风险）

- **`policy.toml`：** 每次风险分类/授权都读盘解析，写入极少（仅 `save_policy_approval_policies`）。缓存安全（policy 只能升级风险，旧值不会放松安全），收益高。接入时在 `save_policy_approval_policies` 成功后 `invalidate()`。
- **`execution_gate.toml`：** 读取最热（每个 mutating op）。**但建议谨慎**——execution gate 是「急停」语义，pause 必须尽快被各进程观察到。`(mtime,len)` 跨进程失效存在亚秒级窗口；对安全急停，新鲜度优先于这点微优化。若要缓存，应让 `pause`/`resume` 写后 `invalidate()`，并接受跨进程亚秒延迟。**当前结论：保持每次读盘，优先正确性。**

### 保持每次读盘

- **`risk_rules.toml`：** legacy 兼容路径，且 `load_risk_rules` 是 async（`ConfigCache::load_with` 为同步 API），接入需额外适配；收益被 `policy.toml` 覆盖，低优先。
- **`playbooks.toml` / `approval_policies.toml` / `remotes.toml`：** 均非 per-exec 热路径（playbook 运行、审批决策、daemon 列举/路由频率远低于 exec），且无内置写入方，缓存收益有限，保持每次读盘以最大化新鲜度、降低复杂度。

## 验收

- 审计结论落档（本文件）。
- `hosts.json` 接入 `ConfigCache` + 写后 `invalidate`，含 `load_config_reflects_saved_hosts_via_cache` 单测。
- 两套 `cargo check`、两套 `cargo test` 全绿。

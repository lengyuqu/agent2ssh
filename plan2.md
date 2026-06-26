# Agent2SSH Plan 2 建议稿

> 日期：2026-06-26  
> 范围：基于当前 `docs/plan.md`、`CHANGELOG.md`、`project-defects-report.md`、架构文档和现有回归报告提出的新一轮建议。  
> 定位：不是继续扩大功能面，而是把 0.2.x 之后的发布可信度、真实使用闭环、低风险债务和维护自动化收紧。

## 1. 当前判断

当前项目已经不是 MVP 状态。桌面端、CLI、MCP、daemon、Web Console、审批、审计、gate、limits、异常检测、加密凭据、WebDAV 同步、真实服务器回归和跨平台验证都已进入可用基线。

因此下一轮不建议继续开“大功能阶段”。更合适的方向是：

- 把 0.2.1 之后新增的凭据加密、WebDAV、i18n、打包修复纳入正式发布回归。
- 把缺陷报告中剩余的低风险债务做成小批量可验收任务。
- 把未运行或未自动化的质量门槛补齐，例如 Clippy、前端 lint、依赖审计、发布资产校验。
- 用真实用户或真实 agent 接入反馈决定是否继续扩展，而不是预先设计团队版或云端版。

## 2. 优先级建议

| 优先级 | 建议主题 | 原因 |
|--------|----------|------|
| P0 | 发布可信度与回归自动化 | 当前能力面已经大，下一次发布最大风险是构建、包、文档、契约或平台行为漂移 |
| P1 | 凭据加密与 WebDAV 同步真实场景回归 | 这是 0.2.1 后最敏感的用户数据路径，涉及迁移、锁定、跨设备和恢复 |
| P1 | 真实接入反馈闭环 | 现有路线图已关闭，新方向应来自实际 dogfood 和外部使用 |
| P2 | 低风险债务收口 | 已无高/中风险待修，但剩余小问题会影响长期可维护性 |
| P2 | 可观测与诊断体验打磨 | 现有诊断能力丰富，但需要确认用户能从错误恢复，而不只是记录错误 |
| P3 | 性能和规模专项 | 只有在真实使用出现规模压力时再深化，不建议抢在 P0/P1 前面 |

## 3. 建议阶段

### Q1 · 0.2.x 发布可信度收口

目标：让下一次发布不依赖人工记忆，发布前检查可以稳定复现。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q1-1 | 高 | 固化完整本地质量门槛 | `npm run build`、两套 Rust test、CLI smoke、daemon integration、`git diff --check` 全部通过并记录到发布报告 |
| Q1-2 | 高 | 补齐 Clippy 检查 | 明确可接受的 clippy profile；至少运行 `cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings`，必要豁免写入源码或计划 |
| Q1-3 | 中 | 补齐前端 lint 或等价静态检查 | 如果引入 ESLint，纳入 `package.json` 脚本；如果暂不引入，记录原因和替代检查 |
| Q1-4 | 高 | 发布资产验证 | 在 macOS 本机完成 `.app` / `.dmg` 打包、sidecar 文件名、主程序入口、图标、版本号同步和 checksum 验证 |
| Q1-5 | 中 | CI 与 release checklist 对齐 | 确认 `.github/workflows/ci.yml` 覆盖文档契约、构建矩阵、Tauri bundle 和 release asset 生成；缺口直接进入 checklist |

### Q2 · 凭据加密与 WebDAV 同步回归

目标：把最敏感的数据路径从“单测证明”提升到“真实迁移和失败恢复证明”。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q2-1 | 高 | 主密码全入口回归 | 桌面解锁、CLI `secrets status/set-password`、MCP/daemon 使用 `AGENT2SSH_MASTER_PASSWORD` 都能访问密码型 host；锁定时失败信息明确 |
| Q2-2 | 高 | 明文凭据迁移回归 | 从旧 `hosts.json` 明文密码迁移到 `$agent2ssh-secret$` 句柄；确认 `secrets.enc` 不含明文，未解锁时不会误清空句柄 |
| Q2-3 | 高 | WebDAV 同步安全边界回归 | push/pull 覆盖 `hosts.json`、`secrets.enc`、policy、limits、anomaly、playbooks；确认不同步 `known_hosts.json`、tokens、audit、logs、私钥 |
| Q2-4 | 中 | WebDAV 失败恢复 | 模拟远端旧 manifest、未知文件、网络失败、认证失败、本地备份恢复；错误提示包含下一步动作 |
| Q2-5 | 中 | 跨设备使用文档 | 在配置指南中补一条“新设备拉取后如何解锁、验证 host-key、避免覆盖本地信任库”的短流程 |

### Q3 · 真实接入与反馈闭环

目标：让后续路线来自真实用户证据，而不是继续在本地闭环里堆能力。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q3-1 | 高 | 外部接入脚本化记录 | 基于 `.github/ISSUE_TEMPLATE/external_adoption_report.md` 收集至少 3 次接入记录，覆盖安装、配置 host、执行命令、查看 audit、MCP 接入 |
| Q3-2 | 高 | Agent 客户端接入复测 | 至少复测 Codex/Claude/Cursor/OpenCode 中两个 MCP 客户端的 `tools/list`、`ssh_exec`、session 或 audit 路径 |
| Q3-3 | 中 | Windows/Linux/macOS 安装 smoke | 每个平台运行 `scripts/verify-install.sh`，并记录 CLI、daemon、MCP server 启动结果 |
| Q3-4 | 中 | 真实服务器回归轻量化 | 复用 `107.174.36.91` fixture 或替代环境，形成只覆盖高频路径的 10 分钟 smoke，不每次跑全量 e2e |
| Q3-5 | 中 | 反馈分级规则 | 新反馈按数据丢失、安全绕过、执行失败、平台阻塞、体验问题分级；只有 P0/P1 反馈可以打开新功能阶段 |

### Q4 · 低风险债务收口

目标：处理已知但不紧急的小问题，降低长期维护成本。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q4-1 | 中 | 生产 `unwrap()` / `panic!` 审计 | 不要求一次清零；先列出非测试路径中的真实风险点，优先替换 I/O、锁、解析、网络边界上的 panic |
| Q4-2 | 中 | 命令长度与输入上限 | 为 exec/session/playbook 参数设定合理上限，超限返回可读错误并写 rejected audit |
| Q4-3 | 中 | 审计保留策略可配置 | 当前审计轮转较保守；增加或确认可配置保留大小/代数，文档说明高频场景风险 |
| Q4-4 | 低 | SFTP 取消语义 | 明确取消是否立即中断当前文件；如果实现代价低，补立即中断；否则在 UI 和文档中说明 |
| Q4-5 | 低 | 开发体验小债 | 例如 HMR 计数器、useEffect 依赖、非关键 copy 问题，统一按低优先级批处理 |

### Q5 · 诊断、错误恢复与用户可解释性

目标：确认出错时用户知道发生了什么，以及下一步该做什么。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q5-1 | 中 | 错误消息分层 | 将认证失败、主密码锁定、host-key 变化、daemon token、approval timeout、WebDAV 失败分成可识别错误类型 |
| Q5-2 | 中 | Diagnostics 导出可读性 | 导出的 bundle 能关联 trace_id、daemon.log、app.log、audit 片段，并默认脱敏敏感字段 |
| Q5-3 | 中 | 桌面错误恢复路径 | 对 SecretsUnlock、Settings、Sync、Terminal、SFTP、Exec 的失败态做一次人工 walkthrough |
| Q5-4 | 低 | Webhook 重试策略评估 | 当前 webhook 是非阻塞 fire；先判断真实使用是否需要重试、退避和死信记录，不建议默认复杂化 |

### Q6 · 性能与规模专项

目标：只在可复现压力场景下推进，避免为了规模假设增加复杂度。

| 任务 | 优先级 | 内容 | 验收标准 |
|------|--------|------|----------|
| Q6-1 | 中 | 500/1000 host plan smoke | 扩展现有 `scripts/e2-scale-plan-smoke.py`，观察计划生成时间、内存和输出体积 |
| Q6-2 | 中 | 审计查询高容量测试 | 构造大 audit.jsonl，测试筛选、导出、桌面渲染和轮转行为 |
| Q6-3 | 低 | 多 session / forward 压测 | 在本机 fixture 下验证 limits、清理、事件流和 UI 状态一致性 |
| Q6-4 | 低 | SFTP 大文件与中断恢复 | 覆盖上传、下载、取消、恢复偏移和失败清理 |

## 4. 不建议现在做的事

- 不建议启动云端控制台、账号体系、组织 RBAC 或 SaaS 化，除非真实用户明确需要。
- 不建议继续增加 MCP 工具数量，除非能指向具体工作流缺口。
- 不建议把桌面端改成大型运维平台；当前更适合作为本地 operator surface。
- 不建议把 WebDAV 变成通用同步系统；它应保持保守同步范围，避免跨设备传播本机信任和运行时状态。
- 不建议在未完成 Q1/Q2 前做大规模 UI 重构。

## 5. 推荐下一步执行顺序

1. 先做 Q1：补齐发布检查，把下一次发布的失败概率降下来。
2. 并行做 Q2 的回归清单：凭据和同步路径优先，因为它们关系到用户数据。
3. 用 Q3 收集真实反馈，决定是否开启新的功能阶段。
4. Q4/Q5 作为小批量维护任务穿插处理。
5. Q6 等出现真实规模压力或发布前需要性能证明时再启动。

## 6. 最小验收命令建议

下一轮每个合并或发布候选至少跑：

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
git diff --check
```

发布候选再加：

```bash
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
npm run tauri:build
./scripts/verify-install.sh
```

Clippy 和前端 lint 建议作为 Q1 的新增门槛确定下来；在规则稳定前先不要阻塞所有开发分支。


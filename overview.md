# Agent2SSH 文档整理与精简

## 做了什么

把工程内零散、重叠的"综合文档"合并为少量职责单一的文档，整体文档数从约 **31 个降到 19 个**（不含 `.workbuddy` 记忆、`.github` 模板与打包 skill）。

| 合并组 | 之前 | 之后 |
|--------|------|------|
| 规划类 | `docs/plan.md`（大计划，全 ✅）+ 根 `plan2.md`（活跃 Plan 2）+ `plan2-q1-q2-execution-report.md` | `docs/PLAN.md`（单一规划源：活跃 Plan 2 为主线 + 大计划降级为历史归档 + 执行报告附录） |
| 报告类 | `docs/reports/` 下 11 份独立报告 | `docs/reports/REGRESSION-LOG.md`（单一日志，含索引表，按时间归档） |
| 发布类 | `versioning.md` + `release-checklist.md` | `docs/RELEASE.md` |
| 缺陷类 | 根 `project-defects-report.md` | 移入 `docs/DEFECTS.md` |

## 最终结构

```
README.md / CHANGELOG.md / AGENTS.md / CLAUDE.md        # 门面与约定（保留）
docs/
  PLAN.md            # 唯一规划源
  RELEASE.md         # 版本策略 + 发布清单
  DEFECTS.md         # 缺陷检查报告
  architecture.md    # 架构说明（保留）
  skills.md          # MCP 工具参考（保留）
  skill-distribution.md  # Skill 分发（保留，链接已更新）
  guides/  (8 个，保留)
  reports/REGRESSION-LOG.md   # 11 份报告合并
```

## 关键决策

- **保留参考类文件名不变**（`skills.md`、`architecture.md`、`skill-distribution.md`、`guides/*` 维持原名），仅合并"综合"文档，避免破坏既有链接。
- 原大计划 `docs/plan.md` 里详尽的逐任务验收表**未逐字搬运**进 PLAN.md，而是压缩为阶段速览；完整内容仍可在 git 历史中找回。
- 已用 grep 全量校验：**无悬空链接**；`docs/api.yaml` 仍有效。

## 验证

- 删除 16 个被合并取代的旧文件后，剩余项目文档 19 个。
- 跨文件链接（README、help.md、skill-distribution.md、PLAN.md、REGRESSION-LOG.md）均已指向新文件或相对路径正确。

## 第二轮补充（"继续做下去"）

| 动作 | 产出 |
|------|------|
| 创建 `docs/README.md` | 文档总入口（快速入门 / 配置参考 / 项目维护 三段导航） |
| 补回 `docs/PLAN.md` 逐任务验收表 | 原 `docs/plan.md` 全部 475 行恢复为历史归档段（含 F1–S9、G/R/E/O/H/I/J/K 逐任务表格），从 git HEAD 精确还原 |
| AGENTS.md ⇄ CLAUDE.md 去重 | AGENTS.md 扩充 binary-specific `cargo check` 成为权威源；CLAUDE.md Build 段 27 行→9 行（引用 AGENTS.md + 保留 feature set 上下文）；`docs/plan.md` → `docs/PLAN.md` |

**当前文档树：**

```
README.md / CHANGELOG.md / AGENTS.md / CLAUDE.md
docs/
  README.md          ← 新建：文档入口与导航
  PLAN.md            ← 新建：749 行含全量历史验收表
  RELEASE.md
  DEFECTS.md
  architecture.md
  skills.md
  skill-distribution.md
  guides/  (8 个)
  reports/REGRESSION-LOG.md
```

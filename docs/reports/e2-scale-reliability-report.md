# E2 可靠性与规模报告

## 目标

建立不依赖 100 台真实 SSH 主机的本机规模基线，覆盖：

- 100+ host 配置读取与批量计划构建。
- 批量执行计划在不打开 SSH 连接时的低成本回归。
- event bus 对 1000 条事件突发的接收稳定性。

## 本次验证范围

### 100 host 批量计划 smoke

新增脚本：

```bash
python3 scripts/e2-scale-plan-smoke.py
```

脚本行为：

1. 创建临时 `AGENT2SSH_CONFIG_DIR`。
2. 写入 100 个 synthetic host profile。
3. 用 `host list --tag scale --json` 验证配置读取。
4. 用 `exec-multi ... --plan --json` 生成 100 target 执行计划。
5. 验证计划为 `low` risk 且不需要 approval。

可用环境变量调整规模：

```bash
AGENT2SSH_SCALE_HOSTS=150 python3 scripts/e2-scale-plan-smoke.py
```

### Rust 回归

新增测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features test_preview_exec_multi_scales_to_100_hosts
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features test_event_bus_handles_1000_event_burst
```

覆盖：

- `build_plan_from_profile` 对 100 hosts 的风险聚合和 target 生成。
- `events` broadcast bus 在 1024 buffer 内容量内接收 1000 条突发事件。

## 验收结果

- 100 host 本机配置与 `exec-multi --plan` smoke 通过。
- 100 host Rust plan 回归通过。
- 1000 event burst 回归通过。

## 边界

本报告不声称已经验证：

- 100 台真实 SSH 主机并发执行。
- 多台真实 daemon 的跨进程/跨机器聚合吞吐。
- 浏览器 EventSource 长连接在长时间网络抖动下的恢复。

这些需要真实环境或专门压测环境，建议在 R4 外部 dogfood 或后续专项压测中继续记录。

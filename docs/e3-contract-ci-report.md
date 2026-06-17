# E3 契约一致性 CI 报告

## 目标

把 S3 阶段已有的文档、OpenAPI、MCP 和 CLI help 一致性检查提升为显式 CI 门槛，避免契约漂移只在人工发布检查或大测试日志中被动发现。

## CI 入口

`.github/workflows/ci.yml` 新增 `contract-consistency` job，触发范围与主 CI 一致：

- push 到 `main`
- pull request 到 `main`
- GitHub release published

`build` matrix 和 release-only `tauri-bundle` job 都依赖该 job。契约检查失败时，跨平台编译和安装包打包不会继续消耗 CI 时间。

## 覆盖范围

### MCP 文档一致性

```bash
cargo test --no-default-features --test daemon_integration mcp_tools_match_skills_md_documentation
```

校验 `docs/skills.md` 表格中的 51 个 MCP 工具名与实现侧预期列表一致。

### OpenAPI / daemon schema fixture

```bash
cargo test --no-default-features --test daemon_integration exec_request_schema_includes_reason_and_change_id
cargo test --no-default-features --test daemon_integration exec_multi_body_schema_matches_contract
cargo test --no-default-features --test daemon_integration exec_multi_batch_result_schema_matches_contract
cargo test --no-default-features --test daemon_integration playbook_run_body_schema_matches_contract
cargo test --no-default-features --test daemon_integration audit_export_response_contract
```

覆盖 `/exec`、`/exec-multi`、`/playbooks/run` 和 `/audit/export` 的高频请求/响应契约。

### CLI help 与文档参数

```bash
cargo test --no-default-features --test cli_smoke cli_exec_help_shows_reason_and_change_id
cargo test --no-default-features --test cli_smoke cli_exec_multi_help_shows_reason_and_change_id
cargo test --no-default-features --test cli_smoke cli_playbook_run_help_shows_reason_and_change_id
```

覆盖 `exec`、`exec-multi`、`playbook run` 的关键参数帮助输出，尤其是 `--reason` 和 `--change-id`。

## 验收结果

- CI workflow 中已有独立 `Contract consistency` job。
- `build` 和 `tauri-bundle` job 已通过 `needs: contract-consistency` 依赖该门槛。
- 本地已运行 `contract-consistency` job 中列出的 9 个目标测试，全部通过。
- `git diff --check` 通过，workflow YAML 基本语法解析通过。

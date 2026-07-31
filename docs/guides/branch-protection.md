# 分支保护策略

本文档描述 Agent2SSH 仓库 `main` 分支的保护规则，包括策略目标、配置要求和启用步骤。

## 策略目标

- **禁止直接推送**：所有变更必须通过 Pull Request 合入 `main`，不允许直接 `git push origin main`。
- **要求 CI 通过**：PR 必须通过 GitHub Actions CI 工作流（`CI` workflow）中所有必需检查后才能合并。
- **要求代码审查**：PR 至少需要 1 位 CODEOWNER 或维护者批准后才能合并。
- **保持线性历史**：建议启用 "Require linear history" 以确保提交历史可追溯。

## 保护规则配置

| 规则 | 设置 |
|------|------|
| Require pull request before merging | ✅ 启用 |
| Required approvals | 1 |
| Require status checks to pass | ✅ 启用 |
| Required status checks | `contract-consistency`, `build` |
| Require branches to be up to date | ✅ 启用 |
| Require linear history | ✅ 启用（推荐） |
| Include administrators | ✅ 启用 |
| Allow force pushes | ❌ 禁用 |
| Allow deletions | ❌ 禁用 |

## 启用步骤（手动操作）

分支保护规则需要在 GitHub 仓库设置中手动配置：

1. 打开仓库页面，进入 **Settings** → **Branches**。
2. 点击 **Add branch protection rule**（或编辑已有的 `main` 规则）。
3. 在 **Branch name pattern** 中填写 `main`。
4. 勾选 **Require a pull request before merging**：
   - 设置 **Required number of approvals before merging** 为 `1`。
   - 勾选 **Require review from Code Owners**。
5. 勾选 **Require status checks to pass before merging**：
   - 搜索并添加 `contract-consistency` 和 `build` 作为必需检查。
   - 勾选 **Require branches to be up to date before merging**。
6. 勾选 **Require linear history**（推荐）。
7. 勾选 **Include administrators**，确保管理员也受规则约束。
8. 确认 **Allow force pushes** 和 **Allow deletions** 未勾选。
9. 点击 **Save changes** 保存规则。

## CODEOWNERS

仓库通过 [`.github/CODEOWNERS`](../../.github/CODEOWNERS) 文件定义代码所有者。当 PR 修改了匹配的文件路径时，GitHub 会自动请求对应所有者进行审查。

## CI 工作流

保护规则依赖 [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) 中定义的 CI 检查：

- **contract-consistency**：验证 MCP 工具契约一致性。
- **build**：跨平台编译（macOS x86_64/aarch64、Linux x86_64、Windows x86_64）、单元测试、集成测试、CLI 冒烟测试和前端构建。

PR 必须通过以上所有检查后，合并按钮才会可用。

## 验证

配置完成后，可通过以下方式验证保护规则生效：

1. **直接推送被阻断**：尝试 `git push origin main`，预期收到类似以下错误：

   ```
   remote: error: GH006: Protected branch update failed for refs/heads/main.
   remote: error: Required status check "build" is expected.
   ```

2. **PR 合并被阻断**：创建 PR 后，在 CI 未通过时合并按钮应显示为灰色不可点击，提示 "Required checks have not passed"。

3. **审查要求**：未获得 CODEOWNER 批准时，合并按钮应提示 "Review required"。

## 相关文档

- [CI/Release 工作流](../../.github/workflows/ci.yml)
- [CODEOWNERS](../../.github/CODEOWNERS)
- [发布与版本](../RELEASE.md)

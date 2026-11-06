# SSH 兼容性矩阵验证

CNB `main` 分支提供手动按钮“SSH 兼容性完整矩阵”，也可通过 `api_trigger_ssh_matrix` 事件运行。它不按计划自动执行，也不作为 PR 门禁；日常推送仍运行单服务器真实 SSH/SFTP 测试。

完整矩阵使用隔离的 CNB Docker 服务，测试 OpenSSH 8.3、8.8、9.3、9.9、10.3、Alpine Dropbear 和单独的 legacy 算法拒绝用例。脚本记录服务端版本、每组 PASS/FAIL/SKIP 和 `MATRIX_RESULT` 汇总。缺 `ssh-keygen`、无法建立独立测试 agent 或实际服务端版本与预期不符时严格模式会失败，不把未运行的认证类型或错误版本算作通过。Dropbear 不支持 OpenSSH 用户证书，SHA-1 密钥交换也仍是 K12 中的已知限制；这些跳过不代表已支持。

本机复现前先构建当前 CLI，并使用独立端口范围：

```sh
cargo build --locked --manifest-path src-tauri/Cargo.toml --release --no-default-features --bin agent2ssh
AGENT2SSH_CONTAINER_RUNTIME=docker AGENT2SSH_MATRIX_REQUIRE_FULL=1 \
  AGENT2SSH_MATRIX_PORT=22130 bash scripts/e2e-ssh-matrix.sh
```

脚本为每次运行创建临时配置与独立容器 ID；Unix 使用临时 SSH agent，不使用调用者的 agent。请确保所选连续端口可用。Windows 系统 agent 默认不修改，显式设置 `AGENT2SSH_MATRIX_WINDOWS_AGENT=1` 才会加入测试密钥；Windows、wslc 和 Apple container 的这条新路径仍需实机验证。

Linux CNB 通过只能证明实际执行的 Linux/容器组合。键盘交互多轮、FIDO、安全密钥、跳板机组合、Windows 自带 OpenSSH 及其他 `KNOWN-ISSUES.md` 中列出的未验证范围不因本矩阵通过而自动变为已验证。

## 本机验证记录（2026-11-06）

在 macOS arm64、Docker 29.9.2 上，用隔离配置、独立 agent 和临时端口运行严格矩阵；脚本 SHA-256 为 `df1b13f922a0d2fc856ca9aa4abc4aaf60d23a48f72ea792142a90570e720e60`。OpenSSH 8.3、8.8、9.3、9.9、10.3 各为 34 PASS；Dropbear 为 23 PASS / 1 SKIP，legacy 为 3 PASS / 1 SKIP。总计 **196 PASS / 0 FAIL / 2 SKIP**。跳过分别是 Dropbear 用户证书和 K12 的 SHA-1 密钥交换。测试容器均已清理。

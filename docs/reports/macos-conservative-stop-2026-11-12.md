# CNB #25：macOS 保守停止与恢复实现

基线 `e94f3a3`；本轮在 macOS 27.2.1 (26B131)、arm64、uid 501 验证。相关进程测试均使用临时 `AGENT2SSH_CONFIG_DIR` 和合成 token，不读取或改写真实 `~/.agent2ssh`。

## 实现边界

- macOS 的普通 CLI、桌面停止及重启对已有 PID 默认拒绝，不发 `/shutdown`、不发 PID 信号、不由客户端清理 PID；桌面退出不主动发送停止请求或信号；无法保证托管环境不会终止 daemon。HTTP `/shutdown` 默认关闭，管理员必须在 **daemon 的启动环境**显式设置 `AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN=1` 才开放旧路径；非回环监听也会受此开关影响。
- `daemon stop --allow-unbound-graceful` 与桌面确认入口允许一次未绑定 HTTP 请求：先核对数字 loopback、PID 记录前后快照、进程可观察性、健康响应 PID 和配置目录，矛盾即拒绝。它们均不能证明身份或发现所有代理；显式请求有停错及 token 泄露风险。请求发出后响应丢失、非 200 或无效回执均不声称进程未停止；即使收到接受回执，CLI 仍以非零状态报告退出未知。客户端不重试、不发信号、不删除 PID 文件；daemon 自身正常退出时可能删除自己的 PID 记录。
- `daemon clear-stale --expect-pid <PID>` 只清理 PID 文件：读取快照并在锁内重读，内容、PID、普通文件类型和 `kill(pid,0)` 的 `ESRCH` 均匹配才删除；不会发信号或删除 `daemon.owner`。新 daemon 写 PID 时独占创建，不自动替换现存文件或跟随悬空链接；启动器和 token 轮换也不自动清理 stale PID。旧程序不遵守锁、非可靠文件系统和锁外被替换仍无法完全防护。
- 这是保守迁移步骤，不实现 #22 的版本化实例记录、本机 IPC 或 `Bound`。历史 daemon 和旧客户端仍保有旧行为；未绑定的显式例外不能被误认作身份保证。Linux/Windows 未采用本次 macOS 专属默认分支。

## 测试与限制

- `cargo test --no-default-features --features daemon --test daemon_macos_conservative_e2e --test daemon_stop_e2e`：macOS 新增真实进程测试 7 项通过（修正测试夹具 socket 非阻塞继承后连续三轮通过）；原停止回归 33 项通过、2 项忽略（旧自动替换 stale 语义，已由新测试替代）。未配置 SSH fixture，原测试的真实 SSH 部分提前跳过。
- 新增测试覆盖默认 HTTP 403 和 CLI 拒绝、实际 daemon 显式旧路线、伪造端点返回 200 时目标仍存活与 PID 仍保留、健康与停止请求间 A/B 切换时新版 B 的 409、健康不匹配、PID 记录在检查中变化、无效/丢失回执、活 PID 与 pid 1 拒绝清理、缺失 PID 清理、悬空符号链接及直接 daemon 启动拒绝覆盖。
- `cargo test --no-default-features --features daemon --bin agent2ssh-daemon --test daemon_integration`：30 项 daemon 单测及 86 项镜像路由/契约测试通过；镜像路由不等同于真实 daemon。`--no-default-features --lib` 868 passed / 1 ignored；默认 Tauri feature 的 `--lib` 885 passed / 1 ignored；CLI smoke 34 项通过。前端 `npm test` 224 项及 `npm run build` 通过，界面测试覆盖用户取消与确认。
- 真实 Tauri 窗口（隔离临时应用包、Vite 开发服务）已验收默认停止拒绝、风险确认取消及确认后的 403 反馈。发现原确认弹窗点击后设置面板会关闭、隐藏反馈，已修复并复测。另两次退出隔离应用后，daemon 均在约一秒后进入关闭流程并退出；应用日志显示停止分支没有主动请求。daemon 的 `shutdown signal received` 日志同时用于系统信号和内部关闭通知，不能仅凭日志确定触发源，也不能声称退出后 daemon 必然存活。后续已定位这次未父窗口化的原生提示问题并修复；真实窗口的活动会话 sheet 显示及取消已复测通过。独立信号探针在本机记录到 `loginwindow` 发送 SIGTERM，但其内部决策与正式包行为仍未验证，详见 [后续排查报告](macos-desktop-exit-investigation-2026-11-12.md)。未实测 Linux/Windows、稳定的 PID 复用或非合作旧程序写入竞态。没有真实 SSH fixture；本次 A/B 使用本机当前版本的两个真实 daemon，不能外推到旧 daemon 忽略 `expect_pid` 的情况。未实现 #22 的完整协议，因此 #25 的完整端到端验收仍未关闭。

## 合并前补充复核

确认窗口曾依赖 `daemon.owner` 和带管理员 token 的活动查询；macOS 的端点尚未绑定，因而最终实现不再自动发送 token 查询活动。只要 PID 记录存在或损坏，或未认证健康端点提供迹象，退出时都提示活动状态未知；没有这些迹象时仍不能证明不存在隐藏 daemon。真实临时 Tauri 窗口中创建 resident session 后删除合成 `daemon.owner`，退出仍出现“无法确认 daemon 的活动状态”sheet；取消后同一 session 仍存活。复核时默认 Tauri 库测试为 **886 passed / 1 ignored**；daemon 处理器单测在默认关闭和显式启用两种环境下各 **30 passed**，真实 macOS 保守停止测试 **8 passed**。

# macOS 桌面退出与确认窗口排查

环境：macOS 27.2.1 (26B131)、arm64、uid 501、SIP enabled。测试使用专门的临时应用包、Vite 开发服务与隔离配置，测试进程已经退出；不能把结果推广到其他 macOS 版本或正式签名包。本地工作基线 `e94f3a3`，工作区含 #25 尚未提交的实现；CNB `main` 在本轮开始时已前进至 `f98bd83`，尚未将该提交合入当前有修改的工作区。

## daemon 为什么进入关闭流程

1. 给 daemon 的既有 `tracing` 关闭事件增加 `trigger` 字段，分别标识 `sigterm`、`ctrl_c`、`http`；默认 info 日志由 launcher 追加到 `daemon.log`，降低日志级别时可能不可见。未另行同步写 `app.log`，避免引入新的关闭时文件锁等待。信号处理器注册失败不当成收到信号。真实 daemon 端到端分别验证 SIGTERM 和 HTTP 会记录各自类型。
2. 隔离桌面应用退出时，真实 daemon 的日志显示 `trigger=sigterm`；先前收到的 `/shutdown` 403 并未触发关闭。原始匿名观察见 [observations.txt](evidence/macos-desktop-exit-2026-11-12/observations.txt)。
3. 为确定发送者，使用 [signal_probe.c](evidence/macos-desktop-exit-2026-11-12/signal_probe.c) 作为仅绑定 loopback、仅接受专用临时目录的合成 daemon。`SA_SIGINFO` 校准时用 Python `os.kill` 向探针发送 SIGTERM，探针记录的发送 PID 与 Python PID 一致。经桌面测试应用启动后退出，探针记录 `signal=15 sender_pid=174 sender_uid=0`；退出前进程快照及随后 `ps` 均将 PID 174 对应到系统 `loginwindow`；退出前探针的进程组 ID 与其 PID 相同、状态含 session leader，说明这次已经历启动路径的 `setsid`，仍未避免该 SIGTERM。因此本机这一次的信号发送者是 `loginwindow`，不是应用中那条已禁用的 HTTP 停止路径。仅凭进程快照不能证明其内部责任归因算法，也不能排除测试托管环境对其决策的影响。
4. 对照：独立 CLI 启动器退出后真实 daemon 仍存活；由 shell 直接启动的测试 GUI 进程被测试脚本以 SIGTERM 结束后，其测试 daemon 仍存活至少 3 秒。这是不同启动/退出路径的对照，**不能单独证明**所有正式 GUI 包或正常退出方式都采用同一种系统策略。
5. Apple 的 [Managing ongoing background processes in your Mac](https://developer.apple.com/documentation/appkit/managing-ongoing-background-processes-in-your-mac) 说明了可见应用直接启动的后台进程与退出生命周期之间的限制，并建议长期运行的服务考虑由 Service Management/launchd 管理。该资料与本机观察一致，但不把“特定 loginwindow 内部算法”当作 Apple 已保证的事实。本轮没有注册 LaunchAgent、登录项或改动系统服务；若产品要求 daemon 在桌面退出后持续运行，需单独设计安装、权限、生命周期和 #22 身份绑定。

## 活动会话退出确认为何不可见

- 依赖源码显示：`tauri-plugin-dialog` 在没设置 parent 时将 macOS 异步确认交给 `rfd` 的 `CFUserNotificationDisplayAlert`；设置 parent 时走附着在窗口上的 `NSAlert` sheet。原实现没有设置 parent。先前进程采样确认线程阻塞在 `blocking_show` / `CFUserNotificationDisplayAlert`，但窗口未出现在测试应用可访问窗口中。
- 现在有活动任务时，先取得并显示主窗口，将确认明确附着到窗口；主窗口不存在、不能显示或无法获得焦点时取消退出并记诊断，不再在该分支故意创建无父窗口提示。托盘或窗口关闭入口继续复用同一 `request_quit`；仍需注意平台窗口句柄失效和其他桌面环境的行为未实测。
- 在真实临时 Tauri 窗口中打开隔离 `local` resident session 后，早期版本点击退出可见带会话数的 sheet。为避免向未绑定端点发送管理员 token，最终 macOS 版本改为只凭 daemon 迹象显示活动状态未知，且不查询受保护的活动端点。 点击“取消”后，界面恢复且同一 resident session 仍存活；再次退出并确认后应用退出，daemon 在此测试环境收到 SIGTERM。这是桌面按钮路径的实测，不代替其他平台或托盘的实机矩阵。

## 仍需留意

- macOS 上本应用自己的默认退出分支不发送未绑定 `/shutdown` 或 PID 信号；这不阻止 `loginwindow` 或其他外部主体按其权限发送信号。GUI 现提示运行环境可能停止 daemon；macOS 上只要 PID 记录存在、记录损坏或无认证的健康端点可见就显示活动状态未知，不依赖归属标记，也不向该端点发送管理员 token。PID 文件与健康端点均没有证据时不会弹窗，无法证明没有隐藏进程。
- 系统信号发送者取自一次校准后的本机 `siginfo` 测试；未使用 Endpoint Security / 特权 DTrace 跟踪 `loginwindow` 的决策，无法说清内部实现及所有 OS 版本。`dtrace` 在当前普通用户、SIP enabled 环境因缺少权限无法启用。
- 正式打包、签名应用及托盘入口还没有实机验证。测试由 PID 快照、隔离测试 token 和合成 resident session 构成；未使用真实凭据或用户会话。

## 本轮验证

- `clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror -O1` 编译信号探针；先校准发送者 PID，再执行隔离 GUI 退出及 CLI／裸进程对照。
- daemon 真实进程测试验证 HTTP 与 SIGTERM 的 `trigger` 字段：`daemon_macos_conservative_e2e` **8 passed**；daemon 自身单测 **30 passed**；镜像 router / API 契约 **86 passed**。所有配置通过临时目录隔离；测试输出通过临时文件读取，不借用真实 daemon。
- `daemon_stop_e2e` **33 passed / 2 ignored**（旧自动替换 stale 语义），未配置 SSH fixture；无默认 feature 的库测试 **868 passed / 1 ignored**，默认 Tauri feature 的库测试 **885 passed / 1 ignored**。`npm test` 和 `npm run build` 通过；`cargo fmt --check`、`git diff --check` 通过。
- 活动 resident session 下，真实临时 Tauri 窗口验证 sheet 可见、取消后同一 session 仍存活、再次确认后应用退出；确认提示通过主窗口 parent 触发。托盘与正式签名包未实测。

## 合并前补充复核

macOS 退出确认不再读取需要管理员 token 的 daemon 活动接口：有 PID 记录、损坏记录或无认证健康端点迹象时直接显示活动未知；没有这些迹象时仍无法证明不存在隐藏 daemon。真实临时 Tauri 窗口中创建隔离 resident session 后删除合成 `daemon.owner`，退出仍显示“无法确认 daemon 的活动状态”的原生 sheet；取消后同一 session 仍存活。此验证不代表能检测没有 PID 和健康响应的所有后台进程。

# 临时 Mach 服务发现实验

2026-11-13，macOS 27.2.1 (26B131)，arm64。`python3 verify.py --archive` 使用随机服务名及自有临时进程，结果见 `results.txt`。三次正常退出及三次强制终止后，服务名消失，可由 B 重新注册；独立客户端之前持有的 A 发送权不会自动指向 B，重新查找才到达 B。

实验使用已弃用的 `bootstrap_register` 和本地临时服务，不证明生产部署、启动时机、持久化或信任根。重复注册的返回码仅作为本机观察，不能单独证明该名称所属进程的身份。

## launchd 管理服务的反例

`python3 verify_launchd.py --archive` 另用用户域临时 launchd job 的 `MachServices` 注册端点；每次由服务 `bootstrap_check_in` 后对 receive right 成功设置 `MPG_IMMOVABLE_RECEIVE`，发送一次合成消息并退出。三轮中，独立客户端持有的**旧发送权在 A 退出后发送成功，同一 job 被 launchd 重启后的 B 实际收到旧发送权上的消息**。详细结果见 `launchd-results.txt`；每轮结束均确认 job 不存在，未安装持久 LaunchAgent。

因此，直接 bootstrap 注册的独立 receive right 的观察不能推广到 launchd 管理端口：实验中设置 guard 后，旧发送权仍能向重启的服务投递；内部接管和重用机制未在本实验中验证。不能在这个端口上凭先前认证直接发送 token，必须另行证明目标端点在重启后不会被新实例接收。此实验使用 ad-hoc 签名，没有证明任何正式发布信任根。

## 独立实例端点的对照

`python3 verify_launchd_private.py --archive` 使用同样的临时 launchd 发现端口，但 A 自行创建并保护另一个 receive right，在无秘密回复中将它的发送权与 task identity token 返回。独立客户端核对回复的 audit token、task identity 和测试 harness 固定的 ad-hoc CDHash，持有独立发送权；三轮中 A 退出、B 被公共端口重新启动并保持存活后，旧独立发送权均返回 `MACH_SEND_INVALID_DEST`。每轮临时 job 均确认注销。结果在 `launchd-private-results.txt`。

该对照仅使用合成消息，没有向 B 发送真实 token；固定 hash 来自测试 harness，不是可信分发或正式签名根，也没有证明客户端能从发送权查询 guard。实例端点的运行时策略、guard 不变量、跨版本升级和其他异常仍须与正式部署一起验证。

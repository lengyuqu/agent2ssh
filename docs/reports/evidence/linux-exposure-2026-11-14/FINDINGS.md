# CNB #28 — Linux `/proc` 跨进程暴露面矩阵（hidepid × 读取身份）

日期：2026-11-14。仓库基线：`cnb/main` = `f92b44b`。这是 #28 在 2026-11-12 macOS 实测
（`../macos-exposure-crash-2026-11-12/`）之后补的 Linux 一腿。全部使用**合成标记**，目标是一个
替身进程，未运行 agent2ssh、未读取或写入任何真实凭据、未触碰 `~/.agent2ssh`。原始输出见同目录
`raw-linux-exposure.txt` / `raw-linux-root-caps.txt`。

## 0. 环境（实测）

| 项 | 值 |
|---|---|
| 宿主 | macOS 27.2（arm64），Apple `container` CLI 1.7.1，每个容器一个轻量 VM |
| 内核 | Linux 6.18.15 aarch64（`container` 自带内核） |
| 用户态 | `rust:bookworm`（Debian 12），`procps` 的 `ps`、util-linux 的 `setpriv`、Python 3.11 |
| 权限 | 容器以 `--cap-add ALL` 启动，容器内 root `CapEff=000001ffffffffff`，可重挂 `/proc` |
| Yama | **不存在**：`# CONFIG_SECURITY_YAMA is not set`，没有 `/proc/sys/kernel/yama/ptrace_scope` |

目标进程：uid 1001 的 `perl -e 'sleep 600' A2S_LINUX_ARGV_4D1A`，环境里带
`A2S_SYNTH=A2S_LINUX_ENV_C7E2`。它代表「把口令放在 argv 或环境变量里的进程」；`/proc` 的可见性
由内核按进程属主与挂载选项判定，与可执行文件是谁无关，但**本轮没有用真实 `agent2ssh` 进程做目标**。

读取身份：

| 标签 | 含义 |
|---|---|
| same-uid | uid 1001，与目标同属主，无附加组 |
| other-uid | uid 1002，无附加组 |
| other-uid-in-gid-group | uid 1003，附加组 2000（`gid=` 豁免组） |
| root-all-caps | 容器内 root，全部能力 |
| root-without-CAP_SYS_PTRACE | root，经 `setpriv --bounding-set=-sys_ptrace` 去掉该能力（`CapEff=000001fffff7ffff`） |

每个身份观察四项：pid 是否出现在 `/proc` 列表、读 `cmdline` 是否含 argv 标记、读 `environ` 是否含
env 标记、`ps -p <pid> -o args=` 是否含 argv 标记。

## 1. 矩阵（全部实测）

`argv` 列合并了 `cmdline` 与 `ps` 两项，两者在所有 25 格里结论一致。「不可见」后括号内是
`open()` 的 errno。

| 挂载 | 身份 | 列表可见 | argv | environ |
|---|---|---|---|---|
| `hidepid=0`（默认） | same-uid | 是 | **可见** | **可见** |
| | other-uid | 是 | **可见** | 拒绝（EACCES 13） |
| | other-uid-in-gid-group | 是 | **可见** | 拒绝（EACCES 13） |
| | root-all-caps | 是 | **可见** | **可见** |
| | root-without-CAP_SYS_PTRACE | 是 | **可见** | **可见** |
| `hidepid=1`（noaccess） | same-uid | 是 | **可见** | **可见** |
| | other-uid | 是 | 拒绝（EPERM 1） | 拒绝（EPERM 1） |
| | other-uid-in-gid-group | 是 | 拒绝（EPERM 1） | 拒绝（EPERM 1） |
| | root-all-caps | 是 | **可见** | **可见** |
| | root-without-CAP_SYS_PTRACE | 是 | **可见** | **可见** |
| `hidepid=2`（invisible） | same-uid | 是 | **可见** | **可见** |
| | other-uid | 否 | 不可见（ENOENT 2） | 不可见（ENOENT 2） |
| | other-uid-in-gid-group | 否 | 不可见（ENOENT 2） | 不可见（ENOENT 2） |
| | root-all-caps | 是 | **可见** | **可见** |
| | root-without-CAP_SYS_PTRACE | 是 | **可见** | **可见** |
| `hidepid=2,gid=2000` | same-uid | 是 | **可见** | **可见** |
| | other-uid | 否 | 不可见（ENOENT 2） | 不可见（ENOENT 2） |
| | other-uid-in-gid-group | 是 | **可见** | 拒绝（EACCES 13） |
| | root-all-caps | 是 | **可见** | **可见** |
| | root-without-CAP_SYS_PTRACE | 否 | 不可见（ENOENT 2） | 不可见（ENOENT 2） |
| `hidepid=4`（ptraceable） | same-uid | 是 | **可见** | **可见** |
| | other-uid | 否 | 拒绝（EPERM 1） | 拒绝（EPERM 1） |
| | other-uid-in-gid-group | 否 | 拒绝（EPERM 1） | 拒绝（EPERM 1） |
| | root-all-caps | 是 | **可见** | **可见** |
| | root-without-CAP_SYS_PTRACE | 否 | 拒绝（EPERM 1） | 拒绝（EPERM 1） |

## 2. 读法

1. **同 uid 在任何 hidepid 取值下都能读到 argv 和 environ。** hidepid 不隔离同一账户下的进程，
   这与 macOS 上 `KERN_PROCARGS2` 的同 uid 结论方向一致。
2. **默认挂载（`hidepid=0`）下，其他 uid 能读 argv、读不到 environ。** 所以旧的
   `--password` / `--passphrase` 把值放进 argv，在默认配置的多用户 Linux 上对**所有本机账户**可见；
   环境变量只对同 uid 与 root 可见。11-06 那次容器复核只证明了同 uid 可读，这一条是新的实测。
3. **`hidepid=1` 与 `hidepid=2` 都挡住了其他 uid 读 argv**，区别只在 pid 是否还出现在列表里，
   以及 errno 是 EPERM 还是 ENOENT。
4. **`gid=` 豁免组只恢复到 `hidepid=0` 的水平**：组内账户重新看得到 argv，environ 仍被拒绝。
5. **root 的两条通路要分开看。** 前三种挂载里「去掉 CAP_SYS_PTRACE 的 root」仍然全部可读，
   是因为 `gid=` 默认是 0，root 的主组正好命中豁免；把豁免组改成 2000 后它就被 hidepid 挡住了。
   另见第 3 节：`hidepid=0` 下 root 读他人 environ 靠的是 DAC 能力，不是 CAP_SYS_PTRACE。
6. `hidepid=4` 下其他 uid 的 pid 不在列表里，但直接 `open` 得到的是 EPERM 而不是 ENOENT，
   与 `hidepid=2` 不同。本轮只记录现象。

对 K8 的含义：`--password-stdin` / `--passphrase-stdin` 把值移出 argv 的收益在 Linux 默认配置下
比 macOS 同 uid 场景更大（argv 对其他账户也可见）。hidepid 是部署方的缓解手段，不是 agent2ssh
能依赖的保证；同 uid 的暴露它完全不处理。

## 3. root 读他人 environ 靠哪个能力（`hidepid=0`，实测）

`run_linux_root_caps.sh`，目标 uid 1005，`/proc/<pid>/environ` 权限位 `400`：

| root 去掉的能力 | `CapEff` | environ |
|---|---|---|
| 无 | `000001ffffffffff` | 可见 |
| `CAP_SYS_PTRACE` | `000001fffff7ffff` | 可见 |
| `CAP_DAC_OVERRIDE` + `CAP_DAC_READ_SEARCH` | `000001fffffffff9` | 拒绝 |
| 以上三项 | `000001fffff7fff9` | 拒绝 |

在这个内核上，只去掉 CAP_SYS_PTRACE 不足以阻止 root 读其他 uid 的 environ；去掉两个 DAC 能力
（保留 CAP_SYS_PTRACE）就读不到了。这是**本内核的实测现象**，没有做内核源码级归因，也没有把
两个 DAC 能力再拆开单测。

## 4. 明确未验证（不得写成平台结论）

- **`kernel.yama.ptrace_scope` 的任何取值**。本机能拿到的两个 Linux 内核都没有编入 Yama：
  Apple `container` 的 6.18.15，以及 Docker Desktop 的 7.0.14-linuxkit（`/proc/sys/kernel/yama/`
  与 `/sys/kernel/security/lsm` 均不存在）。同目录的 `run_linux_ptrace_scope_matrix.py` 是为带
  Yama 的内核（主流发行版的默认内核）准备的驱动，带一条真实 `PTRACE_ATTACH` 作对照，
  **本轮一次也没有运行过**，脚本本身未经验证。「Yama 只约束 attach、不影响 `/proc` 读取」
  是文档层面的预期，这里没有证据。
- 其他内核版本与发行版内核、x86_64、非容器的真实多用户主机。本轮是单一内核上的容器内结果，
  延续 11-06 复核的口径：**不要把容器结论写成平台结论**。
- user namespace（rootless 容器）下的可见性、`subset=pid` 挂载、SELinux / AppArmor 策略的叠加效果。
- 以真实 `agent2ssh host add --password …` 进程为目标的读取（本轮用替身进程）。
- shell history 的持久化点（使用方式推论，非实测）。

## 5. 复现

```bash
container run --rm --cap-add ALL -v "$PWD":/out rust:bookworm \
  sh -c 'python3 /out/run_linux_exposure_matrix.py'
container run --rm --cap-add ALL -v "$PWD":/out rust:bookworm \
  sh /out/run_linux_root_caps.sh
```

两个脚本都会在结束时把 `/proc` 恢复为 `hidepid=0` 并删除自己建的测试账户；请只在一次性的
容器或 VM 里运行。

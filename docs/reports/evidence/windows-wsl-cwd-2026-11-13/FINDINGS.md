# Windows `local:wsl:` 起始目录实测（2026-11-13）

`local_exec.rs::wsl_plan` 组出的命令是
`wsl.exe [-d <distro>] --shell-type login sh -c <command>`，并且**故意不设置 `cwd`**
（`EnvironmentPlan.cwd = None`），所以发行版里的起始目录由「调用进程的 Windows 工作目录」
翻译而来。这份记录把三种起点各跑一次，供 `local:wsl:...` 的文档与 #20 里那条悬着的
「`wsl.exe` 对 Windows cwd 的翻译」定论使用。

原始输出：`raw-wsl-cwd.json`；驱动脚本：`probe_wsl_cwd.py`（`python probe_wsl_cwd.py <outdir>`）。
本机 WSL：

```
NAME              STATE           VERSION
* Ubuntu            Stopped         2
  docker-desktop    Running         2
```

所有命令只打印 `pwd` 与 `$PWD`，不读写仓库文件。

## 结果

| 起点（进程 cwd） | `-d` | 输出 | 判定 |
|---|---|---|---|
| `C:\Users\Administrator` | 无 | `/mnt/c/Users/Administrator` | 本地盘按 `/mnt/<盘符>/<路径>` 翻译 |
| `C:\WINDOWS\System32` | 无 | `/mnt/c/WINDOWS/System32` | 同上；大小写按 `%SystemRoot%` 原样 |
| `D:\` | 无 | `/mnt/d` | 盘根翻译成 `/mnt/<盘符>` |
| `C:\Users\Administrator` | `-d Ubuntu` | `/mnt/c/Users/Administrator` | 指定发行版不改变翻译规则 |
| `\\localhost\C$\Users` | 无 | **`/root`**，stderr `wsl: Failed to translate '\\localhost\C$\Users'` | 普通 UNC 无法翻译，回退到**默认用户的 home**（本机 Ubuntu 默认用户是 root，故 `/root`） |
| `\\wsl.localhost\Ubuntu\home` | 无 | `/home` | `\\wsl.localhost\<distro>\<path>` 直接映射成发行版内路径，不经过 `/mnt` |
| `C:\WINDOWS` | 无 | `/mnt/c/WINDOWS` | 对照：UNC 调用者常见回退值（PowerShell 在 UNC 下会把进程 cwd 落到这里） |

## 结论

1. **起点不是用户主目录**，而是「调用进程的 Windows cwd」的翻译结果。daemon 由谁、从哪个
   目录拉起就决定了它：从 `C:\Windows\System32` 启动的 daemon 会让 `local:wsl:...` 落在
   `/mnt/c/WINDOWS/System32`，而 `cd C:\Users\me` 后执行的 CLI 会让它落在用户目录。
2. **`-d <distro>` 只选发行版，不改变起点**。两处都一样，不存在「指定发行版就回到 home」。
3. **UNC 有两种截然不同的结果**：`\\wsl.localhost\<distro>\<path>` 是发行版内路径（`/home`），
   其余 UNC 一律 `Failed to translate` 并回退到默认用户的 home（stderr 会留一条警告，
   stdout 里看不到任何提示——脚本只看 stdout 时不会察觉起点变了）。
4. 回退目录取决于该发行版的默认用户，本机是 root（`/root`），普通安装会是 `/home/<user>`；
   这与 `wsl.exe` 版本、`.wslconfig` 的 `default` 用户都有关，不能写死。

## 对产品的影响与选项（未改代码）

现状是「继承」，行为随启动位置变化，用户无法解释为什么有时在 `/mnt/c/...`、有时在 `/root`。

- **A. 保持继承，只写文档**：说明起点 = 调用进程 cwd 的翻译结果，UNC 会回退并打一条 stderr
  警告。代价 0，但 `local:wsl` 的结果仍取决于 daemon 的工作目录。
- **B. 显式固定起点**：给 `wsl_plan` 加 `--cd ~`（wsl.exe 支持），起点恒为默认用户 home。
  行为可预测，但改掉当前的「跟着 cwd 走」，属于对外可见变化，需要拍板。
- **C. 按请求传 cwd**：`EnvironmentPlan.cwd` 已经有字段，只是 wsl 分支不用；可以只在调用方
  显式给了目录时才把 `--cd <path>` 拼进去（路径需转成发行版内写法）。

本轮只记录实测，未改代码；B/C 都需要产品决定。

## 未验证

- 其他 WSL 版本（本机 2，`wsl.exe` 随 Windows 11 26300 提供）与 WSL 1。
- 默认用户不是 root 的发行版（本机 Ubuntu 的 OOBE 未跑完，`DefaultUid=0`），因此「回退到
  `/root`」这一格不能代表普通安装。
- 容器化/自定义 `\\wsl$` 共享的别名写法。

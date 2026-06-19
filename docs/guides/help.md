# Agent2SSH 帮助总览

Agent2SSH 是本机使用的 SSH 能力层，覆盖桌面操作、CLI 自动化、daemon 持久会话、端口隧道、SFTP 文件传输和 MCP Agent 集成。本页用于快速定位入口、常用命令和安全模型。

## 首次使用

1. 在桌面端 **Host Management** 页面添加主机，或导入 `~/.ssh/config`：

   ```bash
   agent2ssh host import-config
   ```

2. 用低风险命令验证主机可用：

   ```bash
   agent2ssh exec mybox "hostname && uptime"
   ```

3. 使用持久会话、端口转发、SFTP、Web 控制台或 MCP 前，先启动本地 daemon：

   ```bash
   agent2ssh daemon start
   ```

4. 查看本机状态：

   ```bash
   agent2ssh status
   ```

## 桌面端功能区

- **Host Management**：添加、编辑、连接、分组和删除主机；主机可绑定跳板机、代理、标签、环境、角色和负责人。
- **Proxies**：管理 HTTP CONNECT 和 SOCKS5 代理配置。代理配置保存在本地 `hosts.json`，主机通过 `proxy_id` 引用。
- **Terminal**：打开 SSH 终端，并可在应用主题、高对比度、Tokyo Night、Dracula、Nord、Solarized Light、Amber 等主题之间切换。
- **Execution**：执行单主机命令和多主机命令，并显示风险预览。
- **Files**：通过 SFTP 浏览和传输文件。页面采用左右双栏，支持路径跳转、目录进入、文件选择、新建目录和双向复制。
- **Tunnels**：创建本地 `-L` 或远程 `-R` 端口转发，并在列表中按主机查看和移除活跃隧道。
- **Activity / Audit**：查看 daemon 实时事件、最近审计记录、拒绝记录和异常提示。
- **Keys**：生成或导入本地 SSH 密钥。
- **Playbooks**：运行可复用命令序列。
- **Help**：查看首次使用清单、常用命令、安全说明和文档索引。

## 安全模型

Agent2SSH 会在执行前对命令和变更操作做风险判定。高风险命令需要审批或显式 `--force` / `force: true`；被判定为 `blocked` 的命令不能绕过。execution gate 可在本机暂停非桌面来源的 daemon 执行入口。

SSH 主机指纹不再要求人工确认。首次连接时会自动信任并写入 `~/.agent2ssh/known_hosts.json`；后续如果同一主机的指纹变化，连接会被阻止。

运行时文件、token、策略、审计日志、代理配置、已知主机指纹和本地密钥都保存在 `~/.agent2ssh/`。不要共享该目录。

## 文档索引

- [CLI 快速入门](./cli-quickstart.md)
- [MCP 快速入门](./mcp-quickstart.md)
- [配置指南](./configuration-guide.md)
- [Web 控制台指南](./web-console-guide.md)
- [Daemon API 快速入门](./daemon-api-quickstart.md)
- [MCP 客户端模板](./mcp-client-templates.md)

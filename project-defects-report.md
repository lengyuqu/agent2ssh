# Agent2SSH 项目缺陷检查报告

> 检查日期：2026-06-22  
> 项目版本：0.2.1  
> 检查范围：前端 (React/TS) + Rust 后端 + 构建流程 + 测试

---

## 1. 测试执行状态

| 检查项 | 状态 | 结果 |
|--------|------|------|
| 前端构建 (`tsc && vite build`) | ✅ 通过 | 无类型错误，无构建失败 |
| Rust 库单元测试 | ✅ 通过 | **202 / 202** 全部通过 |
| CLI Smoke 测试 | ✅ 通过 | **29 / 29** 全部通过 |
| Clippy 静态分析 | ⚠️ 未运行 | 沙箱权限限制，未执行 |
| 前端 ESLint | ⚠️ 未检查 | 未执行 |

---

## 2. 缺陷总览

| 模块 | 高风险 | 中风险 | 低风险 | 合计 |
|------|--------|--------|--------|------|
| 前端 (React/TS) | 6 | 12 | 8 | 26 |
| Rust 后端 | 5 | 10 | 7 | 22 |
| **合计** | **11** | **22** | **15** | **48** |

### 关键统计
- 生产代码 `unwrap()` 调用：**519 处**（Rust 源码）
- 生产代码 `panic!` 调用：**3 处**（execution_control.rs，审批逻辑路径中的断言）
- 前端 `as any` 类型断言：**0 处**（✅ 无随意类型绕过）

---

## 3. 高风险缺陷（11 项）

### 3.1 前端：6 项

| # | 位置 | 缺陷 | 风险 |
|---|------|------|------|
| F1 | `api.ts:598` | SSE 事件流 `JSON.parse` **无 try/catch**，单条畸形消息终止整个事件流 | 实时面板、审批通知全部失效，需刷新页面恢复 |
| F2 | `api.ts:517-522` | `approvalApprove` 调用 `fetch` **未检查 `res.ok`**，HTTP 4xx/5xx 仍 resolve 为成功 | 审批实际未执行，但 UI 认为已通过 |
| F3 | `api.ts:526-531` | `approvalReject` 同 F2 模式，拒绝审批操作同样未检查响应状态 | 高风险命令可能被误放行 |
| F4 | `App.tsx:353-369` | `handleApprove`/`handleReject` 在 API 可能静默失败时**仍从 UI 移除审批项** | 审批项永久丢失，用户无法重试 |
| F5 | `AddHostForm.tsx:83-112` | `handleSubmit` **无 try/catch**，API 错误完全未处理，表单直接清空 | 用户数据丢失，无错误提示 |
| F6 | `AddHostForm.tsx:111` | `onSaved()` 是 async 函数但未 await，**Promise 被丢弃**，内部错误静默 | 主机列表可能不刷新，未处理拒绝 |

### 3.2 Rust 后端：5 项

| # | 位置 | 缺陷 | 风险 |
|---|------|------|------|
| R1 | `core.rs:736` | `channel.exec(&command)` **直接传递用户命令**，远程 shell 解释所有元字符 | 若审批/门控被绕过，可执行任意远程命令 |
| R2 | `webdav_sync.rs:119, 147` | WebDAV URL **无校验**（不限制 scheme、不验 IP、跟随重定向），可指向内网服务 | SSRF 攻击，可访问内部 AWS 元数据等 |
| R3 | `gate.rs:74-76` | `source_can_bypass_gate` 仅凭字符串 `"desktop"` 判断，**客户端可伪造 source** | 门控暂停状态下被绕过，继续执行高危命令 |
| R4 | `store.rs:129-131` | 审计日志 `audit.jsonl` **明文存储**，虽经脱敏但包含命令/路径/行为模式 | 文件泄露时暴露大量运维信息 |
| R5 | `webdav_sync.rs:27` | `known_hosts.json` 在 **SYNCABLE_FILES** 中，WebDAV pull 无条件覆盖本地指纹 | 中间人攻击：推送伪造 fingerprint 使客户端信任恶意服务器 |

---

## 4. 中风险缺陷（22 项，摘要）

### 前端重点
- `api.ts:266` 多处 daemon 响应类型断言（`as {id: string}`）无运行时校验，结构不符时级联失败
- `AddHostForm.tsx:94` 密钥认证模式下未选择密钥时**静默提交 null**，导致无法连接
- `SFTPPanel.tsx:172` `ls -l` 日期解析依赖系统 locale，**非英文环境全部返回 null**
- `ExecPanel.tsx:50` 风险预览有 300ms 延迟，用户在预览更新前点击 Run 可**绕过审批对话框**
- `SFTPPanel.tsx:542` remote-to-remote 传输进度只统计 `uploaded`，忽略 `downloaded`，进度条不准确
- `AuditPanel.tsx:96` `Number()` 可产生 `NaN` 传入 API，行为未定义
- 多处 i18n 硬编码："env="、"role="、"owner="、"optional input..." 未走 `t()`

### Rust 后端重点
- `approval.rs:169-183` poll 与 list 超时状态不一致，同一审批在不同 API 间可能看到不同状态
- `embedded_ssh.rs:72-84` `key_path` 不拒绝 `../`，路径遍历风险（恶意配置可指向任意私钥）
- `approval.rs:88-99` 审批请求仅存在**内存中**，daemon 重启即全部丢失，可能中断安全流程
- `session.rs:127-164` 会话锁在持有期间循环调用 `recv_timeout`，高并发下阻塞其他会话操作
- `forward.rs:176` remote forward 目标地址无白名单，可转发到内网资源
- `embedded_ssh.rs:758` 诊断日志记录 `server_banner`，泄露操作系统/SSH 版本信息
- `daemon` token 明文文件存储，虽有 `0600` 但无加密
- WebDAV 密码支持环境变量，同一主机上其他进程可读取 `/proc/<pid>/environ`
- `store.rs:1083` 自定义 `glob_match` 递归回溯，恶意构造 pattern 可导致 ReDoS（指数级时间）
- `daemon_control.rs:53-57` 进程存活检查非原子，竞态下可能产生幽灵 PID 文件

---

## 5. 低风险缺陷（15 项，摘要）

- 前端 `useEffect` 依赖数组问题（如 `App.tsx:220` `refresh` 非 useCallback）
- `SFTPPanel.tsx:574` 并行传输取消有延迟，不会立即中断当前文件
- `TerminalPanel.tsx` 模块级计数器 HMR 时重置（仅开发环境）
- 审计日志仅保留 3 个历史文件，高频场景下可能快速覆盖
- `project!` 宏将版本写入 WebDAV marker，远程服务器可获取客户端版本
- 命令长度无限制，极长命令可能导致内存分配异常

---

## 6. 修复优先级建议

### 紧急（P0 — 直接影响安全性或可用性）
1. **`R2` WebDAV SSRF**：强制 `https://` scheme、禁用重定向、限制目标 IP
2. **`R3` 门控绕过**：将 `source` 与认证 token 的客户端类型绑定，不依赖客户端声明
3. **`R5` known_hosts 覆盖**：从 `SYNCABLE_FILES` 中移除 `known_hosts.json`
4. **`F1` SSE 崩溃**：`api.ts:598` 的 `JSON.parse` 加 `try/catch`，单条失败不影响整体流
5. **`F2/F3` 审批 API 失败**：`approvalApprove`/`approvalReject` 检查 `res.ok`，失败时抛异常

### 高（P1 — 影响功能正确性）
6. **`F5` 表单无错误处理**：`AddHostForm.tsx` 加 `try/catch` + 错误状态展示
7. **`F6` onSaved 未 await**：确保 async 回调被正确等待
8. **`R1` 命令注入缓解**：虽然已有门控/审批，但增加 shell 元字符转义或长度限制（纵深防御）
9. **`M9` glob ReDoS**：替换为成熟库或加递归深度限制
10. **`F4` 审批项丢失**：先确认 API 成功后再移除 UI 项

### 中（P2 — 影响体验或维护性）
11. `F11` 日期 locale 依赖：改用 `ls -l --time-style=full-iso` 或固定格式
12. `F9` 密钥校验缺失：提交前检查 `key_path` 非空
13. 多处类型断言无校验：使用 `zod` 或运行时验证替代 `as` 断言
14. i18n 硬编码：扫描剩余未翻译的字符串
15. 锁竞争与内存审批持久化：会话锁缩小范围，审批状态落盘

---

## 7. 总体评估

| 维度 | 评分 | 说明 |
|------|------|------|
| 构建稳定性 | ⭐⭐⭐⭐☆ | 前端+Rust 构建均通过，测试 100% 通过 |
| 运行时安全 | ⭐⭐⭐☆☆ | 5 个高风险安全问题，需紧急修复 WebDAV 和门控相关 |
| 前端健壮性 | ⭐⭐☆☆☆ | 错误处理缺失较多，SSE 和表单为关键薄弱点 |
| 后端安全 | ⭐⭐⭐☆☆ | 已知防护机制存在，但存在纵深防御缺口和可被绕过的逻辑 |
| 代码质量 | ⭐⭐⭐☆☆ | 519 个 unwrap 依赖服务稳定，3 个 panic 在不应出现的代码路径中 |
| 国际化 | ⭐⭐☆☆☆ | 基础设施已存在，但硬编码字符串未完全覆盖 |

---

*报告结束。共识别 48 项缺陷，其中 11 项高风险，22 项中风险，15 项低风险。*

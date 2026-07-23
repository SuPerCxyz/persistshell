# ADR-0009：有界 Attach 历史回放

状态：Accepted

日期：2026-07-23

## 背景

Running Session 的 Holder Ring Buffer 能保存最近输出，但 `exit` 或 `Ctrl+D` 后 runtime
会被释放。Closed Session 恢复只创建新 Shell 并恢复 cwd/环境，未把仍保留在磁盘的输出
日志提供给 attach client，用户因此看到空白的新 prompt。

PersistShell 必须让 attach 保持快速、有界和安全，不能读取无限日志、阻塞 PTY 写入或变成
终端模拟器。现有配置已经用 `replay_on_attach` 和 `replay_bytes` 定义自动回放边界。

## 决策

采用分层回放：

- Running Session 继续由 Holder Ring Buffer 回放。
- Closed Session 由 Daemon 在恢复 runtime 前读取轮转日志的有界尾部。
- 自动回放统一受现有 `replay_on_attach` 和 `replay_bytes` 控制。
- Daemon 在成功响应后先分片发送旧历史，再进入 Holder 实时代理。
- 日志尾读失败时安全降级为无历史 attach，不阻止 cwd 和环境恢复。

Closed 日志读取必须固定路径、不跟随 symlink，并验证普通文件、owner 和私有权限。历史只发送
给当前 client，不写入新 Ring 或日志。

## 选择理由

该方案不修改 public/private wire、metadata schema 或现有 Holder capability。恢复前快照将
旧日志与新 runtime 输出自然分界，可以稳定保证旧历史先于新 prompt，同时避免重复记录。
Running 路径继续使用内存 Ring，维持快速 attach 和断连期间输出的权威来源。

## 被考虑的方案

### 方案 A：Daemon 分层回放

兼容面小、顺序明确、实现可独立测试。缺点是 Daemon 增加一次有界磁盘尾读。

### 方案 B：预填充 Holder Ring

输出路径统一，但需要新增分片 private protocol、capability 协商和旧 Holder 降级，升级风险
高于本次需求。

### 方案 C：终端屏幕快照

可恢复完整屏幕模型，但需要 ANSI/终端状态解析，超出 PersistShell 非终端模拟器边界。

## 影响

### 正面影响

- Closed attach 可看到退出前上下文。
- Running/Closed 自动回放使用同一配置语义。
- 不改变已有协议和数据格式。

### 负面影响

- Closed attach 增加最多 `replay_bytes` 的磁盘读取和网络输出。
- 原始字节截断可能从 ANSI 序列或多字节字符中间开始。

### 风险

- 轮转顺序错误会打乱历史。
- 文件替换竞态可能破坏安全边界。
- 慢 client 可能扩大 attach 延迟。

这些风险必须通过 dirfd/`O_NOFOLLOW` 等安全打开方式、有界 frame、顺序测试和性能门禁控制。

## 回滚方案

可删除 Closed 日志前缀发送并恢复为仅 Holder Ring；不需要回滚协议、metadata 或配置。回滚
不得删除用户日志、移动历史 tag 或降低文件权限检查。

## 后续任务

- [ ] 实现安全的轮转日志有界尾读。
- [ ] 接入 Closed attach 输出顺序。
- [ ] 补齐 Running public attach 回归测试。
- [ ] 更新架构、用户、限制和变更文档。
- [ ] 构建新 patch 版本并在 `test` 验证。

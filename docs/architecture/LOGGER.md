# PersistShell Logger

本文档描述 PersistShell 的日志系统。

PersistShell 有两类日志：

1. 内部运行日志
2. Session 输出日志

---

## 内部运行日志

内部运行日志用于诊断 PersistShell 自身。

记录：

- Daemon 启动/停止
- Client 错误
- IPC 错误
- PTY 错误
- Metadata 错误
- 权限问题
- GC 行为
- Doctor 诊断信息

---

## Session 输出日志

Session 输出日志记录 PTY 输出。

用途：

- 查看断线期间输出
- 搜索历史错误
- 导出任务输出
- 排查长任务
- 用户审计自己的操作上下文

---

## Session 日志不是安全审计系统

Phase 1 Session 日志不定位为企业审计系统。

默认不记录用户输入。

默认只记录 PTY 输出。

未来可选增加输入审计，但必须明确安全和隐私风险。

---

## 日志路径

建议：

```text
~/.local/share/persistshell/logs/<session-id>.log
```

内部日志：

```text
~/.local/state/persistshell/daemon.log
```

或者遵循 XDG 目录规范。

---

## 权限

所有日志文件必须限制权限：

```text
0600
```

日志目录：

```text
0700
```

---

## 异步写入

Session 输出日志必须异步写入。

PTY 输出路径不能被磁盘写阻塞。

推荐：

```text
PTY Reader
    ↓
Ring Buffer
    ↓
Log Queue
    ↓
Async Log Writer
```

---

## 批量写入

日志写入应批量 flush。

避免每次输出都调用 write/fsync。

配置示例：

```toml
[logging]
flush_interval = "1s"
buffer_size = "1MB"
```

---

## fsync 策略

默认不应每次写入 fsync。

原因：

- 性能差
- 对高输出任务影响大

可以提供安全模式：

```toml
sync = false
```

---

## 日志轮转

必须支持按大小轮转。

示例：

```toml
max_file_size = "100MB"
max_files = 10
```

当日志超过大小：

```text
session.log
session.log.1
session.log.2.gz
```

---

## 日志压缩

旧日志可以压缩。

Phase 1 可选。

Phase 2 完善。

---

## 日志保留

保留策略示例：

```toml
retention_days = 30
max_total_size = "2GB"
```

必须避免日志无限增长。

---

## 日志关闭

用户必须可以关闭 Session 输出日志。

例如：

```toml
[logging]
session_log = false
```

或者：

```bash
persist config set logging.session_log false
```

---

## 日志脱敏

Phase 2 可支持脱敏规则。

例如：

- password=
- token=
- secret=
- private key
- AK/SK

注意：

脱敏不能保证完全安全。

默认不应做复杂语义解析。

---

## 密码输入

PersistShell 默认不记录用户输入，因此 sudo 密码等不会作为输入被记录。

但某些程序可能回显敏感信息到输出。

日志仍可能包含秘密。

必须在文档中提醒用户。

---

## 日志查看

CLI 应支持：

```bash
persist log <id>
persist tail <id>
persist export <id>
```

Phase 1 至少支持查看日志路径或 tail。

---

## 日志错误处理

如果日志写入失败：

- 不得影响 Session 运行。
- 应记录内部错误。
- 应在 `persist ls` 或 `persist info` 中显示 log_error。
- doctor 应能发现日志目录权限或磁盘问题。

---

## 磁盘满

如果磁盘满：

- 日志写入失败
- Daemon 不应崩溃
- PTY 不应阻塞
- Ring Buffer 继续工作
- 用户应收到明确提示

---

## 内部日志级别

支持：

- error
- warn
- info
- debug
- trace

默认建议：

```text
info
```

---

## 日志格式

内部日志建议结构化。

例如：

```text
timestamp level component message fields
```

Session 输出日志保持原始字节流或尽量原样。

---

## 测试

必须测试：

- 日志写入
- 日志轮转
- 日志目录权限错误
- 磁盘写失败模拟
- 大量输出
- 日志关闭
- 多 Session 并发日志
- log writer 不阻塞 PTY

---

## 不变量

1. 日志不得无限增长。
2. 日志写入不得阻塞 PTY。
3. 日志权限必须安全。
4. 日志失败不得导致 Session 失败。
5. 默认不记录用户输入。

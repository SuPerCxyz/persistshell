# PersistShell Internal Logging Rules

本文档定义 PersistShell 内部日志规范。

注意：本文档描述 PersistShell 自身运行日志，不是 Session 输出日志。
Session 输出日志见 `docs/architecture/LOGGER.md`。

---

## 日志目标

内部日志用于：

- 排查 daemon 问题
- 排查 IPC 问题
- 排查 PTY 问题
- 排查 metadata 问题
- 排查 install/uninstall 问题
- 支持 doctor

---

## 日志级别

支持：

```text
ERROR
WARN
INFO
DEBUG
TRACE
```

默认级别：

```text
INFO
```

---

## 日志内容

日志应包含：

- timestamp
- level
- component
- message
- session_id，若适用
- uid
- pid
- error code
- errno，若适用

示例：

```text
2026-07-08T12:00:00Z ERROR pty session=abc uid=1000 msg="openpty failed" errno=EMFILE
```

---

## 组件名称

推荐组件名：

```text
daemon
client
ipc
pty
session
metadata
ringbuffer
logger
installer
doctor
config
```

---

## 禁止记录

禁止记录：

- password
- token
- secret
- private key
- 用户输入内容
- 未脱敏环境变量
- 完整 SSH_AUTH_SOCK 是否视场景谨慎处理

---

## 日志路径

内部日志默认路径：

```text
~/.local/state/persistshell/daemon.log
```

Client 诊断日志可选：

```text
~/.local/state/persistshell/client.log
```

M03 已实现上述默认路径，并通过配置段覆盖：

```toml
[internal_log]
level = "info"
daemon_log = "/home/alice/.local/state/persistshell/daemon.log"
client_log = "/home/alice/.local/state/persistshell/client.log"
max_file_size = "20MB"
max_files = 5
```

---

## 权限

日志目录：

```text
0700
```

日志文件：

```text
0600
```

---

## 日志轮转

内部日志也必须轮转。

示例配置：

```toml
[internal_log]
max_file_size = "20MB"
max_files = 5
level = "info"
```

M03 只实现配置解析、校验和基础文件写入；完整轮转实现保留给后续任务。

---

## Daemon 启动日志

Daemon 启动时应记录：

- version
- uid
- runtime dir
- socket path
- metadata path
- config path
- log path
- protocol version

不要记录敏感配置值。

---

## Session 生命周期日志

应记录：

- session created
- session attached
- session detached
- session closed
- session killed
- session renamed
- session gc

---

## IPC 日志

正常高频 I/O 不应逐条记录。

禁止记录每个 STDIN/STDOUT frame。

只记录：

- connection open/close
- protocol error
- request error
- timeout
- permission denied
- attach/detach

---

## PTY 日志

应记录：

- openpty failed
- fork failed
- exec failed
- read/write error
- PTY HUP
- child exit
- resize error

不要记录 PTY 输出内容到内部日志。

---

## Metadata 日志

应记录：

- database open failed
- migration failed
- transaction failed
- corruption suspected
- permission error

---

## Debug/Trace

Debug 和 Trace 只能在用户显式开启时使用。

默认不得输出大量日志。

---

## 日志性能

日志不能显著影响核心路径。

尤其不能在 PTY I/O 热路径同步写大量内部日志。

---

## 测试

必须测试：

- 日志文件创建
- 权限正确
- 轮转配置解析和校验
- debug level
- 敏感信息不输出
- 日志目录不可写
- daemon 仍能给出明确错误

M03 已覆盖：

- 日志文件创建。
- 日志目录 `0700`。
- 日志文件 `0600`。
- 级别过滤。
- 初始化错误。
- 敏感关键词整条消息脱敏。

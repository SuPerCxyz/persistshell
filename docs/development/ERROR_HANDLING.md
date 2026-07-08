# PersistShell Error Handling

本文档定义 PersistShell 错误处理规范。

---

## 错误处理目标

错误必须：

- 可理解
- 可定位
- 可恢复
- 可测试
- 可记录
- 不泄露敏感信息

PersistShell 是系统工具，错误提示不能只面向开发者。

---

## 错误信息结构

每个用户可见错误应包含：

```text
错误是什么
为什么发生
影响是什么
如何修复
```

示例：

```text
错误：无法连接 PersistShell daemon。

原因：Unix Socket 不存在：
/run/user/1000/persistshell/persist.sock

影响：当前无法创建或恢复 Session。

建议：
1. 执行 persist daemon start
2. 或执行 persist doctor 检查环境
```

---

## 错误码

错误应有稳定错误码。

示例：

```text
E_DAEMON_NOT_RUNNING
E_SOCKET_PERMISSION
E_SESSION_NOT_FOUND
E_SESSION_BUSY
E_PTY_OPEN_FAILED
E_FORK_FAILED
E_EXEC_FAILED
E_METADATA_OPEN_FAILED
E_CONFIG_INVALID
E_UNSUPPORTED_ENVIRONMENT
E_PROTOCOL_VERSION
E_INTERNAL
```

---

## 错误分类

### 用户错误

例如：

- session id 不存在
- 命令参数错误
- 配置格式错误
- attach 已退出 Session

应给出修复建议。

---

### 环境错误

例如：

- runtime dir 不存在
- socket 权限错误
- 数据目录不可写
- shell 不存在
- 数据库打不开

应建议执行：

```bash
persist doctor
```

---

### 系统调用错误

例如：

- openpty failed
- fork failed
- setsid failed
- ioctl failed
- epoll failed
- socket failed

必须记录 errno 和上下文。

---

### 协议错误

例如：

- protocol version mismatch
- invalid frame
- unexpected message
- request timeout

必须记录 client version 和 daemon version。

---

### 内部错误

内部错误应尽量少。

如果出现，必须记录详细上下文，并提示用户提交 issue。

---

## 错误传播

底层错误应带上下文向上传播。

不要丢失原始 errno。

例如：

```text
openpty failed: errno=EMFILE, session_id=abc, uid=1000
```

---

## 日志与用户输出区别

用户输出应简洁。

内部日志应详细。

用户看到：

```text
错误：无法创建 PTY，当前进程打开文件数可能已达到上限。
建议：检查 ulimit -n 或执行 persist doctor。
```

内部日志记录：

```text
openpty failed errno=EMFILE uid=1000 session_id=abc rlimit_no_file=1024
```

---

## 敏感信息

错误和日志不得泄露：

- password
- token
- secret
- private key
- 完整敏感环境变量
- 用户输入内容中的敏感信息

---

## panic / abort 策略

除非发生不可恢复的程序错误，否则不得 panic/abort。

Daemon 尤其不能因为单个 Session 错误而崩溃。

单个 Session 失败应隔离。

---

## Daemon 错误隔离

一个 Session 的错误不应影响其它 Session。

例如：

- 某个 PTY read error
- 某个 log write error
- 某个 client disconnect
- 某个 metadata update error

都不应导致整个 daemon 崩溃。

---

## Client 错误恢复

Client 进入 raw mode 后，如果出错，必须尽量恢复终端状态。

需要处理：

- normal exit
- SIGINT
- SIGTERM
- panic/exception
- socket disconnect
- attach error

---

## doctor 集成

以下错误应建议 doctor：

- socket 权限错误
- daemon 未运行
- metadata 打不开
- log 目录不可写
- profile hook 错误
- shell 不存在
- runtime dir 权限错误
- stale socket

---

## 错误测试

必须测试：

- daemon 不存在
- socket 权限错误
- metadata 权限错误
- openpty 失败模拟
- fork 失败模拟
- shell exec 失败
- session 不存在
- attach busy
- protocol version mismatch
- client 中途断开
- log 写失败
- 磁盘满模拟

---

## 错误处理不变量

1. 错误不得静默吞掉。
2. 用户错误必须给出可理解提示。
3. 系统错误必须记录 errno。
4. 单 Session 错误不得杀死 daemon。
5. raw mode 出错必须恢复本地终端。
6. 安全相关错误不得自动绕过。

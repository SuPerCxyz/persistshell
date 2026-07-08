# PersistShell Session Protocol

本文档定义 PersistShell Session 级别的语义协议。

它描述 Client、Daemon、Session Manager 之间如何创建、恢复、切换、结束 Session。

底层 Socket 帧格式见：

```text
docs/protocol/SOCKET_PROTOCOL.md
```

---

## 协议目标

Session Protocol 需要支持：

- 创建新 Session
- Attach 到已有 Session
- Detach 当前 Session
- List Sessions
- Kill Session
- Rename Session
- 查询 Session 状态
- 同步终端大小
- 传输输入输出
- 返回结构化错误

---

## 基本原则

PersistShell 的 Session Protocol 必须保证：

```text
Client 生命周期不影响 Session 生命周期。
SSH 生命周期不影响 Shell 生命周期。
```

Client disconnect 只能触发 detach，不能触发 shell exit。

用户在 shell 中执行 `exit` 或空行 `Ctrl+D` 时，触发 close，而不是 detach。close 必须释放活动 shell runtime，但保留可恢复的 Session 记录、输出上下文、cwd 和环境变量快照。

---

## Session 创建

### 请求

```text
NEW_SESSION
```

字段：

```text
name optional
rows
cols
term
cwd optional
env allowlist
source_client
```

---

### 行为

Daemon 收到请求后：

1. 校验用户身份。
2. 生成 Session ID。
3. 创建 metadata。
4. 创建 PTY。
5. fork/exec 用户 shell。
6. 创建 ring buffer。
7. 初始化日志。
8. 返回 Session ID。

---

### 响应

```text
NEW_SESSION_RESP
```

字段：

```text
session_id
display_id
name
status
created_at
```

---

## Attach Session

### 请求

```text
ATTACH_SESSION
```

字段：

```text
session_id
rows
cols
term
replay
replay_bytes optional
mode
```

mode：

```text
read_write
read_only
```

Phase 1 只要求 read_write。

read_only 放到后续阶段。

---

### 行为

Daemon 收到 attach 请求后：

1. 校验 Session 是否存在。
2. 校验 owner UID。
3. 校验 Session 状态。
4. 检查 writer lease。
5. 必要时执行 writer takeover。
6. 注册 client。
7. 同步 terminal size。
8. 如果 Session 为 Closed，重新创建 PTY/shell 并恢复 cwd/env snapshot。
9. 回放 ring buffer。
10. 进入 live stream。

---

### 忙碌策略

默认推荐：

```text
同一 Session 同一时刻只有一个 active writer，但另一台电脑可以请求可写接管。
```

如果已有 writer：

可返回：

```text
ERR_SESSION_BUSY
```

也可以在请求带 takeover 选项时执行：

```text
--force
--takeover
```

`--read-only` 只能是可选查看模式，不能替代跨电脑可写进入会话的能力。

---

## Detach Session

### 请求

```text
DETACH_SESSION
```

字段：

```text
session_id
reason
```

reason 示例：

```text
user_request
client_exit
ssh_disconnect
socket_closed
error
```

---

### 行为

Daemon：

1. 注销 client。
2. 保留 PTY。
3. 保留 shell。
4. 更新 last_detached_at。
5. 如果没有 attached clients，标记为 Detached。

---

## Kill Session

### 请求

```text
KILL_SESSION
```

字段：

```text
session_id
signal optional
force optional
```

---

### 行为

Daemon：

1. 校验 owner。
2. 向 Session process group 发送 SIGTERM 或 SIGHUP。
3. 等待 grace period。
4. 如未退出，force 时发送 SIGKILL。
5. 关闭 PTY。
6. 更新 metadata。
7. 标记为 Killed。

---

## Rename Session

### 请求

```text
RENAME_SESSION
```

字段：

```text
session_id
new_name
```

---

### 要求

new_name：

- 不能为空。
- 不能包含控制字符。
- 长度必须有限制。
- 不应影响 Session ID。

---

## List Sessions

### 请求

```text
LIST_SESSIONS
```

字段：

```text
filter optional
sort optional
limit optional
```

---

### 响应字段

```text
session_id
display_id
name
status
created_at
last_active_at
last_attached_at
cwd
foreground_cmd
client_count
exit_code
log_available
pinned
tags
```

Phase 1 可以返回字段子集。

---

## Session Status

状态：

```text
creating
running
detached
closed
killed
zombie
recovering
archived
```

---

## Resize

### 请求

```text
RESIZE
```

字段：

```text
session_id
rows
cols
```

---

### 行为

Daemon：

1. 更新 runtime terminal size。
2. 对 PTY 执行 TIOCSWINSZ。
3. 更新 metadata。
4. 让前台程序收到 SIGWINCH。

---

## STDIN

Client attach 后发送：

```text
STDIN
```

payload：

```text
raw bytes
```

Daemon 将 raw bytes 写入 PTY master。

---

## STDOUT

Daemon 发送：

```text
STDOUT
```

payload：

```text
raw bytes
```

Client 写入本地 stdout。

---

## STDERR

Phase 1 可以不单独区分 STDERR。

PTY 输出通常已经合并 stdout/stderr。

---

## Replay

Attach 后 Daemon 可以先发送 replay 输出。

推荐用普通 STDOUT frame 承载，但附加 flag：

```text
replay = true
```

Replay 结束后发送：

```text
REPLAY_DONE
```

Phase 1 可简化，不一定实现 REPLAY_DONE。

---

## Close

Session 退出时，Daemon 发送：

```text
SESSION_EXITED
```

字段：

```text
session_id
exit_code
exit_time
```

Client 接收到后：

1. 恢复本地终端。
2. 打印退出状态。
3. 正常退出。

---

## 错误

所有请求都可能返回 ERROR。

字段：

```text
code
message
detail
suggestion
```

常见错误：

```text
ERR_SESSION_NOT_FOUND
ERR_PERMISSION_DENIED
ERR_SESSION_BUSY
ERR_SESSION_EXITED
ERR_INVALID_STATE
ERR_INVALID_REQUEST
ERR_PTY_FAILED
ERR_INTERNAL
```

---

## 不变量

1. Detach 不杀 shell。
2. Client disconnect 等价于 detach。
3. Session close 必须记录 exit code、cwd 和环境变量快照。
4. Kill 必须显式更新 Session 状态。
5. Attach 必须检查 owner UID。
6. Resize 只作用于 attached Session。
7. STDIN 只能发送到 read-write attached Session。

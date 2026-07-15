# PersistShell IPC Protocol

本文档描述 PersistShell Client 与 Daemon 之间的 IPC 设计。

Phase 1 使用 Unix Domain Socket。

---

## 设计目标

IPC 协议必须满足：

- 低延迟
- 支持流式 I/O
- 支持 request/response
- 支持控制消息
- 支持版本演进
- 支持错误码
- 支持超时
- 支持权限校验
- 易于调试
- 易于测试

---

## Socket 路径

推荐：

```text
/run/user/$UID/persistshell/persist.sock
```

如果不可用：

```text
$XDG_RUNTIME_DIR/persistshell/persist.sock
```

不推荐默认使用：

```text
/tmp
```

如果使用 `/tmp` fallback，必须保证目录权限安全。

---

## 权限要求

```text
runtime dir: 0700
socket:      0600
```

Daemon 启动时必须检查权限。

Client 连接时也应检查 socket owner 是否为当前用户。

---

## 协议类型

IPC 需要同时支持两类通信：

1. 控制请求
2. Attach 数据流

---

## 控制请求

用于：

- NewSession
- ListSessions
- AttachSession
- KillSession
- RenameSession
- DaemonStatus
- Doctor
- Config
- LogQuery

典型模式：

```text
Client → Request
Daemon → Response
```

---

## Attach 数据流

Attach 后进入双向流模式：

```text
Client stdin → Daemon → PTY
PTY → Daemon → Client stdout
```

同时仍需要控制消息：

- resize
- detach
- error
- heartbeat
- close

---

## 消息帧

Phase 1 推荐使用长度前缀帧。

示例：

```text
uint32 length
uint16 type
uint16 flags
uint32 request_id
payload bytes
```

payload 可以使用简单二进制编码或 MessagePack/CBOR。

如果选择 JSON 作为协议 payload，只能用于控制面，不应用于高频 I/O 数据面。

高频 I/O 必须避免 JSON 编解码开销。

---

## 消息类型

建议类型：

```text
HELLO
HELLO_ACK
ERROR

NEW_SESSION
NEW_SESSION_RESP

LIST_SESSIONS
LIST_SESSIONS_RESP

ATTACH
ATTACH_RESP

DETACH
DETACH_RESP

KILL_SESSION
KILL_SESSION_RESP

RENAME_SESSION
RENAME_SESSION_RESP

RESIZE

STDIN
STDOUT
STDERR

PING
PONG

CLOSE
```

---

## 协议版本

每次连接必须先交换版本。

Client 发送：

```text
HELLO {
  protocol_version,
  client_version,
  uid,
  pid,
  term,
  rows,
  cols
}
```

Daemon 返回：

```text
HELLO_ACK {
  protocol_version,
  daemon_version,
  server_pid,
  capabilities
}
```

如果版本不兼容，返回 ERROR。

---

## 错误码

错误必须结构化。

示例：

```text
ERR_PROTOCOL_VERSION
ERR_PERMISSION_DENIED
ERR_SESSION_NOT_FOUND
ERR_SESSION_EXITED
ERR_SESSION_BUSY
ERR_DAEMON_NOT_READY
ERR_INVALID_REQUEST
ERR_TIMEOUT
ERR_INTERNAL
ERR_UNSUPPORTED
```

错误响应应包含：

- code
- message
- detail
- suggestion

---

## Attach 流程

```text
Client → HELLO
Daemon → HELLO_ACK
Client → ATTACH(session_id)
Daemon → ATTACH_RESP(ok)
Daemon → STDOUT(replay ring buffer)
Client ↔ STDIN/RESIZE
Daemon → STDOUT(live output)
```

---

## New Session 流程

```text
Client → NEW_SESSION
Daemon:
  create metadata
  open pty
  fork shell
  create ring buffer
  create log writer
Daemon → NEW_SESSION_RESP(session_id)
Client → ATTACH(session_id)
```

---

## List Session 流程

```text
Client → LIST_SESSIONS
Daemon → LIST_SESSIONS_RESP
```

Response 包含：

```text
session_id
display_id
name
status
created_at
last_active_at
cwd
foreground_cmd
client_count
exit_code
```

M37 的列表响应额外提供运行中 Session 的 `foreground_pid`、`foreground_name` 和
`foreground_cmd`。无前台进程或 `/proc` 不可读时三个字段为空。

M38 使用 `PROCESS_TREE` 请求，payload 为 Session ID；`PROCESS_TREE_RESP` 返回有界
前台进程树节点列表，每个节点包含 pid、parent_pid、depth、name 与 command。

M41 使用 `SESSION_SNAPSHOT` 请求，payload 为 Session ID；
`SESSION_SNAPSHOT_RESP` 返回最多 16 KiB 的 JSON。响应包含 metadata 摘要、
writer 是否活跃、输出日志路径与前台进程摘要；不包含环境变量、输入、SSH agent 路径或
note/tag 内容。未知 Session 和超出上限通过 JSON 的 `error` 字段返回。

M42 使用无 payload 的 `METRICS` 请求；`METRICS_RESP` 返回最多 16 KiB 的 JSON，
包含 daemon PID 和 Session 聚合计数。它不启动 metrics server 或后台采样；metadata
不可用和超限通过 JSON 的 `error` 字段返回。

---

## Resize 流程

```text
Client receives SIGWINCH
Client reads terminal size
Client → RESIZE(rows, cols)
Daemon → ioctl(TIOCSWINSZ)
```

RESIZE 不需要每次响应，但错误需报告。

---

## Detach 流程

```text
Client → DETACH
Daemon unregisters client
Daemon keeps session alive
Daemon → DETACH_RESP
Client restores terminal
Client exits
```

如果 client 异常断开，Daemon 也应执行 detach。

---

## Heartbeat

Phase 1 可选。

后续用于检测半开连接。

```text
PING
PONG
```

---

## 流控

必须避免慢客户端拖垮 Daemon。

策略：

- 每个 client 有输出队列上限。
- 超过上限可以丢弃旧输出或断开慢 client。
- Ring Buffer 保留最近输出。
- 日志异步写入。

---

## 数据面与控制面分离

高频输出不要走复杂结构。

建议：

```text
STDOUT frame payload = raw bytes
STDIN frame payload = raw bytes
```

控制消息才使用结构化 payload。

---

## 安全检查

Daemon 接受连接后必须确认：

- socket peer uid
- 当前用户 owner
- 请求 session owner
- 权限匹配

Linux 可使用 SO_PEERCRED 获取 peer credentials。

---

## 超时

控制请求应有超时。

Attach 流模式不应因长时间无输出自动断开，除非配置了 idle timeout。

---

## 向后兼容

协议必须保留：

- protocol_version
- feature flags
- capability negotiation

新增字段应向后兼容。

---

## Phase 1 简化

Phase 1 可以实现最小协议：

- HELLO
- NEW_SESSION
- LIST_SESSIONS
- ATTACH
- DETACH
- KILL
- RESIZE
- STDIN
- STDOUT
- ERROR

但必须预留版本字段。

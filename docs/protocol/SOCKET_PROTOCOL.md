# PersistShell Socket Protocol

本文档定义 PersistShell Client 和 Daemon 之间的底层 Socket 帧协议。

---

## 传输方式

Phase 1 使用 Unix Domain Socket。

推荐路径：

```text
/run/user/$UID/persistshell/persist.sock
```

---

## 权限

Socket 目录：

```text
0700
```

Socket 文件：

```text
0600
```

Daemon 必须通过 SO_PEERCRED 校验 peer uid。

---

## 连接流程

```text
Client connect
  ↓
HELLO
  ↓
HELLO_ACK
  ↓
Request/Response or Attach Stream
```

---

## 帧结构

推荐二进制长度前缀帧：

```text
u32 length
u16 type
u16 flags
u32 request_id
bytes payload
```

说明：

```text
length: payload 长度，不包含 header
type: 消息类型
flags: 标记位
request_id: 请求 ID，用于匹配 response
payload: 消息内容
```

---

## 字节序

统一使用 little-endian 或 network byte order。

项目确定后必须固定。

推荐 network byte order，便于调试和协议一致性。

---

## 消息类型

```text
0x0001 HELLO
0x0002 HELLO_ACK
0x0003 ERROR

0x0100 NEW_SESSION
0x0101 NEW_SESSION_RESP
0x0102 LIST_SESSIONS
0x0103 LIST_SESSIONS_RESP
0x0104 ATTACH_SESSION
0x0105 ATTACH_SESSION_RESP
0x0106 DETACH_SESSION
0x0107 DETACH_SESSION_RESP
0x0108 KILL_SESSION
0x0109 KILL_SESSION_RESP
0x010A RENAME_SESSION
0x010B RENAME_SESSION_RESP

0x0200 STDIN
0x0201 STDOUT
0x0202 RESIZE
0x0203 REPLAY_DONE
0x0204 SESSION_EXITED

0x0300 PING
0x0301 PONG
0x0302 CLOSE
```

---

## Payload 编码

控制消息 payload 可以使用：

- MessagePack
- CBOR
- protobuf
- 简单自定义二进制结构

Phase 1 要求：

- 易实现
- 易测试
- 版本可演进

不建议高频 I/O 使用 JSON。

STDIN/STDOUT payload 必须是 raw bytes。

---

## HELLO

Client 首包必须是 HELLO。

字段：

```text
protocol_version
client_version
uid
pid
term
rows
cols
capabilities
```

---

## HELLO_ACK

Daemon 返回：

```text
protocol_version
daemon_version
pid
capabilities
```

---

## 协议版本

如果版本不兼容：

```text
ERR_PROTOCOL_VERSION
```

Client 应清楚提示用户升级 client 或 daemon。

---

## Request ID

每个 request/response 使用 request_id 匹配。

流式消息可以使用 attach request 的 request_id，或使用 0。

具体实现必须统一。

---

## Flags

建议 flags：

```text
FLAG_STREAM
FLAG_REPLAY
FLAG_COMPRESSED
FLAG_ERROR
```

Phase 1 可以只定义，不全部实现。

---

## 最大帧大小

必须限制 frame size。

建议默认：

```text
max_control_frame = 1MB
max_io_frame = 64KB
```

防止异常 client 发送超大 frame。

---

## 超时

控制请求应设置超时。

例如：

```text
default_request_timeout = 5s
```

Attach 流不应因为无输出而超时。

---

## 半关闭

Client 断开时，Daemon 应检测 socket close 并执行 detach。

Daemon 不能因为 client 异常断开而结束 Session。

---

## 错误帧

ERROR payload：

```text
code
message
detail
suggestion
```

错误码必须稳定。

---

## 安全检查

Daemon 接受连接后必须：

1. 获取 peer credentials。
2. 确认 uid 与 daemon owner 一致。
3. 拒绝非 owner。
4. 记录安全相关错误。
5. 不泄露其它 Session 信息。

---

## 调试

可以提供：

```bash
persist debug protocol
```

后续用于打印协议版本和 capability。

Phase 1 不必须。

---

## 不变量

1. 每个连接必须先 HELLO。
2. 不兼容版本必须拒绝。
3. STDIN/STDOUT 使用 raw bytes。
4. 控制 frame 必须限制大小。
5. Socket peer uid 必须校验。
6. Client 断开只触发 detach。

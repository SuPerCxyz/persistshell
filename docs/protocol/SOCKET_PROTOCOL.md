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

所有 header 和结构化整数 payload 固定使用 network byte order（big-endian）。

---

## 消息类型

```text
0x0001 HELLO
0x0002 HELLO_ACK
0x0003 ERROR

0x0010 NEW_SESSION
0x0011 NEW_SESSION_RESP
0x0012 LIST_SESSIONS
0x0013 LIST_SESSIONS_RESP
0x0014 ATTACH
0x0015 ATTACH_RESP
0x0016 DETACH
0x0017 RENAME
0x0018 RENAME_RESP
0x0019 DETACH_SIGNAL
0x001A DETACH_SIGNAL_RESP
0x001B SIGNAL
0x001C SIGNAL_RESP
0x001D NOTE_SET
0x001E NOTE_SET_RESP
0x001F NOTE_GET
0x0020 NOTE_GET_RESP
0x0021 TAG_ADD
0x0022 TAG_ADD_RESP
0x0023 TAG_REMOVE
0x0024 TAG_REMOVE_RESP
0x0025 TAG_LIST
0x0026 TAG_LIST_RESP
0x0027 LIST_SESSIONS_BY_TAG
0x0028 PIN_SET
0x0029 PIN_SET_RESP
0x002A ATTACH_READ_ONLY
0x002B WRITE_REQUEST
0x002C WRITE_GRANTED
0x002D WRITE_REVOKED
0x002E LOCK_SET
0x002F LOCK_SET_RESP
0x0030 PROCESS_TREE
0x0031 PROCESS_TREE_RESP
0x0032 PROCESS_STATS
0x0033 PROCESS_STATS_RESP
0x0034 SESSION_SNAPSHOT
0x0035 SESSION_SNAPSHOT_RESP
0x0036 METRICS
0x0037 METRICS_RESP
0x0038 DASHBOARD_SUMMARY
0x0039 DASHBOARD_SUMMARY_RESP
0x003A DASHBOARD_TREND
0x003B DASHBOARD_TREND_RESP

0x0100 STDIN
0x0101 STDOUT
0x0102 RESIZE
0x0103 SESSION_EXITED

0x0300 PING
0x0301 PONG
0x0302 CLOSE
0x0303 KILL
0x0304 CLOSE_RESP
0x0305 KILL_RESP
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

## Dashboard Payload

Dashboard 使用固定宽度 big-endian 二进制结构。解码必须精确消费整个 payload；截断、尾随数据、
未知枚举或超限计数均视为无效请求。

`DASHBOARD_SUMMARY`：

```text
u32 cursor
u16 limit              # 1..128，u32::MAX 不是合法 cursor
```

`cursor=0` 表示第一页；非零 cursor 必须是上一页最后一个 Session ID。服务端按 Session ID
升序返回下一页，`next_cursor` 在仍有后续数据时等于本页最后一个 Session ID。游标失效、请求
解码失败或 Dashboard 不可用时，服务端仍返回对应响应类型，并将 `completeness` 设为
`unavailable`、数据列表置空；连接保持可用。

`DASHBOARD_SUMMARY_RESP`：

```text
u64 sampled_at_ms
u8  completeness       # 0 complete, 1 partial, 2 stale, 3 unavailable
u32 next_cursor        # u32::MAX 表示没有下一页

daemon:
  u32 pid
  u8  rates_available  # 0 或 1
  u32 cpu_milli_percent
  u64 rss_kib
  u64 read_bytes_per_sec
  u64 write_bytes_per_sec
  u32 session_count
  u32 runtime_count
  u32 active_writer_count
  u32 readonly_client_count

u16 session_count      # 0..128
sessions[]:
  u32 session_id       # 非零
  u32 process_count
  u8  rates_available  # 0 或 1
  u32 cpu_milli_percent
  u64 rss_kib
  u64 read_bytes_per_sec
  u64 write_bytes_per_sec
  u32 foreground_pid   # 0 表示无
  u8  writer_active    # 0 或 1
  u8  collection       # 0 complete, 1 partial, 2 unavailable
```

`DASHBOARD_TREND`：

```text
u8  scope              # 0 daemon, 1 session
u32 session_id         # daemon 必须为 0；session 必须非零
u8  range              # 0 为 15m，1 为 1h，2 为 24h
u16 max_points         # 1..240
```

`DASHBOARD_TREND_RESP`：

```text
u64 sampled_at_ms
u8  completeness
u16 point_count        # 0..240
points[]:
  u64 timestamp_ms
  u32 cpu_avg_milli_percent
  u32 cpu_peak_milli_percent
  u64 rss_avg_kib
  u64 rss_peak_kib
  u64 read_bytes
  u64 write_bytes
  u32 process_count_peak
  u32 session_count
  u32 runtime_count
  u32 active_writer_count
  u32 readonly_client_count
```

CPU 单位是千分之一百分点，`100% = 100000`。实时 I/O 是 bytes/s，趋势 I/O 是对应时间桶累计
bytes。Dashboard payload 不包含命令、输出、环境变量、路径或进程命令行。

15 分钟和 1 小时趋势来自内存采样；24 小时趋势通过 writer 串行读取分钟分段，查询失败或超时
返回 `unavailable`，不会触发新采样。所有趋势响应最多 240 点。

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

## Attach Connection Context

public protocol 当前版本为 `0.2`。legacy Attach payload 仍是唯一的 4-byte big-endian
`session_id`。当 Client 与 Daemon 协商的 minor 均支持 `0.2` 时，Client 可以追加：

```text
u8  extension_version = 1
u8  variable_count
repeat variable_count:
  u8  name_length
  bytes name
  u16 value_length
  bytes value
```

允许的名称固定为：

```text
TERM COLORTERM SSH_AUTH_SOCK SSH_CLIENT SSH_CONNECTION SSH_TTY
DISPLAY WAYLAND_DISPLAY
```

最多 8 项，名称最多 32 bytes，单值最多 4096 bytes。名称和值必须是 UTF-8，值不能为空，
也不能包含控制字符。decoder 必须拒绝未知名、重复名、截断、尾随垃圾和超限输入。

`SSH_AUTH_SOCK` 必须是绝对路径、当前 UID 所有的非 symlink Unix socket。Client 采集时和
Daemon 接收时都必须验证；失败时只丢弃该项，不能回退使用 daemon 启动时继承的旧 agent。

connection context 只属于当前 Attach 请求，不写 metadata、Shell 状态文件或日志。Running
Session attach 不改变 runtime 环境；Closed Session 恢复边界接收该上下文，环境合并
阶段再决定如何应用。

兼容规则：

- 新 decoder 接受精确 4-byte legacy payload。
- 新 Client 收到旧 daemon 的 `0.1` HELLO_ACK 时只发送 legacy payload。
- 旧 decoder 读取新 payload 的前 4 bytes，仍得到相同 Session ID。
- 同一 major 的 minor 差异不导致握手失败。

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

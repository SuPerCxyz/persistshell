# PersistShell Holder 私有协议

本文档定义 `persistd` 与单一 per-user `persist-holder` 之间的内部二进制协议。该协议不面向
`persist` client，不承诺作为第三方公共 API。

## 传输与身份

默认 Unix Socket：

```text
/run/user/$UID/persistshell/holder.sock
```

runtime 目录权限为 `0700`，socket 权限为 `0600`。holder 必须使用 `SO_PEERCRED` 校验连接 UID。
控制连接建立后记录 daemon PID；数据连接的 peer PID 必须与当前控制 daemon 一致。

每个 holder 进程生成 16 字节 `instance_id`。daemon 每次控制连接生成 16 字节 nonce；后续数据
连接必须同时携带 instance ID 和 nonce。nonce 用于绑定连接代次，不替代 peer credential，也不
作为同 UID 进程之间的秘密。

## 固定帧头

所有整数使用大端序。帧头固定 32 字节：

| Offset | Size | Field |
|---:|---:|---|
| 0 | 4 | magic：`PSHH` |
| 4 | 2 | protocol major，当前为 1 |
| 6 | 2 | protocol minor，基线为 1，当前最高为 2 |
| 8 | 4 | payload length |
| 12 | 2 | message type |
| 14 | 2 | flags，当前必须为 0 |
| 16 | 4 | request ID |
| 20 | 8 | holder generation |
| 28 | 4 | reserved，必须为 0 |

控制 payload 最大 1 MiB；`Input`/`Output` 最大 64 KiB。解码必须拒绝错误 magic、版本不兼容、
未知消息、非零保留字段、截断、尾随字节和超限 payload。

## 连接模型

daemon 使用一条持久控制连接执行创建、关闭、终止、inventory 和 shutdown。每个 public attach
额外建立一条数据连接，用于该 attach 的输入、输出、resize、signal 和 writer 状态。

控制连接异常 EOF 时，holder 必须关闭该 daemon 的数据连接并撤销 writer，但不能关闭 PTY 或
Shell。只有认证控制连接发送 `ShutdownAll` 才能级联关闭 runtime。

## 消息类型

### 控制与状态

| Value | Message |
|---:|---|
| `0x0001/0x0002` | `ControlHello` / `ControlHelloAck` |
| `0x0003` | `Error` |
| `0x0004/0x0005` | `Capability` / `CapabilityResp` |
| `0x0010/0x0011` | `Inventory` / `InventoryResp` |
| `0x0012/0x0013` | `Create` / `CreateResp` |
| `0x0014/0x0015` | `Close` / `CloseResp` |
| `0x0016/0x0017` | `Kill` / `KillResp` |
| `0x0018/0x0019` | `ShutdownAll` / `ShutdownAllResp` |
| `0x001a/0x001b` | `GetExitContext` / `GetExitContextResp` |
| `0x001c/0x001d` | `RetireExited` / `RetireExitedResp` |
| `0x0020` | `SessionStarted` |
| `0x0021` | `SessionExited` |
| `0x0022` | `WriterChanged` |
| `0x0023` | `LogDegraded` |

### 数据连接

| Value | Message |
|---:|---|
| `0x0100/0x0101` | `DataHello` / `DataHelloAck` |
| `0x0110/0x0111` | `Attach` / `AttachResp` |
| `0x0112` | `Detach` |
| `0x0120/0x0121` | `Input` / `Output` |
| `0x0122` | `Resize` |
| `0x0123` | `Signal` |
| `0x0124/0x0125` | `WriteGranted` / `WriteRevoked` |

## Payload 约束

### 握手

- `ControlHello`：UID、daemon PID、16 字节 nonce。
- `ControlHelloAck`：holder PID、16 字节 instance ID、原样 nonce、状态。
- `DataHello`：daemon PID、instance ID、nonce。
- `DataHelloAck`：instance ID、原样 nonce、状态。
- PID、instance ID 和 nonce 不得为零；状态只允许 Accepted、VersionMismatch、
  PermissionDenied、Busy。

控制连接必须先使用 minor 1 完成 `ControlHello`。新 daemon 随后使用 minor 2
`Capability` 请求协商最高共同 minor；请求和响应绑定 instance ID 与 nonce。当前唯一能力位为
`environment-exit-context`。不认识 minor 2 的旧 Holder 可关闭探测连接，daemon 必须使用同一
instance 重新建立 minor 1 控制连接，且不得终止 Holder runtime。数据连接必须使用控制连接已
选定的 minor，不得独立协商。

### Inventory

请求包含 cursor 和 1 到 256 的 limit。响应最多 256 项；每项固定包含：

```text
session_id, shell_pid, state, optional exit_code,
created_at_ms, last_active_at_ms, ring_bytes,
writer_active, log_state, exit_context_available
```

Session ID 和 Shell PID 必须非零。Running 不能包含 exit code，Exited 必须包含 exit code；最后
活动时间不能早于创建时间。Running 的 `exit_context_available` 必须为 false；Exited 可用该
字段声明退出上下文仍可查询。分页结束使用内部 sentinel，sentinel 和零不能作为有效 cursor。

### Create

Create 包含 Session ID、Ring Buffer 大小、Shell 绝对路径、有界启动参数、可选 cwd、受限启动
环境、可选 history 路径、可选日志路径、状态文件绝对路径和 16 字节 state incarnation。限制：

- 路径最多 4096 字节且必须为绝对路径。
- 启动参数最多 16 个，单项最多 4096 字节；参数允许为空但拒绝 NUL。
- 环境变量最多 128 个；名称最多 128 字节，值最多 8192 字节。
- 环境名必须匹配 POSIX 标识符形式，值允许为空，所有字符串拒绝 NUL 和非法 UTF-8。
- Ring Buffer 为 1 字节到 64 MiB。
- 状态文件路径最多 4096 字节且必须为绝对路径，state incarnation 不得全零。

共享 `ShellLaunchEnvironment` 将启动环境分为 `saved_set`、`saved_unset`、`connection` 和
`private`。四层合计最多 128 项、64 KiB，不能通过每层分别达到 128 项来扩容。saved 层不能
覆盖或删除身份、基础、连接、敏感、`XDG_*` 或 `PERSIST_*` 变量；connection 只允许固定当前
连接变量；private 只接收 daemon 生成的可信运行时变量。

minor 1 的 Create wire 保持原扁平环境列表。兼容 encoder 按 saved set、connection、private
顺序降级为扁平 set，无法表达的 saved unset 不会伪装成空值。协商 minor 2 且存在环境能力时，
Create 使用结构化 v2 codec 精确保留四层；协商前不得向旧 Holder 发送 v2 payload。

### 操作与事件

Close、Kill、Detach 和 writer 事件使用非零 Session ID。操作响应包含 Session ID、状态和最多
1024 字节消息；状态只允许 Ok、NotFound、Conflict、Rejected、Internal。

SessionExited 包含 Session ID、有符号 exit code 和可选最终 cwd。cwd 必须是最多 4096 字节的
绝对 UTF-8 路径，且拒绝 NUL。LogDegraded 包含 Session ID 和非零丢弃字节数。Resize 的
rows/cols 必须非零；Signal 当前只接受 Linux signal 编号 1 到 64。

minor 2 且存在环境能力时，SessionExited 额外携带可选版本化环境 snapshot；minor 1 保持
原 cwd-only 字节格式。Holder 对 snapshot 执行共享结构、identity、sequence 和容量验证，但
不解释 allowlist。状态缺失或损坏不能丢失有效 exit code/cwd。

GetExitContext 请求复用只含非零 Session ID 的操作请求。成功响应状态为 Ok，必须携带 exit
code，可选携带与 SessionExited 相同约束的 cwd；NotFound、Conflict、Rejected 和 Internal
响应均不得携带 exit code 或 cwd。Running Session 返回 Conflict，不生成伪造退出上下文。
minor 2 成功响应可携带与 SessionExited 相同的环境 snapshot；该上下文由 Holder 保留到
RetireExited，确保 daemon 离线期间退出后仍可查询。

RetireExited 请求同样复用操作请求，响应复用普通操作响应。它只允许回收已经退出且已由 daemon
完成 metadata 持久化的 Session；Running Session 返回 Conflict，未知 Session 返回 NotFound。

Attach 包含 Session ID、读写/只读模式和最多 64 MiB 的 `replay_bytes`；零表示本次不回放。
Holder 只排队 Ring Buffer 尾部的指定字节，不能因完整 Ring 大于连接待写上限而拒绝 attach。

## Generation 与对账

holder 每次可观察状态变化后递增 generation。响应和状态事件携带生成号；daemon 完成 inventory
快照后必须确认快照 generation。如果读取期间 generation 改变，daemon 必须消费后续事件或重新
读取快照，不能在状态窗口未闭合时开放 Session 操作。

离线期间不保存无限事件序列。holder 保存每个 Session 最新幂等状态、有界 Ring Buffer、日志
降级计数和可用的最终退出上下文；daemon 重连以 inventory 为事实来源。

Inventory 的 `exit_context_available` 仅表示 Holder 当前仍可响应 GetExitContext。daemon 对
Exited Session 必须先查询退出上下文并原子更新 metadata，确认持久化成功后才能发送
RetireExited。metadata 更新失败时不得 retire；重连后必须依据 inventory 重试该顺序。这样即使
SessionExited 事件在 daemon 离线期间丢失，最终 cwd 和 exit code 仍能恢复。

## 错误处理

- 身份、PID、instance 或 nonce 不匹配：拒绝并关闭连接。
- major 不兼容或字段损坏：返回协议错误后关闭连接。
- 未知 Session：返回 NotFound，不创建隐式 runtime。
- 数据连接慢客户端队列超限：关闭该连接并撤销 writer，不阻塞 PTY。
- 控制连接异常断开：保留 runtime，等待新 daemon。

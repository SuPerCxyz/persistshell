# PersistShell Lifecycle

本文档描述 PersistShell 中 SSH、Client、Daemon、Session、PTY 和 Shell 的生命周期关系。

---

## 生命周期核心原则

SSH 生命周期不能决定 Shell 生命周期。

```text
SSH Disconnect != Shell Exit
```

以下情况会结束活动 Shell runtime：

- 用户在 Shell 内执行 exit
- 用户执行 persist kill
- Shell 进程自然退出
- Daemon 根据明确策略 GC 已关闭 Session
- 系统重启或 Holder 崩溃导致不可恢复

其中 `exit` 和 `Ctrl+D` 不应让 Shell 在后台继续运行。它们应释放 PTY 和 shell 进程，但保留
可恢复的 Session 记录、输出、最终 cwd 和受限动态环境快照。正常退出时 M55 side channel
采集允许的已导出变量和精确 unset；失败时回退上一可信环境。

---

## SSH 登录生命周期

用户执行：

```bash
ssh node
```

流程：

```text
OpenSSH authentication
    ↓
login shell/profile
    ↓
PersistShell hook
    ↓
persist client
    ↓
connect/start daemon
    ↓
create new session
    ↓
attach session
    ↓
interactive shell
```

---

## 新 Session 创建生命周期

```text
Client requests NewSession
    ↓
Daemon validates request
    ↓
Session Manager allocates SessionID
    ↓
Daemon creates runtime identity and private state path
    ↓
Daemon requests Holder Create
    ↓
Holder PTY Engine openpty()
    ↓
Holder forks; child setsid() and TIOCSCTTY
    ↓
Child exec user shell
    ↓
Holder owns PTY master, replay buffer and log writer
    ↓
Metadata created
    ↓
Session status = Running
    ↓
Client attached
```

---

## Attach 生命周期

```text
Client requests Attach(session_id)
    ↓
Daemon checks ownership
    ↓
Daemon checks session status
    ↓
Daemon authenticates client and requests Holder attach
    ↓
Client enters raw mode
    ↓
Holder replays bounded ring buffer
    ↓
Live IO forwarding starts
```

---

## Detach 生命周期

Detach 可以由以下事件触发：

- SSH 断开
- Client 退出
- 网络错误
- 用户显式 detach
- Client 崩溃
- Terminal 关闭

流程：

```text
Client disconnect detected
    ↓
Daemon unregisters client
    ↓
Session remains alive
    ↓
PTY remains open
    ↓
Shell continues running
    ↓
Session status = Detached if no clients
```

---

## Shell Close 生命周期

用户在 Session 内执行：

```bash
exit
```

或者在 shell 空行按 `Ctrl+D`，或 shell 自然退出：

```text
Shell hook atomically commits cwd and runtime identity
    ↓
Shell exits
    ↓
Holder drains PTY, reaps Shell and reads the private state file
    ↓
Holder retains exit code, optional final cwd and validated environment snapshot
    ↓
Holder emits SessionExited when daemon is connected
    ↓
Daemon calls GetExitContext for online/offline reconciliation
    ↓
Daemon commits exit code, final cwd and accepted environment to metadata
    ↓
Daemon calls RetireExited only after metadata succeeds
    ↓
Holder releases the retained runtime and state file
    ↓
Session status = Closed; log and metadata retained
    ↓
Session waits for retention/GC
```

状态文件使用每次 runtime 唯一的 identity 和单调 sequence。读取失败、身份不匹配、文件损坏、
非 UTF-8 cwd 或 hook 降级时，不阻塞 Shell 退出；daemon 使用退出前最后一次 `/proc`/metadata
cwd 和上一可信环境作为回退。Holder 在 daemon 离线时仍保留退出上下文，重启对账可重复执行
上述 metadata-first 流程。旧 Holder 的 minor 1 上下文不包含环境，daemon 必须按兼容回退处理。

Closed Session 可以再次 attach。再次 attach 时：

```text
Client requests Attach(closed_session)
    ↓
Client sends validated current terminal/SSH/display context
    ↓
Daemon validates ownership, retention and connection context
    ↓
Daemon opens a new PTY
    ↓
Holder starts user shell with saved cwd and allowed startup env snapshot
    ↓
Daemon replays retained output context
    ↓
Live IO forwarding starts
```

这不是恢复旧进程，而是恢复旧 Shell 会话的上下文和历史。

当前连接上下文只在该次 Closed restore 调用期间存在。Running Session attach 不修改其
现有环境，connection context 也不写 metadata、最终状态 side channel 或日志。

---

## Kill 生命周期

用户执行：

```bash
persist kill <id>
```

流程：

```text
Client sends KillSession
    ↓
Daemon checks ownership
    ↓
Daemon sends signal to session process group
    ↓
Grace period
    ↓
If alive, send SIGKILL
    ↓
Close PTY
    ↓
Update metadata
    ↓
Session status = Killed
```

---

## Daemon 生命周期

### 启动

Daemon 可以由以下方式启动：

- persist daemon start
- persist client 自动启动
- systemd user service
- login hook 按需启动

启动流程：

```text
Load config
    ↓
Create runtime dir
    ↓
Check permissions
    ↓
Acquire daemon lock
    ↓
Open metadata store
    ↓
Recover metadata
    ↓
Start/connect Holder and reconcile stable inventory
    ↓
Create Unix socket
    ↓
Start event loop
```

---

### 停止

Daemon 停止必须谨慎。

Phase 1 默认不允许在存在 Running Session 时直接退出。

停止策略：

```text
If no running sessions:
    daemon exits

If running sessions exist:
    reject stop unless --force
```

---

### 崩溃

Daemon 崩溃是严重事件。

当前 PTY 所有权已迁移到单一 per-user holder；metadata 对账由 M53 阶段 6 完成：

```text
Daemon crash
    ↓
Holder detects control disconnect
    ↓
Holder keeps draining PTY and retaining bounded state
    ↓
New daemon authenticates and reads inventory
    ↓
Stable snapshot and idempotent metadata reconciliation
    ↓
Client can attach again
```

对账完成并清退离线期间已退出的 Holder 项后才绑定 public socket。活动 metadata 缺少 runtime 时
标记为 `lost`；缺少 metadata 的 Holder runtime 作为 orphan 隔离并拒绝 attach。相同对账可在
daemon 再次崩溃后重复执行。

显式 `persist daemon stop` 使用认证的 `ShutdownAll` 级联关闭；异常断开和 `SIGKILL` 不发送该
消息。holder 自身崩溃后的恢复不属于 M53 承诺。

---

## GC 生命周期

GC 只处理不再活跃、已关闭或已归档的 Session。

示例策略：

```text
Closed Session 保留 30 天
Killed Session 保留 7 天
Archived Session 按配置保留
Pinned Session 不自动清理
Log 按大小和时间轮转
```

Phase 1 可以实现简单 GC。

复杂 GC 放到 Phase 2。

---

## Session 状态流转

```text
Creating
   ↓
Running
   ↓
Detached
   ↓
Running
   ↓
Closed
   ↓
Archived
   ↓
Deleted
```

异常路径：

```text
Running → Zombie
Running → Killed
Detached → Killed
Closed → Running
Closed → Deleted
```

---

## Client 生命周期

Client 是短生命周期进程。

启动：

```text
parse args
load config
connect daemon
send request
if attach: enter raw mode
forward IO
restore terminal
exit
```

Client 必须保证：

- 退出时恢复本地终端状态。
- 异常退出时尽量恢复终端。
- 不持有长期 Session 状态。
- 不因异常导致 daemon 崩溃。

---

## 安装生命周期

```text
persist install
    ↓
detect shell
    ↓
backup profile file
    ↓
inject hook
    ↓
create config dirs
    ↓
create runtime dirs if needed
    ↓
verify
```

---

## 卸载生命周期

```text
persist uninstall
    ↓
remove profile hook
    ↓
restore backup if possible
    ↓
stop daemon if safe
    ↓
keep user data unless --purge
```

默认 uninstall 不删除日志和 metadata。

删除数据必须显式：

```bash
persist uninstall --purge
```

---

## Bypass 生命周期

如果设置：

```bash
PERSIST_DISABLE=1
```

或：

```bash
SH_DISABLE=1
```

PersistShell hook 必须立即退出，不接管当前 SSH。

用户应进入普通 Shell。

---

## 生命周期不变量

必须始终满足：

1. Client 退出不杀 Session。
2. SSH 断开不杀 Session。
3. Session close 不杀 Daemon。
4. 一个用户不能访问另一个用户的 Session。
5. 非交互 SSH 不进入 PersistShell。
6. uninstall 后用户能恢复普通 SSH。

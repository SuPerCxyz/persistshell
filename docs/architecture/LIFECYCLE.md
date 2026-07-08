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
- 系统重启或 Daemon 崩溃导致不可恢复

其中 `exit` 和 `Ctrl+D` 不应让 Shell 在后台继续运行。它们应释放 PTY 和 shell 进程，但保留可恢复的 Session 记录、输出、cwd 和环境变量快照。

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
PTY Engine openpty()
    ↓
Daemon fork()
    ↓
Child setsid()
    ↓
Child TIOCSCTTY
    ↓
Child exec user shell
    ↓
Parent stores PTY master
    ↓
Metadata created
    ↓
Ring Buffer created
    ↓
Logger initialized
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
Daemon registers client
    ↓
Client enters raw mode
    ↓
Daemon replays ring buffer
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
Shell exits
    ↓
Daemon receives SIGCHLD
    ↓
PTY reaches EOF
    ↓
Session Manager records exit code
    ↓
Session Manager stores last cwd and env snapshot
    ↓
PTY is closed
    ↓
Shell process resources are released
    ↓
Session status = Closed
    ↓
Ring Buffer retained
    ↓
Log retained
    ↓
Metadata updated
    ↓
Session waits for retention/GC
```

Closed Session 可以再次 attach。再次 attach 时：

```text
Client requests Attach(closed_session)
    ↓
Daemon validates ownership and retention
    ↓
Daemon opens a new PTY
    ↓
Daemon starts user shell with saved cwd and env snapshot
    ↓
Daemon replays retained output context
    ↓
Live IO forwarding starts
```

这不是恢复旧进程，而是恢复旧 Shell 会话的上下文和历史。

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

Phase 1 不承诺 daemon 崩溃后恢复 PTY fd。

原因：

PTY master fd 由 daemon 持有，daemon 崩溃后 fd 关闭，Shell 可能收到 SIGHUP 或因为 PTY 关闭而结束。

文档必须诚实说明：

```text
Phase 1 目标是 SSH 断开不丢 Session；
不是 daemon 崩溃不丢 Session。
```

后续版本可以研究更复杂的 fd 继承、supervisor 或 helper 进程模型。

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

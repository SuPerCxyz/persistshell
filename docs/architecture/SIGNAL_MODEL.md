# PersistShell Signal Model

本文档描述 PersistShell 的信号处理模型。

信号处理是 PersistShell 正确性的关键。

---

## 核心原则

用户在远程 Shell 中按下控制键时，信号应该作用于远程 Session 内的前台进程，而不是 PersistShell Client、Daemon 或 sshd。

---

## 需要支持的信号和控制事件

PersistShell Phase 1 必须支持：

- Ctrl+C / SIGINT
- Ctrl+\ / SIGQUIT
- Ctrl+Z / SIGTSTP
- Ctrl+D / EOF
- SIGWINCH / window resize
- SIGCHLD / child exit
- SIGHUP / session kill 或 terminal hangup
- SIGTERM / graceful shutdown
- SIGKILL / force kill

---

## Ctrl+C

用户按下 Ctrl+C。

预期：

```text
Session 前台进程收到 SIGINT
```

例如：

```bash
sleep 100
```

按 Ctrl+C 后 sleep 应结束。

不应该：

- 杀死 persist daemon
- 杀死 persist client
- 杀死 sshd
- 关闭整个 Session，除非 shell 自己退出

---

## Ctrl+Z

用户按下 Ctrl+Z。

预期：

```text
Session 前台进程收到 SIGTSTP
```

例如：

```bash
vim
```

按 Ctrl+Z 后 vim 应暂停，Shell 返回 job control 提示。

---

## Ctrl+\

用户按下 Ctrl+\。

预期：

```text
Session 前台进程收到 SIGQUIT
```

---

## Ctrl+D

Ctrl+D 是 EOF 字符，不是普通 signal。

行为取决于当前程序。

在 shell 空行按 Ctrl+D：

```text
Shell close
```

在某些程序中可能表示输入结束。

PersistShell 应原样传递 EOF 控制字符到 PTY。

当 EOF 导致用户 shell 退出时，Daemon 必须把 Session 标记为 Closed，释放 PTY 和 shell 进程，并保存 cwd、环境变量快照、输出上下文和 exit code。不得把 Ctrl+D 解释成 detach，也不得让已退出 shell 在后台继续占用资源。

---

## SIGWINCH

当用户终端尺寸变化：

```text
local terminal resize
    ↓
client receives SIGWINCH
    ↓
client reads rows/cols
    ↓
client sends Resize message to daemon
    ↓
daemon ioctl(TIOCSWINSZ)
    ↓
foreground process receives SIGWINCH
```

必须测试：

- vim resize
- less resize
- top resize
- htop resize
- shell prompt resize

---

## SIGCHLD

Daemon fork 出 Shell 后，必须处理 SIGCHLD。

当 Shell 退出：

```text
daemon receives SIGCHLD
    ↓
waitpid
    ↓
collect exit status
    ↓
store cwd/env snapshot if available
    ↓
mark session closed
    ↓
cleanup pty
    ↓
update metadata
```

不能产生 zombie process。

---

## SIGHUP

SIGHUP 行为要谨慎。

SSH 断开时，不应该向 Session Shell 发送 SIGHUP。

因为 PersistShell 的目标就是 SSH 断开不杀 Shell。

只有在以下情况可以发送 SIGHUP：

- 用户显式 kill Session
- daemon shutdown --force
- Session 清理策略需要终止进程

---

## SIGTERM

SIGTERM 用于优雅终止。

场景：

- persist kill
- daemon stop
- system shutdown

对于 Session kill，可以先发送 SIGTERM 或 SIGHUP，再等待 grace period。

---

## SIGKILL

SIGKILL 只作为最后手段。

场景：

- grace period 后进程仍未退出
- 用户显式 force kill

---

## Foreground Process Group

正确信号语义依赖前台进程组。

在 PTY 中，terminal driver 通常会处理控制字符并向 foreground process group 发送信号。

PersistShell 不应手动错误转发所有控制字符为信号。

需要确保：

- PTY controlling terminal 正确
- foreground process group 正确
- raw mode 与远端 PTY termios 配合正确

---

## Client 本地信号处理

Client 自己也会收到本地终端信号。

Client 必须：

- 对 SIGWINCH：转发 resize
- 对 SIGTERM/SIGINT：清理本地 raw mode，detach
- 不让本地 Ctrl+C 直接杀死 client 而不通知 daemon
- 退出时恢复 termios

---

## Raw Mode 与信号

Client 进入 raw mode 后，本地终端不会像普通 cooked mode 那样处理 Ctrl+C。

Ctrl+C 会作为字节传给远端 PTY。

这是期望行为。

远端 PTY 决定如何产生 SIGINT。

---

## Attach/Detach 信号

Detach 不应该向 Session 发送 Ctrl+C、SIGHUP 或 SIGTERM。

Detach 只是断开 client 与 session 的 I/O 连接。

---

## Kill Session 信号策略

推荐策略：

```text
persist kill <id>
    ↓
send SIGHUP or SIGTERM to session process group
    ↓
wait grace period
    ↓
send SIGKILL if still alive
```

需要配置 grace period。

例如：

```text
kill_grace = 3s
```

---

## Daemon Shutdown 信号策略

如果 daemon 收到 SIGTERM：

- 如果没有 running session，可以退出。
- 如果有 running session，默认拒绝或延迟退出。
- 如果 force shutdown，则按策略终止 Sessions。

Phase 1 可以简单处理，但必须安全。

---

## Signal 测试矩阵

必须测试：

```bash
sleep 100
Ctrl+C
```

```bash
vim
Ctrl+Z
fg
```

```bash
cat
Ctrl+D
```

```bash
top
resize terminal
```

```bash
less /var/log/messages
resize terminal
```

```bash
bash
exit
```

---

## 不变量

1. SSH 断开不发送 SIGHUP 给 Session Shell。
2. Ctrl+C 影响 Session 前台进程，不影响 daemon。
3. SIGCHLD 必须 waitpid。
4. Client 异常退出必须尽量恢复本地终端。
5. Resize 必须同步到 PTY。
6. Kill Session 必须明确更新 metadata。
7. exit/Ctrl+D 进入 Closed，不进入 Detached。

# PersistShell PTY Engine

本文档描述 PersistShell PTY Engine 的设计。

PTY Engine 是 PersistShell 最核心、最底层的模块之一。

---

## PTY Engine 职责

PTY Engine 负责创建和管理伪终端。

职责包括：

- openpty()
- fork()
- setsid()
- ioctl(TIOCSCTTY)
- execve 用户 shell
- 配置 termios
- 设置窗口尺寸
- 设置非阻塞 I/O
- 读取 PTY 输出
- 写入 PTY 输入
- 处理 EOF
- 处理 SIGCHLD
- 管理 PTY 生命周期

---

## PTY Engine 不负责

PTY Engine 不负责：

- CLI 参数解析
- Session 列表输出
- SQLite metadata 细节
- 日志轮转策略
- SSH 自动接管
- 用户配置文件注入
- 权限策略展示

这些由上层模块处理。

---

## PTY 创建流程

标准流程：

```text
openpty(master, slave)
    ↓
fork()
    ↓
child:
    setsid()
    ioctl(slave, TIOCSCTTY)
    dup2(slave, STDIN_FILENO)
    dup2(slave, STDOUT_FILENO)
    dup2(slave, STDERR_FILENO)
    close(master)
    close(unused fds)
    set env
    execve(user shell)

parent:
    close(slave)
    set master non-blocking
    store master fd
```

---

## 为什么需要 setsid()

setsid() 用于创建新的 session。

它让 child 脱离原控制终端，并成为新的 session leader。

随后通过 TIOCSCTTY 将 PTY slave 设置为控制终端。

---

## 为什么需要 TIOCSCTTY

Shell 和交互式程序需要控制终端。

例如：

- Ctrl+C
- Ctrl+Z
- job control
- foreground process group
- terminal resize
- password prompt

如果没有正确设置控制终端，很多交互式程序会异常。

---

## Shell 选择

默认 shell 应从系统用户数据库读取：

```bash
getent passwd "$USER"
```

对应字段为用户 login shell。

不能假设所有用户都使用 bash。

必须支持：

- bash
- zsh
- fish
- sh

如果读取失败，可 fallback 到：

```text
/bin/sh
```

---

## 环境变量

创建 Session 时需要设置环境变量。

启动环境分层：

```text
当前 runtime 身份和基础环境
saved_set
saved_unset
current connection override
private PersistShell environment
```

注意：

- 所有名称、值、冲突、数量和容量必须在 fork 前验证并转换为 CString。
- child 只按固定顺序执行有界 `setenv`/`unsetenv`，unset 不能用空字符串代替。
- `HOME`、`USER`、`LOGNAME`、`SHELL`、`PATH`、`PWD`、`XDG_*` 和 `PERSIST_*`
  不能由 saved snapshot 覆盖或删除。
- 当前连接变量覆盖 child 继承环境；private runtime 变量最后应用。
- Shell exec 和用户 rc 仍可在启动后修改普通变量。
- 不要盲目复制所有 SSH 环境变量。
- 不要记录敏感环境变量。
- Running Session attach 不覆盖已有 Session 环境。

---

## Working Directory

新 Session 默认 cwd：

优先级：

1. 用户当前登录 shell cwd
2. HOME
3. /

如果 chdir 失败，应 fallback 到 HOME 或 /。

---

## Non-blocking I/O

PTY master 必须设置为 non-blocking。

原因：

- 避免 Daemon 被单个 Session 卡住。
- 支持 epoll。
- 支持高并发 Session。
- 支持慢客户端处理。

---

## epoll 集成

PTY master fd 应注册到 Daemon event loop。

关注事件：

- EPOLLIN
- EPOLLOUT
- EPOLLHUP
- EPOLLERR

必须正确处理：

- partial read
- partial write
- EAGAIN
- EINTR
- EOF
- HUP

---

## 输入写入

Client 输入通过 IPC 到 Daemon。

Daemon 写入 PTY master。

注意：

- 写入可能 partial。
- 写入可能 EAGAIN。
- 需要 per-session input queue。
- input queue 必须有上限。
- 慢 PTY 不能拖垮 Daemon。

---

## 输出读取

PTY 输出由 Daemon 从 master fd 读取。

输出路径：

```text
PTY master
    ↓
Ring Buffer
    ↓
Attached clients
    ↓
Async log writer
```

读取应尽量批量。

避免每字节处理。

---

## EOF 处理

PTY EOF 可能表示：

- Shell 退出
- PTY slave 关闭
- Session 结束
- 异常状态

PTY Engine 应通知 Session Manager 更新状态。

---

## Window Size

Client attach 时应发送当前终端尺寸。

PTY Engine 设置：

```text
ioctl(master_fd, TIOCSWINSZ, winsize)
```

当用户终端 resize：

```text
Client SIGWINCH
    ↓
send resize message
    ↓
Daemon updates PTY size
    ↓
foreground process receives SIGWINCH
```

---

## Signal 与前台进程组

在 PTY 中，Ctrl+C 等通常作为控制字符进入终端，由内核 terminal driver 发送给前台进程组。

但 PersistShell 仍需正确管理：

- foreground process group
- SIGWINCH
- SIGHUP on kill
- SIGTERM/SIGKILL on kill

禁止把 Ctrl+C 误处理成杀 daemon。

---

## Raw Mode

Client attach 后，Client 本地终端需要进入 raw mode。

这样用户输入能原样传输到远端 PTY。

Client 退出时必须恢复原 termios。

即使异常退出，也应尽量恢复。

---

## 特殊控制键

必须测试：

- Ctrl+C
- Ctrl+D
- Ctrl+Z
- Ctrl+\
- Backspace
- Delete
- Arrow keys
- Tab
- Enter
- Home/End
- PageUp/PageDown

这些多数以字节序列形式传递到 PTY。

---

## 中文和宽字符

PTY Engine 不应主动解释字符宽度。

但 Ring Buffer、日志、回放和 UI 展示要注意：

- UTF-8 不应被截断破坏。
- 中英文混合输出不能导致程序崩溃。
- ANSI escape sequence 不应被随意改写。

Phase 1 可以以字节流保存。

---

## 全屏 TUI 程序

需要兼容：

- vim
- nano
- less
- top
- htop
- btop
- watch

PersistShell 不承诺完美恢复所有 TUI 画面，但 attach 后应能继续交互。

Ring Buffer 回放可能不能完整还原 alternate screen。

后续可以设计 terminal state cache。

---

## PTY 生命周期清理

Session 结束时必须：

- 关闭 PTY master fd
- 清理 input queue
- 清理 output watchers
- 注销 epoll
- 更新 metadata
- flush log
- 保留 ring buffer 或转储

---

## 错误处理

PTY Engine 应明确返回错误：

- openpty failed
- fork failed
- setsid failed
- TIOCSCTTY failed
- dup2 failed
- execve failed
- chdir failed
- set nonblock failed
- ioctl failed
- read/write failed

错误必须能被 doctor 和日志定位。

---

## Phase 1 要求

Phase 1 PTY Engine 必须支持：

- 启动用户默认 shell
- 基础交互
- Ctrl+C/Ctrl+D/Ctrl+Z
- resize
- detach 后 shell 继续运行
- exit/Ctrl-D 后释放 shell runtime
- Closed Session attach 后可恢复输出、cwd 和允许持久化的环境变量
- attach 后继续操作
- bash/zsh/fish 基础兼容
- vim/top/less 基础可用

---

## 不做的事情

Phase 1 PTY Engine 不做：

- Pane
- Window
- 自定义终端模拟器
- ANSI 语义解析
- 完整屏幕快照
- 多用户共享 PTY
- 跨 daemon 崩溃恢复 PTY fd

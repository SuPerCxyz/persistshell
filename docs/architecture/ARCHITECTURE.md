# PersistShell Architecture

本文档描述 PersistShell 的整体架构。

PersistShell 的核心目标是：

```text
让交互式 Shell 的生命周期独立于 SSH 连接生命周期。
```

---

## 总体架构

PersistShell 由以下核心组件组成：

```text
SSH Client
    ↓
User Login Shell / Profile Hook
    ↓
PersistShell Client
    ↓
Unix Domain Socket
    ↓
PersistShell Daemon
    ↓
Session Manager
    ↓
PTY Engine
    ↓
User Shell
    ↓
Foreground / Background Processes
```

---

## 核心原则

### SSH 只负责连接

OpenSSH 继续负责：

- 网络连接
- 用户认证
- 加密传输
- 登录目标机器

PersistShell 不替代 SSH。

---

### Daemon 持有 PTY

PersistShell Daemon 是持久 Session 的所有者。

Daemon 持有：

- PTY master
- Shell 进程
- Session metadata
- Ring Buffer
- Session log
- Client attach 状态

SSH Client 断开时，Daemon 不退出，PTY 不关闭，Shell 不结束。

---

### Client 只是输入输出通道

PersistShell Client 的职责是：

- 检测当前是否交互式 SSH
- 连接本机 PersistShell Daemon
- 创建新 Session
- Attach 到 Session
- 转发 stdin/stdout/stderr
- 同步终端尺寸
- 处理 detach
- 提供 CLI 命令

Client 不持有 Shell 生命周期。

Client 退出不应导致 Session 结束。

---

## 进程模型

典型进程关系：

```text
sshd
 └── user login shell
      └── persist client
            ↔ Unix Socket ↔ persist daemon
                              └── shell
                                   └── foreground process
```

注意：

Shell 不应该是 SSH 登录 shell 的直接生命周期依赖对象。

实际实现上，Shell 可以是 Daemon fork 出来的子进程。

SSH 断开时，退出的是 `persist client`，不是 Shell。

---

## 默认登录行为

每次交互式 SSH 登录：

```text
ssh node
```

PersistShell 应该：

1. 检测这是交互式 SSH。
2. 检查是否设置了 bypass。
3. 启动或连接 per-user daemon。
4. 创建一个新的 Session。
5. Attach 到该 Session。
6. 用户看到普通 Shell。

默认不自动进入历史 Session。

历史 Session 需要用户显式 attach。

---

## 手动恢复行为

用户登录新 Session 后，可以执行：

```bash
persist ls
```

查看历史 Session。

再执行：

```bash
persist attach <id>
```

进入旧 Session。

此时 PersistShell Client 会 detach 当前 Session 或根据策略处理当前 Session，然后 attach 到目标 Session。

---

## 数据流

### 输入路径

```text
User Keyboard
    ↓
SSH Client
    ↓
sshd
    ↓
persist client stdin
    ↓
Unix Socket
    ↓
persist daemon
    ↓
PTY master
    ↓
PTY slave
    ↓
Shell / Foreground Process
```

---

### 输出路径

```text
Shell / Foreground Process
    ↓
PTY slave
    ↓
PTY master
    ↓
persist daemon
    ↓
Ring Buffer
    ↓
Async Logger
    ↓
Attached Clients
    ↓
Unix Socket
    ↓
persist client stdout
    ↓
SSH
    ↓
User Terminal
```

---

## 为什么需要 Ring Buffer

SSH 断开期间，任务可能继续输出。

用户重新 attach 时，需要看到最近输出。

Ring Buffer 提供：

- 最近输出缓存
- 快速 attach 回放
- 避免日志磁盘读取阻塞 attach
- 防止内存无限增长

Ring Buffer 必须固定大小。

---

## 为什么需要异步日志

如果每次 PTY 输出都同步写磁盘，会导致：

- 高输出任务变慢
- Daemon 被磁盘 I/O 阻塞
- 慢磁盘拖垮所有 Session

因此输出路径必须是：

```text
PTY -> Ring Buffer -> Client
                 \
                  -> Async Log Writer
```

日志写入不得阻塞 PTY 读取。

---

## Metadata Store

Metadata Store 存储 Session 的长期状态，例如：

- Session ID
- Name
- Owner UID
- Created At
- Last Active At
- Status
- Shell PID
- Exit Code
- CWD
- Log Path
- Ring Buffer 配置
- Tags
- Notes

Phase 1 推荐 SQLite。

禁止使用 JSON 作为主要数据库。

---

## Unix Socket IPC

Client 与 Daemon 通过 Unix Domain Socket 通信。

推荐路径：

```text
/run/user/$UID/persistshell/persist.sock
```

如果 `/run/user/$UID` 不可用，可 fallback 到：

```text
$XDG_RUNTIME_DIR/persistshell/persist.sock
```

不建议默认使用 `/tmp`。

如果必须使用 `/tmp`，必须防范：

- symlink attack
- 权限错误
- socket 劫持
- stale socket

---

## 安全边界

Phase 1 使用 per-user daemon。

原则：

- 每个用户拥有自己的 daemon。
- 每个用户只能访问自己的 socket。
- 每个用户只能 attach 自己的 Session。
- root 行为后续单独设计，Phase 1 不做复杂多用户管理。

权限要求：

```text
runtime dir: 0700
socket:      0600
metadata:    0600
logs:        0600
```

---

## Signal 处理

PersistShell 必须正确处理：

- SIGINT
- SIGQUIT
- SIGTSTP
- SIGWINCH
- SIGCHLD
- EOF

关键要求：

Ctrl+C 等信号应作用于 PTY 前台进程组，而不是错误地杀死 daemon 或 client。

---

## Terminal Size

Client attach 时必须上报当前终端尺寸。

终端大小变化时：

1. Client 捕获 SIGWINCH。
2. Client 查询本地 rows/cols。
3. Client 通过 IPC 通知 Daemon。
4. Daemon 对 PTY 执行 TIOCSWINSZ。
5. Daemon 通知前台进程组 SIGWINCH。

---

## SSH 自动接管

PersistShell 只能接管交互式 SSH。

接管条件示例：

```text
存在 SSH_CONNECTION
stdin 是 TTY
stdout 是 TTY
没有 SSH_ORIGINAL_COMMAND
没有 SH_DISABLE=1
没有 PERSIST_DISABLE=1
当前不在 PersistShell 内部
```

不得接管：

- scp
- sftp
- rsync
- ansible
- git over ssh
- ssh node command
- cron
- systemd service

---

## 模块边界

核心模块：

```text
Client
Daemon
IPC
Session Manager
PTY Engine
Ring Buffer
Logger
Metadata Store
Config
Doctor
Installer
```

模块之间只能通过公开接口通信。

禁止跨模块访问内部状态。

---

## Phase 1 架构目标

Phase 1 的架构只需要支持：

- 单机
- 单用户 daemon
- 多 Session
- 多电脑可写 attach
- 单 active writer / takeover 策略
- 基础 ring buffer
- 基础日志
- 基础 metadata
- 基础 SSH 接管
- 基础 CLI

不需要支持：

- 多 pane
- 多 window
- Web UI
- REST API
- Cluster
- Plugin
- 多 active writer 协作模式
- 系统级多用户 daemon

---

## 架构红线

禁止：

- 每 Session 一个线程作为长期设计
- 无限制日志
- 无限制内存 buffer
- 忙轮询
- sleep polling
- 全局大锁
- Daemon 阻塞等待磁盘写
- Client 断开导致 Shell 退出
- 非交互 SSH 被接管
- scp/sftp/rsync 被破坏

---

## 架构演进原则

如果未来需要修改架构，必须：

1. 新增 ADR。
2. 更新 ARCHITECTURE.md。
3. 更新相关模块文档。
4. 更新协议文档。
5. 更新测试计划。
6. 再修改代码。

文档先于代码。

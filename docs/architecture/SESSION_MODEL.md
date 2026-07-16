# PersistShell Session Model

本文档定义 PersistShell Session 的数据模型、状态机和管理规则。

---

## Session 定义

PersistShell Session 是一个由 Daemon 管理的持久交互式 Shell 环境。

每个 Session 至少包含：

- 一个 PTY
- 一个 Shell 进程
- 一个 Session ID
- 一份 Metadata
- 一个 Ring Buffer
- 可选输出日志
- 零个或多个 attached clients

---

## Session 不是 tmux Session

PersistShell Session 不包含：

- Pane
- Window
- Layout
- Status Bar
- Prefix Key

一个 Session 就是一个持久 Shell。

---

## Session ID

Session ID 必须唯一。

要求：

- 同一用户下唯一。
- 稳定。
- 可用于 attach/kill/rename。
- 不应过长。
- 不应泄露敏感信息。

建议：

```text
短 ID + 内部 UUID
```

例如：

```text
显示 ID: 7
内部 ID: 01JXYZ...
```

---

## Session Name

Session Name 用于人类识别。

默认可以基于：

- 创建时间
- 用户名
- 来源
- 当前工作目录
- 前台进程

示例：

```text
ssh-20260708-153011
root-var-log
make-build
fio-test
```

Phase 1 可以使用简单名称。

Phase 2 再实现自动命名。

---

## Session Metadata

每个 Session 至少记录：

```text
session_id
display_id
name
owner_uid
owner_username
hostname
status
created_at
last_active_at
last_attached_at
last_detached_at
shell_pid
shell_path
foreground_pid
foreground_cmd
cwd
rows
cols
term
source_ip
source_tty
client_count
log_path
ring_buffer_size
exit_code
exit_time
tags
notes
pinned
schema_version
```

Phase 1 不需要全部实现，但 schema 应考虑演进。

---

## Session 状态

### Creating

Session 正在创建。

此状态短暂存在。

---

### Running

Session 中 Shell 仍然存活。

可能有 client attached，也可能没有。

如果没有 client attached，更准确状态可显示为 Detached。

---

### Attached

至少有一个 client 正在连接。

Phase 1 可将 Attached 作为 Running 的附加属性，而不是独立状态。

---

### Detached

Shell 存活，但没有 client attached。

SSH 断开后 Session 应进入 Detached。

---

### Closed

用户在 shell 中执行 `exit` 或在 shell 空行按 `Ctrl+D` 后，Shell 进程和 PTY 已释放。

Closed Session 不在后台继续运行 Shell，也不继续占用 PTY、前台进程和长期运行资源。

Closed Session 仍然保留：

- Session ID
- metadata
- 最后 cwd
- 环境变量快照
- ring buffer
- 持久日志
- exit code

用户之后可以 attach 回这个 Session。此时 Daemon 应重新创建 PTY 和用户 shell，并用保存的 cwd、环境变量快照和输出历史恢复上下文。

注意：Closed 只能恢复 Shell 上下文，不能复活已经随 `exit` 结束的前台进程或普通子进程。

---

### Killed

用户显式 kill 了 Session。

---

### Zombie

Daemon 发现 Session metadata 存在，但对应进程或 PTY 状态异常。

例如：

- shell_pid 不存在
- PTY 不存在
- metadata 与实际状态不一致

---

### Recovering

Daemon 启动后正在检查 Session 状态。

---

### Archived

Session 已归档，不再常规显示，但数据未删除。

---

## 状态流转

正常流转：

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

异常流转：

```text
Running → Killed
Detached → Killed
Running → Zombie
Detached → Zombie
Closed → Running
Closed → Deleted
Killed → Deleted
```

---

## Attach 规则

默认规则：

```text
多客户端可进入同一 Session，但同一时刻只有一个 active writer。
```

也就是：

- 一台电脑正在操作某个 Session 时，另一台电脑允许 attach 到该 Session 并获取可写操作权。
- 为避免两个终端同时向同一个 PTY 写入导致输入交错，默认使用 writer lease。
- 新 client attach 时可以请求 takeover，Daemon 将旧 writer 降级、detach 或提示其失去写权限。
- read-only attach 是可选模式，不是跨电脑进入会话的唯一方式。
- 用户必须能从另一台电脑进入旧会话继续操作，而不是只能查看。

---

## Detach 规则

以下情况应触发 detach：

- SSH 断开
- Client 进程退出
- Client socket 断开
- 用户执行 detach
- 网络错误
- Terminal 关闭

Detach 不应杀死 Shell。

---

## Exit 规则

用户在 Session shell 内执行：

```bash
exit
```

含义是：

```text
关闭当前 Session 的活动 Shell runtime
```

不是 detach，也不是让 Shell 在后台继续运行。

PersistShell 必须：

1. 记录 exit code。
2. 保存最后 cwd。
3. 保存允许持久化的环境变量快照。
4. 保留 ring buffer 和日志。
5. 关闭 PTY。
6. 释放 shell 进程和前台进程资源。
7. 将状态标记为 Closed。

之后用户执行：

```bash
persist attach <id>
```

Daemon 应将 Closed Session 作为可恢复 Session 处理，重新创建 shell，并恢复上次保存的 cwd、环境变量和输出上下文。

这与 SSH 断开不同。

---

## Kill 规则

`persist kill <id>` 应显式终止 Session。

推荐流程：

1. 向 Session process group 发送 SIGHUP 或 SIGTERM。
2. 等待 grace period。
3. 如果仍然存活，发送 SIGKILL。
4. 关闭 PTY。
5. 更新状态为 Killed。

---

## CWD 跟踪

Session 当前工作目录很重要。

Phase 1 可以通过 shell pid 的 procfs 尝试读取：

```text
/proc/<shell_pid>/cwd
```

前台进程运行时，也可以读取前台进程 cwd。

读取失败时显示 unknown。

---

## Foreground Process 跟踪

用于让用户知道 Session 在运行什么。

可通过：

- tcgetpgrp()
- /proc/<pid>/stat
- /proc/<pid>/cmdline
- process group

M37 使用 `tcgetpgrp()` 获取前台进程组 leader，并读取其 `comm` 与 `cmdline`。
读取失败时列表字段为空，不影响 Session 列表。

---

## Client Source

记录来源有助于识别 Session。

可记录：

- SSH_CONNECTION
- SSH_CLIENT
- TTY
- TERM
- hostname
- login time

注意不要记录过多敏感信息。

---

## Environment

Session 创建时继承一组环境变量。

注意：

不应每次 attach 都完全覆盖旧环境。

可动态同步的环境变量：

- TERM
- COLORTERM
- LANG
- LC_*
- SSH_AUTH_SOCK（后续阶段）

不应随意同步：

- PATH
- HOME
- USER
- SHELL
- SECRET
- TOKEN
- PASSWORD

### Closed Session 环境快照

Session 进入 Closed 状态时，PersistShell 应保存环境变量快照。

恢复 Closed Session 时，应使用该快照启动新的 shell，使用户看到的基础环境与上次退出前一致。

如果某些变量只对当前 SSH 连接有效，例如 `SSH_AUTH_SOCK`、`DISPLAY`、`WAYLAND_DISPLAY`，恢复时必须按安全 allowlist 和当前 attach 请求重新计算，不能盲目使用旧值。

---

## History

每个 Session 使用独立 Shell history，并额外维护供 `persist ls` 查询的实时命令记录：

```text
~/.local/share/persistshell/history/<session-id>.history
~/.local/share/persistshell/history/<session-id>.commands
```

`.history` 由 Shell 原生 history 机制管理；`.commands` 只镜像已被原生 history 接受的命令，
不解析 PTY 输入。记录包含顺序、完成时间、Shell 类型和完整多行命令，默认最多 10,000 条或
4 MiB，目录权限为 `0700`，文件权限为 `0600`。

`persist ls` 的历史视图按序号倒序分页，最新命令优先。Running 和 Closed Session 都可以查看。
临时 Shell hook 必须先加载用户原配置，再组合已有 hook；不得编辑用户 dotfile。hook 失败时
Shell 继续正常运行，只将实时命令历史视为不可用。详细决策见 ADR-0003。

---

## Logs

每个 Session 可以有独立日志。

日志命名建议：

```text
~/.local/share/persistshell/logs/<session-id>.log
```

要求权限：

```text
0600
```

---

## GC 策略

Session GC 不应误删用户仍需要的数据。

建议：

```text
Closed: 30 天
Killed: 7 天
Detached Running: 不自动清理，除非超过配置上限
Pinned: 不自动清理
Logs: 按大小和时间轮转
```

Phase 1 先实现保守策略。

---

## Session 列表展示

`persist ls` 应尽量展示关键信息：

```text
ID   NAME              STATUS    AGE     LAST     CWD              CMD
7    ssh-153011        running   2h      1m       /root            bash
8    make-build        detached  5h      3h       /usr/src/linux   make -j64
9    fio-test          closed    1d      1d       /data            exit=0
```

---

## Session 不变量

必须始终满足：

1. Session 属于唯一 owner UID。
2. 非 owner 不能 attach。
3. Session ID 不可复用到造成混淆。
4. Running Session 必须有有效 PTY。
5. Closed Session 必须可 attach 恢复，但恢复的是 Shell 上下文，不是已经退出的进程。
6. Detached Session 的 Shell 仍应存活。
7. Kill 必须更新 metadata。
8. GC 必须遵守 pinned 和 retention。

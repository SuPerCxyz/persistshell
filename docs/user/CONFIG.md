# PersistShell Config

本文档描述 PersistShell 配置。

---

## 配置文件路径

用户配置：

```text
~/.config/persistshell/config.toml
```

系统配置：

```text
/etc/persistshell/config.toml
```

M02 已支持用户配置和系统配置。配置文件不存在时会被忽略；配置文件存在但无法读取、格式错误或校验失败时，命令必须返回明确错误。

---

## 配置优先级

当前实现的字段优先级：

```text
用户配置 > 系统配置 > 默认值
```

`HOME`、`XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_STATE_HOME` 和 `XDG_RUNTIME_DIR`
只用于计算目录位置，不覆盖 TOML 字段。当前没有通用的配置字段环境变量或命令行覆盖。

---

## 当前实现状态

- 内置安全默认配置。
- 加载 `/etc/persistshell/config.toml`。
- 加载 `~/.config/persistshell/config.toml`。
- 用户配置覆盖系统配置。
- TOML 格式错误会返回路径和解析错误。
- 配置校验错误会返回明确字段名。
- `persist config` 和 `persist config show` 显示当前有效配置。
- client/daemon 内部日志路径解析、级别过滤、权限限制和基础文件写入。

---

## 示例配置

```toml
[daemon]
auto_start = true
idle_exit = true
idle_exit_after = "10m"
gc_idle_timeout = "0s"
gc_interval = "60s"

[runtime]
socket_dir = "/run/user/%UID%/persistshell"

[session]
new_session_on_ssh = true
default_shell = ""
kill_grace = "3s"

[ring_buffer]
default_size = "8MB"
max_size = "128MB"
replay_on_attach = true
replay_bytes = "512KB"

[logging]
session_log = true
max_file_size = "100MB"
max_files = 10
retention_days = 30
flush_interval = "1s"

[internal_log]
level = "info"
daemon_log = "/home/alice/.local/state/persistshell/daemon.log"
client_log = "/home/alice/.local/state/persistshell/client.log"
max_file_size = "20MB"
max_files = 5

[security]
allow_root_attach_others = false
enable_input_recording = false

[ssh]
auto_hook = true
bypass_env = "PERSIST_DISABLE"

[recovery.environment]
include = []
max_variables = 128
max_bytes = "64KiB"
```

---

## 值格式

大小字段支持整数或字符串：

```toml
default_size = "8MB"
replay_bytes = "512KB"
max_size = 134217728
```

支持单位：

```text
B, KB, MB, GB, TB
```

时长字段支持整数秒或字符串：

```toml
idle_exit_after = "10m"
kill_grace = "3s"
flush_interval = 1
```

支持单位：

```text
ms, s, m, h
```

---

## daemon

### auto_start

保留的 daemon 自动启动配置字段。当前普通 CLI 命令不会依据此字段自动启动 daemon；
SSH hook 会显式执行 `persist daemon start`。

默认：

```toml
auto_start = true
```

---

### idle_exit

保留的 daemon 空闲退出配置字段。当前 daemon 不会依据此字段自动退出，但仍会校验
`idle_exit = true` 时的 `idle_exit_after` 值。

默认：

```toml
idle_exit = true
```

### gc_idle_timeout / gc_interval

控制 Idle GC。`gc_idle_timeout = "0s"`（默认）表示禁用；设为正时，daemon 每隔
`gc_interval` 检查 idle、未 attach、未 pinned、未 locked 的 running Session，并移除满足
超时条件的 Session runtime。`persistd foreground --idle-timeout <duration>` 可临时覆盖
`gc_idle_timeout`。当前 `persist config show` 尚不打印这两个字段，但 daemon 会读取它们。

```toml
gc_idle_timeout = "2h"
gc_interval = "5m"
```

---

## runtime

### socket_dir

Unix Socket 目录。

默认由运行目录推导：

```text
/run/user/$UID/persistshell
```

配置值必须是绝对路径。配置中可以使用 `%UID%` 占位符：

```toml
socket_dir = "/run/user/%UID%/persistshell"
```

修改 `runtime.socket_dir` 后，`persist.sock` 路径同步变为：

```text
<socket_dir>/persist.sock
```

---

## recovery.environment

M55 使用该配置限制 Closed Session 动态环境恢复。正常退出时，隐藏 helper 采集允许的已导出
变量和精确 unset；Holder 在 daemon 离线期间保留快照，metadata 成功后才回收退出上下文。
再次 attach 会按当前策略过滤快照并启动新的 Shell runtime。

默认只允许 `LANG` 和 `LC_*`。`include` 可增加精确变量名或仅尾部 `*` 的前缀：

```toml
[recovery.environment]
include = ["EDITOR", "MY_PROJECT_*"]
max_variables = 32
max_bytes = "16KiB"
```

`max_variables` 必须在 1 到 128 之间，`max_bytes` 必须在 1 byte 到 64 KiB 之间；用户只能
收紧硬上限。身份、基础路径、当前连接、`XDG_*`、`PERSIST_*` 和名称匹配敏感标记的规则
始终拒绝，include 不能覆盖。用户显式 include 自定义变量即授权持久化其值，不应 include
可能承载凭据的变量。

当前连接的 `TERM`、SSH、display 和有效 agent socket 只用于本次 attach，不写入环境快照。
未导出 Shell 变量无法恢复。旧 Holder 不支持环境 capability 时保留 cwd-only 降级。

---

## session

### new_session_on_ssh

交互式 SSH 登录是否自动创建新 Session。

默认：

```toml
new_session_on_ssh = true
```

该字段当前由配置解析并显示；SSH hook 的实际行为以安装的 hook 为准。

---

### default_shell

默认 shell。

空字符串表示读取用户 login shell。

```toml
default_shell = ""
```

---

### kill_grace

kill Session 时发送 SIGTERM 后等待多久再 SIGKILL。

```toml
kill_grace = "3s"
```

---

## ring_buffer

### default_size

每个 Session 默认 ring buffer 大小。

```toml
default_size = "8MB"
```

---

### max_size

允许的最大 ring buffer。

```toml
max_size = "128MB"
```

---

### replay_on_attach

attach 时是否回放最近输出。

```toml
replay_on_attach = true
```

---

### replay_bytes

默认回放字节数。

```toml
replay_bytes = "512KB"
```

---

## logging

### session_log

是否记录 Session 输出日志。

```toml
session_log = true
```

---

### max_file_size

单个日志文件最大大小。

```toml
max_file_size = "100MB"
```

---

### max_files

每个 Session 保留轮转日志数量。

```toml
max_files = 10
```

---

### retention_days

日志保留天数配置。当前字段会被解析和校验，但没有后台按天删除日志的任务；不要把它当成
自动清理保证。

```toml
retention_days = 30
```

### flush_interval

日志 flush 间隔配置。当前字段会被解析和校验；Session log writer 的实际写入和轮转以
`session_log`、`max_file_size` 与 `max_files` 为准。

```toml
flush_interval = "1s"
```

---

## internal_log

`internal_log` 描述 PersistShell 自身运行日志，不是 Session 输出日志。

默认路径：

```text
~/.local/state/persistshell/daemon.log
~/.local/state/persistshell/client.log
```

### level

内部日志级别。

默认：

```toml
level = "info"
```

可选值：

```text
error, warn, info, debug, trace
```

### daemon_log

daemon 内部日志文件路径。

必须是绝对路径。

### client_log

client 内部日志文件路径。

必须是绝对路径。

### max_file_size

内部日志单个文件最大大小。当前会解析和校验；内部日志尚无完整轮转实现。

### max_files

内部日志保留轮转文件数量。当前会解析和校验；内部日志尚无完整轮转实现。

---

## security

### allow_root_attach_others

root 是否允许 attach 其他用户 Session。当前 per-user daemon 不支持该模型，配置校验会拒绝
将此值设置为 `true`。

```toml
allow_root_attach_others = false
```

---

### enable_input_recording

是否记录用户输入。当前没有输入记录实现，默认 `false`；设置为 `true` 不应被视为启用
输入审计。

```toml
enable_input_recording = false
```

---

## ssh

### auto_hook

SSH 自动接管配置字段。当前安装器写入的 hook 不读取该字段；是否接管由 hook 是否安装及
`PERSIST_DISABLE` 是否存在决定。

```toml
auto_hook = true
```

---

### bypass_env

绕过环境变量配置字段。当前安装器生成的 hook 固定识别 `PERSIST_DISABLE`；修改此字段不会
改变已安装 hook 的绕过变量。

```toml
bypass_env = "PERSIST_DISABLE"
```

---

## 配置校验

PersistShell 启动时必须校验配置。

错误配置应给出明确提示。

例如：

```text
配置错误：ring_buffer.default_size 超过 ring_buffer.max_size。
```

当前至少校验：

- `runtime.socket_dir` 不能为空，且必须是绝对路径。
- `daemon.idle_exit_after` 在 `daemon.idle_exit = true` 时必须大于 0。
- `session.kill_grace` 必须大于 0。
- `ring_buffer.default_size` 必须大于 0。
- `ring_buffer.max_size` 必须大于 0。
- `ring_buffer.default_size` 不能超过 `ring_buffer.max_size`。
- `ring_buffer.replay_bytes` 不能超过 `ring_buffer.max_size`。
- `logging.max_file_size` 必须大于 0。
- `logging.max_files` 必须大于 0。
- `logging.retention_days` 必须大于 0。
- `logging.flush_interval` 必须大于 0。
- `internal_log.daemon_log` 必须是绝对路径。
- `internal_log.client_log` 必须是绝对路径。
- `internal_log.max_file_size` 必须大于 0。
- `internal_log.max_files` 必须大于 0。
- `ssh.bypass_env` 不能为空。
- `security.allow_root_attach_others` 必须为 `false`。

---

## 查看配置

```bash
persist config show
```

也可以使用：

```bash
persist config
```

---

## 不变量

1. 默认配置必须安全。
2. 配置错误不能静默忽略。
3. 配置变更必须更新本文档。
4. 敏感配置不得输出到日志。

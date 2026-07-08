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

完整目标优先级：

```text
命令行参数 > 环境变量 > 用户配置 > 系统配置 > 默认值
```

M02 已实现：

```text
用户配置 > 系统配置 > 默认值
```

命令行参数和环境变量覆盖保留给后续具体命令实现。

---

## 当前实现状态

M02 实现以下能力：

- 内置安全默认配置。
- 加载 `/etc/persistshell/config.toml`。
- 加载 `~/.config/persistshell/config.toml`。
- 用户配置覆盖系统配置。
- TOML 格式错误会返回路径和解析错误。
- 配置校验错误会返回明确字段名。
- `persist config` 和 `persist config show` 显示当前有效配置。

---

## 示例配置

```toml
[daemon]
auto_start = true
idle_exit = true
idle_exit_after = "10m"

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

[security]
allow_root_attach_others = false
enable_input_recording = false

[ssh]
auto_hook = true
bypass_env = "PERSIST_DISABLE"
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

是否允许 client 自动启动 daemon。

默认：

```toml
auto_start = true
```

---

### idle_exit

daemon 在没有 running session 时是否空闲退出。

默认：

```toml
idle_exit = true
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

## session

### new_session_on_ssh

交互式 SSH 登录是否自动创建新 Session。

默认：

```toml
new_session_on_ssh = true
```

Phase 1 默认必须为 true。

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

日志保留天数。

```toml
retention_days = 30
```

---

## security

### allow_root_attach_others

root 是否允许 attach 其他用户 Session。

Phase 1 不支持，M02 校验会拒绝将该值设置为 `true`。

```toml
allow_root_attach_others = false
```

---

### enable_input_recording

是否记录用户输入。

默认必须为 false。

```toml
enable_input_recording = false
```

---

## ssh

### auto_hook

是否启用 SSH 自动接管。

```toml
auto_hook = true
```

---

### bypass_env

绕过环境变量。

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

M02 至少校验：

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
- `ssh.bypass_env` 不能为空。
- `security.allow_root_attach_others` 在 Phase 1 必须为 `false`。

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

# M44 安全审查记录

## 已修复

| 范围 | 风险 | 修复与证据 |
|---|---|---|
| `persist daemon start` runtime log | `File::create` 依赖 umask，已有文件可保留宽松权限 | 以 `OpenOptions` 的 `0600` 模式创建并显式重设权限；`create_daemon_log_enforces_private_mode` 覆盖新建/已有文件。 |
| metadata 路径 | data 目录和 SQLite DB 依赖 umask | `MetadataStore::open` 强制父目录 `0700`、DB `0600`；`open_enforces_private_database_permissions` 覆盖。 |
| stale socket 清理 | 污染 runtime 目录时可能删除普通文件或 symlink | 仅删除 `symlink_metadata` 识别为 Unix socket 的路径；IPC 回归测试覆盖普通文件拒绝。 |
| session 输出日志 | 初次创建/轮转创建依赖 umask，父目录未收紧 | session 日志目录强制 `0700`、文件强制 `0600`；`log_open_enforces_private_parent_and_file_modes` 覆盖。 |

## 已检查的边界

| 范围 | 结论 | 依据 |
|---|---|---|
| Unix socket | 可接受 | socket 目录 `0700`、socket `0600`，并使用 `SO_PEERCRED` 限制为同 UID；stale 清理拒绝非 socket。 |
| SSH agent | 可接受 | 仅继承绝对路径的 Unix socket，普通文件、相对路径和不可读路径均不会传给 child。 |
| metadata SQL | 可接受 | 创建、查询、更新和标签操作使用 `rusqlite::params!` 绑定参数，未发现用户输入拼接 SQL。 |
| shell 启动 | 可接受 | PTY child 使用 `execvp` 的单一 shell 路径参数，不通过 `sh -c` 拼接用户输入。 |

## 边界与验证

runtime、data 与配置路径由当前用户控制；同 UID 用户可主动将其替换为 symlink 不属于
per-user daemon 的跨用户攻击面。实现不接受其他 UID 的 socket peer。

本地 `cargo test --workspace --all-features`、clippy 和 release 构建均通过。`test`
主机原生构建后，隔离 XDG 端到端验证确认 runtime/data/sessions 目录为 `0700`，socket、
metadata DB 和 session log 为 `0600`。

# M14 Closed Session 恢复设计

## 目标

让已关闭 Session 保留可恢复的工作上下文。shell 因 `exit` 或 `Ctrl+D`
退出后，daemon 记录最后 cwd、会话启动时继承的允许持久化环境快照和 exit code；
用户再次 attach 该 Session 时，daemon 使用这些状态创建新的 PTY shell。

## 持久化模型

sessions 表新增 `env_snapshot` 文本列，保存受限环境变量的 JSON 对象。schema
版本升级为 6。`cwd` 保持现有列，在关闭时以 `/proc/<shell-pid>/cwd` 更新；无法读取
时保留先前值或 NULL。环境快照来自 Shell 启动环境；Linux `/proc/<pid>/environ`
无法可靠观察 shell 运行中后续 `export` 的变更。

只保存以下键：`TERM`、`COLORTERM`、`LANG` 与 `LC_*`。不保存 `PATH`、`HOME`、
`USER`、`SHELL`、`SSH_AUTH_SOCK`、`DISPLAY`、`WAYLAND_DISPLAY`，也不保存名称含
`TOKEN`、`SECRET`、`PASSWORD` 或 `KEY` 的变量。

## 恢复流程

1. `ATTACH` 发现 runtime 中不存在、metadata 状态为 `closed` 的 Session。
2. daemon 从 metadata 读取 shell、cwd、环境快照和 history 路径。
3. PTY child 在 exec 前尝试 `chdir(cwd)` 并写入允许变量；cwd 不存在或无权限时
   保持当前 cwd，仍继续启动。
4. 新 PTY 使用相同 session ID 加回 SessionManager，将 metadata 状态改为 running，
   然后走既有 attach/writer 交接流程。

## 失败处理与验证

- JSON 解析失败视为无环境快照，不拒绝恢复。
- metadata 不存在或 shell 启动失败时返回 attach 错误，不创建半成品 runtime。
- 测试覆盖 schema migration、环境过滤、cwd 回退、closed attach 恢复与敏感变量不
  被写入数据库。

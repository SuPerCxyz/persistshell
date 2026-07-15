# M40 SSH Agent Socket 继承设计

## 目标

让新建或冷恢复的 Session 继承当前 daemon 启动环境中的 `SSH_AUTH_SOCK`，使前台 shell
可继续使用本机 SSH agent。该能力不代理、转发或复制 agent，只传递本机 Unix socket 路径。

## 边界

- 仅允许绝对路径且存在 Unix socket 的 `SSH_AUTH_SOCK`。
- 不写入 SQLite、日志或输出记录。
- closed Session 恢复时使用 daemon 当前可用的 agent socket，而不是持久化旧路径。
- 不实现跨主机同步；远端 SSH agent 转发仍由 SSH 自身负责。

## 实现

PTY child 创建前检查 daemon 的 `SSH_AUTH_SOCK`。路径通过 `symlink_metadata` 验证为
socket 后传给 child `setenv`；缺失、不是 socket 或不可访问时移除该变量并继续启动。

## 验证计划

后续统一验证有效 socket、无效路径、closed recovery 与不持久化 metadata 的路径。

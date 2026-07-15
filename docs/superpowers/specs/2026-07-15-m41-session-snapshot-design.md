# M41 Session Snapshot 设计

## 目标

提供 `persist snapshot <session-id>`，输出可供排查与恢复前检查的 Session 状态快照。
快照是即时只读 JSON，不创建 checkpoint，不复制 PTY 内存，也不承诺恢复前台进程。

## 内容

包含 session ID、名称、状态、创建/更新时间、shell、cwd、前台进程摘要、writer 状态、
锁定/pin/note/tag 标记、exit code、关闭时间与输出日志路径。note 和 tag 仅提供是否存在
的标记，不返回实际内容。敏感环境变量、输入内容和 SSH agent 路径不包含在快照内。

## IPC 与非目标

新增 `SESSION_SNAPSHOT` / `SESSION_SNAPSHOT_RESP`，采用最多 16 KiB 的 JSON 文本。closed
Session 返回 metadata 可用字段和空 runtime 字段。未知 Session 或超过长度上限时返回带
`error` 字段的 JSON，CLI 映射为非零退出。

不保存磁盘快照、不重放内存、不中断 Session、不替代 M14 cold recovery。

## 验证计划

后续统一验证运行中/closed/locked Session、JSON 长度限制与敏感字段排除。

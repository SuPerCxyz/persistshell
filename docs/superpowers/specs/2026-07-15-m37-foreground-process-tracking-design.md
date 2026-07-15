# M37 前台进程跟踪设计

## 目标

让 `persist ls` 展示每个运行中 Session 当前控制终端的前台进程：PID、进程名和
截断后的命令行摘要。该信息仅用于观察，不写入 metadata，不引入进程树或资源监控。

## 数据来源与边界

daemon 对 Session PTY master 调用 `tcgetpgrp()` 获取前台进程组 ID。若返回正值，
使用该值作为前台进程 PID，读取：

- `/proc/<pid>/comm`：进程名；
- `/proc/<pid>/cmdline`：以空格连接的命令行摘要，最多 160 字节。

Linux 的前台进程组可能有多个成员。M37 以 group leader PID 为观察对象，不枚举
成员；完整 Process Tree 留给 M38。

## IPC 与 CLI

在 `SessionEntry` 增加可选字段：

```text
foreground_pid: Option<u32>
foreground_name: String
foreground_cmd: String
```

它们由 `LIST_SESSIONS` 与 `LIST_SESSIONS_BY_TAG` 共用的编码扩展传输。CLI 的
`persist ls` 增加 `FOREGROUND` 列，优先显示命令行摘要，只有进程名时显示名称，
无法获得时显示 `-`。

## 降级与错误处理

- closed Session、PTY 已失效、没有前台进程组：三个字段为空。
- `tcgetpgrp` 或 `/proc` 读取失败：字段为空，不记录错误，不影响整个列表响应。
- `/proc/<pid>/cmdline` 为空：保留 `comm`；`comm` 也不可读时显示 `-`。
- 命令行包含 NUL 时以空格替换；超过 160 字节按 UTF-8 边界截断并附加 `...`。

## 测试

- PTY 层测试：可取得 `/bin/sh` 的前台进程信息，缺失 `/proc` 条目返回空。
- IPC 测试：扩展字段编码/解码往返一致，旧字段不受影响。
- daemon 测试：运行 Session 的列表包含 PID/名称；closed Session 字段为空。
- CLI 测试：`persist ls` 对命令行、仅名称和空值的格式化稳定。

## 非目标

- 不持久化或历史化前台进程信息。
- 不扫描 process group 成员。
- 不增加新的 IPC 命令。
- 不实现 CPU、内存、I/O 或进程树。

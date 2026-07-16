# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 4：发布和长期维护

---

## 当前里程碑

M52：Performance dashboard

---

## 当前唯一任务

M52 阶段 5：使用 TDD 实现 Dashboard worker、writer 与 daemon 生命周期，不接入 Dashboard
IPC 或 TUI。

### 前置已完成

- M51 的完整用户手册、交互式 Session 选择和实时命令历史已完成。
- Ubuntu 26.04 tar/deb 与 Rocky Linux 9.7 RPM 已验证携带完整用户手册。
- Rocky test 主机已验证列表选择、菜单 attach、退出后返回和最新优先历史。
- M52 中文设计规范已确认，`ADR-0004` 已接受。
- 实施计划已拆分为 IPC、内存模型、procfs、存储、worker、daemon、TUI 和验证阶段。
- 阶段 1 Dashboard IPC 已完成，新增受限 summary/trend 编解码和协议文档。
- 阶段 2 有界内存模型已完成，包含速率、聚合、64 MiB/1 小时/720 帧硬上限。
- 阶段 3 单次 procfs 聚合已完成，包含多 Session 归属、失败状态和受限真实 source。
- 阶段 4 版本化小时分段存储已完成，包含 CRC、尾部修复、安全权限和 24 小时/128 MiB
  轮转。

---

## 任务范围

- 实现容量为 1 的采样触发通道和容量为 2 的磁盘批次通道，过载时合并或丢弃旧工作。
- daemon 主循环每 5 秒复制 Session 根 PID 与聚合计数后使用非阻塞发送。
- worker 在不持有 `SessionManager` 锁时扫描 procfs、更新有界内存历史并生成分钟批次。
- 使用短写锁替换已构建的共享只读快照，查询方不得触发采样或磁盘 I/O。
- 实现启动恢复、shutdown 通知、writer 刷新和有界退出；Dashboard 故障不得终止 daemon。

---

## 完成标准

1. 测试覆盖采样触发合并、不重入、磁盘队列过载、worker 退出和正常 shutdown。
2. accept、PTY 和 GC 主路径不等待 procfs 扫描或指标磁盘 I/O。
3. worker 不持有 `SessionManager` 锁扫描 `/proc`，共享快照替换保持短临界区。
4. 存储损坏、不可写或 worker 故障仅更新 Dashboard 状态，不影响既有 daemon 服务。
5. `cargo test -p persistd`、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得实现 Dashboard IPC 或 TUI，不得修改 metadata schema 或 `persist metrics` 语义，不得让 daemon
主循环执行 procfs 扫描或指标磁盘 I/O，不得新增依赖。远端 push 仍须维护者授权。

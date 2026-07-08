# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 1：MVP Core

---

## 当前里程碑

M03：基础日志框架

---

## 当前唯一任务

完善 PersistShell 基础内部日志框架。

本任务应在现有 Rust workspace 上继续推进，不实现 Session 输出日志、PTY、daemon runtime、IPC streaming 或 SSH 自动接管。

---

## 任务范围

需要实现：

- 内部日志配置结构与默认值。
- 内部日志文件路径解析。
- 日志目录和日志文件权限策略。
- 基础日志写入接口。
- 日志级别过滤。
- 日志初始化错误处理。
- 不记录敏感内容的最小测试。
- 日志相关文档同步。

---

## 完成标准

本任务完成时必须满足：

1. `cargo fmt --all -- --check` 通过。
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过。
3. `cargo test --workspace --all-features` 通过。
4. 内部日志初始化和写入有单元测试。
5. 日志路径和权限行为有测试。
6. `TODO.md`、`MILESTONES.md`、`CHANGELOG.md` 已更新。
7. `NEXT_TASK.md` 更新为下一个唯一任务。

---

## 禁止事项

本任务期间禁止：

- 实现 Session 输出日志。
- 实现日志轮转的完整生产逻辑。
- 实现 PTY Engine。
- 实现真实 daemon runtime。
- 实现 Unix Socket streaming。
- 安装 shell profile hook。
- 修改 Session Protocol 语义。
- 扩展 Phase 2/Phase 3 功能。

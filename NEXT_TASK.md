# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 1：MVP Core

---

## 当前里程碑

M04：错误处理框架

---

## 当前唯一任务

完善 PersistShell 统一错误处理框架。

本任务应在现有 Rust workspace 上继续推进，不实现 PTY、daemon runtime、IPC streaming、Session 输出日志或 SSH 自动接管。

---

## 任务范围

需要实现：

- 稳定错误码定义。
- 用户可读错误输出格式。
- 错误分类：用户错误、环境错误、系统调用错误、协议错误、内部错误。
- 错误到退出码的映射。
- `persist` 和 `persistd` 的错误输出统一。
- 配置和日志错误接入统一错误码。
- 错误相关测试。
- 错误处理文档同步。

---

## 完成标准

本任务完成时必须满足：

1. `cargo fmt --all -- --check` 通过。
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过。
3. `cargo test --workspace --all-features` 通过。
4. 错误码和错误分类有单元测试。
5. CLI 错误输出有集成测试。
6. `TODO.md`、`MILESTONES.md`、`CHANGELOG.md` 已更新。
7. `NEXT_TASK.md` 更新为下一个唯一任务。

---

## 禁止事项

本任务期间禁止：

- 实现 PTY Engine。
- 实现真实 daemon runtime。
- 实现 Unix Socket streaming。
- 实现 Session 输出日志。
- 安装 shell profile hook。
- 修改 Session Protocol 语义。
- 扩展 Phase 2/Phase 3 功能。

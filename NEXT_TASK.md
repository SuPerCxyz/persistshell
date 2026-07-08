# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 1：MVP Core

---

## 当前里程碑

M02：基础配置系统

---

## 当前唯一任务

实现 PersistShell 基础配置系统。

本任务应在现有 Rust workspace 上继续推进，不实现 PTY、daemon runtime、IPC streaming 或 SSH 自动接管。

---

## 任务范围

需要实现：

- 默认配置结构。
- 用户配置路径解析。
- 系统配置路径解析。
- 配置文件加载框架。
- 配置校验。
- `persist config` 或等价的配置查看命令，若 CLI 设计需要可先新增子命令骨架。
- 配置相关错误类型和测试。
- 文档中与配置行为相关的说明同步。

---

## 完成标准

本任务完成时必须满足：

1. `cargo fmt --all -- --check` 通过。
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过。
3. `cargo test --workspace --all-features` 通过。
4. 配置加载和校验有单元测试。
5. CLI 暴露配置查看或诊断入口。
6. `TODO.md`、`MILESTONES.md`、`CHANGELOG.md` 已更新。
7. `NEXT_TASK.md` 更新为下一个唯一任务。

---

## 禁止事项

本任务期间禁止：

- 实现 PTY Engine。
- 实现真实 daemon runtime。
- 实现 Unix Socket streaming。
- 安装 shell profile hook。
- 修改 Session Protocol 语义。
- 扩展 Phase 2/Phase 3 功能。


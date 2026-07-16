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

M52 阶段 1：使用 TDD 定义并实现 Dashboard IPC 数据结构与编解码，不接入 daemon 或 TUI。

### 前置已完成

- M51 的完整用户手册、交互式 Session 选择和实时命令历史已完成。
- Ubuntu 26.04 tar/deb 与 Rocky Linux 9.7 RPM 已验证携带完整用户手册。
- Rocky test 主机已验证列表选择、菜单 attach、退出后返回和最新优先历史。
- M52 中文设计规范已确认，`ADR-0004` 已接受。
- 实施计划已拆分为 IPC、内存模型、procfs、存储、worker、daemon、TUI 和验证阶段。

---

## 任务范围

- 新增独立 `dashboard` 协议模块和四个消息类型。
- 定义 summary 分页请求响应、trend scope/range 请求响应和完整性状态。
- 使用固定宽度、显式大小端的受限二进制编码。
- 测试非法枚举、非法游标、超限页大小、超过 240 点、截断 payload 和尾随数据。
- 更新客户端与 Socket 协议文档。

---

## 完成标准

1. 先提交失败测试，再完成最小协议实现。
2. 新消息 round-trip、错误路径和边界测试通过。
3. 最大合法响应小于 `MAX_CONTROL_FRAME`。
4. 既有 IPC 消息编号和 round-trip 测试不变。
5. `cargo test -p persist-ipc`、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得接入 daemon 采样、磁盘存储或 TUI，不得修改 `persist metrics` 语义，不得暴露 Session
内容、命令历史、环境变量或敏感路径。远端 push 仍须维护者授权。

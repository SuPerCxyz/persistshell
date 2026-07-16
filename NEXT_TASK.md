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

M52 阶段 4：使用 TDD 实现版本化小时分段存储，不接入 daemon 生命周期或 TUI。

### 前置已完成

- M51 的完整用户手册、交互式 Session 选择和实时命令历史已完成。
- Ubuntu 26.04 tar/deb 与 Rocky Linux 9.7 RPM 已验证携带完整用户手册。
- Rocky test 主机已验证列表选择、菜单 attach、退出后返回和最新优先历史。
- M52 中文设计规范已确认，`ADR-0004` 已接受。
- 实施计划已拆分为 IPC、内存模型、procfs、存储、worker、daemon、TUI 和验证阶段。
- 阶段 1 Dashboard IPC 已完成，新增受限 summary/trend 编解码和协议文档。
- 阶段 2 有界内存模型已完成，包含速率、聚合、64 MiB/1 小时/720 帧硬上限。
- 阶段 3 单次 procfs 聚合已完成，包含多 Session 归属、失败状态和受限真实 source。

---

## 任务范围

- 定义带 magic、版本、记录长度、时间、payload 和 CRC32 的二进制格式。
- 限制目录项数量、单文件大小、记录长度和所有解码分配。
- 安全创建 `0700` 目录和 `0600` 文件，拒绝 symlink、错误 owner 和过宽权限。
- 实现尾部不完整记录截断、损坏分段跳过、时间回退新分段和重启加载。
- 实现最多 24 个小时分段及 128 MiB 容量轮转，容量优先。

---

## 完成标准

1. 先提交格式与临时目录失败测试，再完成最小存储实现。
2. round-trip、CRC、截断、未知版本和超限记录测试通过。
3. 权限、owner、symlink、文件数量和容量边界测试通过。
4. 损坏或不可写指标存储不修改 metadata，也不导致无界分配。
5. `cargo test -p persistd`、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得接入 daemon 生命周期或 TUI，不得修改 metadata schema 或 `persist metrics` 语义，不得使用
JSON 时序存储或新增依赖。远端 push 仍须维护者授权。

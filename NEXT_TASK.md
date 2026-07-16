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

M52 阶段 2：使用 TDD 实现 Dashboard 有界内存模型，不接入 `/proc`、磁盘、daemon 或 TUI。

### 前置已完成

- M51 的完整用户手册、交互式 Session 选择和实时命令历史已完成。
- Ubuntu 26.04 tar/deb 与 Rocky Linux 9.7 RPM 已验证携带完整用户手册。
- Rocky test 主机已验证列表选择、菜单 attach、退出后返回和最新优先历史。
- M52 中文设计规范已确认，`ADR-0004` 已接受。
- 实施计划已拆分为 IPC、内存模型、procfs、存储、worker、daemon、TUI 和验证阶段。
- 阶段 1 Dashboard IPC 已完成，新增受限 summary/trend 编解码和协议文档。

---

## 任务范围

- 新增 daemon dashboard model/history 独立模块，不修改现有 `server.rs` 行为。
- 定义原始计数、展示速率、分钟聚合、完整性和数据年龄模型。
- 使用可注入时间测试首点、CPU/I/O 差值、计数器回退和系统时间回退。
- 实现按时间片组织的环形历史和精确内存容量记账。
- 达到 64 MiB 后统一淘汰最旧时间片，并实现最多 240 桶降采样。

---

## 完成标准

1. 先提交失败测试，再完成最小内存模型实现。
2. 首点、差值、回退、聚合、空区间和部分数据测试通过。
3. 点数、时间窗口和 64 MiB 容量均有硬上限测试。
4. 淘汰后最新点始终存在，所有 Session 保持相同起始时间片。
5. `cargo test -p persistd`、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得扫描 `/proc`，不得接入 daemon 生命周期、磁盘存储或 TUI，不得修改 `persist metrics`
语义，不得暴露 Session 内容或敏感数据。远端 push 仍须维护者授权。

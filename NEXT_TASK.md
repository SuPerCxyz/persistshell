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

M52 阶段 3：使用 TDD 实现单次 `/proc` 进程树聚合，不接入磁盘、daemon 生命周期或 TUI。

### 前置已完成

- M51 的完整用户手册、交互式 Session 选择和实时命令历史已完成。
- Ubuntu 26.04 tar/deb 与 Rocky Linux 9.7 RPM 已验证携带完整用户手册。
- Rocky test 主机已验证列表选择、菜单 attach、退出后返回和最新优先历史。
- M52 中文设计规范已确认，`ADR-0004` 已接受。
- 实施计划已拆分为 IPC、内存模型、procfs、存储、worker、daemon、TUI 和验证阶段。
- 阶段 1 Dashboard IPC 已完成，新增受限 summary/trend 编解码和协议文档。
- 阶段 2 有界内存模型已完成，包含速率、聚合、64 MiB/1 小时/720 帧硬上限。

---

## 任务范围

- 新增可使用 fixture 的只读 procfs source 和进程记录解析器。
- 单次枚举 PID、PPID、CPU ticks、RSS 和 I/O，不读取敏感文件。
- 以各 Session 根 Shell PID 聚合全部后代，防止重复归属。
- 处理进程消失、权限失败、损坏记录、缺失根 PID 和部分采集。
- 增加真实 Linux 子进程树定向测试，不依赖脆弱的精确资源值。

---

## 完成标准

1. 先提交 fixture 失败测试，再完成最小 procfs 实现。
2. 嵌套树、多个 Session、重复归属和损坏/消失进程测试通过。
3. unavailable 与 partial 状态准确，不使用伪造零值表示成功。
4. 扫描实现不访问 `cmdline`、`environ`、`cwd` 或 fd 目标。
5. `cargo test -p persistd`、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得接入 daemon 生命周期、磁盘存储或 TUI，不得修改 `persist metrics` 语义，不得读取命令、
输出、环境变量、路径或进程命令行。远端 push 仍须维护者授权。

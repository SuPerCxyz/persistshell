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

M52 阶段 7：使用 TDD 实现 `persist top` 命令入口和 Dashboard 数据客户端，不实现全屏 TUI。

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
- 阶段 5 worker 与 daemon 生命周期已完成，包含容量 1/2 队列、5 秒触发、2 秒截止、分钟
  writer、启动恢复、故障隔离和有界 shutdown。
- 阶段 6 daemon Dashboard IPC 已完成，包含稳定 summary 分页、15m/1h 内存趋势、24h 分段
  趋势、writer 串行查询、Unavailable 降级和真实 socket 集成测试。

---

## 任务范围

- 按已确认版本添加 `ratatui 0.29.0` 和 `crossterm 0.28.1`，仅加入 CLI crate。
- 解析 `persist top` 并更新 help/completion/man page；非 TTY 时返回稳定可诊断错误。
- 实现 Dashboard client，读取全部 summary 页并请求 daemon/Session 的 15m、1h、24h 趋势。
- 每次请求限制在 128 个 Session/页和 240 个趋势点，校验响应类型、request ID 和游标推进。
- 实现 5 秒刷新节拍及有上限的断线重连退避，不做 busy loop；本阶段用最小占位渲染验证数据流。

---

## 完成标准

1. CLI 测试覆盖 help、未知参数、非 TTY、daemon 不可用、分页和协议响应校验。
2. 数据客户端测试覆盖多页去重/推进、趋势点上限、断线、过期 request ID 和错误消息类型。
3. 重连退避和刷新等待有上限且不忙循环，不改变 `persist metrics` 或其它命令行为。
4. man page、shell completion 和用户手册先记录 `persist top` 命令入口及当前阶段限制。
5. `cargo test -p persist-cli`、工作区检查、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得实现阶段 8 的全屏布局、图表或终端 raw/alternate-screen 生命周期，不得修改 metadata schema
或 `persist metrics` 语义，不得扩大 IPC 上限。远端 push 仍须维护者授权。

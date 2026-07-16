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

M52 阶段 6：使用 TDD 实现 daemon Dashboard summary/trend IPC，不实现 CLI 或 TUI。

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

---

## 任务范围

- 在 daemon 中处理 `DashboardSummary` 和 `DashboardTrend` 请求及对应响应。
- summary 使用稳定 Session ID 顺序分页，限制每页最多 128 条并正确生成 `next_cursor`。
- 15 分钟和 1 小时趋势读取有界内存历史，24 小时趋势读取有效分钟分段。
- 趋势按 scope/range 聚合且最多返回 240 点，非法 Session、游标、范围和点数返回明确错误。
- 查询使用共享只读状态或独立存储读取，不触发采样，不持有 `SessionManager` 锁执行磁盘 I/O。

---

## 完成标准

1. IPC 测试覆盖 summary 空数据、单页、多页、稳定游标和超限请求。
2. 趋势测试覆盖 daemon/Session、15m/1h/24h、未知 Session、空历史和最多 240 点。
3. 响应不包含命令、输出、环境变量、cwd、note、tag 或其它敏感 Session 内容。
4. 损坏或不可用磁盘历史只使 24 小时趋势降级，不影响实时 summary 和 daemon 服务。
5. `cargo test -p persistd`、IPC 回归、格式检查和定向 Clippy 通过。

---

## 禁止事项

不得实现 CLI 或 TUI，不得修改 metadata schema 或 `persist metrics` 语义，不得让查询触发采样，
不得扩大既有 IPC 帧和分页上限，不得新增依赖。远端 push 仍须维护者授权。

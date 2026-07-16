# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 4：发布和长期维护

---

## 当前里程碑

待维护者确定。

---

## 当前唯一任务

确认 M52 之后的下一里程碑范围和优先级，不开始未确认的新功能实现。

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
- 阶段 7 CLI 数据客户端已完成，包含 `persist top` TTY 门禁、summary 全分页、趋势校验、
  request ID 校验、5 秒刷新/有界退避策略及兼容 MSRV 的 Ratatui/Crossterm 锁定依赖。
- 阶段 8 Ratatui 全屏界面已完成，包含 daemon 摘要、Session 稳定排序、详情趋势、紧凑终端
  降级、5 秒刷新、4 Hz 重绘上限、有界重连和 RAII 终端恢复。
- 阶段 9 性能、文档与发布验证已完成，包含 100/1000 Session 基准、全 workspace 门禁、
  Ubuntu tar/deb、RHEL 9 tar/RPM 和 Rocky test 主机端到端审计。

---

## 任务范围

- 审查 `TODO.md` 中仍未完成的条目，区分真实缺口、历史状态未同步和明确暂缓项。
- 由维护者确认下一里程碑的用户价值、边界、风险和验收标准。
- 确认后先更新设计、`MILESTONES.md`、`TODO.md` 和本文件，再开始代码实现。

---

## 完成标准

1. 下一里程碑由维护者明确确认。
2. 唯一任务、范围、禁止事项和完成标准写入本文件。
3. 相关里程碑、TODO 和设计文档状态一致。

---

## 禁止事项

在维护者确认下一里程碑前不得开始新功能，不得把历史 TODO 直接视为当前需求。不得创建
release、tag 或远端 push。

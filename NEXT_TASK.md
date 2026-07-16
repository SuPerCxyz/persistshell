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

M52 阶段 9：完成 Performance dashboard 性能、文档、打包和 test 主机发布验证。

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

---

## 任务范围

- 增加 Dashboard benchmark，比较采样关闭基线与 100/1000 个活跃 Session 的 CPU、RSS、
  采样窗口和队列状态。
- 验证 100 Session 平均 CPU 附加开销不超过单核 1%，并审计 64 MiB 内存、128 MiB 磁盘、
  2 秒采样和 IPC 上限。
- 运行全 workspace 测试、全 targets Clippy、格式检查和既有 benchmark 回归。
- 构建 tar/deb/RPM，确认 `persist top`、新增依赖和用户手册进入发布包。
- 部署到 `ssh test`，验证实时视图、趋势、daemon 重启恢复、终端恢复和核心 Session 回归。
- 更新用户、协议、限制和任务状态文档，形成 M52 验证审计。

---

## 完成标准

1. Dashboard 性能阈值和所有容量上限具有可复现的命令输出或审计记录。
2. 全 workspace test、全 targets Clippy、格式检查和 benchmark 回归通过。
3. tar/deb/RPM 均包含可执行的 `persist top` 和完整文档。
4. test 主机验证实时/历史趋势、重启恢复、全部终端退出路径和核心 Session 行为。
5. 设计规范中的验收项全部可追溯，M52 状态文档一致且 `NEXT_TASK.md` 指向下一任务。

---

## 禁止事项

不得放宽已确认的资源上限，不得为 benchmark 修改正式采样语义，不得增加 Web、鼠标依赖或
脚本 JSON 模式。不得创建 release、tag 或远端 push；远端部署仅限已授权的 `ssh test` 主机。

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

M52 阶段 8：使用 TDD 实现 `persist top` Ratatui 全屏界面、交互和终端恢复。

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

---

## 任务范围

- 使用 Ratatui 构建 daemon 摘要、Session 表格和详情趋势视图，紧凑终端自动降级。
- 主视图默认按 CPU 降序，支持 CPU/RSS/I/O/进程数/Session ID 排序和稳定选择。
- 支持方向键或 `j/k`、`Enter`、`Esc`、`q`、`Ctrl+C`，详情切换 15m/1h/24h 范围。
- 指标每 5 秒刷新，本地重绘不超过每秒 4 次；断线时显示状态并按有界退避重连。
- 所有成功、错误、信号和 panic 路径通过 RAII 恢复 raw mode、alternate screen 和光标。

---

## 完成标准

1. 纯 App 测试覆盖选择、排序、视图切换、范围切换、空数据和 Session 消失。
2. 渲染测试覆盖常规、窄屏和短屏，不 panic、不越界且文本不重叠。
3. 伪终端测试覆盖正常退出、`Ctrl+C`、连接错误和 panic 后终端模式恢复。
4. 断线重连不忙循环，刷新和重绘频率遵守 5 秒/4 Hz 上限。
5. `cargo test -p persist-cli`、格式检查、Clippy 和既有 CLI 回归通过。

---

## 禁止事项

不得增加 Web、鼠标依赖或脚本 JSON 模式，不得修改 metadata schema、`persist metrics` 语义或
IPC 上限，不得让 TUI 控制 Session writer。远端 push 仍须维护者授权。

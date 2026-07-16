# M52 Performance dashboard 设计

## 状态

- 日期：2026-07-16
- 状态：已确认
- 里程碑：M52

## 背景

PersistShell 已提供一次性的 `persist metrics`、`persist stats <id>` 和
`persist snapshot <id>`，但不能连续观察 daemon 或 Session 的资源变化。用户需要在终端中
同时查看实时状态和最近 24 小时趋势，并要求采集本身不能破坏 Shell runtime 的资源边界。

## 目标

- 提供统一的 `persist top` 全屏 TUI。
- 展示 daemon 汇总以及活跃 Session 整个进程树的 CPU、RSS、I/O 和进程数。
- 内存保留最近 1 小时的 5 秒采样，磁盘保留最近 24 小时的分钟聚合。
- 为内存、磁盘、采样任务和 IPC 响应设置硬上限。
- 采集、存储或 TUI 故障不得影响 PTY、attach、日志和 Session 恢复。

## 非目标

- Web dashboard、HTTP 服务、Prometheus endpoint 或集中式监控。
- 跨机器汇总、告警、通知和长期容量分析。
- 查看命令、输出、环境变量、路径或进程命令行。
- 精确的共享内存归属、cgroup 核算或系统级性能分析。
- 从 dashboard 执行 attach、takeover、关闭或修改 Session。

## 方案选择

采用 daemon 内置有界采样器。客户端驱动采样无法覆盖无人查看的时段；独立 collector 会增加
部署、权限和生命周期复杂度。`persist metrics` 保持现有一次性聚合语义，dashboard 使用独立
IPC，不把时序数据塞入现有响应。

## 采集架构

`persistd` 运行一个进程级采样 worker，每 5 秒由 daemon 主循环通过容量为 1 的通道触发。
worker 使用超时接收等待，不做 sleep polling。通道已满时跳过本轮，不积压任务；单轮截止时间
为 2 秒，超时后保留已完成部分并标记不完整。

每轮只扫描一次 `/proc`，使用 PID/PPID 构建进程关系，再以各运行中 Session 的根 Shell PID
聚合进程树。Session 关闭后停止新增采样，旧数据按轮转规则过期。采样工作不得运行在 PTY、
IPC 或日志写入的关键路径上。

## 数据模型

实时采样点包含：

```text
timestamp
daemon:
  cpu_user_ticks, cpu_system_ticks, rss_kib
  read_bytes, write_bytes
  session_count, runtime_count
  active_writer_count, readonly_client_count
sessions[]:
  session_id, process_count
  cpu_user_ticks, cpu_system_ticks, rss_kib
  read_bytes, write_bytes
  foreground_pid, collection_status
```

CPU 和 I/O 速率根据相邻点差值计算；第一点显示暂无数据。CPU 按整台机器容量展示，多核任务
允许超过 `100%`。Session RSS 是进程树 RSS 之和，共享页可能重复计算。进程退出或字段不可读
时使用部分采集状态，不能以零值伪装成功。

一分钟聚合点保存 CPU 平均值和峰值、RSS 平均值和峰值、I/O 增量、进程数峰值以及会话和
连接计数。速率使用单调时钟，展示时间使用系统时钟；系统时间回退时开始新分段。

## 内存边界

- 5 秒采样的理论窗口为 1 小时，每条序列最多 720 点。
- 数据按时间片存入环形缓冲，不为每个 Session 创建线程或定时器。
- 包含索引在内的总内存硬上限为 `64 MiB`。
- 达到上限后统一淘汰最旧时间片，使所有 Session 保持相同的可用时间范围。
- 最新采样优先保留；压力过高时缩短历史窗口并暴露降级状态。

## 磁盘持久化

指标写入 `~/.local/state/persistshell/metrics/`。目录权限为 `0700`，文件权限为 `0600`。
一分钟聚合点使用版本化二进制格式，按小时写入分段文件：

- 最多保留 24 个小时分段，磁盘硬上限为 `128 MiB`。
- 时间或容量超限时删除最旧完整分段，容量上限优先。
- 每条记录包含长度和校验信息；读取时限制文件数量、文件大小和记录长度。
- 重启时忽略并截断末尾不完整记录，其他损坏分段只跳过并告警。
- 不要求每分钟 `fsync`；正常关闭时刷新，崩溃允许丢失尚未落盘的一分钟数据。
- 不修改 metadata schema，也不使用 JSON 保存时序数据。

磁盘写入由容量为 2 的队列交给单个 writer worker。过载时合并或丢弃旧批次，
不得反向阻塞 daemon 核心路径。磁盘不可用时内存实时采样继续工作。

## TUI 交互

`persist top` 只在 TTY 中运行。主视图显示 daemon PID、采样时间、数据新鲜度、完整性状态、
daemon 汇总和活跃 Session 表格。Session 默认按 CPU 降序排列，并可按 RSS、I/O、进程数或
Session ID 排序。

方向键或 `j/k` 选择 Session，`Enter` 打开详情，`Esc` 返回，`q` 或 `Ctrl+C` 退出。详情可
切换 15 分钟、1 小时和 24 小时范围，显示 CPU、RSS、I/O 和进程数趋势。15 分钟和 1 小时
优先使用内存数据，24 小时使用分钟聚合。服务端按时间桶聚合，单次最多返回 240 个点。

TUI 本地重绘不超过每秒 4 次，指标 IPC 每 5 秒刷新一次。小终端使用紧凑列；无法可靠绘图时
改为数值摘要。所有正常、错误、信号和 panic 退出路径都必须还原备用屏幕和终端模式。

客户端断线后显示断开状态，并使用有上限的退避间隔重连。退出只关闭客户端；daemon 的全局
采样器继续运行以维持历史。第一版不增加 Web 服务或 `--json`，脚本继续使用 `persist metrics`。

## IPC

新增独立且分页的请求：

```text
DashboardSummary { cursor, limit }
DashboardSummaryResp { daemon, sessions, next_cursor, sampled_at }

DashboardTrend {
  scope: daemon | session_id,
  range: 15m | 1h | 24h,
  max_points
}
DashboardTrendResp { points, sampled_at, completeness }
```

响应继续遵守现有 IPC 帧大小限制。Session 列表分页读取，趋势最多返回 240 点，非法游标、
无效 Session、超限点数和版本不兼容返回明确协议错误。该公共协议与持久化决策由
`ADR-0004` 记录。

## 隐私与安全

采样只读取 `/proc/<pid>/stat`、`status` 和 `io` 的必要数值字段及 PID/PPID。不得读取或保存
`cmdline`、`environ`、`cwd`、文件描述符目标、终端输入输出、命令历史、note/tag 内容或
SSH Agent 信息。指标文件只包含 Session ID、时间戳和数值指标。

指标目录不得为符号链接且必须归当前用户所有。权限过宽时拒绝磁盘历史并告警，不能静默修复
或放宽权限。文件创建不得跟随符号链接。Dashboard 不改变 writer 所有权。

## 失败与降级

- 单个进程消失或不可读时标记部分采集，其余 Session 继续。
- 连续失败时保留最后成功值并显示数据年龄，不把旧值标为实时。
- 指标损坏、磁盘满或权限错误只停用磁盘历史。
- Daemon 重启加载有效分段，首轮 CPU/I/O 速率仍显示暂无数据。
- Dashboard 的任何失败必须降级为无历史或部分数据，不能阻止 daemon 启动。

## 测试与验收

测试覆盖 `/proc` 解析和进程树归属、进程退出、权限失败、差值计算、时间回退、分钟聚合、环形
淘汰、分段编码校验、损坏恢复、轮转、容量上限、安全权限、符号链接、IPC 分页和帧边界。

使用伪终端测试 TUI 导航、排序、范围切换、缩放、断线和终端恢复。Linux 集成测试启动真实
Shell 子进程树并观察 CPU、RSS 和 I/O。最终运行完整 Rust 测试、Clippy、格式检查和既有
attach/detach/恢复回归。

Benchmark 比较关闭采样器的基线与 100、1000 个活跃 Session：

- 指标内存始终不超过 `64 MiB`，磁盘始终不超过 `128 MiB`。
- 100 个活跃 Session 时，平均 CPU 附加开销不超过单核 `1%`。
- 1000 Session 时无任务积压或无界增长，超载时明确缩短窗口。
- 趋势最多返回 240 点，任何响应不突破现有 IPC 帧限制。
- 指标故障不影响 Session、PTY、attach、日志和恢复。
- 用户手册、协议、ADR、限制文档和里程碑状态在实现完成时同步更新。

## 实施约束

第一版使用上述固定采样周期和容量边界，不新增用户配置，避免配置迁移或错误配置绕过保护。
实现必须先完成协议和存储格式测试，再接入采样器，最后实现 TUI 和性能验证。

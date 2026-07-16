# ADR-0004：使用 Daemon 内置有界采样器提供性能仪表盘

## 状态

Accepted

## 日期

2026-07-16

## 背景

现有 `persist metrics`、`persist stats` 和 `persist snapshot` 都是一次性查询，不保留历史。
M52 需要通过 `persist top` 同时查看 daemon 汇总和活跃 Session 进程树的实时资源状态与最近
24 小时趋势。

持续采样会引入新的 CPU、内存、磁盘和隐私风险。PersistShell 的核心 PTY 和 Session runtime
不能因 dashboard 负载而阻塞，也不能引入默认 Web 服务、无界时序存储或每 Session 长期线程。

## 决策

在 `persistd` 中增加单一进程级采样 worker：

- daemon 主循环每 5 秒通过容量为 1 的通道触发一次 `/proc` 扫描；通道已满时跳过。
- worker 使用超时接收等待，不做 sleep polling，一次扫描聚合所有活跃 Session 进程树。
- 内存环形缓冲保存最近 1 小时，总上限 `64 MiB`。
- 每分钟聚合到版本化二进制小时分段，最多 24 小时和 `128 MiB`。
- 使用独立、分页且受现有帧大小约束的 Dashboard IPC。
- `persist top` 是唯一新增界面，不启动 HTTP 或其他监听服务。
- 采样和存储失败只降低 dashboard 完整性，不影响核心 Session 功能。

`persist metrics` 保持一次性查询和既有输出语义。第一版的周期与容量是固定安全边界，不开放
配置项。

## 原因

Daemon 持有 Session runtime 与根 Shell PID，是唯一能连续、准确建立 Session 和进程树关系的
组件。单 worker 可以复用一次 `/proc` 扫描，并集中实施时间、内存和磁盘上限，同时保持当前
同步 `poll` 事件循环不引入 Tokio runtime。

内存细粒度加磁盘降采样能同时满足实时观察和重启后 24 小时趋势；小时分段便于按时间和容量
淘汰，也能把损坏限制在单个分段。独立 IPC 保持 M42 的轻量脚本接口兼容。

## 被考虑的方案

### 客户端驱动采样

只在 `persist top` 运行时有开销，但无人查看时历史中断，不能满足连续趋势要求。

### 独立 Collector

故障隔离更强，但增加进程生命周期、部署、权限和一致性复杂度，不适合当前 per-user 架构。

### 默认 Web 或 Metrics Server

便于浏览器和外部系统接入，但扩大监听面与安全边界，并偏离本地终端产品定位。

### 无持久化实时 TUI

实现最小，但 daemon 重启后没有历史，也不能查看最近 24 小时趋势。

## 影响

### 正面影响

- 用户获得统一的实时和历史终端视图。
- 所有新增资源都有硬上限和明确降级路径。
- 既有 `persist metrics` 和核心 Session 协议语义不变。

### 负面影响

- Daemon 即使没有 dashboard 客户端，也会承担受控的周期采样开销。
- 引入新的二进制持久化格式、轮转逻辑和公共 IPC 消息。
- 进程树 RSS 会重复计算共享页，短生命周期进程可能在扫描中遗漏。

### 风险

- 大量进程可能导致 `/proc` 扫描超时。
- 损坏分段、磁盘满和系统时间回退会产生不连续趋势。
- TUI 异常退出可能破坏终端状态。

上述风险分别通过 2 秒采样截止、单任务不重入、有界队列、校验分段、安全降级和伪终端退出
测试控制。

## 回滚方案

可以禁用采样器并移除 `persist top` 与 Dashboard IPC，保留既有 `persist metrics`、`stats` 和
`snapshot`。指标分段不属于 Session metadata，停止读取后可独立清理，不需要数据库迁移。

## 后续任务

- [x] 定义版本化指标记录和 Dashboard IPC 编码。
- [x] 实现有界采样、聚合、轮转和恢复。
- [x] 实现 `persist top` 与伪终端测试。
- [x] 完成 100/1000 Session benchmark 和用户文档。

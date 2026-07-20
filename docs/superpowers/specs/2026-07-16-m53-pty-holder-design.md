# M53 单一 per-user PTY Holder 设计

## 状态

- 日期：2026-07-16
- 状态：已确认
- 里程碑：M53
- ADR：`docs/adr/ADR-0005-per-user-pty-holder.md`

## 背景

当前 `persistd` 同时承担控制面和 PTY 数据面职责，并直接持有所有 PTY master fd。SSH client
断开不会结束 Shell，但 `persistd` 崩溃会关闭 PTY master，使 Shell 和前台任务无法保证继续
运行。M53 将 PTY 所有权迁移到独立、单一的 per-user `persist-holder`，使 daemon 崩溃不再
决定 Session runtime 生命周期。

## 目标

- `persistd` 被 `SIGKILL`、panic 或异常退出后，运行中的 Shell 和任务继续存活。
- daemon 离线期间持续排空 PTY，并按有界策略保存输出和退出事件。
- daemon 重启后重新发现 holder，恢复 Session 列表、attach、输入、resize、signal 和 takeover。
- 100/1000 Session 下不引入每 Session 长期线程或 helper 进程。
- 保持现有 per-user 权限边界、CLI、Session ID 和 metadata 兼容性。
- 不依赖 systemd，在当前支持的 Linux 环境中按需启动。

## 非目标

- holder 自身崩溃后的 PTY 恢复。
- 系统重启后的运行中进程恢复。
- 多机复制、集中式 holder 或跨用户 Session 管理。
- M54 的最终 cwd side channel、M55 的动态环境恢复和 M56 的时间化 replay。
- 多个 writer 同时输入；继续使用单 active writer 和 takeover。

## 进程架构

```text
persist client
      |
      v
   persistd              控制面、公共 IPC、metadata、权限和策略
      |
      v
persist-holder           PTY、Shell、Ring Buffer、Session 输出日志
      |
      v
 Shell / foreground jobs
```

每个用户最多运行一个 `persist-holder`。holder 使用单一 `epoll` 循环管理所有 PTY master、
daemon 控制连接和内部唤醒 fd，不为每个 Session 创建长期线程或进程。阻塞磁盘写入通过一个
有界日志 worker 隔离，队列满时按明确策略降级，不能阻塞 PTY drain。

## 职责边界

`persistd` 负责：

- 面向 `persist` 的公共 Unix Socket 和协议兼容。
- peer credential、Session 权限、命令参数和策略校验。
- SQLite metadata、note/tag/pin/lock、GC 和 Dashboard 控制面。
- writer/read-only client 注册、takeover 决策和用户可见错误。
- holder 启动、重连、状态对账和显式关闭编排。

`persist-holder` 负责：

- 创建 PTY、fork/exec Shell、持有 master fd 和回收子进程。
- 非阻塞读取 PTY、写入输入、应用 resize 和发送 signal。
- 维护每 Session 有界 Ring Buffer、活动状态和退出结果。
- 维护 Session 输出日志及 daemon 离线期间的有界事件状态。
- 向唯一已认证 daemon 提供 Session inventory 和实时输出流。

holder 不直接读取或修改 SQLite，不接受 `persist` client 连接，也不实现用户级策略。

## 私有协议

新增内部版本化二进制协议，不能复用公共 client socket。默认路径：

```text
/run/user/$UID/persistshell/holder.sock
```

握手至少包含协议 major/minor、UID、PID、daemon 连接 nonce 和 holder instance ID。双方通过
`SO_PEERCRED` 验证同一 UID；nonce 由 daemon 每次连接重新生成并绑定请求/响应，不能替代
peer credential，也不作为同 UID 进程之间的秘密。

核心消息分为四组：

- 控制：`Hello`、`Inventory`、`Create`、`Close`、`Kill`、`ShutdownAll`。
- I/O：`Attach`、`Detach`、`Input`、`Output`、`Resize`、`Signal`。
- 状态：`SessionStarted`、`SessionExited`、`WriterChanged`、`LogDegraded`。
- 对账：`SnapshotBegin`、`SnapshotEntry`、`SnapshotEnd`、`AckGeneration`。

所有 payload、Session 数量、单次 inventory 和输出帧均有硬上限。未知类型、截断、尾随数据、
版本不兼容和 request ID 不匹配必须返回协议错误并断开控制连接。

daemon 与 holder 使用一条持久控制连接处理创建、状态和 inventory。每个 public client attach 时，
daemon 额外建立一条数据连接；数据连接必须携带当前 holder instance ID、daemon nonce 和 attach
request，并由 holder 校验其 `SO_PEERCRED` PID 与当前控制 daemon 一致。这样现有 client handler
可独立代理 I/O，而 holder 仍通过单一 `epoll` 循环管理所有数据连接。控制连接断开后，holder
立即撤销并关闭属于该 daemon 的数据连接和 writer，但保留 PTY runtime。

## 启动与重连

daemon 启动时先检查 holder socket。连接成功则完成认证和全量 inventory 对账；连接失败且锁和
socket 均不存在时才启动 holder。遇到 stale socket、存活但不兼容的 holder 或身份不匹配时，
daemon 必须拒绝继续，不能覆盖 socket 或启动第二个 holder。

holder 为每次进程启动生成 instance ID，并为状态变化维护单调 generation。daemon 记录最近已
确认 generation；重连时先读取完整快照，再接收快照之后的事件。快照是当前事实来源，事件只
用于避免对账和实时流之间出现窗口，不建立无限事件日志。

## Daemon 崩溃期间

控制连接意外关闭时，holder 进入 `orphaned-control` 状态：

- 保留所有 PTY、Shell、Ring Buffer 和日志 writer。
- 持续读取 PTY，避免任务因内核 PTY 缓冲区填满而阻塞。
- 拒绝新的外部 client；现有 client 因 daemon 消失而断开。
- 继续回收退出 Shell，并在内存中保留最终状态、exit code 和当前受限恢复上下文。
- 等待同 UID、协议兼容且成功取得 daemon 单实例锁的新 daemon 接管。

离线事件不按次数无限积累。holder 保存每个 Session 的最新幂等状态和有界 Ring Buffer；重连
通过 inventory 重建事实，不依赖完整事件历史。

## 显式停止

异常断开与显式停止必须严格区分。`persist daemon stop` 在通过现有策略校验后发送带 instance
ID 的 `ShutdownAll`。holder 停止接受输入，向运行中 Session 执行有界关闭，回收 Shell，刷新
日志，删除 holder socket 和锁后退出。只有收到已认证控制连接的显式消息才执行级联关闭。

holder 未确认关闭时，daemon stop 返回非零并报告仍存活的 holder，不能伪报成功。daemon
崩溃或被 `SIGKILL` 不会产生 `ShutdownAll`，因此不会结束 Session。

## I/O 与背压

holder 的 PTY fd 和 daemon 控制 socket 均使用非阻塞模式。每个 Session 的输出先写入 Ring
Buffer，再投递日志队列和在线 daemon。daemon 不可写或离线时停止投递实时副本，但继续 drain
PTY。重连后的 attach 从 Ring Buffer 回放，再进入实时流。

日志队列必须有容量和字节上限。队列过载时记录 `LogDegraded`，允许丢弃待写日志片段，但不得
丢弃 PTY drain、阻塞 Shell 或无限分配内存。日志缺口必须向 daemon 和用户可见。

## Metadata 对账

holder inventory 至少包含 Session ID、Shell PID、状态、exit code、创建时间、最后活动时间、
Ring Buffer 状态和日志降级状态。daemon 按以下规则与 SQLite 对账：

- holder 为 running、metadata 为 running/detached：以 holder runtime 为准。
- holder 为 exited、metadata 为 running/detached：原子更新为 Closed 并保存 exit code。
- metadata 为 running/detached、holder 不存在：标记 `lost`，不得伪装为 Closed 或自动重建。
- holder 存在、metadata 不存在：隔离为 orphan，不允许 attach，等待显式修复或安全清理。

对账必须幂等，daemon 在任意步骤再次崩溃后可重新执行。M53 如需新增 `lost` 状态或 holder
instance 字段，必须通过向前 migration 完成，并保留旧数据库升级测试。

## 安全边界

- runtime 目录 `0700`，holder socket、锁和状态文件 `0600`，拒绝符号链接和 owner 不匹配。
- holder 只接受同 UID daemon；root 跨用户接管不在本里程碑范围。
- PTY fd、控制 fd 和日志 fd 设置正确的 `CLOEXEC`，只有 Shell 所需 slave fd 进入 child。
- 启动参数、环境、cwd、输入和输出帧均执行长度限制，不拼接 shell 命令。
- holder 不记录用户输入，不把环境变量或 SSH agent 路径写入内部日志。

## 失败与降级

- holder 启动失败：daemon 不创建 Session，返回明确环境错误。
- holder 协议不兼容：拒绝接管并保留 holder，不得终止现有 Shell。
- holder 崩溃：daemon 将受影响 runtime 标记为 `lost` 并提示；M53 不承诺恢复。
- metadata 不可用：禁止创建和变更 Session，现有 holder runtime 继续运行。
- 日志不可用：Ring Buffer 和交互继续，暴露日志降级状态。
- daemon 重连超时：保持 holder 存活，client 收到可重试错误。

## 兼容与迁移

升级时，旧 daemon 直接持有的 runtime 无法无损迁移给 holder。首次启用新架构前必须要求没有
旧版 running Session，或提供明确的维护窗口检查并拒绝升级启动。现有 Closed metadata、日志、
history、note、tag、pin 和 lock 保持兼容。

发布包将内部二进制安装到 `/usr/libexec/persistshell/persist-holder`。它不加入用户 PATH，不提供
公开 CLI，只能由 `persistd` 使用构建时确定的绝对路径启动；测试构建可使用受限测试注入路径。

## 测试策略

单元测试覆盖内部协议编解码、generation 对账、状态机、背压上限、权限检查和 stale socket。
进程集成测试使用真实 PTY 和三个独立进程，禁止用纯 mock 代替崩溃边界。

关键故障注入场景：

1. Shell 和前台任务运行时 `SIGKILL persistd`，确认 PID、PTY 和任务继续存活。
2. daemon 离线期间持续输出超过内核 PTY 缓冲区，任务继续推进且 holder 内存有界。
3. daemon 重启后完成 inventory 对账，`persist ls` 和读写 attach 恢复。
4. 重连后验证 resize、SIGINT/SIGTSTP、read-only attach 和 writer takeover。
5. daemon 离线期间 Shell 退出，重启后 Closed 状态、exit code 和输出一致。
6. 显式 stop 与 `SIGKILL` 走不同路径，只有前者级联关闭 holder。
7. holder 身份、权限、符号链接、协议版本和超限帧错误路径均拒绝访问。
8. 100/1000 Session benchmark 对比 M52 基线，记录 CPU、RSS、创建、列表和 attach 开销。
9. Ubuntu 26.04、RHEL 9 包内容及 Rocky test 主机端到端故障注入通过。

## 完成标准

- `persist-holder` 和内部协议实现完成，所有资源上限可配置或有稳定默认值。
- daemon 崩溃和重启场景不终止 holder、Shell 或前台任务。
- daemon 离线期间输出持续被排空，重连后可回放并继续交互。
- metadata 对账幂等，`lost`、orphan 和日志降级状态对用户可见。
- 显式 stop、安全权限、升级拒绝路径和 holder 崩溃路径有自动化测试。
- workspace fmt、clippy、test、package、兼容性和性能门禁通过。
- 用户手册、架构、协议、限制、TODO、里程碑和 changelog 同步。

## 后续里程碑边界

M53 只建立 daemon 崩溃可恢复的数据平面。完成后按已确认顺序推进：

- M54：bash/zsh/fish 最终 cwd 状态 side channel。
- M55：安全恢复动态导出环境。
- M56：版本化时间日志、`replay --speed` 和事件驱动 `--follow`。

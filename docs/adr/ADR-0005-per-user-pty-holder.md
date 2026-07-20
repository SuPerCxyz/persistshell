# ADR-0005：使用单一 per-user PTY Holder 隔离 Daemon 崩溃

状态：Accepted

日期：2026-07-16

---

## 背景

当前 `persistd` 直接持有所有 PTY master fd。client 或 SSH 断开不会结束 Session，但 daemon
崩溃会关闭这些 fd，Shell 和前台任务可能收到 HUP、终止或因失去控制终端而不可恢复。这使
Shell 生命周期仍部分依赖 daemon 生命周期，与 PersistShell 的长期恢复目标不一致。

解决该问题的进程必须在 daemon 崩溃期间继续持有并排空 PTY，否则高输出任务会在内核 PTY
缓冲区耗尽后阻塞。该进程还必须满足 per-user 权限边界和 100/1000 Session 资源目标。

## 决策

新增单一 per-user `persist-holder` 数据平面进程。holder 使用 `epoll` 管理该用户的全部 PTY，
负责 Shell 子进程、Ring Buffer 和 Session 输出日志。`persistd` 保留公共 IPC、metadata、权限
和策略职责，并通过独立私有 Unix Socket 与 holder 通信。

daemon 控制连接异常断开时，holder 继续运行并等待新 daemon 接管。只有已认证 daemon 发送
显式 `ShutdownAll` 时，holder 才级联关闭 runtime。daemon 重启后通过版本化 inventory 和
generation 协议与 holder 状态对账。

holder socket 位于 `/run/user/$UID/persistshell/holder.sock`，使用 `0700` 目录、`0600` 文件、
`SO_PEERCRED` 和启动 nonce 验证控制连接。holder 不接受普通 `persist` client。

## 原因

- 单一 holder 能在 daemon 崩溃期间持续 drain 所有 PTY。
- 一个 `epoll` 数据平面不会引入每 Session 长期线程或 helper 进程。
- 控制面和数据面分离后，daemon 可独立重启并重建可派生状态。
- 不依赖 systemd，保留最小 Linux、容器和不同发行版环境的兼容性。
- 私有协议允许设置严格边界，不扩大现有公共 IPC 和用户攻击面。

## 被考虑的方案

### 方案 A：单一 per-user holder

资源开销低，适合 1000 Session；需要实现内部协议、状态对账和升级边界。选择该方案。

### 方案 B：每 Session 一个 holder

故障隔离和局部状态较直观，但每个 Session 增加一个长期进程，1000 Session 时进程数和内存
成本不可接受，也会增加日志、回收和升级编排复杂度。

### 方案 C：依赖 systemd user service

systemd 可管理进程重启，但不能替代持续持有 PTY 的数据平面；无 user systemd 或无 linger 的
环境也无法保证行为一致。后续可以用 systemd 启动组件，但不作为核心正确性依赖。

### 方案 D：复制 fd 后由新 daemon 取回

仅保留 master fd 而不持续读取 PTY，会让高输出任务阻塞。使用 pidfd 或调试接口取回 fd 还受
内核版本和权限策略限制，无法作为支持基线。

## 影响

### 正面影响

- daemon 崩溃不再关闭 PTY 或终止 Shell。
- daemon 离线期间任务输出仍被有界读取和保存。
- daemon 可升级为可重启控制面，Session runtime 由稳定数据面管理。
- 后续 cwd/env side channel 可以直接由 holder 在 Shell 退出边界接收。

### 负面影响

- 发布包增加内部二进制和私有协议。
- PTY、Ring Buffer 和 Session 日志所有权需要从 daemon 迁移到 holder。
- daemon 与 holder 状态可能短暂不一致，需要幂等对账和明确 `lost`/orphan 状态。
- 旧版 running Session 不能无损热迁移到新 holder。

### 风险

- holder 成为新的单点故障；M53 只隔离 daemon 崩溃，不承诺 holder 崩溃恢复。
- 内部输出队列或日志队列设计错误可能造成内存增长或 Shell 反压。
- 错误的 stop/crash 判定可能意外结束 Session，必须只接受显式认证的关闭消息。
- 对账遗漏可能产生幽灵 Session、错误 Closed 状态或未授权 orphan attach。

## 被拒绝的方案

- 继续让 daemon 持有 PTY：不能满足 daemon 崩溃恢复目标。
- 让 client 持有备用 master fd：client 生命周期依赖 SSH，且扩大 fd 和权限暴露面。
- 仅靠自动重启 daemon：PTY master 已关闭后重启无法恢复原 fd 和 Shell 状态。
- 无限保存离线事件或输出：违反资源边界，必须使用快照和有界 Ring Buffer。

## 回滚方案

在尚无 holder 管理的 running Session 时，可以停止新 holder，恢复 daemon 直接创建和持有 PTY
的旧路径。metadata migration 只能追加字段和状态，旧版本未知字段应可忽略；`lost` 状态需要在
回滚前转换为旧版本可识别的 Closed/Error 表达。

已经由 holder 管理的 running Session 不允许在线降级给旧 daemon。回滚工具必须先拒绝存在
活动 runtime 的操作，要求用户显式结束 Session 或继续使用兼容 holder 的版本。

## 后续任务

- [x] 编写 M53 中文设计规范。
- [ ] 定义 holder 内部协议和资源上限。
- [ ] 实现 `persist-holder` 与 daemon 接管状态机。
- [ ] 实现 metadata 对账和升级拒绝路径。
- [ ] 增加崩溃注入、性能、打包和远程测试。
- [x] 更新架构、限制、TODO、里程碑和 CHANGELOG 的设计状态。
- [ ] 实现完成后更新用户手册和最终限制状态。

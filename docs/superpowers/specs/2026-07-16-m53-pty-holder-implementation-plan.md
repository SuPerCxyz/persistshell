# M53 单一 per-user PTY Holder 实施计划

## 进度

- [x] 阶段 1：Holder 私有协议
- [x] 阶段 2：Holder 安全生命周期
- [x] 阶段 3：Holder PTY 与有界 I/O 数据面
- [x] 阶段 4：Daemon Holder 控制客户端
- [x] 阶段 5：Session 管理和 public attach 迁移
- [x] 阶段 6：崩溃重连与 Metadata 对账
- [x] 阶段 7：Dashboard、日志和兼容路径收尾
- [x] 阶段 8：CLI、打包与升级边界
- [x] 阶段 9：故障注入、性能和平台验证

## 目标与边界

按照已确认的 M53 设计和 `ADR-0005`，将 PTY master、Shell 子进程、Ring Buffer 和 Session
输出日志迁移到单一 per-user `persist-holder`。`persistd` 崩溃后 holder 必须继续 drain PTY，
daemon 重启后通过 inventory 恢复管理。M53 不实现最终 cwd side channel、动态环境恢复、
时间化日志或 replay speed/follow。

本任务涉及核心并发、私有协议、进程生命周期、metadata 和打包，采用 Level 2 TDD。共享协议、
workspace、CI、根配置和发布脚本由主 Agent 串行修改，不进行并行写入。

## 技术选择

- Rust MSRV 保持 1.80，不引入 Tokio。
- 在现有 `persist-ipc` 增加隔离的 `holder` 私有协议模块，不复用 public `MessageType`。
- 新增 workspace crate `persist-holder`，同时提供可测试 library 和内部 binary。
- holder 使用 `epoll`、非阻塞 Unix Socket、非阻塞 PTY 和一个进程级日志 worker。
- daemon 使用一条持久控制连接；每个 attach 使用一条同 PID 认证的数据连接。
- 控制请求串行化，数据连接独立代理，避免在 daemon 内新增全局输出 dispatcher。
- holder 状态以有界内存 inventory 为准；SQLite 继续由 daemon 独占。
- 不新增 JSON 持久化文件，不使用 sleep polling，不为每 Session 创建线程或 helper。

## 全局不变量

1. 只有 holder 持有 PTY master 和回收 Shell child。
2. 控制连接 EOF 不关闭 PTY；只有认证的 `ShutdownAll` 才级联关闭。
3. holder 的所有输入队列、输出队列、Ring Buffer、日志队列和协议帧有硬上限。
4. public client 不直接访问 holder socket，holder 拒绝非当前 daemon PID 的数据连接。
5. metadata 与 holder 对账幂等；未知 holder runtime 不允许 attach。
6. 任一阶段结束时 workspace 可编译，现有 public CLI 测试保持通过。

## 阶段 1：Holder 私有协议

修改或新增：

- `crates/persist-ipc/src/holder.rs`
- `crates/persist-ipc/src/holder_tests.rs`
- `crates/persist-ipc/src/lib.rs`
- `docs/protocol/HOLDER_PROTOCOL.md`
- `docs/INDEX.md`

步骤：

1. 定义独立 magic、协议版本、最大控制帧、最大 I/O 帧和固定 header。
2. 定义控制握手、控制请求/响应、inventory entry、状态事件和数据连接握手。
3. 为 Session 状态、attach mode、退出结果和日志降级使用显式有界枚举/字段。
4. 编写统一 `Reader`/encoder，拒绝截断、尾随、非法 UTF-8、非法枚举和超限长度。
5. nonce、instance ID、generation 和 request ID 使用固定宽度，不接受隐式默认值。
6. 测试所有消息 round-trip、最大合法边界和每种损坏输入。

验收：`cargo test -p persist-ipc holder` 和 `cargo clippy -p persist-ipc --all-targets -- -D warnings`
通过，既有 public protocol 编号和测试不变。

## 阶段 2：Holder 安全生命周期

新增：

- `crates/persist-holder/Cargo.toml`
- `crates/persist-holder/src/lib.rs`
- `crates/persist-holder/src/main.rs`
- `crates/persist-holder/src/lifecycle.rs`
- `crates/persist-holder/src/socket.rs`
- `crates/persist-holder/tests/lifecycle.rs`

修改：

- `Cargo.toml`
- `Cargo.lock`

步骤：

1. 新增 workspace crate 和最小内部 binary，拒绝公开业务子命令。
2. 安全创建 runtime 目录、holder lock、PID 文件和 socket，权限分别为 `0700`/`0600`。
3. 拒绝 symlink、owner 不匹配、过宽权限、重复 holder 和无法证明 stale 的 socket。
4. 使用 `SO_PEERCRED` 验证 UID/PID，实现单控制 daemon claim 和 instance ID。
5. 实现 SIGTERM 正常退出、控制连接 EOF 保持进程和显式 `ShutdownAll` 退出骨架。
6. 测试重复启动、stale 文件、权限错误、伪造 PID、控制断线和显式关闭。

验收：holder 生命周期测试通过；控制连接断开后进程仍存活，显式关闭后清理全部 runtime 文件。

## 阶段 3：Holder PTY 与有界 I/O 数据面

新增：

- `crates/persist-holder/src/runtime.rs`
- `crates/persist-holder/src/reactor.rs`
- `crates/persist-holder/src/connection.rs`
- `crates/persist-holder/src/log_worker.rs`
- `crates/persist-holder/src/server_handlers.rs`
- `crates/persist-holder/tests/runtime.rs`
- `crates/persist-holder/tests/runtime_advanced.rs`

复用：

- `crates/persist-pty`
- `crates/persist-core` 的 Ring Buffer 和配置类型

步骤：

1. 实现 `epoll` reactor，注册 listener、控制/数据连接、PTY fd 和内部唤醒 fd。
2. 实现 Create/Close/Kill、Shell waitpid、exit code、当前受限恢复上下文和 inventory。
3. 将输入、resize、signal 和 output 映射到 session/attach ID，保持单 writer takeover。
4. 每个数据连接使用有界待写队列，处理 partial write；超限时断开慢客户端并释放 writer。
5. PTY 输出先进入 Ring Buffer，再进入数据连接和有界日志队列；任何下游不能阻塞 drain。
6. 将现有 Session 日志轮转迁入单一进程级日志 worker，不创建每 Session writer thread。
7. 控制断线时撤销数据连接但保留 PTY；无 daemon 时继续读取、记录和回收 Shell。
8. 测试 echo、大输出、partial write、慢客户端、输出风暴、takeover、resize、signal 和 exit。

验收：真实 PTY 测试证明 daemon/data socket 全断开时 Shell 仍运行且大输出继续推进；内存和队列
始终低于硬上限。

实施结果：单一 epoll reactor 已接管控制/data socket、PTY、signalfd 和日志 eventfd；控制与输入
待写队列、Ring Buffer、日志积压均有硬上限。专项测试覆盖真实 PTY、离线 drain/replay、takeover、
read-only、resize、signal、exit、Close/Kill、大输出和日志轮转；生命周期并行压力 80 轮、workspace
Clippy 与全量测试通过。

## 阶段 4：Daemon Holder 控制客户端

新增：

- `crates/persistd/src/holder/mod.rs`
- `crates/persistd/src/holder/client.rs`
- `crates/persistd/src/holder/process.rs`
- `crates/persistd/src/holder/reconcile.rs`
- `crates/persistd/src/holder/tests.rs`

修改：

- `crates/persistd/src/main.rs`
- `crates/persistd/src/server.rs`
- `crates/persistd/Cargo.toml`

步骤：

1. 实现安全 holder 路径解析：安装路径为 `/usr/libexec/persistshell/persist-holder`，开发测试
   允许显式受限注入，生产环境不读取不可信 PATH。
2. daemon 取得自身单实例锁后连接或启动 holder，再进行版本/UID/PID/nonce 握手。
3. 使用互斥串行控制请求，校验 request ID、instance ID、响应类型和 payload 上限。
4. 实现有界启动等待和重连，不使用 sleep polling；通过 socket 可写/进程退出事件推进。
5. 把 holder inventory 转为 daemon 内只读 runtime cache，供 list、Dashboard 和对账读取。
6. holder 不兼容、身份错误或状态损坏时拒绝接管，不能终止 holder 或现有 Shell。
7. 测试已运行 holder、自动启动、并发请求、断线、重连、错误版本和伪造响应。

验收：daemon 可稳定管理空 holder，重复 daemon/holder 均被拒绝，holder 断线错误可诊断。

实施结果：daemon 使用固定安装路径或 debug 显式可信路径连接/启动 Holder，inotify/pidfd 提供
有界事件等待；控制请求串行并校验 ID、类型、generation、instance 和异步事件。只读 inventory
cache 支持同 instance 传输重连。测试覆盖 fake-holder 伪造响应/并发请求，以及真实 daemon
SIGKILL 后 Holder 保活、第二 daemon 接管和正常 ShutdownAll；workspace 全量门禁通过。

## 阶段 5：Session 管理和 Public Attach 迁移

修改或拆分：

- `crates/persistd/src/server.rs`
- `crates/persistd/src/session.rs`（新增）
- `crates/persistd/src/public_attach.rs`（新增）
- `crates/persistd/src/shell_history.rs`
- `crates/persistd/tests/persistd.rs`

步骤：

1. 将 Session metadata/policy 与 PTY runtime 操作分离，删除 daemon 对 `PtySession` 的所有权。
   Holder Create 必须携带有界 argv，保留现有临时 shell helper，不能覆盖或修改用户配置。
2. New/Close/Kill/Closed restore 改为 holder 控制请求，metadata 写入保持 daemon 事务边界。
3. List/Snapshot/ProcessTree/Stats 合并 holder runtime cache 与 SQLite metadata。
4. public attach handler 创建 holder 数据连接，完成认证后在 public/data socket 间代理帧。
5. 保持 public ATTACH/STDIN/STDOUT/RESIZE/SIGNAL/DETACH 协议和 CLI 行为不变。
6. writer/read-only/takeover 由 holder 执行，daemon 继续做 lock 和权限策略校验。
7. Closed Session 仍使用当前 cwd/环境白名单创建新 runtime，不提前实现 M54/M55。
8. 删除旧 daemon `io_loop` 和只读广播路径前，运行所有现有 attach/Session 回归测试。

验收：现有 CLI 和 public IPC 测试无需协议变更即可通过，代码中只有 holder 调用
`PtyEngine::open_session*` 或持有 PTY master。

实施结果：生产版 daemon 的 New/Close/Kill/Closed restore 均通过 Holder 控制请求，public
attach 使用认证数据连接代理现有 IPC，支持读写、只读、takeover、replay、resize、signal 和
退出事件。List、按标签 List、Snapshot、ProcessTree、Stats、Metrics 和 Dashboard 已合并
Holder inventory 与 metadata。旧 `PtyEngine`、Ring Buffer、日志 writer 和 `io_loop` 仅保留在
`cfg(test)` legacy 单元夹具中，生产启动缺少可信 Holder 时明确失败。真实进程测试证明 daemon
SIGKILL 后已有 Shell 继续运行并产生输出，第二 daemon 接管同一 Holder 后可通过 attach replay
读取；workspace fmt、全 targets Clippy 和完整测试通过。

## 阶段 6：崩溃重连与 Metadata 对账

修改：

- `crates/persist-metadata/src/migration.rs`
- `crates/persist-metadata/src/store.rs`
- `crates/persistd/src/holder/reconcile.rs`
- `crates/persistd/tests/persistd.rs`
- `docs/architecture/METADATA.md`

步骤：

1. 增加最小向前 migration，表达 `lost` 状态和 holder instance/generation 对账信息。
2. 实现 holder running/exited/missing/orphan 与 metadata 状态的幂等规则。
3. daemon crash 后新 daemon 先完成全量 snapshot，再开放 public socket 接受 Session 操作。
4. 快照后 generation 变化必须通过事件或重新快照闭合，不允许遗漏退出状态。
5. daemon 离线期间 Shell 退出时，恢复 Closed、exit code、日志和现有受限上下文。
6. metadata 中 running 但 holder 缺失时标记 `lost`；holder orphan 默认隔离且不可 attach。
7. 测试在 create、metadata commit、exit、对账各步骤注入 daemon 崩溃并重复重启。

验收：任意对账步骤重复执行结果一致，不产生重复 Session、错误 ID 或可访问 orphan。

结果：Metadata schema v7 已记录 Holder instance/generation；daemon 在 public socket 前完成稳定
snapshot 和 running/exited/missing/orphan 幂等对账，运行期周期刷新。debug-only 故障注入覆盖
create、metadata commit、Shell exit 和 reconcile 窗口，验证离线退出码、日志、orphan 隔离、
`lost` 状态和 ID 单调性；persistd 123 个单元测试（1 个既有忽略）及 8 个进程测试通过。

## 阶段 7：Dashboard、日志和兼容路径收尾

修改：

- `crates/persistd/src/dashboard/worker.rs`
- `crates/persistd/src/dashboard/ipc.rs`
- `crates/persistd/src/server.rs`
- `crates/persistd/src/log_writer.rs`（迁移完成后删除）
- `docs/known/KNOWN_ISSUES.md`
- `docs/known/LIMITATIONS.md`

步骤：

1. Dashboard 采样根 PID、writer 和 runtime 计数改从 holder inventory cache 获取。
2. Session 日志路径、轮转和 log/replay 读取行为保持兼容，日志降级状态进入 snapshot/doctor。
3. 清除 daemon 中遗留 PTY、Ring Buffer、Session logger 和 waitpid 所有权。
4. 验证 Idle GC、pin/lock、note/tag/history、metrics、snapshot 和 top 行为不回归。
5. holder 崩溃时将 runtime 标记 `lost`，更新用户可见提示但不伪称可恢复。

验收：`rg` 审计证明 PTY 数据面所有权唯一；全量 daemon/CLI/dashboard 定向测试通过。

结果：Dashboard root/runtime/writer 计数已统一读取 Holder inventory；Snapshot、Metrics 和 doctor
展示日志 degraded 与 `lost`，Holder pidfd 退出会清空缓存并将活动 metadata 标记 `lost`。production
二进制审计不含 legacy PTY/I/O/logger 符号；真实测试覆盖日志 symlink 降级、Holder SIGKILL、
Idle GC 及 pin/lock 豁免。workspace Clippy 和全量测试通过。

## 阶段 8：CLI、打包与升级边界

修改：

- `crates/persist-cli/src/daemon.rs`
- `scripts/package-release.sh`
- `.github/workflows/ci.yml`
- `.github/workflows/package.yml`
- `docs/user/USER_GUIDE.md`
- `docs/user/INSTALL.md`
- `docs/user/TROUBLESHOOTING.md`
- `docs/man/persistd.1`

步骤：

1. daemon status/doctor 展示 holder PID、instance、连接和 degraded/lost 状态。
2. tar/deb/rpm 将 holder 安装到 `/usr/libexec/persistshell/`，卸载时安全清理二进制但不删除日志。
3. source/dev 启动和安装路径均有测试，禁止生产环境通过 PATH 选择 holder。
4. 升级检测到旧架构 running metadata 且无 holder 时明确拒绝或标记 lost，不能伪造热迁移。
5. 更新 man page、完整用户手册、故障排查和绕过方式。

验收：三种包内容、权限、安装、升级拒绝和卸载测试通过；GitHub Actions 构建路径同步。

结果：Metrics、`persist daemon status` 和 `persist doctor` 统一展示 Holder PID、instance、控制
连接状态及 degraded/lost 汇总；release 固定使用 `/usr/libexec/persistshell/persist-holder`，debug
保留可信开发路径。tar/deb/RPM 均包含 `0755` Holder 且普通卸载不携带用户 XDG 数据删除逻辑。
真实 daemon/Holder 诊断、旧 metadata `lost`、固定路径选择、包内容/checksum、workspace Clippy
和全量测试通过；RHEL9 原生 ABI 与安装运行验证进入阶段 9。

## 阶段 9：故障注入、性能和平台验证

新增或修改：

- `scripts/test-holder-recovery.sh`
- `scripts/benchmark-sessions.sh`
- `docs/audit/2026-07-19-m53-pty-holder-validation.md`
- `CHANGELOG.md`
- `TODO.md`
- `MILESTONES.md`
- `NEXT_TASK.md`

步骤：

1. 本地执行 fmt、clippy、workspace test 和全部真实 PTY/IPC 集成测试。
2. 故障脚本在运行任务、大输出、Shell exit 和 takeover 各阶段 `SIGKILL persistd`。
3. 验证 holder/Shell PID 存活、输出持续推进、daemon 重启、attach 和 metadata 一致。
4. 运行 100/500/1000 Session benchmark，与 M52 基线比较 CPU、RSS 和延迟。
5. 构建 Ubuntu 26.04 tar/deb 与 RHEL 9 tar/rpm，检查 holder 路径、权限和依赖 ABI。
6. 部署到 `ssh test`，执行真实故障注入、重启、attach、signal、resize 和显式 stop。
7. 记录所有命令、结果、限制和性能数据，完成文档状态收尾。

验收：设计中的九项测试场景全部有证据；M53 标记完成并将 `NEXT_TASK.md` 指向 M54。

结果：九项场景均有自动化或 Rocky test 真实进程证据；daemon SIGKILL 后 Holder/Shell 和
1 MiB 输出任务存活，重启后 readonly/replay、takeover、resize、signal 和 exit code 通过。
100/500/1000 Session 基准、Ubuntu 26.04 tar/deb、RHEL 9 tar/RPM、GLIBC 2.34、卸载保留
用户数据及 workspace 全门禁通过。详细记录见
`docs/audit/2026-07-19-m53-pty-holder-validation.md`。

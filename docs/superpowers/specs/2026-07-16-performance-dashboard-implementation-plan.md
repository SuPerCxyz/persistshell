# M52 Performance dashboard 实施计划

## 进度

- [x] 阶段 1：Dashboard IPC
- [x] 阶段 2：有界内存模型
- [x] 阶段 3：单次 `/proc` 进程树聚合
- [x] 阶段 4：版本化小时分段
- [ ] 阶段 5：Worker 与 Daemon 生命周期
- [ ] 阶段 6：Daemon Dashboard IPC
- [ ] 阶段 7：CLI 数据客户端与命令入口
- [ ] 阶段 8：Ratatui 全屏界面
- [ ] 阶段 9：性能、文档与发布验证

## 目标与边界

按照已确认的设计和 `ADR-0004` 实现 `persist top`，提供 daemon 汇总和活跃 Session 进程树的
实时与 24 小时趋势。实现不得改变 `persist metrics` 语义，不启动 Web 服务，不采集敏感内容，
不突破 64 MiB 内存、128 MiB 磁盘、2 秒单轮采样和现有 IPC 帧上限。

## 技术选择

- Rust MSRV 保持 1.80。
- CLI 使用 `ratatui = 0.29.0` 和 `crossterm = 0.28.1`。
- 两者的声明 MSRV 分别为 1.74 和 1.63，满足项目要求。
- daemon 保持同步 `poll` 架构，不引入 Tokio。
- 采样使用一个进程级 worker，磁盘使用一个 writer worker；禁止每 Session worker。
- 二进制记录使用固定版本头、长度和 CRC32，不新增 checksum crate。

## 测试策略

本功能属于共享协议、后台采样和持久化行为变化，采用 Level 2 TDD。每一步先增加失败测试，再做
最小实现并运行对应 crate 测试。每个阶段完成后执行 `cargo fmt --check` 和定向 Clippy；最终执行
全 workspace 测试、集成测试、benchmark、本地包验证和 test 主机部署验证。

## 阶段 1：Dashboard IPC

修改：

- `crates/persist-ipc/src/protocol.rs`
- `crates/persist-ipc/src/dashboard.rs`（新增）
- `crates/persist-ipc/src/lib.rs`
- `docs/protocol/CLIENT_PROTOCOL.md`
- `docs/protocol/SOCKET_PROTOCOL.md`

步骤：

1. 为 summary/trend 请求响应定义固定宽度、显式大小端的数据结构。
2. 增加四个消息类型，保持现有消息编号不变。
3. 实现分页游标、时间范围、scope、完整性和 collection status 编解码。
4. 拒绝非法枚举、超限页大小、超过 240 点、截断 payload 和尾随数据。
5. 测试最大合法响应小于 `MAX_CONTROL_FRAME`，并验证旧消息 round-trip 不变。

验收：`cargo test -p persist-ipc` 全部通过。

## 阶段 2：有界内存模型

新增：

- `crates/persistd/src/dashboard/mod.rs`
- `crates/persistd/src/dashboard/model.rs`
- `crates/persistd/src/dashboard/history.rs`

步骤：

1. 定义原始计数、展示速率、分钟聚合、完整性和数据年龄模型。
2. 使用可注入的单调时间与系统时间测试首点、差值、计数器回退和时间回退。
3. 实现按时间片组织的环形历史和精确容量记账。
4. 达到 64 MiB 时按最旧时间片统一淘汰，始终保留最新点。
5. 实现最多 240 桶的服务端降采样，并测试空区间、部分数据和窗口缩短。

验收：模型测试证明点数、容量和时间窗口均有硬上限。

## 阶段 3：单次 `/proc` 进程树聚合

新增：

- `crates/persistd/src/dashboard/procfs.rs`
- `crates/persistd/src/dashboard/procfs_tests.rs`

步骤：

1. 将 `/proc` 文件读取抽象为可使用 fixture 的只读 source。
2. 一次枚举解析 PID、PPID、CPU ticks、RSS 和 I/O 数值，不读取敏感文件。
3. 以根 Shell PID 归属所有后代，处理嵌套、进程消失、权限错误和 PID 数据损坏。
4. 防止同一 PID 重复归属；根 PID 缺失时标记 unavailable，部分缺失时标记 partial。
5. 使用真实 Linux 子进程树增加定向集成测试，断言数值可观察而不使用脆弱精确值。

验收：fixture 与真实 `/proc` 测试通过，扫描代码不访问 `cmdline`、`environ` 或 `cwd`。

## 阶段 4：版本化小时分段

新增：

- `crates/persistd/src/dashboard/format.rs`
- `crates/persistd/src/dashboard/storage.rs`
- `crates/persistd/src/dashboard/storage_tests.rs`

步骤：

1. 定义 magic、版本、记录长度、时间、payload 和 CRC32 格式。
2. 限制目录项数量、单文件大小、记录长度和解码分配。
3. 安全创建 `0700` 目录和 `0600` 文件，拒绝 symlink、错误 owner 和过宽权限。
4. 实现尾部不完整记录截断、损坏分段跳过、系统时间回退新分段和重启加载。
5. 实现最多 24 个分段和 128 MiB 容量轮转，容量淘汰优先。

验收：临时目录测试覆盖正常、损坏、安全和容量边界，不修改 metadata 数据库。

## 阶段 5：Worker 与 Daemon 生命周期

新增或修改：

- `crates/persistd/src/dashboard/worker.rs`
- `crates/persistd/src/dashboard/writer.rs`
- `crates/persistd/src/main.rs`
- `crates/persistd/src/server.rs`

步骤：

1. 实现容量为 1 的采样触发通道和容量为 2 的磁盘批次通道。
2. daemon 主循环每 5 秒复制 `(session_id, root_pid)` 和聚合计数后使用 `try_send`。
3. worker 单次扫描、更新内存历史并提交分钟批次，不持有 `SessionManager` 锁扫描 `/proc`。
4. 用共享只读快照服务查询，写锁范围只覆盖替换已构建数据。
5. 实现启动恢复、shutdown 通知、writer 刷新和有界 join；任何失败只记录 dashboard 状态。
6. 测试触发合并、不重入、worker panic/退出、磁盘队列过载和 daemon 正常关闭。

验收：现有 daemon accept、PTY 和 GC 路径不等待采样或磁盘 I/O。

## 阶段 6：Daemon Dashboard IPC

新增或修改：

- `crates/persistd/src/dashboard/ipc.rs`
- `crates/persistd/src/server.rs`
- `crates/persistd/tests/persistd.rs`

步骤：

1. 将 dashboard handle 注入 client handler，只暴露有界只读查询。
2. 实现 summary 稳定排序与分页，游标失效时返回明确错误。
3. 实现 daemon/Session trend 查询、范围选择、降采样和完整性状态。
4. 覆盖无样本、未知 Session、关闭中 Session、超限请求和 worker 不可用。
5. 验证 `persist metrics` 响应和既有协议测试不变。

验收：daemon IPC 集成测试通过，所有响应均低于控制帧上限。

## 阶段 7：CLI 数据客户端与命令入口

修改或新增：

- `Cargo.lock`
- `crates/persist-cli/Cargo.toml`
- `crates/persist-cli/src/command.rs`
- `crates/persist-cli/src/cli.rs`
- `crates/persist-cli/src/dashboard/client.rs`
- `crates/persist-cli/src/dashboard/mod.rs`
- `crates/persist-cli/tests/cli.rs`

步骤：

1. 添加已确认的 Ratatui/Crossterm 版本并保持 daemon crate 依赖不变。
2. 解析 `persist top`，非 TTY 时返回稳定、可诊断错误。
3. 客户端读取全部 summary 页，趋势请求限制为当前终端宽度且不超过 240 点。
4. 实现 5 秒数据刷新和有上限退避重连，禁止忙循环。
5. 测试命令 help、非 TTY、分页、断线、过期响应和协议错误。

验收：`cargo test -p persist-cli` 通过，`persist metrics` 输出不变。

## 阶段 8：Ratatui 全屏界面

新增或修改：

- `crates/persist-cli/src/dashboard/app.rs`
- `crates/persist-cli/src/dashboard/ui.rs`
- `crates/persist-cli/src/dashboard/terminal.rs`
- `crates/persist-cli/src/dashboard/ui_tests.rs`

步骤：

1. 用 RAII guard 管理 raw mode、备用屏幕、光标和 panic hook 恢复。
2. 实现 daemon 汇总、Session 表格、排序、选择和完整性/数据年龄状态。
3. 实现详情趋势和 15 分钟、1 小时、24 小时切换。
4. 为窄终端提供紧凑列和数值摘要，保证文本不重叠。
5. 使用 Ratatui `TestBackend` 做确定性布局测试，使用 PTY 测试正常退出、Ctrl+C、错误和 resize。

验收：导航、绘图、降级和所有终端恢复路径均有自动化证据。

## 阶段 9：性能、文档与发布验证

修改：

- `scripts/benchmark-dashboard.sh`（新增）
- `docs/development/BENCHMARK.md`
- `docs/user/USER_GUIDE.md`
- `docs/user/COMMANDS.md`
- `docs/user/TROUBLESHOOTING.md`
- `docs/known/LIMITATIONS.md`
- `docs/known/KNOWN_ISSUES.md`
- `docs/man/persist.1`
- `README.md`
- `TODO.md`、`MILESTONES.md`、`CHANGELOG.md`、`NEXT_TASK.md`

步骤：

1. 比较关闭采样器基线与 100/1000 个活跃 Session 的 CPU、RSS、窗口和队列状态。
2. 验证 100 Session 平均 CPU 附加开销不超过单核 1%，所有容量硬上限成立。
3. 运行 `cargo fmt --check`、全 workspace test、全 targets Clippy 和现有 benchmark 回归。
4. 构建 tar/deb/RPM，确认新增依赖和 `persist top` help 正确进入发布包。
5. 部署到 `ssh test`，验证实时视图、趋势、重启恢复、退出恢复及核心 Session 回归。
6. 更新全部用户、协议、限制和任务状态文档；形成 M52 验证审计。

验收：设计规范中的所有验收项有命令输出或审计记录，`NEXT_TASK.md` 指向下一唯一任务。

## 提交边界

每个阶段形成独立、可测试的本地提交，不跨阶段混入无关改动。任何 push、release、tag 或远程
仓库操作仍需维护者单独授权。test 主机只在本地完整验证通过后的阶段性里程碑使用。

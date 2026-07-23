# PersistShell Changelog

所有重要变更都记录在本文档。

格式参考 Keep a Changelog。

---

## Unreleased

### Added

- 确认 M57 Attach 历史连续性设计与 ADR：Running Session 使用 Holder Ring，Closed
  Session 在恢复前安全读取有界轮转日志尾部，并保持旧历史、新 prompt、实时输出顺序。
- 实现 M57 Attach 历史连续性：Closed Session 使用 dirfd、`O_NOFOLLOW` 和 fd metadata
  安全读取轮转日志尾部，并通过 1 MiB 有界代理队列分片发送；Running Session 继续使用
  Holder Ring，断线期间输出可在重新 attach 时回放。
- 新增真实 public IPC、SSH 断开、`exit`、空行 `Ctrl+D`、严格输出顺序、512 KiB 跨轮转
  截断及 symlink/FIFO/权限降级回归，不修改 wire、metadata schema 或日志格式。
- 修复 SSH PTY 的 stdin/stdout/stderr 共享 nonblocking 状态时，大回放丢失 partial stdout
  write 且 writer 状态提示因 `EAGAIN` panic 的问题；客户端改为完整输出并在终端关闭时安全
  detach。
- 升级到 0.2.1；Rocky 8 基线 x86_64 RPM/tar.xz 保持 GLIBC 2.28 且各约 1.4 MiB，
  Rocky 9.7 完成 Running/Closed/Ctrl+D、512 KiB、日志关闭和 daemon 重启验证。

## [0.2.0] - 2026-07-20

### Added

- 确认 M56 通用 Linux 多架构发布包设计与 ADR：采用 glibc 2.28 单一 ABI、x86_64/
  ARM64 原生构建、通用 RPM/DEB/tar.xz、包体积硬门禁和旧内核 pidfd fallback。
- 完成 M56 通用打包实现：每架构单次构建，RPM/DEB/tar.xz 使用通用命名，release 启用
  LTO/strip，包体积约 1.4 MiB，并由 3/3.5 MiB 硬门禁限制。
- 新增无 pidfd 内核 fallback，使用 PID、procfs start time 和 zombie 状态验证进程身份；
  x86_64/aarch64 ELF 均保持 GLIBC_2.28，GitHub 原生双架构构建和 17 组
  Rocky/CentOS Stream、Ubuntu/Debian 安装与 Session smoke 已通过。
- 修复 aarch64 的 `libc::c_char` 符号类型编译错误、旧版 DEB 容器的 Bash 执行约束，
  以及 stale socket 测试仅依赖 inode 导致的文件系统复用误判；正式 RPM、DEB 和
  tar.xz 均约 1.3 MiB，证据记录于 M56 审计文档。
- 确认 M55 安全动态环境恢复设计与 ADR：采用内置白名单加用户扩展、不可绕过敏感禁区、
  精确 unset、当前连接变量优先、state helper 原子提交和 Holder capability 降级。
- 确认 M55 八阶段 TDD 实施计划，按共享策略、helper、public attach、PTY unset、
  Holder 兼容、metadata-first、故障安全和平台收尾顺序推进。
- 完成 M55 阶段 1：新增恢复环境配置、共享 allowlist/硬禁区策略、确定性 fingerprint、
  精确 set/unset snapshot、legacy v1/envelope v2 严格兼容和 72 KiB 私有原子状态边界。
- 完成 M55 阶段 2：隐藏 helper 使用继承环境和共享策略采集动态 exported environment，
  支持精确 unset、可信快照保留和 unavailable 降级，并保持三种 Shell 的 M54 hook 契约。
- 完成 M55 阶段 3：public protocol `0.2` 为 Attach 增加有界当前连接上下文，保留 legacy
  4-byte 兼容，并对固定 allowlist、agent socket 和请求级不持久化边界执行双重校验。
- 完成 M55 阶段 4：共享结构化 PTY 启动环境区分 saved set/unset、connection 和 private，
  fork 前统一验证，child 执行真实 unset；Holder Create 提供 minor 1 降级和 v2 精确 codec。
- 完成 M55 阶段 5：Holder 使用 minor 1 基线握手和 minor 2 capability 协商；新协议保留
  envelope v2 退出环境，旧 Holder 探测断线后由 daemon 以同 instance 降级重连且 runtime
  持续运行，legacy SessionExited/GetExitContext 保持 cwd-only wire。
- 完成 M55 阶段 6：`env_snapshot` 保持 schema v7，独立 codec 兼容 legacy map 并只写
  确定性 v2；在线/周期/启动退出均先提交环境再 retire，Closed attach 已通过真实 PTY
  set、恢复、unset 和再次恢复验证。
- 完成 M55 阶段 7-8：跨客户端连接覆盖、敏感值泄漏、故障窗口和三 Shell 验证通过；
  增加原子提交/恢复性能采样、Ubuntu tar/deb、Rocky tar/RPM 与隔离安装验证，证据记录于
  `docs/audit/2026-07-20-m55-dynamic-environment-recovery-validation.md`。
- 确认 M54 最终 Shell cwd side channel 设计与 ADR：bash/zsh/fish 通过私有原子 JSON 状态
  文件提交 cwd，Holder 在 daemon 离线期间保留退出上下文，metadata 成功后才 retire runtime。
- 完成 M54 七阶段实施计划，覆盖安全文件 I/O、Holder 协议、用户 hook 兼容、崩溃对账、
  性能、Ubuntu 26.04/RHEL 9 包和 Rocky test 验证；动态环境恢复明确留给 M55。
- 完成 M54 阶段 1 共享 Shell 状态基础：新增严格 JSON envelope、128-bit runtime identity、
  private `session-state` 目录、dirfd 原子替换、`O_NOFOLLOW` 安全读取和受限清理。
- 完成 M54 阶段 2 Holder 退出上下文协议：minor 1 新增状态身份、最终 cwd、离线查询和
  metadata-first retire wire contract，并通过完整 `persist-ipc` 门禁。
- 完成 M54 阶段 3 Holder runtime：退出后安全读取并保留最终 cwd，支持离线重连查询和显式
  retire；状态缺失、损坏、身份错误或 symlink 均降级为仅保留真实 exit code。
- 完成 M54 阶段 4 隐藏 state helper 与 Bash/Zsh/Fish 私有 hook：实时提交 cwd，不修改用户
  rc，不替换已有 EXIT trap、prompt、precmd/postexec 或 history filter。
- 完成 M54 阶段 5 metadata-first 对账：在线/离线退出先保存 exit code/cwd 再 retire，
  两个 crash window 均可幂等恢复，`/proc` cwd 回退和既有 env 白名单快照保持兼容。
- 完成 M54 最终门禁：正常 `exit`、空行 `Ctrl+D`、快速 `cd; exit`、daemon 离线退出、
  Bash 用户 trap 保护和显式清理通过真实 PTY 测试；Ubuntu 26.04 tar/deb、RHEL 9 tar/RPM
  与 Rocky 9.7 安装验证均通过，证据记录于 M54 审计文档。
- 新增 13 个 Shell 状态测试，覆盖身份、版本、sequence、cwd/文件容量、owner/mode、特殊权限
  位、symlink、非普通文件、原子替换和失败保留；`umask 000` 下仍保持 `0700`/`0600`。
- 确认 M53 单一 per-user `persist-holder` 设计与 ADR，规定 daemon 崩溃期间持续持有并排空 PTY、
  重启后进行有界 inventory 对账，以及显式 stop 与异常断开的严格生命周期边界。
- 完成 M53 九阶段实施计划，按私有协议、安全生命周期、PTY 数据面、daemon 接管、metadata
  对账、兼容迁移、打包和故障注入顺序推进。
- 完成 M53 阶段 1 Holder 私有协议，新增独立 magic/版本/帧头、控制和数据握手、inventory、
  Session 创建与操作事件模型，并拒绝截断、尾随、非法枚举、路径和超限 payload。
- 完成 M53 阶段 2 `persist-holder` 安全生命周期，新增严格 `0700`/`0600` 路径、PID flock、
  stale socket 证明、`SO_PEERCRED` claim、signalfd 退出和控制断线重连；修复旧控制 EOF 与新
  claim 同时发生时错误返回 Busy 的竞态。
- 完成 M53 阶段 3 Holder PTY 数据面：单一 epoll reactor 管理控制/data socket、PTY、signalfd
  和日志 eventfd；支持 Create/Inventory/Close/Kill、读写/只读 attach、takeover、resize、signal、
  exit 和离线 Ring replay，并以有界队列隔离慢客户端及单线程轮转日志。
- 完成 M53 阶段 4 daemon Holder 控制客户端：可信路径自动启动、inotify/pidfd 有界等待、严格
  instance/nonce/request/generation 校验、串行请求、异步事件队列和只读 inventory cache；真实
  测试证明 daemon SIGKILL 后 Holder 保活并可由第二 daemon 接管。
- 完成 M53 阶段 5 Session/public attach 迁移：生产 daemon 不再持有 PTY master、Ring Buffer 或
  Session 日志 writer；现有 public IPC 支持 Holder 读写/只读 attach、takeover、resize、signal、
  Close/Kill、退出和 closed restore，查询与 Dashboard 合并 inventory 和 metadata。
- 新增真实 PTY 崩溃接管测试：命令输入到 Holder 后 `SIGKILL` daemon，Shell 继续执行并输出，
  第二 daemon 接管同一 Holder 后通过只读 attach replay 读取该输出。
- 完成 M53 阶段 6 崩溃重连与 metadata 对账：schema v7 记录 Holder instance/generation，启动在
  public socket 前对 running/exited/missing/orphan 做稳定快照和幂等恢复，运行期周期刷新。
- 新增 debug-only 崩溃注入集成测试，覆盖 create、metadata commit、Shell exit 和 reconcile；
  验证 `lost`、orphan attach 拒绝、离线退出码与日志、重复重启和 Session ID 单调性。
- 修复 Holder 前置启动时 runtime 目录尚未收紧为 `0700`、Holder 日志目录未创建，以及离线退出
  Session 清退错误等待历史退出事件的问题。
- 完成 M53 阶段 7 兼容收尾：Dashboard runtime/writer 统计改以 Holder inventory 为准，Session
  Snapshot/Metrics 和 `persist doctor` 可见日志 degraded 与 `lost` 状态。
- Holder 进程由 pidfd 检测；异常退出后 daemon 清空旧 inventory、将活动 Session 标记 `lost`、
  拒绝 attach 并继续提供只读 metadata 操作，正常停止不再向已退出 Holder 发送关闭请求。
- production `persistd` 不再编译 legacy PTY I/O loop、Ring Buffer 或 Session logger；真实测试新增
  日志 symlink 降级、Holder SIGKILL、Idle GC 及 pinned/locked 豁免覆盖。
- 完成 M53 阶段 8 CLI、打包与升级边界：daemon status/doctor 展示 Holder PID、instance、连接、
  degraded/lost；tar/deb/RPM 安装固定路径 `persist-holder`，release 禁止 PATH/环境覆盖。
- 明确旧架构 running metadata 缺少 Holder 时标记 `lost` 且不伪造热迁移；普通包卸载保留用户
  metadata、历史和日志，并同步完整用户手册、安装、排障、man page 和 GitHub Package workflow。
- 完成 M53 阶段 9 故障、性能和平台验证：新增统一 recovery fault suite 和协议级 attach
  benchmark，覆盖 daemon/Holder SIGKILL、离线 1 MiB 输出、重连、takeover、resize 和 signal。
- Ubuntu 26.04 tar/deb 与 RHEL 9 tar/RPM 原生构建通过，三个 RHEL 9 二进制最高 GLIBC 2.34；
  Rocky test 完成最终 RPM、100/500/1000 Session、卸载保留日志和显式 stop 验证。
- 完成 M52 Performance dashboard 设计与 ADR，确定 `persist top`、Daemon 有界采样 worker、
  1 小时内存趋势、24 小时磁盘聚合及严格资源和隐私边界。
- 新增 Dashboard summary/trend 二进制 IPC，限制每页 128 个 Session、每次 240 个趋势点，并
  拒绝非法游标、枚举、截断、尾随和超限 payload。
- 新增 Dashboard 纯内存速率与聚合模型，首点或计数器回退不伪造零速率，并限制历史为
  64 MiB、1 小时和 720 个时间片，趋势降采样最多 240 点。
- 新增受限 procfs 单次扫描和多 Session 进程树聚合，按最近根 PID 防止重复归属，限制扫描为
  262,144 个 PID 和单文件 4 KiB，并对消失、损坏和权限失败显式降级。
- 新增版本化小时指标分段，使用 CRC32 校验、尾部截断恢复、严格 `0700`/`0600` 权限和
  owner/symlink 检查，并限制为 24 个分段、128 MiB 总容量和 1 MiB 单记录。
- 新增 daemon 内置 Dashboard worker 和独立 writer，以容量 1/2 的非阻塞队列每 5 秒采样，
  单轮 2 秒截止；procfs、分钟落盘、启动恢复和有界退出均不阻塞 accept、PTY 或 GC 路径。
- 新增 daemon Dashboard summary/trend IPC：Session ID 稳定分页，15 分钟/1 小时内存趋势和
  24 小时分段趋势均限制为 240 点；磁盘读取经 writer 串行化，错误返回 Unavailable。
- 新增 `persist top` TTY 命令入口和有界 Dashboard 客户端，校验分页游标、消息类型和 request
  ID，并加入 5 秒刷新/有界重连策略；锁定 Ratatui 0.29/Crossterm 0.28 的兼容 MSRV 依赖链。
- 新增 `persist top` Ratatui 全屏界面，提供 daemon 摘要、Session 稳定排序、15 分钟/1 小时/
  24 小时趋势和紧凑终端降级；退出、连接错误、`Ctrl+C` 和 panic 均恢复终端状态。
- 新增 Dashboard 双二进制性能基准；100 Session 本地附加 CPU 为单核 0.398%，并完成
  1000 Session、全 workspace、Ubuntu tar/deb、RHEL 9 tar/RPM 和 Rocky test 部署验证。
- 新增 `persist ls` TTY 交互选择、`persist ls <id>` 直接菜单和 `--plain` 脚本模式；历史页按
  最新优先每页 50 条显示，并可返回菜单、attach 或退出。
- 新增有界结构化 Shell 命令历史、受限 stdin helper，以及不修改 dotfile 的 bash/zsh/fish
  临时 hook；历史文件限制为 10,000 条或 4 MiB，父目录 `0700`、文件 `0600`。
- 新增单文件完整用户手册，并纳入 tar、deb 和 RPM 标准文档路径。
- GitHub Package workflow 分离 Ubuntu 26.04 与 RHEL 9 ABI 构建，分别上传带平台标识的
  tar/deb 与 tar/`.el9` RPM，并校验 checksum、包内容和 RHEL 9 GLIBC 2.34 上限。

### Fixed

- 修复 `persist doctor` 将安全创建的 `0700` data/state 目录误报为应为 `0755` 的问题。
- 修复 CLI 非阻塞读取将 `EAGAIN` 当 EOF，以及 0x0 terminal resize 被转成非法 Holder 帧后
  断开 data socket的问题；异常/自动化终端现在可正常输入并以 `exit` 释放 runtime。
- 修复 Metrics/Dashboard 在 Holder inventory 刷新间隔内漏报当前 public active writer。
- 修复 Holder 在 `EPOLLIN|EPOLLRDHUP` 同时到达时丢弃最后输入帧，以及 raw fd 快速复用导致旧
  epoll 事件错误关闭新连接的问题；每次注册现使用唯一 token，生命周期并行压力 80 轮通过。
- 清理 `TODO.md` 的历史未同步状态和重复条目，将 daemon、PTY、Session、CLI、诊断及测试中
  已有实现的项目同步为完成，并明确保留真实功能缺口与暂缓项。
- 修复从 `persist ls <id>` 菜单 attach 返回后 stdin 保留 `O_NONBLOCK`、导致菜单读取
  `EAGAIN` 的问题；文件状态 flags 现在由 RAII 守卫恢复。
- zsh/fish 检测到自定义 history 过滤器时优先保留用户配置并明确降级，不重复调用或绕过过滤
  逻辑；历史写入拒绝符号链接、超限和损坏文件。
- shell 自然退出后向 writer/read-only 客户端发送 `SessionExited`，并修复 read-only stdout
  裸字节广播、attach ring replay 缺失和超大 replay frame 未分片问题。
- PTY child 在 exec 前恢复 daemon 忽略的交互信号，Ctrl+C 可再次终止前台进程。
- Session stdout 重新接入异步日志；Idle GC 现在同步关闭 metadata，不再留下 running 幽灵记录。
- CLI help 补齐 lock/unlock/log export，并统一命令展示。
- 打包脚本规范化自定义输出目录，修复 `PERSIST_PACKAGE_DIST` 为绝对路径时 RPM 产物定位失败。

## [0.1.0] - 2026-07-15

### Added

- 完成 M50 发布就绪审计和维护者 release checklist；明确区分已验证的本地/test 包与尚需授权的
  版本、tag、push、GitHub workflow、签名和公开发布操作。

- 实现 M49 用户文档完善：新增故障排查文档，完成命令、配置、安装和 FAQ 与当前实现的一致性审计。

- 实现 M48 bash/zsh/fish completion，动态 Session ID 仅读取 `persist ls` 且失败时静默回退。
- 新增补全定向验证脚本，并将三个 completion 纳入 tarball、deb 与 rpm 标准路径。

- 实现 M47 Unix man page：新增 `persist(1)`、`persistd(1)`，并纳入 tarball、deb 和 rpm。
- 本机完成 groff/man/tar/deb 验证，test Rocky 完成原生 rpm 与压缩 man page 验证。

- 实现 M46 发布打包入口，支持 Linux tarball、Debian `.deb`、RPM `.rpm` 与 SHA-256 checksum。
- GitHub Actions package workflow 复用该入口，在 Ubuntu runner 安装 `rpm` 后上传全部 release artifact。
- 本机验证 tarball/deb，test Rocky 主机原生验证 rpm；GitHub workflow 尚待 mirror 同步触发。

### Fixed

- GitHub CI 显式安装 zsh 和 fish，确保 PTY shell 兼容性测试不依赖 hosted runner 的预装软件。

- 修复 Closed Session 恢复上下文在部分 `/proc` 捕获后可能丢失已有环境快照的问题；关闭时逐字段
  合并 cwd 和允许恢复的环境。

- 修复 release checksum 中的 `dist/` 相对路径，使下载后的 GitHub artifact 可独立校验。

- 实现 M45 兼容性矩阵脚本，覆盖当前 Ubuntu bash/zsh/fish 与 Rocky bash 的 daemon/new/list/close 基线。

- 实现 M43 Session Benchmark，新增隔离 XDG 的 `scripts/benchmark-sessions.sh`。
- 基准覆盖 100/500/1000 Session 的创建、列表和关闭路径，使用 1 KiB ring buffer 且关闭 session log 以排除可配置 buffer 开销。

- 实现 M42 基础 Metrics。
- CLI 新增 `persist metrics`，以最多 16 KiB 的 JSON 返回 daemon PID 和 Session 聚合计数。
- Metrics 不启动 metrics server、不保存采样历史，也不暴露 Session 内容或敏感数据；自动化测试执行延后。

- 实现 M41 Session Snapshot。
- CLI 新增 `persist snapshot <id>`，以最多 16 KiB 的 JSON 输出 metadata、writer、日志路径与前台进程摘要。
- Snapshot 不暴露环境变量、输入内容、SSH agent 路径或 note/tag 实际内容；未知 Session 与超限快照返回非零错误。
- 增加 M41 命令解析和快照 JSON 长度边界测试；自动化测试执行延后。

- 实现 M37 Foreground Process Tracking。
- `persist ls` 新增 `FOREGROUND` 列，展示运行中 Session 前台进程的命令摘要。
- daemon 使用 `tcgetpgrp()` 与 `/proc/<pid>/comm`、`cmdline` 获取前台进程信息；读取失败不影响列表。
- IPC `SessionEntry` 增加前台 PID、名称和命令行字段，并补齐 PTY、IPC、daemon 与 CLI 测试。

- 实现 M14 Closed Session 冷恢复。
- MetadataStore schema v6 新增 `env_snapshot`，关闭时保存 cwd、受限启动环境快照和 exit code。
- closed Session 再次 attach 会以原 Session ID 创建新的 PTY Shell，恢复 cwd 和允许环境，并恢复为可写 `running` 状态。
- 新增 schema/PTY/daemon/foreground 进程集成测试覆盖恢复路径。
- 仅恢复 `TERM`、`COLORTERM`、`LANG` 和 `LC_*`；动态 shell export 的限制已记录。

- 实现 M36 Session Lock。
- MetadataStore schema v5 新增持久化 `locked` 状态，支持 lock/unlock 更新。
- IPC 新增 `LockSet` / `LockSetResp` 与 Session 列表锁定 flag。
- CLI 新增 `persist lock <id>` 和 `persist unlock <id>`，`persist ls` 显示 LOCK 列。
- 锁定 Session 会拒绝读写 attach 和 kill，Idle GC 跳过锁定 Session。
- 新增锁定状态、IPC 编解码、CLI 参数、IPC attach 拒绝和 GC 的测试覆盖。

- 实现 M35 多 active writer 协作。
- IPC 新增 `WriteRequest`、`WriteGranted`、`WriteRevoked` 控制消息。
- 新 RW client attach 时通知并撤销旧 writer，随后获得写入权。
- Daemon 按 active writer fd 校验 `STDIN`，旧连接的延迟输入不会写入 PTY。
- 新增双客户端交接集成测试，并修复 PTY 大输出测试的时序问题。

- 实现 M34 Read-only Attach（只读查看实时输出）。
- CLI `persist attach --readonly <id>` / `persist attach -r <id>`。
- 新增 IPC `AttachReadOnly` 消息类型。
- Daemon 侧 `SessionManager` 支持多 RO 客户端注册、取消注册、Stdout 广播。
- RO 客户端不进 raw mode、不读 stdin、不发 Resize。
- 单元测试覆盖 RO 命令解析。

- 实现 M33 Replay Mode MVP（Session 历史输出回放）。
- CLI `persist replay <session-id> [--tail <n>] [--head <n>] [--speed <f>] [--follow]`。
- `replay_session()` 从日志文件读取并按条件输出。
- `--tail N` 只显示最后 N 行，`--head N` 只显示前 N 行。
- 命令解析 + 回放逻辑共 9 个单元测试。

- 实现 M32 更完善 doctor 诊断工具。
- 新增 `doctor_check_pty()` — 检查 PTY 可用性。
- 新增 `doctor_check_shell_hook()` — 检测 Shell hook 安装状态。
- 新增 `doctor_check_dir_perms()` — 检查目录权限。
- 新增 `doctor_check_daemon_health()` — 增强版 daemon 健康检查（IPC hello）。
- 新增 `doctor_check_config_sanity()` — 配置合理性检查。
- 3 个 doctor 单元测试。

- 实现 M31 Idle GC MVP（空闲 Session 自动清理）。
- `DaemonConfig` 新增 `gc_idle_timeout` / `gc_interval` 配置项（默认 0 表示禁用）。
- `SessionManager` 新增 `gc_idle_timeout`、`set_gc_idle_timeout()`、`gc_run()`。
- `gc_run()` 自动清理超过空闲阈值的 Session（跳过 attached / pinned）。
- `persistd foreground --idle-timeout <duration>` 启动时自动运行 GC 线程。
- `remove()` 方法增强：清理 ring buffer、log handle、last_activity。
- 5 个 GC 单元测试（零超时不清理/跳过 attached/跳过 pinned/清理空闲/保留活跃）。

- 实现 M30 Idle Detection MVP（IPC + Server + CLI）。
- IPC `SessionEntry` 新增 `idle: String` 字段（flags bit 0x20），编码/解码兼容。
- `SessionManager` 新增 `last_activity: HashMap<u32, Instant>`、`record_activity()`、`idle_string()`。
- `io_loop` 在 stdin 读取和 stdout 写入后自动调用 `record_activity`。
- `handle_client` 中 Stdin handler 写入后自动调用 `record_activity`。
- `persist ls` 新增 `IDLE` 列显示空闲时长（如 `2m30s`）。
- 3 个空闲检测单元测试。

- 实现 M29 独立 History MVP。
- `PtyEngine::open_session_with_shell()` 新增 `histfile: Option<&str>` 参数。
- `child_setup()` 在 `execvp` 前设置 `HISTFILE` 环境变量，使每个 Session 拥有独立命令历史文件。
- `SessionManager` 新增 `history_dir` 字段，创建 Session 时自动创建 history 目录并传入 `HISTFILE` 路径。
- 新增 `pty_histfile_env_set` 单元测试验证 HISTFILE 正确设置。
- 新增 `session_histfile_is_created` 单元测试验证 daemon 创建 history 目录。

- 实现 M28 Session 日志导出 MVP。
- CLI `persist log export <session-id> [--output <path>]` 命令。
- `export_session_log()` 函数：读取 Session 日志文件并输出到 stdout 或指定文件路径。
- 4 个单元测试：stdout 输出/文件导出/Session 不存在/日志关闭状态。

- 实现 M27 Session 日志搜索 MVP。
- CLI `persist log search <keyword>` 命令，支持 `--session <id>` 限定搜索范围，`-i` / `--ignore-case` 大小写不敏感。
- `log_search()` 函数：遍历 Session 输出日志文件，逐行匹配关键字，输出 Session ID、行号、匹配行。
- 5 个单元测试：搜索匹配/大小写不敏感/限定 Session/无匹配/空日志目录。

- 实现 M26 Pin Session MVP。
- MetadataStore: 新增 `pinned` 列（version 4），`set_session_pinned` 方法，`SessionRecord.pinned` 字段。
- IPC 新增 `PinSet`（0x0028）/`PinSetResp`（0x0029）消息类型，`PinPayload` 及 encode/decode。
- `SessionEntry` 新增 `is_pinned: bool`（flag bit 0x10），编码/解码兼容。
- Daemon 新增 PinSet IPC handler，ListSessions/ListSessionsByTag 填充 `is_pinned`。
- CLI `persist pin <id>` / `persist unpin <id>` 命令。
- `persist ls` 新增 PIN 列显示 pinned 标记。
- 5 个 MetadataStore 新增单元测试（pin/unpin/not-pinned/list/错误路径）。
- 4 个 CLI 命令解析新增测试（pin/unpin/缺少参数）。
- 1 个 Daemon IPC 新增测试（client_pin_set_flow：pin/unpin/列表验证）。
- 补全 CLI 集成测试：所有 daemon-required 命令统一验证 E_IO 连接错误输出。
- 新增 Daemon IPC 集成测试：`client_note_set_get_flow`（note set/get/clear 完整流程），`client_tag_add_list_remove_flow`（tag add/list/remove/ListSessionsByTag 完整流程）。

- 实现 M25 Session Tags MVP。
- MetadataStore: 新增 `session_tags` 表（version 3），`add_session_tag`/`remove_session_tag`/`list_session_tags`/`find_sessions_by_tag`/`session_has_tag` 方法。
- IPC 新增 `TagAdd`（0x0021）/`TagAddResp`/`TagRemove`/`TagRemoveResp`/`TagList`/`TagListResp`/`ListSessionsByTag`（0x0027）消息类型。
- 新增 `TagPayload`、`TagListRespPayload` 及 encode/decode 函数。
- `SessionEntry` 新增 `has_tags: bool`（flags bit 0x08），编码/解码兼容。
- Daemon 新增 tag IPC handlers：TagAdd/TagRemove/TagList/ListSessionsByTag。
- CLI `persist tag <id> add|remove|list [<tag>]` 命令。
- `persist ls --tag <tag>` 按标签筛选（通过 `ListSessionsByTag` IPC）。
- `persist ls` 新增 TAGS 列显示标签标记。
- 8 个 MetadataStore 新增单元测试（add/remove/list/find/has_tag/校验）。
- 6 个 CLI 命令解析新增测试（tag add/remove/list/错误路径）。
- Clippy 零新增警告，fmt 一致。

- 实现 M24 Session Notes MVP。
- `MetadataStore::set_session_note` 方法 + SQL 迁移（version 2，添加 `note` 列）。
- `SessionRecord` 新增 `note: Option<String>` 字段。
- IPC 新增 `NoteSet`（0x001D）/ `NoteSetResp`（0x001E）/ `NoteGet`（0x001F）/ `NoteGetResp`（0x0020）消息类型。
- 新增 `NotePayload`、`encode_note`/`decode_note`、`encode_note_get_resp`/`decode_note_get_resp`。
- `SessionEntry` 新增 `has_note: bool` 字段，编码/解码兼容扩展。
- Daemon NoteSet/NoteGet handler：通过 MetadataStore 持久化/读取备注。
- CLI `persist note <id> [<text>]` 命令：无 text 查看备注，有 text 设置备注。
- `persist ls` 在列表中添加 NOTE 列显示备注标记。
- 5 个 MetadataStore 新增单元测试（note 默认值、set、clear、不存在错误、list 持久化）。
- 所有 15 个 metadata 测试、3 个 persistd 测试、15 个 CLI 命令解析测试通过。
- Clippy 零新增警告，fmt 一致。

- 实现 M18 CLI 基础命令 MVP。
- `persist doctor` — 完整诊断命令，检查 daemon 状态、socket 权限、目录结构、日志配置，输出检查和修复建议。
- `persist rename <id> <name>` — 重命名 Session（IPC Rename/RenameResp 协议，MetadataStore.rename_session）。
- `persist attach <id>` — attach 到指定已存在 Session（跳过 NewSession，直接 ATTACH）。
- IPC 新增 Rename（0x0017）/ RenameResp（0x0018）消息类型 + encode/decode。
- MetadataStore 新增 `rename_session` 方法。
- help 文本更新：`rename` 从 Planned 移到可用命令群。
- Clippy 零警告，112 个单元+集成测试通过。

- 实现 M20 基础兼容性测试 MVP。
- 新增 PTY 集成测试（8 个）：bash echo/pipe/多命令/重定向，zsh echo，fish echo/variable。
- 新增 `run_pty_test` 测试辅助函数：创建 daemon → socket 连接 → HELLO → NEW_SESSION → ATTACH → 运行测试闭包。
- 新增 `raw_write_frame` / `poll_for_output` 辅助函数用于 PTY 测试。
- 添加 `tempfile` dev-dependency。
- `write_frame_raw` 帧格式修复：使用大端序（BE）匹配标准 IPC 格式。
- `read_nonblock` 修复：移除 POLLHUP 提前检查，避免 `poll` 返回 `POLLIN|POLLHUP` 时丢失数据。
- Clippy 零警告，121 个单元+集成测试通过。

- 实现 M21 基础压力测试 MVP。
- 新增 `stress_multi_session_concurrent`：15 个并发 Session，每个执行独立 echo 命令，验证全部并行完成。
- 新增 `stress_large_output`：通过 `dd|wc -c` 向 PTY 输出 1.5MB 数据，验证大输出转发不丢失。
- 新增 `stress_large_output_pv`：通过 shell loop 生成 1MB 数据，验证 shell 构造的批量输出处理正确。
- 新增 `stress_frequent_attach_detach`：对单个 Session 反复 attach/detach 20 次，验证 io_loop 正确释放和重新接管。
- 所有 31 个测试通过（30 unit + 3 integration），1 ignored。
- Clippy 零警告，fmt 一致。

- 实现 M22 Signal 处理 MVP。
- IPC 新增 `Signal`（0x001B）/ `SignalResp`（0x001C）消息类型。
- 新增 `SignalPayload` 结构体（session_id + signal 信号编号）。
- 新增 `encode_signal` / `decode_signal` 序列化函数。
- Daemon `io_loop` Signal 处理：接收 Signal 帧 → `tcgetpgrp(master_fd)` 获取前台进程组 → `kill(-pgid, signal)` 转发信号。
- Daemon `handle_client` Signal 处理：从非 attach 上下文发送信号到会话前台进程组。
- 新增 `stress_signal_sigint_via_ipc` 测试：通过 IPC Signal 发送 SIGINT，验证 `trap 'echo TRAPPED_INT' INT` 触发。
- 新增 `stress_signal_sigtstp_via_ipc` 测试：通过 IPC Signal 发送 SIGTSTP，验证 `trap 'echo TRAPPED_TSTP' TSTP` 触发。

- 实现 M23 自动 Session 命名 MVP。
- `NewSessionRespPayload` 新增 `name` 字段，编码/解码兼容扩展。
- `SessionManager` 新增 `session_info: HashMap<u32, SessionInfo>` 存储会话名称和 shell 路径。
- `generate_session_name` 函数：格式 `<shell>@<cwd>`（如 `bash@persistshell`、`zsh@~`）。
- `create_with_shell` 创建 Session 时自动生成名称。
- `ListSessions` 和 `NewSession` handler 使用存储的名称替代占位符 `session-{id}`。
- 新增 `generate_session_name_uses_shell_and_cwd` 单元测试。
- 新增 `session_manager_create_remove_list` 名称断言。
- 新增 `client_hello_new_session_list_sessions_detach` 名称一致性验证。
- 所有 31 个 persistd 测试通过（+1 新测试 `generate_session_name`）。

- 实现 M19 CLI 补全与改进 MVP。
- `persist detach <id>` — 远程分离命令（IPC DetachSignal/DetachSignalResp 协议，复用 takeover pipe 机制）。
- `completions/persist.bash` — bash 补全脚本（子命令 + session ID 动态补全）。
- Daemon crash prompt — client 连接断开时打印 `[daemon disconnected — session preserved]`。
- `persist ls` 输出改进 — 固定宽度列对齐、列标题、分隔线。
- 移除所有 `Planned` 命令（全部命令已实现）。
- Clippy 零警告，114 个单元+集成测试通过。

- 实现 M16 SSH 自动接管 MVP。
- 创建 `crates/persist-cli/src/installer.rs`：`install`（注入 shell hook）、`uninstall`（移除 hook）、`purge`（删除所有数据）。
- Shell hook 脚本：检测 `SSH_TTY`（交互式 SSH）、`PERSIST_DISABLE` 绕过、`command -v persist` 检查，自动执行 `persist attach`。
- 支持 bash（`.bashrc` / `.bash_profile`）和 zsh（`.zshrc`），auto-detect via `$SHELL`。
- Hook 注入 idempotent：marker guard 防止重复安装，精准移除。
- CLI `persist install` / `persist uninstall [--purge]` 命令（之前是 Planned）。
- 4 个单元测试：profile detection、hook roundtrip、no-hook detection、script content。
- 实现 M15 多电脑可写 attach (takeover) MVP。
- `SessionManager` 新增 `attached_sessions: HashMap<u32, RawFd>` 跟踪哪个 Session 正被 client 使用（io_loop 中）。
- `io_loop` 新增 `takeover_fd` 参数（pipe read end），与 socket_fd、pty_fd 一同 poll。
- `handle_client` ATTACH 路径：检测 `is_attached` → 调用 `signal_takeover`（写 pipe）→ 等待原 io_loop 释放 → 创建新 pipe → `mark_attached` → io_loop。
- `close_session`/`kill_session` 调用 `clear_attached` 清理 pipe fd。
- pipe 唤醒机制：`libc::pipe()` 创建一对 fd，takeover 信号通过写入 pipe 唤醒 poll。
- 单元测试全部通过，clippy 零警告。
- 实现 M14 Closed Session 恢复 MVP。
- `io_loop` 检测 PTY EOF：`read_output` 返回 `Ok(0)` 或 POLLHUP 时调用 `wait_exit()` 获取退出码，发送 `SessionExited` 帧给 client，返回 `Result<Option<i32>>`。
- `handle_client` ATTACH 路径：`io_loop` 返回退出码后调用 `metadata_store.close_session()` 持久化退出码和关闭时间，从 SessionManager 移除（不推回）。
- `PtySession::exit_code()` 新增公共方法，返回缓存的退出码。
- `SessionEntry` 扩展：新增 `exit_code: Option<i32>` 和 `closed_at: Option<String>` 字段。
- `encode_list_sessions_resp` / `decode_list_sessions_resp`：兼容扩展，flags 字节控制可选字段。
- `ListSessions` handler：合并 SessionManager 活跃会话和 MetadataStore 已关闭会话。
- `persist ls`：Closed 会话显示退出码和关闭时间。
- 实现 M13 Session 输出日志 MVP。
- 创建 `crates/persistd/src/log_writer.rs`：`SessionLogHandle`（cloneable channel sender），`spawn_session_logger`（后台线程，即时写入 + 轮转 + 0600 权限）。
- `SessionLogFileWriter`：`open` 创建/追加文件，`write_all` 写入并触发轮转，轮转时 shift .1→.2→.N，移除 .N+1。
- `rotate_path`：追加 `.N` 到已有扩展名。
- 配置集成：使用 `config.logging.session_log` 控制开关，`max_file_size` / `max_files` 控制轮转。
- Daemon `SessionManager`：新增 `log_handles: HashMap<u32, SessionLogHandle>`，create/close/kill 时自动管理。
- `io_loop` 接受 `Option<SessionLogHandle>`，PTY 输出同时写入日志文件、ring buffer 和发送给 client。
- CLI `persist log <id>`：直接读取本地日志文件显示（无需 IPC）。
- 单元测试（4 个）：写入/轮转、多次轮转、权限设置、路径生成。
- 修复 clippy 警告：移除冗余 `.write(true)`（`append(true)` 已隐含），折叠嵌套 if。
- 实现 M12 Ring Buffer MVP。
- 创建 `persist-core/src/ringbuf.rs`：`RingBuffer` 固定大小字节环形缓冲区。
- `RingBuffer` 支持：`write`、`read_all`、`read_replay`、`len`、`capacity`。
- 配置集成：使用 `config.ring_buffer.default_size` 作为大小，`config.ring_buffer.replay_bytes` 控制 replay 量。
- Daemon `SessionManager`：新增 `ring_buffers: HashMap<u32, Arc<Mutex<RingBuffer>>>`，create/close/kill 时自动管理。
- `io_loop` 接受 `Option<Arc<Mutex<RingBuffer>>>`，PTY 输出同时写入 ring buffer 和发送给 client。
- Attach 时：先回放 ring buffer（最多 replay_bytes），再发送 ATTACH_RESP，然后进入实时流。
- 单元测试（11 个）：写入、精确装满、覆盖旧数据、大写入、replay 截断、空 buffer、空写入等。
- 实现 M11 Metadata Store MVP。
- 创建 `persist-metadata` crate：`store` 子模块（SQLite 后端），`schema` 子模块（版本管理）。
- `MetadataStore` 接口：`open`、`open_in_memory`、`create_session`、`get_session`、`update_status`、`close_session`、`list_sessions`、`list_sessions_by_status`。
- SQLite schema：`sessions` 表（session_id, name, status, created_at, updated_at, closed_at, cwd, shell, exit_code）+ `schema_version` 表。
- Schema migration 自动执行（`execute_batch`）。
- ISO 8601 时间戳生成器（纯 Rust，无外部依赖）。
- Daemon 集成：`run_foreground` 打开 `$data_dir/sessions.db`，`handle_client` 接收 `Option<Arc<Mutex<MetadataStore>>>`，NEW_SESSION 持久化，CLOSE/KILL 更新状态和退出码。
- `PersistError` 新增 `MetadataOpen`、`MetadataOperation` 变体。
- 单元测试（10 个）：open、create/get、缺失查询、update_status、close+exit_code、list_all、list_by_status、多 ID、ISO 格式。
- 实现 M10 Session Manager CLI。
- IPC: 新增 Kill/CloseResp/KillResp 消息类型，新增 OpRespPayload 编码解码。
- Daemon SessionManager: 新增 close_session/kill_session，将 PtySession 移出锁范围后清理。
- persist-cli: 新增 `session` 模块，实现 `persist new`（创建）、`persist ls`（列表）、`persist close <id>`（关闭）、`persist kill <id>`（强制终止）。
- persist-pty: PtySession 新增 `signal_child(sig)` 方法。
- 集成测试：`client_new_list_close_kill_flow`（全流程验证）。
- CLI help 更新：new/ls/close/kill 移至 Available now。
- 实现 M09 Signal & Resize MVP。
- Client SIGPIPE 忽略（`libc::signal(SIGPIPE, SIG_IGN)`）。
- Client SIGWINCH 处理（AtomicBool flag + io_loop 中检查 + RESIZE 帧发送）。
- Daemon SIGPIPE 忽略（加入 `lifecycle.rs` ignore 列表）。
- `persist-cli/src/attach.rs::send_resize()`：TIOCGWINSZ + encode_resize + write_frame。
- 集成测试：`client_attach_resize_and_stdin_stdout_flow`（RESIZE 帧 + STDIN/STDOUT 验证）。
- 实现 M08 Client Attach MVP。
- 新增 IPC 消息类型：NewSession/NewSessionResp、ListSessions/ListSessionsResp、Attach/AttachResp、Detach、Stdin、Stdout、Resize、SessionExited。
- 新增 `FrameAccumulator`：支持从字节流非阻塞解析 Frame。
- 新增 payload encode/decode 函数：attach、detach、resize、session_exited、list_sessions、new_session_resp。
- 重写 Daemon server：accept 循环改为 thread-per-connection，新增 `SessionManager`（创建/列出/移除/放回）。
- 实现 I/O 转发循环（io_loop）：poll(socket_fd, pty_fd) + FrameAccumulator + PTY read/write。
- 实现 Client `persist attach`：连接 → HELLO → NEW_SESSION → ATTACH → raw mode → I/O 循环 → DETACH。
- 实现 Terminal raw mode（`persist-cli/src/terminal.rs`）：tcgetattr 保存 → cfmakeraw 等效设置 → Drop 恢复。
- 集成测试：`client_hello_new_session_list_sessions_detach`（协议层完整流程）、`session_manager_create_remove_list`。
- 暴露 PtySession::master_fd() getter。
- 更新 CLI help：attach 移至 Available now。
- 实现 M07 PTY Engine MVP。
- 新增 `persistd lifecycle` 模块：PID 文件管理（`PidFile` + `flock` 单实例锁）、信号处理（SIGTERM 优雅退出、SIGINT/SIGHUP/SIGQUIT 忽略）。
- 新增 `persistd foreground` 子命令：加载配置、初始化日志、创建 PID 文件、绑定 Unix Socket、accept 循环、优雅关闭。
- 新增 `persist-cli daemon` 模块：`daemon start`（子进程启动 + 2s 超时轮询）、`daemon stop`（SIGTERM + 5s 超时 + SIGKILL fallback）、`daemon status`（PID 文件 + /proc 检查 + uptime）。
- 新增 `persist-core pidfile` 模块：`read_pid`、`is_process_alive`、`is_running`、`send_signal` 公共函数。
- 添加 `PersistError::DaemonNotRunning` 和 `PersistError::DaemonAlreadyRunning` 错误变体及构造器。
- 添加 8 个 lifecycle 单元测试（PID 文件创建/锁定/重入/进程检查）+ 更新 3 个错误测试。
- 更新 CLI 帮助输出，在 Available now 中显示 daemon 命令。
- 持久化 daemon 日志到 `$runtime_dir/daemon.log`。
- 实现 M05 Unix Socket IPC 雏形。
- 创建 `persist-ipc` crate，实现二进制 Frame 协议（HEADER_SIZE=12）。
- 实现 `Frame` 类型：`[u32 BE length][u16 BE type][u16 BE flags][u32 BE request_id][payload]`。
- 实现 `write_frame`/`read_frame`：完整帧读写、分段读取、大帧拒绝（MAX_CONTROL_FRAME=1MB）。
- 实现 HELLO/HELLO_ACK 握手协议及 payload 编码解码。
- 实现协议版本协商（`ProtocolVersion::CURRENT = 0.1`，major 必须匹配）。
- 实现 `DaemonSocket`：bind、listen（backlog=128）、accept（SO_PEERCRED）、receive_hello、send_ack。
- 实现 `ClientSocket`：connect（超时 5s）、send_hello。
- 实现权限检查：socket 文件 0600、目录 0700、peer credential uid 校验。
- 实现 socket 清理（显式 `cleanup()` 和 `Drop`）。
- 连接超时处理：DEFAULT_CONNECT_TIMEOUT=5s，DEFAULT_HANDSHAKE_TIMEOUT=10s。
- 添加 17 个 persist-ipc 单元测试（协议 11 个 + socket 6 个），全部通过。
- 引入 `libc` 依赖用于 SO_PEERCRED。
- 添加 `PersistError::Internal` 变体及 `internal_error()` 构造器。
- 更新 NEXT_TASK.md 从 M05 到 M06（Daemon 基础生命周期）。
- 实现 M04 统一错误处理框架。
- 定义 `ErrorCode` 枚举，含 29 种稳定错误码（`E_INVALID_ARGUMENT`, `E_CONFIG_PARSE` 等）。
- 定义 `ErrorKind` 错误分类（UserError/EnvironmentError/SyscallError/ProtocolError/InternalError）。
- 实现 `exit_code()` 退出码映射（UserError=1, EnvironmentError=2, SyscallError=3, ProtocolError=4, InternalError=5）。
- 实现 `user_facing()` 统一用户可见错误输出格式。
- 实现 `suggestion()` 修复建议方法。
- 统一 `persist` CLI 和 `persistd` 错误输出格式。
- 添加错误码、错误分类、退出码映射和用户输出格式的单元测试（13 个）。
- 添加 CLI 和 Daemon 错误输出集成测试（4 个）。
- 更新 `docs/development/ERROR_HANDLING.md` 文档，包含完整错误码表、退出码映射、API 文档。
- 初始化 Rust Cargo workspace，新增 `persist` CLI、`persistd` daemon 骨架和 core/pty/ipc/metadata crate 边界。
- 添加 Rust fmt、clippy、test 验证，以及 GitHub Actions CI/package workflow。
- 添加基础错误、配置路径、日志初始化和 Session 状态模型。
- 添加基础配置系统，支持默认值、系统配置、用户配置、TOML 解析、配置校验和 `persist config show`。
- 添加基础内部日志框架，支持 `internal_log` 配置、文件日志初始化、级别过滤、权限设置和敏感关键词最小脱敏。
- 添加 GitHub Actions CI 与发布包构建要求，记录 GitHub 镜像仓库 `https://github.com/SuPerCxyz/persistshell`。
- 初始化 PersistShell 项目文档体系。
- 添加 README.md。
- 添加项目原则文档。
- 添加产品哲学文档。
- 添加非目标文档。
- 添加路线图。
- 添加里程碑。
- 添加 TODO。
- 添加 NEXT_TASK。
- 添加架构设计文档。
- 添加开发规范文档。
- 添加协议文档。
- 添加用户文档。
- 添加已知问题和限制文档。
- 添加 ADR 模板。

### Changed

- 将 M00/M01 标记完成，下一任务更新为 M02 基础配置系统。
- 确定 PersistShell 主开发语言为 Rust，并同步 Agent 规则、开发规范和目录结构文档。
- 调整 Session 退出语义：`exit`/`Ctrl-D` 进入 Closed 状态，释放 shell runtime，但保留可恢复的输出、cwd、环境变量快照和 metadata。
- 明确另一台电脑可以 attach 到已有 Session 并获取可写操作权；只读 attach 只是可选模式，不是跨电脑进入会话的唯一方式。
- 将 M02/M03 标记完成，下一任务更新为 M04 错误处理框架。
- 将 M04 标记完成，下一任务更新为 M05 Unix Socket IPC 雏形。
- 将 M05 标记完成，下一任务更新为 M06 Daemon 基础生命周期。
- 将 M06 标记完成，下一任务更新为 M07 PTY Engine MVP。
- 将 M09 标记完成（Signal & Resize MVP），下一任务更新为 M10 Session Manager CLI。
- 将 M10 标记完成（Session Manager CLI），下一任务更新为 M11 Metadata Store MVP。
- 将 M11 标记完成（Metadata Store MVP），下一任务更新为 M12 Ring Buffer MVP。
- 将 M12 标记完成（Ring Buffer MVP），下一任务更新为 M13 Session 输出日志。
- 将 M13 标记完成（Session 输出日志 MVP），下一任务更新为 M14 Closed Session 恢复。
- 将 M14 标记完成（Closed Session 恢复 MVP），下一任务更新为 M15 多电脑可写 attach。
- 将 M15 标记完成（多电脑可写 attach MVP），下一任务更新为 M16 SSH 自动接管。
- 将 M16 标记完成（SSH 自动接管 MVP），下一任务更新为 M17 非交互兼容。
- 将 M17 标记完成（非交互兼容），下一任务更新为 M18 CLI 基础命令。

### Fixed

- 修复 installer profile 测试依赖 runner `SHELL` 和真实 HOME 的问题，改用隔离临时目录，避免
  GitHub Actions 在无 `SHELL` 环境下误报失败。
- 修复 CLI 集成测试依赖 runner `XDG_RUNTIME_DIR` 或 `UID` 的问题，所有子进程改用测试专属
  XDG 目录，避免 runner 环境差异和宿主 daemon 干扰。
- 修复 zsh history 集成测试用固定延时猜测 Shell 就绪状态的竞态，在并行 PTY 压力下使用
  有界 30 秒 ready 截止、排空受限 PTY 输出，并隔离 runner 的 `ZDOTDIR` 与全局 zsh rc。
- 完成 M44 安全审查：收紧 metadata、session log 和 stale socket 清理边界，并补齐权限测试。
- `persist daemon start` 现在显式以 `0600` 创建并重设 runtime daemon log 权限，不再依赖 umask。
- 修复 ClientSocket 将 5 秒握手超时保留到后续操作的问题；高负载 `persist new` 不再因该超时误报 `EAGAIN`。

- PTY 集成测试修复：`write_frame_raw` 改为大端序（BE）以匹配标准 IPC 帧格式；`read_nonblock` 移除 POLLHUP 提前检查，避免 `poll` 返回 `POLLIN|POLLHUP` 时丢失数据后再返回 Err。
- `pty_zsh_pipe_command` 忽略：Ubuntu `/etc/zsh/zshrc` 定义的终端函数在测试 PTY 环境中失败。
- 集成测试 `persistd_unknown_command_*` 断言修复：匹配实际错误格式 `"invalid argument"` 而非 `"E_INVALID_ARGUMENT"`。
- Clippy lint 修复：Resize handler 中 `for..break` 替换为 `if let Some`。

### Removed

- 无。

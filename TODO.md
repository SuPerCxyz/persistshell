# PersistShell TODO

本文件是 PersistShell 项目的统一 TODO 列表。

状态标记：

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[-] 暂缓
[!] 阻塞
```

任何开发者或大模型新增任务时，必须写入本文件。

不得绕过 TODO.md 直接实现新功能。

---

## Phase 0：项目准备

### 文档体系

- [x] 创建 README.md
- [x] 创建 PROJECT_PRINCIPLES.md
- [x] 创建 PRODUCT_PHILOSOPHY.md
- [x] 创建 NON_GOALS.md
- [x] 创建 ROADMAP.md
- [x] 创建 MILESTONES.md
- [x] 创建 NEXT_TASK.md
- [x] 创建 TODO.md
- [x] 创建 CHANGELOG.md
- [x] 创建 CONTRIBUTING.md
- [x] 创建 SECURITY.md
- [x] 创建 SUPPORT.md
- [x] 创建 CODE_OF_CONDUCT.md
- [x] 创建 ADR 模板
- [x] 创建 docs 目录结构
- [x] 创建 architecture 文档
- [x] 创建 development 文档
- [x] 创建 protocol 文档
- [x] 创建 benchmark 文档
- [x] 创建 user 文档
- [x] 创建 known 文档

### 工程初始化

- [x] 选择主要开发语言：Rust
- [x] 初始化 Cargo workspace
- [x] 确定 Rust MSRV
- [x] 初始化构建系统
- [x] 初始化测试框架
- [x] 初始化 GitHub Actions CI
- [x] 初始化 GitHub Actions package workflow
- [x] 初始化 rustfmt
- [x] 初始化 clippy
- [x] 初始化目录结构
- [x] 创建最小 CLI 骨架
- [x] 创建版本命令
- [x] 创建基础错误处理框架
- [x] 创建基础配置加载框架
- [x] 创建内部日志框架

---

## Phase 1：MVP Core

### 配置系统

- [x] 支持默认配置
- [x] 支持用户配置
- [x] 支持系统配置
- [x] 支持配置校验
- [x] 支持打印当前配置
- [x] 支持安全默认值

### 错误处理框架

- [x] 定义 ErrorCode 枚举 (29 种稳定错误码)
- [x] 定义 ErrorKind 分类 (User/Environment/Syscall/Protocol/Internal)
- [x] 实现 exit_code() 映射
- [x] 实现 user_facing() 统一输出格式
- [x] 实现 suggestion() 修复建议
- [x] 错误码单元测试 (13 个)
- [x] CLI 错误输出集成测试 (2 个)
- [x] Daemon 错误输出集成测试 (2 个)
- [x] 统一 persist 和 persistd 错误输出
- [x] 错误处理文档同步

### 内部日志

- [x] 支持内部日志配置结构与默认值
- [x] 支持内部日志文件路径解析
- [x] 支持日志目录权限 0700
- [x] 支持日志文件权限 0600
- [x] 支持基础日志写入接口
- [x] 支持日志级别过滤
- [x] 支持日志初始化错误处理
- [x] 支持敏感关键词最小脱敏测试
- [ ] 支持内部日志轮转完整实现

### Daemon

- [ ] 实现 per-user daemon
- [ ] 实现 daemon start
- [ ] 实现 daemon stop
- [ ] 实现 daemon status
- [ ] 实现 daemon 自动启动
- [ ] 实现 daemon 空闲退出策略
- [ ] 实现 daemon 日志
- [ ] 实现 daemon 错误处理
- [ ] 实现 daemon 崩溃提示
- [ ] 实现 daemon 单实例锁

### IPC

- [x] 设计 Unix Socket 协议（SOCKET_PROTOCOL.md）
- [x] 实现 client connect（ClientSocket::connect）
- [x] 实现 request/response（Frame + write_frame/read_frame）
- [x] 实现 streaming attach（M08：poll 驱动 + STDIN/STDOUT 帧转发）
- [x] 实现 resize 帧转发（M09：SIGWINCH → RESIZE → TIOCSWINSZ）
- [x] 实现 SIGPIPE 忽略（M09：daemon + client 两端）
- [x] 实现协议版本（ProtocolVersion + HELLO handshake）
- [x] 实现超时（set_stream_timeout）
- [x] 实现错误码（ErrorPayload）
- [x] 实现 socket 权限检查（0600 文件 / 0700 目录）
- [x] 实现 socket 清理（Drop + cleanup）

### Daemon

- [x] 实现 per-user daemon
- [x] 实现 daemon start（通过 persist CLI）
- [x] 实现 daemon stop
- [x] 实现 daemon status
- [x] 实现 daemonize 后台化（PID 文件 + foreground 进程）
- [x] 实现单实例锁（PID 文件 + flock）
- [x] 实现优雅退出（SIGTERM → 清理 socket/PID → 退出）
- [x] 实现 daemon 日志（使用 M03 日志框架）
- [x] 实现 daemon 错误处理（使用 M04 错误框架）
- [ ] 实现 daemon 崩溃提示（M06 之后）

### PTY Engine

- [x] 实现 openpty（posix_openpt + grantpt + unlockpt）
- [x] 实现 fork
- [x] 实现 setsid
- [x] 实现 TIOCSCTTY
- [x] 实现 exec 用户默认 shell
- [x] 支持读取用户默认 shell（getpwuid_r，fallback SHELL env，fallback /bin/sh）
- [ ] 支持 termios 初始化
- [ ] 支持 raw mode
- [x] 支持 PTY master 非阻塞
- [x] 支持 PTY 生命周期清理（drop 自动清理）
- [-] 支持 SIGCHLD（M08+ — daemon io_loop 通过 PTY read 返回 0 检测 exit，当前用 poll 轮询）
- [x] 支持 shell exit code（waitpid + WEXITSTATUS/WTERMSIG）
- [x] 支持 Closed Session 可恢复（M14）
- [x] 保存 Closed Session cwd（M14）
- [x] 保存 Closed Session 环境变量快照（M14：启动环境白名单）

### I/O

- [x] 实现 client stdin 到 PTY（通过 STDIN frame → write_input）
- [x] 实现 PTY output 到 client stdout（read_output → STDOUT frame）
- [x] 实现 poll 驱动（io_loop 使用 poll() + FrameAccumulator）
- [ ] 避免阻塞写
- [ ] 慢客户端处理
- [ ] 大输出处理
- [ ] 输出风暴保护
- [x] EOF 处理（raw mode + STDIN 字节 → shell 自行处理）

### Signal

- [x] 支持 SIGINT（通过 raw mode + STDIN 字节 → PTY master → line discipline → 前台进程组）
- [x] 支持 SIGQUIT（同上）
- [x] 支持 SIGTSTP（同上）
- [x] 支持 SIGWINCH（M09：AtomicBool flag + RESIZE 帧 + io_loop 检查）
- [x] 支持 Ctrl+D（raw mode + STDIN 字节 → shell 自行处理）
- [x] 正确处理 foreground process group（由 PTY 内核驱动自动完成）
- [x] 正确处理终端 resize（M09：TIOCGWINSZ → RESIZE → TIOCSWINSZ）
- [x] 实现 daemon 端 Signal 消息处理（转发到 PTY 前端进程组）
- [x] 实现 client 端本地终端信号转换为 IPC Signal 消息
- [x] 新增 IPC 消息类型 Signal/SignalResp
- [x] SIGINT 转发集成测试
- [x] SIGTSTP 转发集成测试
- [ ] 测试 vim/top/less resize

### Session Manager

- [x] 创建 Session（NEW_SESSION → 创建 PtySession）
- [x] Attach Session（ATTACH → 取出 PtySession → I/O 循环）
- [x] Detach Session（DETACH → 放回 SessionManager）
- [x] Kill Session（M10：SIGKILL + 清理）
- [x] Close Session（M10：SIGHUP + 等待退出）
- [x] Rename Session
- [x] List Session（LIST_SESSIONS → 返回列表）
- [ ] 查询 Session 详情
- [ ] 更新 Session 状态
- [x] 处理 Closed Session（M14：释放 runtime、保存恢复上下文并支持 attach 冷恢复）
- [x] 支持 Closed Session attach 冷恢复（M14）
- [ ] 处理 Zombie Session
- [ ] 处理 Detached Session
- [ ] Session ID 生成
- [ ] Session Name 生成
- [ ] Session 权限检查

### Metadata Store

- [x] 选择 SQLite（bundled rusqlite）
- [x] 定义 schema（sessions 表 + schema_version 表）
- [x] 支持 schema version
- [x] 支持 migration（自动 execute_batch）
- [x] 存储 Session metadata（session_id, name, status, timestamps）
- [x] 存储 exit code（close_session / kill_session 时记录）
- [x] 存储 cwd（create_session 时记录）
- [ ] 存储 shell pid（M11 范围外）
- [x] 存储 created/active time（created_at, updated_at, closed_at）
- [ ] 存储 source client（M11 范围外）
- [x] 权限安全检查（DB 文件继承目录权限）

### Ring Buffer (M12)

- [x] 实现固定大小 buffer（RingBuffer in persist-core）
- [x] 支持循环覆盖
- [x] 支持 attach 回放（AttachResp 后、实时 I/O 前发送受限 Stdout frames）
- [x] 支持配置大小（config.ring_buffer.default_size）
- [ ] 支持慢客户端丢弃策略（M12 范围外）
- [ ] 添加 benchmark（M12 范围外）

### Logging

- [x] 每 Session 独立日志
- [x] 异步写入
- [ ] 批量 flush
- [x] 日志轮转
- [ ] 日志保留策略
- [x] 日志权限 0600
- [x] 支持关闭日志
- [ ] 支持清理日志
- [x] 支持输出日志查看
- [ ] 为 Session 日志增加兼容时间戳格式并实现 replay `--speed`
- [ ] 使用事件通知实现 replay `--follow`，禁止 sleep polling
- [ ] 为快速 `cd; exit` 设计跨 Shell cwd 退出状态 side channel

### CLI

- [ ] persist version
- [ ] persist daemon start
- [ ] persist daemon stop
- [ ] persist daemon status
- [ ] persist new
- [ ] persist ls
- [x] persist attach（M08：raw mode + poll 驱动 I/O）
- [x] persist new（M10）
- [x] persist ls（M10）
- [x] persist close（M10）
- [x] persist kill（M10）
- [x] persist detach
- [x] persist rename
- [x] persist log
- [ ] persist tail
- [x] persist doctor
- [x] persist install
- [x] persist uninstall
- [x] persist config

### SSH 接管

- [x] 检测交互式 SSH（hook 中 `$SSH_TTY` 检查）
- [x] 不接管非交互命令（shell hook 只在交互式 SSH 中执行，非交互无影响）
- [x] 不影响 scp（非交互自动绕过）
- [x] 不影响 sftp（非交互自动绕过）
- [x] 不影响 rsync（非交互自动绕过）
- [x] 不影响 ansible（非交互自动绕过）
- [x] 不影响 git over ssh（非交互自动绕过）
- [x] 支持 PERSIST_DISABLE=1 绕过
- [ ] 支持 uninstall 回滚
- [x] 支持 shell profile 注入（bash/zsh）
- [x] 支持 zsh/fish PTY 集成测试

### 安装与诊断

- [x] persist install
- [x] persist uninstall
- [x] persist doctor
- [x] 检查 socket 权限
- [x] 检查 daemon 状态
- [ ] 检查 profile 注入
- [x] 检查日志目录权限
- [ ] 检查 metadata 权限
- [x] 输出修复建议

### 测试

- [x] 单元测试
- [x] PTY 集成测试（echo/pipe/多命令/重定向/zsh/fish）
- [x] CLI 集成测试（daemon-required 命令连接错误输出）
- [x] Daemon IPC 集成测试（note/tag 完整流程）
- [ ] Signal 集成测试（M22）
- [ ] SSH 接管测试
- [x] 大输出测试（M21）
- [x] 多 Session 测试（M21）
- [x] 频繁 attach/detach 测试（M21）
- [ ] 断线恢复测试
- [ ] vim/top/less 兼容测试

---

## Phase 2：易用性增强

- [x] Session 自动命名
- [x] Session note
- [x] Session tag
- [x] Session pin
- [ ] Session search
- [x] Log grep
- [x] Log export
- [ ] Log redact
- [x] Independent history
- [x] Idle detection
- [x] Idle GC
- [ ] Better list output
- [x] Better doctor
- [x] Replay mode
- [x] Shell completion（M48：bash/zsh/fish、包接入和定向验证）
- [x] Man page（M47：persist/persistd、groff 与三种包验证）

---

## Phase 3：恢复与观测增强

- [x] Replay mode
- [x] Read-only attach
- [x] 多客户端可写 attach（M15：pipe 信号 takeover）
- [x] Single active writer policy（M15：单 active writer，second client 触发 takeover）
- [x] Attach takeover 策略（M15：pipe 唤醒原 io_loop，释放后转交新 client）
- [x] 多 active writer 协作（M35：WRITE_REQUEST/GRANTED/REVOKED 通知后立即交接）
- [x] Session lock（M36：持久化锁定状态，阻止 attach/kill/Idle GC）
- [x] Foreground process detection（M37：PTY 前台进程组与 `/proc` 命令摘要）
- [x] Process tree（M38）
- [x] Resource monitor（M39）
- [x] SSH_AUTH_SOCK sync（M40）
- [x] Snapshot（M41）
- [x] Metrics（M42）
- [-] Performance dashboard（M52：CLI 数据客户端已完成，当前实现 Ratatui 全屏界面）

---

## Phase 4：发布和长期维护

- [x] deb package（M46：本机构建、解包和 checksum 验证）
- [x] rpm package（M46：test Rocky 原生构建、内容和 checksum 验证）
- [x] tarball release（M46：本机构建、解包执行和 checksum 验证）
- [x] GitHub Actions package build（M46：workflow 复用本地打包入口，待 mirror 触发）
- [x] GitHub Actions artifact upload（M46：上传 tarball/deb/rpm 与 checksums）
- [x] 生成 release checksums（M46）
- [x] Security review（M44：socket、日志、metadata 权限与输入边界审查）
- [x] Compatibility matrix（M45：Ubuntu bash/zsh/fish、Rocky bash 基线）
- [x] Performance regression（M43：本地和 test 主机 100/500/1000 Session 基准）
- [x] v1.0 release checklist（M50：0.1.0 tag、平台 workflow、artifact 复核与 test 部署已完成）
- [-] GitHub Release、artifact 附件、签名与 SBOM（维护者决定暂缓）
- [x] User documentation complete（M49：命令、配置、安装、FAQ 一致性审计）
- [x] Troubleshooting guide complete（M49：daemon、socket、权限、恢复、日志和绕过）
- [x] M51：编写单文件完整用户手册并接入三种发布包验证
- [x] M51：`persist ls` 支持 TTY 交互选择和 `persist ls <id>` 菜单
- [x] M51：bash/zsh/fish 实时命令历史，不修改用户 dotfile 或覆盖已有 hook
- [x] M51：命令历史最新优先分页、0600 权限及 10,000 条/4 MiB 上限

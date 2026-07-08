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

- [ ] 创建 README.md
- [ ] 创建 PROJECT_PRINCIPLES.md
- [ ] 创建 PRODUCT_PHILOSOPHY.md
- [ ] 创建 NON_GOALS.md
- [ ] 创建 ROADMAP.md
- [ ] 创建 MILESTONES.md
- [ ] 创建 NEXT_TASK.md
- [ ] 创建 TODO.md
- [ ] 创建 CHANGELOG.md
- [ ] 创建 CONTRIBUTING.md
- [ ] 创建 SECURITY.md
- [ ] 创建 SUPPORT.md
- [ ] 创建 CODE_OF_CONDUCT.md
- [ ] 创建 ADR 模板
- [ ] 创建 docs 目录结构
- [ ] 创建 architecture 文档
- [ ] 创建 development 文档
- [ ] 创建 protocol 文档
- [ ] 创建 benchmark 文档
- [ ] 创建 user 文档
- [ ] 创建 known 文档

### 工程初始化

- [x] 选择主要开发语言：Rust
- [ ] 初始化 Cargo workspace
- [ ] 确定 Rust MSRV
- [ ] 初始化构建系统
- [ ] 初始化测试框架
- [ ] 初始化 GitHub Actions CI
- [ ] 初始化 GitHub Actions package workflow
- [ ] 初始化 rustfmt
- [ ] 初始化 clippy
- [ ] 初始化目录结构
- [ ] 创建最小 CLI 骨架
- [ ] 创建版本命令
- [ ] 创建基础错误处理框架
- [ ] 创建基础配置加载框架
- [ ] 创建内部日志框架

---

## Phase 1：MVP Core

### 配置系统

- [ ] 支持默认配置
- [ ] 支持用户配置
- [ ] 支持系统配置
- [ ] 支持配置校验
- [ ] 支持打印当前配置
- [ ] 支持安全默认值

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

- [ ] 设计 Unix Socket 协议
- [ ] 实现 client connect
- [ ] 实现 request/response
- [ ] 实现 streaming attach
- [ ] 实现协议版本
- [ ] 实现超时
- [ ] 实现错误码
- [ ] 实现 socket 权限检查
- [ ] 实现 socket 清理

### PTY Engine

- [ ] 实现 openpty
- [ ] 实现 fork
- [ ] 实现 setsid
- [ ] 实现 TIOCSCTTY
- [ ] 实现 exec 用户默认 shell
- [ ] 支持读取用户默认 shell
- [ ] 支持 termios 初始化
- [ ] 支持 raw mode
- [ ] 支持 PTY master 非阻塞
- [ ] 支持 PTY 生命周期清理
- [ ] 支持 SIGCHLD
- [ ] 支持 shell exit code
- [ ] 支持 Closed Session 可恢复
- [ ] 保存 Closed Session cwd
- [ ] 保存 Closed Session 环境变量快照

### I/O

- [ ] 实现 client stdin 到 PTY
- [ ] 实现 PTY output 到 client stdout
- [ ] 实现 epoll 驱动
- [ ] 避免阻塞写
- [ ] 慢客户端处理
- [ ] 大输出处理
- [ ] 输出风暴保护
- [ ] EOF 处理

### Signal

- [ ] 支持 SIGINT
- [ ] 支持 SIGQUIT
- [ ] 支持 SIGTSTP
- [ ] 支持 SIGWINCH
- [ ] 支持 Ctrl+D
- [ ] 正确处理 foreground process group
- [ ] 正确处理终端 resize
- [ ] 测试 vim/top/less resize

### Session Manager

- [ ] 创建 Session
- [ ] Attach Session
- [ ] Detach Session
- [ ] Kill Session
- [ ] Rename Session
- [ ] List Session
- [ ] 查询 Session 详情
- [ ] 更新 Session 状态
- [ ] 处理 Closed Session
- [ ] 支持 Closed Session attach 冷恢复
- [ ] 处理 Zombie Session
- [ ] 处理 Detached Session
- [ ] Session ID 生成
- [ ] Session Name 生成
- [ ] Session 权限检查

### Metadata Store

- [ ] 选择 SQLite 或 BoltDB
- [ ] 定义 schema
- [ ] 支持 schema version
- [ ] 支持 migration
- [ ] 存储 Session metadata
- [ ] 存储 exit code
- [ ] 存储 cwd
- [ ] 存储 shell pid
- [ ] 存储 created/active time
- [ ] 存储 source client
- [ ] 权限安全检查

### Ring Buffer

- [ ] 实现固定大小 buffer
- [ ] 支持循环覆盖
- [ ] 支持 attach 回放
- [ ] 支持配置大小
- [ ] 支持高吞吐写入
- [ ] 支持慢客户端丢弃策略
- [ ] 添加 benchmark

### Logging

- [ ] 每 Session 独立日志
- [ ] 异步写入
- [ ] 批量 flush
- [ ] 日志轮转
- [ ] 日志保留策略
- [ ] 日志权限 0600
- [ ] 支持关闭日志
- [ ] 支持清理日志
- [ ] 支持输出日志查看

### CLI

- [ ] persist version
- [ ] persist daemon start
- [ ] persist daemon stop
- [ ] persist daemon status
- [ ] persist new
- [ ] persist ls
- [ ] persist attach
- [ ] persist detach
- [ ] persist kill
- [ ] persist rename
- [ ] persist log
- [ ] persist tail
- [ ] persist doctor
- [ ] persist install
- [ ] persist uninstall
- [ ] persist config

### SSH 接管

- [ ] 检测交互式 SSH
- [ ] 不接管非交互命令
- [ ] 不影响 scp
- [ ] 不影响 sftp
- [ ] 不影响 rsync
- [ ] 不影响 ansible
- [ ] 不影响 git over ssh
- [ ] 支持 SH_DISABLE=1 绕过
- [ ] 支持 uninstall 回滚
- [ ] 支持 shell profile 注入
- [ ] 支持 bash/zsh/fish 接入策略

### 安装与诊断

- [ ] persist install
- [ ] persist uninstall
- [ ] persist doctor
- [ ] 检查 socket 权限
- [ ] 检查 daemon 状态
- [ ] 检查 profile 注入
- [ ] 检查日志目录权限
- [ ] 检查 metadata 权限
- [ ] 输出修复建议

### 测试

- [ ] 单元测试
- [ ] 集成测试
- [ ] PTY 测试
- [ ] Signal 测试
- [ ] SSH 接管测试
- [ ] 非交互兼容测试
- [ ] 大输出测试
- [ ] 多 Session 测试
- [ ] 断线恢复测试
- [ ] vim/top/less 兼容测试

---

## Phase 2：易用性增强

- [ ] Session 自动命名
- [ ] Session note
- [ ] Session tag
- [ ] Session pin
- [ ] Session search
- [ ] Log grep
- [ ] Log export
- [ ] Log redact
- [ ] Independent history
- [ ] Idle detection
- [ ] Auto cleanup
- [ ] Better list output
- [ ] More detailed session info
- [ ] Shell completion
- [ ] Man page

---

## Phase 3：恢复与观测增强

- [ ] Replay mode
- [ ] Read-only attach
- [ ] 多客户端可写 attach
- [ ] Single active writer policy
- [ ] Attach takeover 策略
- [ ] Session lock
- [ ] Foreground process detection
- [ ] Process tree
- [ ] Resource monitor
- [ ] SSH_AUTH_SOCK sync
- [ ] Snapshot
- [ ] Metrics
- [ ] Performance dashboard

---

## Phase 4：发布和长期维护

- [ ] deb package
- [ ] rpm package
- [ ] tarball release
- [ ] GitHub Actions package build
- [ ] GitHub Actions artifact upload
- [ ] 生成 release checksums
- [ ] Security review
- [ ] Compatibility matrix
- [ ] Performance regression
- [ ] v1.0 release checklist
- [ ] User documentation complete
- [ ] Troubleshooting guide complete

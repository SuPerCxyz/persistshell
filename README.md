# PersistShell

PersistShell 是一个面向 Linux 的持久交互式 Shell 运行时。

它的目标不是替代 SSH，也不是替代 tmux、screen、Zellij 或其他终端复用器。

PersistShell 解决的是一个更具体的问题：

> 让 Shell 的生命周期不再依赖 SSH 连接的生命周期。

在普通 SSH 使用场景中，如果用户直接通过 SSH 登录远程机器执行任务，一旦 SSH 断开，前台任务通常会中断，Shell 上下文也会丢失。

PersistShell 希望让这个过程变成：

```bash
ssh node1
```

用户仍然像平时一样 SSH 登录。

但实际上，用户进入的是一个由 PersistShell 管理的持久 Session。

即使 SSH 断开，Session、Shell、前台任务、后台任务和输出历史仍然保留。

用户可以从另一台电脑重新 SSH 到同一台机器，然后手动切换到之前的 Session，继续查看输出和继续操作。

---

## 项目定位

PersistShell 是：

```text
Persistent Interactive Shell Runtime
```

也就是：

```text
持久交互式 Shell 运行时
```

它不是：

- SSH Server
- SSH Client
- Terminal Emulator
- Terminal Multiplexer
- Remote Desktop
- Bastion Host
- IDE
- 文件管理器
- AI Agent
- Shell 解释器

PersistShell 不创造新的远程登录协议。

PersistShell 只管理远程机器上的持久 Shell 生命周期。

---

## 为什么需要 PersistShell

典型问题：

```bash
ssh node1
make -j64
```

如果此时：

- 网络断开
- 本地电脑关机
- VPN 掉线
- 浏览器远程终端刷新
- 跳板机连接断开
- 用户切换到另一台电脑

传统 SSH 会话中的任务可能中断，或者用户无法再回到原来的操作上下文。

PersistShell 的目标是：

```text
SSH 断开 != Shell 结束
```

SSH 只是输入输出通道。

Shell 本身由 PersistShell Daemon 持有。

---

## 默认行为

每次交互式 SSH 登录时，PersistShell 默认创建一个新的持久 Session。

例如：

```bash
ssh node1
```

第一次登录创建：

```text
Session A
```

第二次登录创建：

```text
Session B
```

之前的 Session 不会被自动复用。

如果用户需要回到旧 Session，需要手动执行：

```bash
persist ls
persist attach <session-id>
```

这个设计是为了避免用户从多台电脑登录时误操作同一个 Session。

---

## 基本使用体验

日常使用：

```bash
ssh node1
```

查看历史 Session：

```bash
persist ls
```

切换到旧 Session：

```bash
persist attach 3
```

创建新 Session：

```bash
persist new
```

退出当前 Shell：

```bash
exit
```

或在 shell 空行按 `Ctrl+D` 时，PersistShell 会关闭当前活动 Shell runtime。

这不会让该 Shell 在后台继续运行，也不会继续占用 PTY 和 shell 进程资源。PersistShell 会保留该 Session 的输出、日志、最后工作目录、环境变量快照和 exit code，之后仍可通过 `persist attach <session-id>` 回到这个 Session 的上下文。

SSH 断开时：

```text
Session 自动 Detached
Shell 和任务继续运行
```

`exit`/`Ctrl+D` 与 SSH 断开不同：

```text
SSH 断开      -> detach，Shell 和任务继续运行
exit/Ctrl-D   -> close，释放 Shell runtime，但保留可恢复上下文
```

从另一台电脑 attach 到正在使用的 Session 时，应允许获取可写操作权。默认使用单 active writer 策略避免两个终端同时写入同一 PTY；新电脑可以接管写入，而不是只能只读查看。

---

## 和 tmux/screen 的区别

tmux 和 screen 是 Terminal Multiplexer。

它们的重点是：

- Session
- Window
- Pane
- Prefix Key
- 多窗口
- 多面板
- 终端复用

PersistShell 不做这些。

PersistShell 的重点是：

- SSH 无感接入
- Shell 生命周期持久化
- SSH 断开后任务继续运行
- 重新登录后可恢复
- 不要求用户学习 tmux/screen 的操作模型

PersistShell 不提供 Pane、Window、Layout 或 Prefix Key。

---

## 和 shpool/abduco 的区别

shpool 和 abduco 更接近 PersistShell 的方向，它们关注 Session 生命周期管理。

PersistShell 在此基础上强调：

- SSH Native
- 每次交互式 SSH 默认自动创建新 Session
- 用户不需要主动运行 attach 命令才能开始使用
- 更强的 Session Metadata
- 更强的日志、搜索、恢复、观测和可维护性
- 更严格的长期项目文档和开发流程

---

## 核心架构

```text
SSH Client
    ↓
PersistShell Client
    ↓
Unix Domain Socket
    ↓
PersistShell Daemon
    ↓
PTY Engine
    ↓
User Shell
    ↓
Foreground Process
```

关键点：

- Daemon 持有 PTY。
- Shell 是 Daemon 的子进程或由 Daemon 管理的进程。
- SSH Client 只负责输入输出。
- SSH 断开只会断开 Client。
- Daemon 和 Shell 不应被 SSH 生命周期影响。

---

## 技术栈

PersistShell 的主开发语言是 Rust。

Rust 被选为主语言，是因为 PersistShell 需要长期维护 Linux 基础设施能力，同时大量接触 PTY、进程、信号、Unix socket、文件权限和高性能 I/O。Rust 能在系统编程能力、内存安全、可维护性和性能之间取得较好的平衡。

默认工程形态应采用 Cargo workspace，至少包含 CLI、daemon、core、PTY、IPC 和 metadata 相关 crate。

---

## CI 和发布包

代码会自动同步到 GitHub 仓库：

```text
https://github.com/SuPerCxyz/persistshell
```

GitHub Actions 必须支持：

- Rust fmt / clippy / test。
- Linux release tarball 构建。
- Debian `.deb` 和 RPM `.rpm` 包构建。
- release artifact 上传。
- SHA256 checksum 生成。

GitHub Actions workflow 不应依赖自建 Git 服务、开发者本机路径或私有 SSH 配置。
workflow 定义已就绪；实际 GitHub hosted runner 的执行需要镜像仓库同步后触发，当前没有
将其伪报为已运行。

---

## Phase 1 MVP 目标

第一阶段只做核心能力：

- PTY Engine
- Daemon
- Client
- Session Manager
- Ring Buffer
- Logging
- Metadata Store
- SSH 自动接管
- Attach / Detach / List / New / Kill / Rename
- Signal 转发
- Window Resize
- 基础兼容性测试
- install / uninstall / doctor / bypass

不做：

- Pane
- Window
- Web UI
- REST API
- Plugin
- 协作模式
- Terminal Emulator
- SSH Server

---

## 性能目标

PersistShell 是基础设施软件，性能是第一等级要求。

目标：

```text
Attach 延迟：< 20ms
Idle CPU：接近 0%
单 Session 管理开销：< 500KB，不含 Shell 进程自身
支持 Session 数：1000+
```

禁止：

- Busy Loop
- Sleep Polling
- 无限 Buffer
- 无限日志
- 每 Session 一个线程
- 阻塞 I/O
- 全局大锁

优先：

- epoll
- Unix Domain Socket
- eventfd
- signalfd
- timerfd
- 固定 Ring Buffer
- 异步日志
- 批量写入
- 明确的资源上限

---

## 安全原则

PersistShell 必须遵守最小权限原则。

要求：

- 同一用户只能访问自己的 Session。
- Socket 目录权限必须是 0700。
- Socket 文件权限必须是 0600。
- 日志文件权限必须是 0600。
- Metadata 权限必须限制在用户自己。
- 默认不记录密码输入。
- 支持关闭日志。
- 支持日志清理。
- 支持绕过 PersistShell 进入普通 Shell。

---

## 当前状态

PersistShell 已有可运行的 Rust CLI、per-user daemon、PTY Session、SQLite metadata、日志、
Closed Session 恢复、单 active writer、观察命令、man page、bash/zsh/fish completion 以及
tarball/deb/rpm 打包入口。验证记录位于 `docs/audit/`；当前已知限制位于
`docs/known/`。

开发或自动化 Agent 的阅读入口为 `docs/INDEX.md`，当前唯一开发任务以 `NEXT_TASK.md` 为准。

---

## License

PersistShell is distributed under the MIT License.  See `LICENSE`.

---

## 开发规则摘要

每次开发只允许完成一个明确任务。

完成后必须同步更新：

- TODO.md
- CHANGELOG.md
- NEXT_TASK.md
- MILESTONES.md
- 相关设计文档
- 测试文档

不得在未更新文档的情况下修改架构。

不得自行扩大项目范围。

不得提前实现后续阶段功能。

---

## 项目愿景

PersistShell 的最终目标是成为一个稳定、轻量、高性能、可长期维护的 Linux 基础设施工具。

它应该像 SSH 一样自然，像 systemd 服务一样可靠，像 Unix 工具一样克制。

PersistShell 只做一件事：

> 让 Shell 活得比 SSH 更久。

# PersistShell 非目标

本文件定义 PersistShell 明确不做的事情。

这些非目标和功能目标一样重要。

它们用于防止项目范围失控，避免 PersistShell 变成另一个复杂终端平台。

---

## 不做 Terminal Multiplexer

PersistShell 不做：

- Pane
- Window
- Layout
- Workspace
- Split
- Tab
- Prefix Key
- Status Bar
- 内置窗口管理

原因：

PersistShell 的目标是 Shell 生命周期持久化，不是终端复用。

如果用户需要 Pane 和 Window，可以继续使用 tmux、Zellij、WezTerm 等工具。

---

## 不做 SSH Server

PersistShell 不实现：

- SSH 协议
- SSH 密钥交换
- SSH 认证
- SSH 加密传输
- SSH 端口监听
- SSH 会话管理

PersistShell 依赖 OpenSSH 等已有 SSH 实现。

PersistShell 只在用户成功登录后接管交互式 Shell。

---

## 不做 SSH Client

PersistShell 不实现本地 SSH 客户端。

不做：

- Host 配置
- 密钥管理
- Known Hosts 管理
- ProxyJump
- SSH config 解析
- SSH 连接复用

用户继续使用原生 ssh 命令。

---

## 不做 Terminal Emulator

PersistShell 不负责：

- 字体渲染
- 图形界面
- 颜色主题
- 输入法
- 鼠标选择
- 本地剪贴板
- 标签页
- GPU 渲染

这些属于终端模拟器的职责。

---

## 不做 IDE

PersistShell 不提供：

- 编辑器
- 代码补全
- 文件树
- 语言服务器
- 调试器
- 项目管理
- Git UI

PersistShell 可以运行 vim、nano、emacs、git 等程序，但不替代它们。

---

## 不做堡垒机

PersistShell 不做：

- 企业认证
- 统一授权
- 命令审批
- 运维审计平台
- 多租户权限系统
- 跳板机功能
- 账号托管

未来可以提供审计增强，但它不是核心目标。

---

## 不做 Web Terminal

Phase 1 和 Phase 2 不开发 Web UI。

不做：

- 浏览器终端
- WebSocket Terminal
- 多人在线协作 Web 页面
- Web 管理控制台

Web UI 属于 Phase 3 或更远期规划。

---

## 不做文件传输

PersistShell 不替代：

- scp
- sftp
- rsync
- lftp
- rclone

PersistShell 必须保证这些工具不受影响。

---

## 不做容器/Kubernetes 管理

PersistShell 不直接管理：

- Docker
- Podman
- Kubernetes
- Helm
- Namespace
- Pod
- Container Exec

用户可以在 PersistShell Session 里运行这些命令，但 PersistShell 不内置容器管理功能。

---

## 不做 AI 助手

PersistShell 不内置：

- LLM
- Copilot
- 自动命令生成
- 自动排障
- 自动执行任务

它是基础设施工具，不是智能代理。

---

## 不做 Shell 解释器

PersistShell 不解析用户命令语义。

PersistShell 不替代：

- bash
- zsh
- fish
- sh

PersistShell 只负责 PTY、进程、I/O 和 Session 生命周期。

---

## 不做命令安全审批

PersistShell 默认不拦截命令。

不会默认阻止：

- rm
- dd
- mkfs
- reboot
- shutdown
- lvremove

未来可以支持可选危险命令提示，但绝不能作为核心路径强依赖。

---

## 不做完整终端状态完美恢复承诺

PersistShell 目标是恢复 PTY Session 并继续交互。

对于普通 Shell、长任务、日志输出，应该可靠恢复。

对于复杂全屏 TUI 程序，例如 vim、top、htop、less，PersistShell 应尽力恢复最近终端状态和输出，但不得承诺在所有终端、所有程序、所有 ANSI 状态下做到完全像从未断开。

必须在文档中诚实说明限制。

---

## 不做无限资源保留

PersistShell 不允许无限保留：

- Session
- Ring Buffer
- 日志
- Metadata
- 历史输出

必须有明确资源限制、日志轮转和垃圾回收策略。

---

## 不做系统级多用户复杂权限模型作为第一版

Phase 1 优先实现 per-user daemon。

不做复杂系统级 daemon。

原因：

- 权限复杂
- 安全边界复杂
- 多用户 Session 隔离复杂
- root attach 策略复杂

系统级增强可以后续再设计。

---

## 不做跨机器统一管理作为第一版

PersistShell Phase 1 只管理当前 Linux 机器上的本地 Session。

不做：

- 集群
- 中心控制面
- 多机器聚合
- 远程 Session 索引
- 分布式存储

---

## 不做复杂插件系统作为第一版

插件系统属于后期能力。

Phase 1 不引入插件抽象。

原因：

- 增加 API 稳定性负担
- 增加安全风险
- 增加文档成本
- 容易导致范围失控

---

## 非目标总结

PersistShell 只做一件事：

```text
让交互式 Linux Shell 的生命周期独立于 SSH 连接。
```

任何不能直接服务于这个目标的功能，都应该拒绝、延后或移出项目。

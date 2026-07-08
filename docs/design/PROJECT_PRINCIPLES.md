# PersistShell 项目原则

本文件是 PersistShell 项目的最高原则文档。

任何架构设计、功能开发、代码实现、性能优化和问题修复，都必须遵守这些原则。

当具体实现和本文件冲突时，以本文件为准。

---

## 原则一：No Context Lost

用户的工作上下文不应因为 SSH 断开而丢失。

这里的上下文包括：

- Shell 进程
- 当前工作目录
- 前台进程
- 后台进程
- 输出历史
- 命令历史
- 环境变量
- PTY 状态
- 终端尺寸
- ANSI 终端状态
- Session Metadata

PersistShell 的核心目标不是重新打开一个 Shell，而是保留原来的工作上下文。

---

## 原则二：Shell 生命周期独立于 SSH 生命周期

SSH 是连接方式。

Shell 是工作环境。

两者生命周期必须解耦。

```text
SSH Disconnect 不应该导致 Shell Exit
```

SSH 断开时，PersistShell 应该执行 Detach。

只有用户显式退出 Shell 或显式 Kill Session，Session 才应该结束。

---

## 原则三：Zero Learning Cost

PersistShell 不应要求普通 Linux 用户学习复杂概念。

用户默认只需要：

```bash
ssh node
```

不要引入：

- Pane
- Window
- Prefix Key
- Layout
- Workspace
- 复杂状态栏
- 复杂快捷键系统

高级功能可以存在，但默认体验必须简单。

---

## 原则四：SSH Native

PersistShell 不替代 SSH。

PersistShell 与 SSH 配合。

交互式 SSH 登录时，PersistShell 可以自动接管。

非交互式 SSH 命令不能被破坏。

必须保证以下场景不受影响：

```bash
ssh node hostname
scp file node:/tmp/
sftp node
rsync file node:/tmp/
ansible all -m ping
git clone user@node:repo.git
```

---

## 原则五：默认新建 Session

每次交互式 SSH 登录，默认创建新的 Session。

PersistShell 不应该自动复用旧 Session。

原因：

- 防止多台电脑同时误操作同一个 Shell。
- 防止用户进入一个仍在执行危险任务的旧上下文。
- 保持默认 SSH 体验接近“新登录一个 shell”。

用户只有显式执行 attach/switch 时，才进入旧 Session。

---

## 原则六：One Thing Well

PersistShell 只做一件事：

```text
持久化交互式 Shell 生命周期
```

不做：

- 终端模拟器
- SSH Server
- IDE
- Web Terminal
- 文件管理器
- 堡垒机
- AI 助手
- Terminal Multiplexer

范围控制是项目长期成功的核心。

---

## 原则七：Performance First

PersistShell 是基础设施软件，性能必须从第一天考虑。

设计必须避免：

- Busy Loop
- Sleep Polling
- 阻塞 I/O
- 无限内存增长
- 无限日志增长
- 每 Session 一个线程
- 全局大锁
- 不受控 goroutine/thread 创建

优先使用：

- epoll
- eventfd
- signalfd
- timerfd
- Unix Domain Socket
- 固定 Ring Buffer
- 异步日志
- 批量写
- 明确的资源上限

---

## 原则八：安全默认

默认配置必须安全。

要求：

- per-user daemon 优先。
- 同一用户只能访问自己的 Session。
- Socket 目录权限为 0700。
- Socket 文件权限为 0600。
- 日志权限为 0600。
- Metadata 权限限制在用户自己。
- 不记录密码输入。
- 支持关闭日志。
- 支持清理日志。
- 支持绕过 PersistShell。

---

## 原则九：Escape Hatch 必须永远存在

PersistShell 不能让用户失去普通 SSH 登录能力。

必须提供可靠绕过方式。

例如：

```bash
SH_DISABLE=1 ssh node
```

或：

```bash
ssh node 'bash --noprofile --norc'
```

或：

```bash
persist uninstall
```

如果 PersistShell 出现故障，用户仍应能进入系统修复。

---

## 原则十：文档是单一事实来源

docs/ 是项目的 Single Source of Truth。

任何架构、协议、接口、状态机、开发流程的变化，必须先更新文档，再更新代码。

代码是文档的实现。

文档不能追着代码补。

---

## 原则十一：小步迭代

每次只完成一个功能。

禁止：

- 顺手做多个功能
- 大规模无关重构
- 提前实现未来阶段能力
- 引入未被当前需求使用的抽象
- 为了“以后可能需要”增加复杂性

---

## 原则十二：测试定义完成

没有测试的功能不算完成。

没有文档更新的功能不算完成。

没有 TODO/NEXT_TASK/MILESTONES 更新的功能不算完成。

功能完成必须同时满足：

- 代码完成
- 测试通过
- 边界处理完成
- 错误处理完成
- 文档更新
- 进度标记更新

---

## 原则十三：Benchmark First

任何性能优化都必须有 Benchmark 支撑。

禁止凭感觉宣称性能提升。

必须记录：

- 优化前数据
- 优化后数据
- 测试环境
- 测试命令
- 数据解释
- 是否存在副作用

---

## 原则十四：模块可替换

所有核心模块必须通过公开接口交互。

核心模块包括：

- Daemon
- Client
- PTY Engine
- Session Manager
- Ring Buffer
- Logger
- Metadata Store
- IPC Protocol
- Config

模块不得依赖彼此内部实现。

---

## 原则十五：长期可维护

PersistShell 不是一次性 Demo。

它应该按长期维护的开源基础设施项目标准开发。

每个设计都要考虑：

- 三个月后还能否理解
- 半年后能否继续开发
- 换一个大模型后能否接手
- 换一个维护者后能否维护
- 出现 Bug 后能否定位

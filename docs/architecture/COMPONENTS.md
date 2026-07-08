# PersistShell Components

本文档描述 PersistShell 的主要组件及职责边界。

---

## 组件总览

PersistShell 包含以下核心组件：

```text
persist client
persist daemon
session manager
pty engine
ipc layer
ring buffer
logger
metadata store
config system
installer
doctor
```

---

## persist client

### 职责

Client 是用户直接交互的命令行入口。

负责：

- 自动接管交互式 SSH
- 创建新 Session
- Attach 到 Session
- 转发终端输入输出
- 同步终端大小
- 发送控制命令
- 展示 Session 列表
- 展示错误信息
- 执行 install/uninstall/doctor 等用户命令

---

### 不负责

Client 不负责：

- 持有 PTY
- 持有 Shell 生命周期
- 存储 Session metadata
- 长期保存日志
- 管理其他用户 Session
- 实现 SSH 协议

---

### 典型命令

```bash
persist
persist new
persist ls
persist attach <id>
persist kill <id>
persist rename <id> <name>
persist log <id>
persist tail <id>
persist doctor
persist install
persist uninstall
```

---

## persist daemon

### 职责

Daemon 是 PersistShell 的核心后台进程。

负责：

- 管理所有 Session
- 持有 PTY master
- fork/exec 用户 Shell
- 维护 Session 状态
- 接收 Client attach/detach
- 分发 PTY 输出
- 接收 Client 输入
- 写入 Ring Buffer
- 调度异步日志写入
- 更新 Metadata
- 处理 SIGCHLD
- 处理 GC
- 处理资源限制

---

### 不负责

Daemon 不负责：

- SSH 认证
- 终端图形渲染
- 解析用户命令语义
- 实现 shell
- 实现 pane/window/layout
- 管理其它机器

---

## Session Manager

### 职责

Session Manager 管理 Session 生命周期。

负责：

- Create
- Attach
- Detach
- Kill
- Rename
- List
- Update Status
- Mark Closed
- Mark Zombie
- GC
- 权限校验
- Session ID 生成
- Session Name 生成

---

### 状态管理

Session Manager 必须维护状态机。

Session 状态包括：

- Running
- Detached
- Closed
- Recovering
- Zombie
- Killed
- Archived

---

## PTY Engine

### 职责

PTY Engine 封装底层 PTY 操作。

负责：

- openpty()
- fork()
- setsid()
- ioctl(TIOCSCTTY)
- execve()
- termios
- raw mode
- PTY master non-blocking
- window size
- foreground process group
- EOF
- PTY close

---

### 不负责

PTY Engine 不负责：

- Session metadata
- CLI 输出格式
- 日志轮转
- SQLite 写入
- 用户权限策略

PTY Engine 只处理 PTY 和进程生命周期。

---

## IPC Layer

### 职责

IPC Layer 负责 Client 和 Daemon 之间通信。

使用 Unix Domain Socket。

支持：

- request/response
- streaming attach
- control message
- resize event
- input bytes
- output bytes
- error message
- protocol version

---

### 协议要求

IPC 协议必须：

- 有版本号
- 可扩展
- 支持错误码
- 支持超时
- 支持半关闭
- 支持流式数据
- 支持控制消息和数据消息区分

---

## Ring Buffer

### 职责

Ring Buffer 存储最近 PTY 输出。

用于：

- attach 时回放最近输出
- 避免读取磁盘日志
- 缓冲输出风暴
- 支持慢客户端策略

---

### 要求

Ring Buffer 必须：

- 固定大小
- 循环覆盖
- 非阻塞
- 可配置大小
- 高吞吐
- 不无限增长

---

## Logger

### 职责

Logger 分为两类：

1. 内部运行日志
2. Session 输出日志

内部运行日志记录：

- daemon 状态
- 错误
- 警告
- 诊断信息

Session 输出日志记录：

- PTY 输出
- attach/detach 事件
- session lifecycle 事件

---

### 要求

Session 输出日志必须：

- 异步写入
- 支持轮转
- 支持压缩
- 支持保留策略
- 权限安全
- 支持关闭

---

## Metadata Store

### 职责

Metadata Store 存储长期元数据。

包括：

- Session ID
- Name
- Owner UID
- Status
- PID
- Created At
- Last Active At
- Exit Code
- CWD
- Log Path
- Tags
- Notes
- Schema Version

---

### 推荐实现

Phase 1 推荐 SQLite。

要求：

- schema version
- migration
- 事务
- 权限检查
- 损坏检测
- doctor 可诊断

---

## Config System

### 职责

Config System 负责加载配置。

配置来源：

- 编译期默认值
- 系统配置
- 用户配置
- 环境变量
- 命令行参数

优先级：

```text
命令行参数 > 环境变量 > 用户配置 > 系统配置 > 默认值
```

---

## Installer

### 职责

Installer 负责安装 SSH 自动接管逻辑。

可能涉及：

- shell profile 注入
- bashrc/zshrc/fish 配置
- systemd user service
- 目录创建
- 权限设置
- uninstall 回滚信息保存

---

### 要求

安装必须可逆。

uninstall 必须能恢复用户原始配置。

---

## Doctor

### 职责

Doctor 负责诊断环境问题。

检查：

- daemon 是否运行
- socket 是否存在
- socket 权限
- runtime dir 权限
- metadata 权限
- log 权限
- profile hook 是否正确
- 配置是否有效
- systemd user service 状态
- 是否存在 stale socket
- 是否存在 zombie session

---

## 组件交互图

```text
Client
  ├── Config
  ├── Doctor
  ├── Installer
  └── IPC Client
          ↓
      Unix Socket
          ↓
Daemon
  ├── IPC Server
  ├── Session Manager
  ├── PTY Engine
  ├── Ring Buffer
  ├── Logger
  ├── Metadata Store
  └── GC
```

---

## 模块隔离原则

每个组件必须有清晰公开接口。

禁止：

- Client 直接访问 Daemon 内部结构
- PTY Engine 直接写 SQLite
- Logger 直接修改 Session 状态
- Metadata Store 持有 PTY fd
- Config System 启动进程
- Doctor 修改业务状态，除非用户显式要求修复

---

## Phase 1 最小组件

Phase 1 必须至少实现：

- Client
- Daemon
- IPC
- Session Manager
- PTY Engine
- Ring Buffer
- Logger
- Metadata Store
- Config
- Installer
- Doctor

但每个组件只实现 MVP 能力。

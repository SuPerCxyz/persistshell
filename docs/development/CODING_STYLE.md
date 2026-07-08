# PersistShell Coding Style

本文档定义 PersistShell 的代码风格要求。

PersistShell 的主开发语言是 Rust。

Rust 代码必须使用：

- `rustfmt` 作为格式化工具。
- `clippy` 作为静态检查工具。
- `cargo test` 作为基础测试入口。
- `cargo bench` 或项目约定的 benchmark runner 作为性能测试入口。

底层 Linux 系统调用应优先通过成熟 crate 或受控的 `libc`/`nix` 封装调用。所有 `unsafe` 代码必须局部化，注释其安全前提，并尽量包在安全抽象之后。

---

## 基本原则

代码应优先满足：

1. 正确性
2. 可读性
3. 可测试性
4. 可维护性
5. 性能

不要为了“聪明”牺牲清晰。

---

## 文件大小

建议：

- 单个源文件尽量不超过 500 行。
- 超过 800 行必须考虑拆分。
- 自动生成文件除外。

---

## 函数大小

建议：

- 单个函数尽量不超过 80 行。
- 超过 120 行必须考虑拆分。
- 一个函数只做一件事。

---

## 模块边界

模块之间只能通过公开接口通信。

禁止：

- 直接访问其它模块内部结构
- 为了方便绕过接口
- 循环依赖
- 把所有逻辑堆到 daemon 主循环里

---

## 命名规则

命名应表达业务含义。

推荐：

```text
session_manager
pty_engine
ring_buffer
metadata_store
ipc_server
attach_request
```

Rust 模块、函数和变量使用 `snake_case`，类型和 trait 使用 `UpperCamelCase`，常量使用 `SCREAMING_SNAKE_CASE`。

避免：

```text
data
info
tmp
obj
manager2
handler_ex
```

---

## 错误处理

错误必须显式处理。

禁止静默吞掉错误。

错误应包含：

- 错误类型
- 上下文
- 用户可理解信息
- 可诊断细节
- 修复建议，必要时

例如：

```text
无法连接 PersistShell daemon。
原因：socket 权限错误。
建议：执行 persist doctor。
```

---

## 日志风格

日志要有上下文。

不要写：

```text
failed
```

应写：

```text
failed to open pty: user=1000 session=abc err=...
```

日志必须避免泄露：

- password
- token
- secret
- private key
- SSH_AUTH_SOCK 具体敏感路径是否需要谨慎

---

## 注释

注释解释“为什么”，不是重复“做什么”。

好的注释：

```text
这里不能在 PTY 输出路径同步写日志，否则磁盘抖动会阻塞所有 attached clients。
```

差的注释：

```text
// 写入日志
```

---

## Public API

公共接口必须稳定、简单、文档化。

任何 public API 修改必须：

- 更新文档
- 更新测试
- 更新调用方
- 说明兼容性影响

---

## 配置默认值

所有配置必须有安全默认值。

默认值必须写入：

- 配置代码
- CONFIG.md
- 示例配置
- 测试

---

## 常量

禁止魔法数字。

应定义具名常量。

例如：

```text
default_ring_buffer_size
default_socket_timeout
default_kill_grace_period
```

---

## 并发

并发代码必须简单明确。

禁止无说明地引入复杂并发结构。

涉及锁时，必须明确：

- 锁保护什么
- 锁顺序
- 是否可能死锁
- 是否在 I/O 路径持锁
- 是否在日志路径持锁

---

## I/O

所有可能阻塞的 I/O 必须明确处理。

包括：

- socket I/O
- PTY I/O
- file I/O
- database I/O

核心事件循环不得被磁盘 I/O 阻塞。

---

## 资源管理

所有资源必须明确生命周期。

包括：

- fd
- socket
- PTY
- child process
- timer
- buffer
- database connection
- log file

必须有关闭路径。

---

## 测试友好

代码应方便测试。

避免：

- 隐藏全局状态
- 难以替换的硬编码路径
- 难以模拟的系统调用
- 无法注入的时间源
- 无法替换的日志输出

---

## 平台假设

PersistShell Phase 1 面向 Linux。

可以使用 Linux 专有能力：

- epoll
- signalfd
- eventfd
- timerfd
- procfs
- Unix credentials

但必须在文档中明确平台要求。

---

## 用户输出

CLI 输出必须清晰。

错误信息应面向用户，而不是只面向开发者。

例如：

```text
错误：PersistShell daemon 未运行，且自动启动失败。
原因：/run/user/1000/persistshell 权限不是 0700。
建议：执行 persist doctor --fix。
```

---

## 表格输出

`persist ls` 等命令应支持人类可读输出。

后续可支持：

```bash
persist ls --json
```

用于脚本。

Phase 1 可以先做表格输出。

---

## 向后兼容

协议、metadata schema、配置项都要考虑版本。

新增字段应向后兼容。

删除字段必须有 migration 和 release note。

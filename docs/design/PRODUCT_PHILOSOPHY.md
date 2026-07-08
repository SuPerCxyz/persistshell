# PersistShell 产品哲学

PersistShell 的哲学很简单：

> 让 Shell 活得比 SSH 更久。

这是整个项目唯一不可动摇的核心。

---

## PersistShell 不是 tmux

tmux 是优秀的 Terminal Multiplexer。

它解决的问题是：

- 多窗口
- 多面板
- 一个终端里管理多个工作区
- 复杂终端布局
- 高级快捷键

PersistShell 不解决这些问题。

PersistShell 不提供 Pane。

PersistShell 不提供 Window。

PersistShell 不提供 Prefix Key。

PersistShell 不要求用户学习新的终端操作系统。

---

## PersistShell 不是 screen

screen 的核心价值是让终端会话可恢复。

但它仍然要求用户主动学习和使用 screen 命令。

PersistShell 的目标是更无感。

用户平时只需要：

```bash
ssh node
```

默认即进入 PersistShell 管理的持久 Session。

---

## PersistShell 不是 SSH

SSH 是安全远程登录协议。

PersistShell 不实现 SSH Server。

PersistShell 不替代 OpenSSH。

PersistShell 不参与认证、加密、密钥交换、网络传输。

PersistShell 只在 SSH 登录后的本机环境中接管交互式 Shell。

---

## PersistShell 不是堡垒机

堡垒机关注：

- 身份认证
- 权限审计
- 命令审批
- 多租户管理
- 集中运维
- 访问控制

PersistShell 不做这些。

PersistShell 只关心单机上的用户 Shell 生命周期。

---

## PersistShell 不是终端模拟器

终端模拟器负责：

- 绘制界面
- 字体渲染
- 颜色渲染
- 输入法
- 剪贴板
- 标签页
- 本地图形界面

PersistShell 不做这些。

PersistShell 面向的是远程 Linux 上的 PTY、Shell 和进程。

---

## PersistShell 的核心体验

用户从电脑 A 登录：

```bash
ssh node1
```

进入一个新的 Persist Session。

用户执行：

```bash
make -j64
```

然后电脑 A 断网。

任务继续运行。

用户从电脑 B 登录：

```bash
ssh node1
```

默认创建新的 Persist Session。

用户查看历史：

```bash
persist ls
```

然后恢复旧 Session：

```bash
persist attach <id>
```

用户继续查看 make 输出，并可以继续操作原来的 Shell。

---

## 为什么默认不自动恢复旧 Session

因为自动恢复旧 Session 存在误操作风险。

例如：

- 用户从另一台电脑登录，原本只是想查看系统状态，却意外进入正在执行生产操作的旧 Shell。
- 两个客户端同时写入同一个 Session，导致命令混乱。
- 用户忘记当前 Shell 的上下文，误执行危险命令。

因此 PersistShell 的默认策略是：

```text
每次交互式 SSH 登录自动创建新 Session。
只有手动 attach 才进入旧 Session。
```

这是安全性和易用性之间的平衡。

---

## 为什么不做 Pane 和 Window

Pane 和 Window 会显著增加复杂度。

它们会引入：

- 布局状态
- 快捷键系统
- 多层焦点管理
- 复杂 UI
- 更多终端兼容问题
- 更多用户学习成本

PersistShell 的目标不是成为另一个 tmux。

PersistShell 的目标是让 SSH Shell 持久化。

---

## 为什么重视 Metadata

用户恢复 Session 时，最常见的问题是：

```text
我不知道哪个 Session 是我要找的。
```

因此 PersistShell 需要记录尽可能有用的元信息：

- 创建时间
- 最后活跃时间
- 当前目录
- 前台进程
- 来源 IP
- Session 名称
- 标签
- 备注
- 退出码
- 日志路径

这样用户执行：

```bash
persist ls
```

应该能直观看到每个 Session 在做什么。

---

## 为什么重视日志和回放

断线后，用户不仅需要任务继续运行，还需要看到断线期间发生了什么。

因此 PersistShell 需要：

- Ring Buffer
- 持久日志
- attach 时回放最近输出
- 日志搜索
- 日志导出
- 日志轮转
- 日志清理

但日志不能无限增长，也不能泄露敏感信息。

---

## 为什么重视逃生能力

任何自动接管 SSH 的工具都必须非常谨慎。

如果工具损坏，用户不能因此无法登录机器。

因此 PersistShell 必须始终提供：

- 环境变量绕过
- 命令绕过
- uninstall
- doctor
- 配置回滚
- 明确错误提示

用户永远应该保留普通 Shell 的入口。

---

## 最终愿景

PersistShell 应该成为一个“安装后几乎忘记存在”的工具。

它不应该打扰用户。

它不应该改变用户对 SSH 的理解。

它只在用户需要恢复上下文时提供能力。

理想状态是：

```text
平时像普通 SSH。
断线时保护任务。
恢复时找到上下文。
不用时完全无感。
```

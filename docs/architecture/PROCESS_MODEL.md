# PersistShell Process Model

本文档描述 PersistShell 的进程模型。

---

## 主要进程

PersistShell 涉及以下进程：

```text
sshd
login shell/profile
persist client
persist daemon
user shell
foreground process
background processes
```

---

## 典型关系

```text
sshd
 └── persist client
       ↔ Unix Socket ↔ persist daemon
                         ├── shell(session 1)
                         │    ├── foreground process
                         │    └── background processes
                         ├── shell(session 2)
                         └── shell(session N)
```

---

## SSH 登录进程

OpenSSH 创建用户登录进程。

PersistShell 可以通过 shell profile hook 接管交互式登录。

例如：

```text
sshd → user shell → persist client
```

如果检测到应接管，则：

```text
exec persist
```

这样用户看到的是 PersistShell-managed Session。

---

## Client 进程

Client 是短生命周期进程。

它的生命周期与当前 SSH 连接绑定。

Client 退出原因：

- 用户 detach
- SSH 断开
- 网络错误
- 本地 terminal 关闭
- 用户执行命令结束
- attach 的 Session 退出

Client 退出不应导致 daemon 或 session 退出。

---

## Daemon 进程

Daemon 是 per-user 长生命周期进程。

Daemon 负责管理该用户所有 Session。

Daemon 可以由 Client 按需启动。

推荐路径：

```text
/run/user/$UID/persistshell/
```

Daemon 必须防止同一用户启动多个实例。

需要单实例锁。

---

## Shell 进程

每个运行中的 Session 对应一个 Shell 进程。

Shell 由单一 per-user Holder 通过 PTY Engine 创建；Daemon 只通过 Holder 私有协议管理
runtime，不持有 PTY master，也不直接 reap Shell。

Shell 应拥有：

- 独立控制终端
- 独立进程组/session
- 正确 job control
- 正确环境变量
- 正确 cwd

对 bash、zsh 和 fish，PTY Engine 可在 Shell 进程内安装临时命令历史 hook。集成层必须先加载
用户原配置，不编辑 dotfile，不覆盖已有 prompt/history hook，并在失败时降级为普通 Shell。
hook 通过短生命周期 `persist` helper 写入受限的 Session 命令记录；helper 失败不能阻塞 prompt
或改变上一条用户命令的退出状态。

同一集成层还为每次 runtime 注入独立的最终状态 hook，通过隐藏 `persist __state-commit`
helper 将 cwd、runtime identity 和 sequence 原子写入 `0700` 状态目录中的 `0600` 文件。该
hook 不修改用户 rc、不包装 `cd`/`exit`，失败时降级且不得改变用户命令或退出语义。

---

## Foreground Process

用户在 Shell 中运行前台命令：

```bash
vim
top
make
fio
```

这些进程属于 Session 内的进程树。

PersistShell 应尽量识别当前前台进程，用于：

- Session 列表展示
- 资源监控
- Signal 处理
- 用户判断要 attach 哪个 Session

Phase 1 可简化实现。

---

## Background Processes

用户可能运行：

```bash
cmd &
nohup cmd &
```

这些后台进程可能在 Shell 退出后继续存在。

Session Manager 需要区分：

- shell alive
- foreground process alive
- background children alive
- session closed

Phase 1 可以优先以 shell runtime 生命周期作为 Session 活跃状态边界。

用户执行 `exit` 或 `Ctrl+D` 后，PersistShell 不应继续保留该 shell runtime 在后台运行。若用户显式使用 `nohup`、systemd、daemonize 或其他 Linux 机制让进程脱离 shell 继续运行，这是操作系统进程模型的结果，不等于 PersistShell 继续持有该 Session 的 PTY 或 shell。

后续增强 orphan/background 检测。

---

## Process Group

PTY job control 依赖进程组。

关键概念：

- session leader
- controlling terminal
- foreground process group
- background process group

PersistShell 必须避免错误地向 daemon/client 发送控制信号。

---

## Signal 归属

Ctrl+C 应影响 Session 前台进程，而不是：

- persist daemon
- persist client
- sshd

Ctrl+Z 应暂停 Session 前台进程，而不是暂停 client。

SIGWINCH 应同步给 PTY 对应的前台程序。

---

## Daemon 与 Shell 的父子关系

Phase 1 推荐：

```text
daemon fork shell
```

优点：

- 实现简单
- 管理清晰
- SIGCHLD 直接可见
- PTY fd 由 daemon 持有

缺点：

- daemon 崩溃可能导致 PTY fd 丢失
- daemon 崩溃恢复能力有限

这是 Phase 1 可接受限制。

---

## PTY Holder 模型

M53 采用单一 per-user 数据平面：

```text
persistd
   ↕ private Unix Socket
persist-holder
   ├── shell(session 1)
   ├── shell(session 2)
   └── shell(session N)
```

`persist-holder` 使用 `epoll` 统一持有和排空 PTY。daemon 负责控制面、公共 IPC 和 metadata；
daemon 崩溃后 holder 与 Shell 继续运行，daemon 重启后通过 inventory 对账恢复控制。

holder 自身崩溃和系统重启后的 runtime 恢复不属于 M53 承诺。

---

## Zombie 处理

Holder 必须通过 `signalfd`/`waitpid` 处理 Shell 的 SIGCHLD。

当 Shell 子进程退出：

- waitpid
- 获取 exit code
- 更新 Session 状态
- 清理 PTY
- 验证状态文件并保留可选最终 cwd 和版本化环境 snapshot
- 标记 Closed

Holder 向 daemon 提供 `SessionExited` 和 `GetExitContext`；minor 1 保持 cwd-only，控制连接
协商 minor 2 和 environment capability 后可附带版本化环境 snapshot。daemon 先提交 metadata，
再调用 `RetireExited`。daemon 离线时 Holder 继续保留有界退出上下文，直至重连对账。不能产生
zombie process，也不能在 metadata 失败时提前丢弃退出上下文。

---

## Orphan 处理

M53 实施前，daemon 异常退出仍可能使 Shell 成为 orphan 或被 SIGHUP 影响。M53 完成后，
Shell 是 holder 的子进程；daemon 控制连接异常断开只进入等待接管状态，不结束 runtime。

---

## systemd user service

后续可使用 systemd --user 管理 daemon。

但要注意：

如果没有 linger，用户完全登出后 user service 可能被停止。

PersistShell 需要明确策略：

- 按需 daemon
- systemd user daemon
- loginctl enable-linger 是否可选

Phase 1 可以先实现按需 daemon。

---

## 非 systemd 环境

很多服务器、容器、最小系统可能没有 systemd user session。

必须支持 fallback：

- client auto-spawn daemon
- daemon lock file
- runtime dir fallback

但不能牺牲安全。

---

## 进程模型不变量

必须满足：

1. Client 退出不杀 Session Shell。
2. M53 完成后 Holder 是唯一 PTY master owner。
3. Shell 不依赖 SSH 连接存活。
4. Ctrl+C 不杀 daemon。
5. SIGCHLD 被正确回收。
6. 一个用户的 daemon 不管理其它用户 Session。
7. 非交互 SSH 不进入 PersistShell 进程模型。
8. Daemon 控制连接异常断开不能触发 Holder 级联关闭。

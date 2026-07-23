# PersistShell FAQ

## PersistShell 是什么？

PersistShell 是一个 Linux 持久交互式 Shell 运行时。

它让 Shell 生命周期独立于 SSH 连接。

SSH 断开只会 detach client；daemon 仍存活时，Shell 和任务继续运行。

---

## PersistShell 是 tmux 替代品吗？

不是。

tmux 是 terminal multiplexer。

PersistShell 不做 pane、window、layout、prefix key。

PersistShell 只解决 SSH 断开导致 Shell 上下文丢失的问题。

---

## 每次 SSH 会进入同一个 Session 吗？

不会。

PersistShell 的默认策略是：

```text
每次交互式 SSH 登录都创建新的 Session。
```

如果要进入旧 Session，需要手动：

```bash
persist ls
persist attach <id>
```

---

## 为什么不默认恢复上一次 Session？

为了避免误操作。

如果自动恢复旧 Session，用户从另一台电脑登录时，可能误进入正在执行重要任务的 Shell。

默认新建更安全。

---

## SSH 断开后任务真的不会停吗？

只要 PersistShell daemon 仍然运行，Session 的 PTY 和 Shell 会继续存在。

daemon 崩溃后的 PTY 恢复不受保证，见 `docs/known/LIMITATIONS.md`。

---

## daemon 崩溃会怎样？

daemon 持有 PTY master fd。

如果 daemon 崩溃，PTY fd 可能关闭，Session 可能丢失。

这是已知限制。

---

## 会不会影响 scp/sftp/rsync？

不应该。

PersistShell 必须只接管交互式 SSH。

这些命令必须不受影响：

```bash
scp
sftp
rsync
ansible
git over ssh
ssh node command
```

---

## 如何临时绕过 PersistShell？

```bash
PERSIST_DISABLE=1 ssh node
```

---

## exit 或 Ctrl+D 后还能回到 Session 吗？

可以。自然退出会释放 shell runtime，但保留 Session metadata、输出、cwd、受限启动环境和
exit code。之后 `persist attach <id>` 会先回放退出前最近的有界输出，再创建新的可写 PTY；
允许恢复的动态 export 变量按 `recovery.environment_include` 和安全禁区策略处理，当前连接变量
取自本次 attach。已退出的进程不会复活。

cwd 依赖 `/proc` 周期采样。若在一次采样间隔内执行 `cd` 后立即退出，可能保留上一次成功
采样的目录；正常运行窗口内的 cwd 会随 Closed Session 恢复。

---

## 如何卸载？

```bash
persist uninstall
```

完全删除数据：

```bash
persist uninstall --purge
```

---

## 日志会不会记录密码？

默认不记录用户输入。

但如果程序把敏感信息输出到屏幕，Session 输出日志可能会保存这些内容。

可以关闭 Session 日志。

## `persist ls` 的命令历史会记录密码吗？

实时命令历史不读取 PTY 原始输入，只镜像已经被 bash、zsh 或 fish 原生 history 接受的命令。
密码程序直接从终端读取的内容不会记录。用户在普通命令行中直接写入的 token 或密码，是否进入
记录仍由 Shell 原生规则决定。bash 的 `HISTCONTROL`、`HISTIGNORE` 可直接组合；检测到自定义
`zshaddhistory`、zsh history 过滤选项或 `fish_should_add_to_history` 时，实时镜像会停用，避免
绕过或重复执行用户过滤逻辑。敏感命令应继续使用 Shell 自身的 history 排除机制。

PersistShell 不修改用户的 `.bashrc`、`.zshrc` 或 `config.fish`。临时 history hook 只存在于
当前 Session 的根 Shell 进程中；安装失败时 Shell 仍可使用，但实时历史会显示为不可用。

---

## SSH agent 会传给 Session 吗？

会。启动时仅当 `SSH_AUTH_SOCK` 是绝对 Unix socket 路径时才传入 Shell；普通文件、相对路径
或失效路径会被忽略。不会把该路径写入 snapshot。

---

## 支持哪些 Shell？

当前 M45 基线已验证 bash、zsh、fish 的基础 Session 创建、列表和关闭；未验证的发行版、
终端和复杂 shell 插件不能据此推断为已支持。

---

## 支持 vim/top/less 吗？

目标是支持继续交互。

但 PersistShell 不承诺所有全屏 TUI 程序在所有终端下都能完美恢复画面。

---

## 支持多客户端同时 attach 吗？

支持从另一台电脑 attach 到已有 Session 并继续操作。

同一时刻只有一个 active writer 向 PTY 写入，避免两个终端输入交错。新的可写客户端 attach
后会撤销旧 writer 的写入权；旧客户端不会继续向 PTY 写入。

只读 attach 可以作为可选模式，但不能作为另一台电脑进入会话的唯一方式。

---

## 可以在 root 下用吗？

可以作为当前登录用户使用。

Session 由 per-user daemon 管理，不支持 root 跨用户 attach。

---

## 可以作为堡垒机吗？

不可以。

PersistShell 不是堡垒机，不做集中认证、审批和企业审计。

---

## 为什么不用 JSON 存 metadata？

因为 JSON 并发、迁移、查询和损坏恢复能力较弱。

PersistShell 推荐 SQLite 或 BoltDB。

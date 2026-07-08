# PersistShell FAQ

## PersistShell 是什么？

PersistShell 是一个 Linux 持久交互式 Shell 运行时。

它让 Shell 生命周期独立于 SSH 连接。

SSH 断开后，Shell 和任务继续运行。

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

目标是不会停。

只要 PersistShell daemon 仍然运行，Session 的 PTY 和 Shell 就继续存在。

注意：

Phase 1 不承诺 daemon 崩溃后仍能恢复。

---

## daemon 崩溃会怎样？

Phase 1 中，daemon 持有 PTY master fd。

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

或：

```bash
SH_DISABLE=1 ssh node
```

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

---

## 支持哪些 Shell？

目标支持：

- bash
- zsh
- fish
- sh

Phase 1 至少应支持 bash。

---

## 支持 vim/top/less 吗？

目标是支持继续交互。

但 PersistShell 不承诺所有全屏 TUI 程序在所有终端下都能完美恢复画面。

---

## 支持多客户端同时 attach 吗？

支持从另一台电脑 attach 到已有 Session 并继续操作。

默认策略应是单 active writer：同一时刻只有一个客户端向 PTY 写入，避免两个终端输入交错。新的客户端可以请求接管写入权，旧客户端会被降级、detach 或收到明确提示。

只读 attach 可以作为可选模式，但不能作为另一台电脑进入会话的唯一方式。

---

## 可以在 root 下用吗？

可以作为当前登录用户使用。

Phase 1 不做复杂 root attach 其他用户 Session 的权限模型。

---

## 可以作为堡垒机吗？

不可以。

PersistShell 不是堡垒机，不做集中认证、审批和企业审计。

---

## 为什么不用 JSON 存 metadata？

因为 JSON 并发、迁移、查询和损坏恢复能力较弱。

PersistShell 推荐 SQLite 或 BoltDB。

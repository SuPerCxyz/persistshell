# PersistShell Limitations

本文档记录 PersistShell 当前阶段明确限制。

---

## Phase 1 限制

### 不支持 Pane / Window

PersistShell 不支持：

- pane
- window
- layout
- prefix key

这是非目标，不是缺陷。

---

### 不支持 Web UI

Phase 1 不提供 Web UI。

---

### 不支持 REST API

Phase 1 不提供 REST API。

---

### 不支持 Cluster

Phase 1 只管理本机 Session。

---

### 不支持复杂多用户 daemon

Phase 1 使用 per-user daemon。

不做系统级多用户 daemon。

---

### 不保证 daemon 崩溃后 Session 恢复

Phase 1 只保证：

```text
SSH 断开不导致 Session 结束。
```

不保证：

```text
daemon 崩溃后 Session 仍可恢复。
```

---

### 不保证所有 TUI 完美屏幕恢复

对于 vim/top/less 等全屏程序：

- 应能继续交互。
- 不承诺所有屏幕状态完美恢复。

---

### 默认不记录用户输入

Phase 1 默认只记录输出，不记录输入。

---

### Read-only Attach 暂不支持

Phase 1 可以暂不实现只读查看模式。

---

### 多 active writer 暂不支持

Phase 1 默认只允许一个 active writer，避免两个终端同时写入同一 PTY。

但另一台电脑必须可以 attach 到已有 Session 并请求可写接管，不能只能以只读方式进入。

---

### SSH Agent 同步暂不支持

`SSH_AUTH_SOCK` 动态同步放到后续版本。

---

### 日志脱敏暂不完整

Phase 1 可支持关闭日志。

复杂脱敏放到后续版本。

---

## 限制记录原则

任何暂时接受的限制都必须记录到本文件。

不得把限制伪装成已完成功能。

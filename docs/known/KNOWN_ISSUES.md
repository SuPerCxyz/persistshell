# PersistShell Known Issues

本文档记录当前已知问题。

任何发现但暂未修复的问题，都应记录在这里。

---

## 当前阶段

项目仍处于设计和初始化阶段，尚未实现代码。

---

## KI-0001：Daemon 崩溃恢复限制

状态：

```text
已知限制
```

说明：

Phase 1 设计中，daemon 持有 PTY master fd。

如果 daemon 崩溃，PTY fd 会关闭，Session 可能无法恢复。

影响：

- SSH 断开可恢复。
- Daemon 崩溃不保证恢复。

处理计划：

- Phase 1 文档明确限制。
- 后续研究 supervisor / pty-holder 模型。

---

## KI-0002：全屏 TUI 完美恢复不承诺

状态：

```text
已知限制
```

说明：

vim、top、less、htop 等程序可能使用 alternate screen 和复杂 ANSI 状态。

Ring Buffer 字节回放不一定能完美恢复屏幕状态。

处理计划：

- Phase 1 保证可继续交互。
- 后续研究 terminal state cache。

---

## KI-0003：默认 Ring Buffer 与内存目标存在权衡

状态：

```text
设计注意
```

说明：

基础管理开销目标小于 500KB/session，但 Ring Buffer 按配置额外占用。

例如：

```text
1000 sessions × 8MB = 8GB
```

处理计划：

- 明确区分基础开销和 buffer 开销。
- 默认 ring buffer 不宜过大。
- 支持配置上限。

---

## KI-0004：SSH 自动接管存在登录风险

状态：

```text
设计注意
```

说明：

任何 shell profile hook 都可能导致用户登录异常。

处理计划：

- 必须支持 bypass。
- 必须支持 uninstall。
- install 必须备份。
- hook 必须保守。

# PersistShell Known Issues

本文档记录当前已知问题。

任何发现但暂未修复的问题，都应记录在这里。

---

## 当前阶段

Rust runtime、发布包、完整用户手册和 Performance dashboard 已完成阶段性验证。当前处于
Phase 4 维护阶段；本文件只记录仍未修复的实际限制或可用性问题。

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

---

## KI-0005：`persistd foreground --help` 会启动 daemon

状态：

```text
已知可用性问题
```

说明：

`persistd` 当前只在顶层解析 `help` 或 `--help`。`foreground --help` 会被当成
foreground 启动命令，而不是子命令帮助。

临时做法：

```bash
persistd help
```

处理计划：

- 后续 CLI 可用性任务中支持子命令帮助。

---

## KI-0006：安装器不备份 profile，且不读取 SSH 配置字段

状态：

```text
已知可用性问题
```

说明：

`persist install` 当前直接向 bash/zsh profile 追加 hook，不创建备份。生成的 hook 固定使用
`PERSIST_DISABLE`，不读取 `ssh.auto_hook` 或 `ssh.bypass_env` 配置字段。

临时做法：

- 首次安装前手动备份对应 profile。
- 使用 `PERSIST_DISABLE=1 ssh host` 绕过已安装 hook。

处理计划：

- 后续安装器任务中增加备份、fish profile 支持和配置字段接入。

---

## KI-0007：快速 `cd; exit` 可能保留上一次 cwd

状态：已知恢复精度限制。

说明：cwd 来自 `/proc/<shell-pid>/cwd` 采样。shell 在下一次采样前退出时，进程进入 zombie
后无法再读取最终 cwd。正常运行窗口内的 cwd 与 closed attach 恢复已通过测试。

处理计划：为受支持 Shell 设计不会解析用户命令的退出状态 side channel。

---

## KI-0008：Replay speed/follow 参数尚未生效

状态：已知功能缺口。

说明：`--head`、`--tail` 已生效；`--speed` 与 `--follow` 当前只解析参数。现有纯文本日志
没有时间戳，无法还原原始输出间隔。

处理计划：先定义兼容旧日志的时间信息格式，再用 Linux 事件通知实现 follow。

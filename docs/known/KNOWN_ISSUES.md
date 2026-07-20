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
部分解决，M53 平台和规模验证尚未完成
```

说明：

当前生产 runtime 已由单一 per-user `persist-holder` 持有 PTY master。真实进程测试已证明 daemon
被 `SIGKILL` 后 Shell 继续执行和输出，第二 daemon 可以接管同一 Holder 并 attach replay。

Holder inventory 与 metadata 的 `lost`、orphan、离线退出和 generation 窗口已完成幂等对账；
create、metadata commit、Shell exit 和 reconcile 崩溃窗口已有重复重启集成测试。
Holder 自身异常退出时 daemon 会在有界采样周期内将活动 Session 标记为 `lost` 并拒绝 attach；
该行为防止伪称可恢复，但不能恢复已丢失的 PTY。

影响：

- SSH 断开可恢复。
- Daemon 崩溃后 runtime 可保活并在重启时先对账再开放 public socket。
- Holder 自身崩溃或系统重启后，活动 PTY runtime 仍不可恢复。

处理计划：

- M53 后续阶段完成故障注入、性能、平台和打包验证。
- Holder 自身崩溃后的 PTY 恢复继续作为后续限制。

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

## KI-0007：快速 `cd; exit` 最终 cwd 竞态

状态：已在 M54 解决。

说明：默认 bash、zsh 和 fish 通过私有原子状态文件提交最终 cwd；Holder 在 Shell 退出后保留
上下文，daemon 采用 metadata-first 顺序保存后再 retire。正常 `exit`、空行 `Ctrl+D`、
快速 `cd; exit`、daemon 离线退出和两个崩溃窗口均已验证。

验证证据：`docs/audit/2026-07-20-m54-final-shell-state-validation.md`。

剩余的用户 hook 冲突、强制终止、非 UTF-8 cwd 等降级边界记录在
`docs/known/LIMITATIONS.md`，不再作为本问题的未实现状态。

---

## KI-0008：Replay speed/follow 参数尚未生效

状态：已知功能缺口。

说明：`--head`、`--tail` 已生效；`--speed` 与 `--follow` 当前只解析参数。现有纯文本日志
没有时间戳，无法还原原始输出间隔。

处理计划：先定义兼容旧日志的时间信息格式，再用 Linux 事件通知实现 follow。

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

### 多 writer 同时输入不支持

M35 支持另一台电脑接管写权限，但同一 Session 同一时刻仍只允许一个
active writer。新 writer attach 后会立即撤销旧 writer，避免两个终端的输入交错。

---

### SSH Agent 仅在 PTY 启动时继承

M40 会在创建 PTY 时继承 `SSH_AUTH_SOCK`，但只接受当前用户环境中的绝对 Unix socket 路径；
普通文件、相对路径和失效路径会被忽略。

Session 已启动后不会动态跟踪 `SSH_AUTH_SOCK` 的变化；重新 attach 或修改环境不能替换已启动
Shell 的 agent socket。

---

### Closed Session 环境变量范围

M14 只恢复 Shell 启动时继承的受限环境快照：`TERM`、`COLORTERM`、`LANG` 与
`LC_*`。Shell 运行期间通过 `export`、脚本或插件动态修改的环境变量不保证可恢复。

---

### Closed Session 快速退出的 cwd 采样竞态

cwd 通过 `/proc/<shell-pid>/cwd` 周期采样。若 shell 在一次采样间隔内完成 `cd` 并立即退出，
daemon 只能保留上一次成功采样的 cwd。彻底消除该竞态需要跨 bash/zsh/fish 的退出状态
side channel，当前版本尚未实现。

---

### Replay speed/follow 尚未实现

`persist replay` 已实现完整输出与 `--head`、`--tail` 过滤。当前日志格式没有时间戳，
`--speed` 尚不能按原始节奏回放；`--follow` 尚未持续监听文件变化。这两个参数当前可解析但
不会改变输出行为。

### 实时命令历史依赖 Shell 原生 history

PersistShell 不解析 PTY 输入，也不绕过用户的 history 过滤。用户禁用原生 history 时，实时命令
记录为空。复杂 prompt hook 无法安全组合时，系统优先保持用户配置行为。自定义
`zshaddhistory`、zsh history 过滤选项或 fish `fish_should_add_to_history` 会触发明确降级，实时
历史状态标记为不可用；原生 history、用户 hook、attach 和输出日志不受影响。

实时记录只安装到 PersistShell 启动的根 Shell。用户在其中再次启动的嵌套 Shell 不保证继续同步。

---

### 日志脱敏暂不完整

Phase 1 可支持关闭日志。

复杂脱敏放到后续版本。

### 内部日志轮转暂不完整

M03 只实现内部日志配置解析、基础文件写入、权限设置和级别过滤。

`internal_log.max_file_size` 和 `internal_log.max_files` 已作为配置项存在，但完整轮转逻辑保留到后续任务。

### Performance dashboard 范围

`persist top` 只聚合本机 per-user daemon 和运行中 Session 的进程树，不提供跨机器监控、Web、
Prometheus、cgroup 精确归属或脚本 JSON 输出。共享子进程只归属到最近的一个 Session 根 PID。

15 分钟和 1 小时趋势来自内存，daemon 重启后不会恢复；24 小时趋势来自分钟分段，当前分钟
尚未落盘时可能暂时为空。指标目录最多 24 个分段和 128 MiB，旧数据会自动淘汰。

---

## 限制记录原则

任何暂时接受的限制都必须记录到本文件。

不得把限制伪装成已完成功能。

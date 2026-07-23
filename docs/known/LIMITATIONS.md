# PersistShell Limitations

本文档记录 PersistShell 当前阶段明确限制。

---

## Phase 1 限制

### 发布平台边界

正式包仅支持 Linux x86_64 和 ARM64、glibc 2.28 或更高。当前不提供 i686、ARMv7、
musl/Alpine、EL7 或 macOS 包。

Rocky Linux、CentOS Stream、Ubuntu 和 Debian 由公开 CI 镜像验证。RHEL 与 AlmaLinux
按 EL8/9/10 ABI compatible 提供，不代表对应厂商认证。容器验证共享 runner 内核，因此
旧内核 pidfd fallback 主要由强制 procfs 路径测试覆盖。

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

### daemon 崩溃恢复尚未完成平台规模验证

当前已经验证：

```text
daemon 被 SIGKILL 后 Holder 保持 PTY 和 Shell，第二 daemon 可接管并恢复 attach。
新 daemon 在开放 public socket 前完成 generation 稳定快照和 metadata 幂等对账。
离线退出、missing runtime、orphan 和重复对账均有真实进程测试覆盖。
```

当前仍不保证：

```text
Holder 自身崩溃或系统重启后恢复活动 PTY runtime。
```

Holder 自身崩溃时 daemon 会将受影响 Session 标记为 `lost`，列表、metrics 和 doctor 均明确提示
attach 不可用；这属于一致性降级，不是 PTY 恢复。

100/1000 Session 压力、发布包升级和 test 主机验证将在 M53 后续阶段完成；完成前不扩大当前
本地功能验证结论。

---

### 不保证所有 TUI 完美屏幕恢复

对于 vim/top/less 等全屏程序：

- 应能继续交互。
- Running attach 回放最近的原始 PTY 字节，Closed attach 回放最近的原始日志字节。
- 回放上限由 `ring_buffer.replay_bytes` 控制，超过边界的旧输出不会自动显示。
- 不维护终端模拟器屏幕快照，不承诺所有屏幕状态完美恢复。
- `logging.session_log=false` 时，Closed Session 没有退出前输出可供回放。

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

M55 恢复 `LANG`、`LC_*` 和用户明确 include 的已导出变量，并保留精确 unset。未导出的局部
变量、未允许名称和敏感禁区不会恢复。当前终端、SSH、display 和 agent 变量来自每次 attach，
不会持久化。旧 Holder 没有环境 capability 时降级为 cwd-only 和旧 metadata 快照。

---

### 最终 cwd side channel 的降级边界

默认 bash、zsh 和 fish 已使用私有原子状态文件解决正常退出、空行 `Ctrl+D` 和快速
`cd; exit` 的最终 cwd 采样竞态。PersistShell 以用户配置完整性为第一优先级，因此已有 Bash
`EXIT` trap 时不替换用户 trap，只保留 prompt 提交。

Shell 被 `SIGKILL`、通过 `exec` 替换、用户删除 hook、未安装 hook 的嵌套/不支持 Shell、
非 UTF-8 cwd、状态文件损坏或身份校验失败时，最终提交不可用，系统回退到最近一次
`/proc`/metadata cwd 和上一可信环境。此降级不会阻止 Shell 退出。

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

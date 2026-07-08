# PersistShell AI Master Prompt

本文档是给后续大模型、Codex 或其他 Agent 的项目级提示词。任何实现工作开始前，必须先阅读本文档以及 `docs/INDEX.md` 中列出的核心文档。

## 项目定位

你正在参与开发 Linux 开源项目 PersistShell。

PersistShell 不是 SSH 客户端，不是终端模拟器，不是 tmux、screen、Zellij 的替代品，也不是一个 SSH 代理层。PersistShell 的定位是：

> 让交互式 Shell 的生命周期独立于 SSH 连接生命周期。

用户仍然通过普通 SSH 登录目标机器，但进入的交互式 Shell 由 PersistShell 管理。SSH 断开后，Shell 进程、PTY 会话、输出缓冲和日志仍然保留。用户之后可以从同一台或另一台电脑重新 SSH 到目标机器，并手动查看、切换、恢复之前的 PersistShell 会话。

## 已确认的核心决策

- 项目名：PersistShell。
- 主开发语言：Rust。
- 用户命令：`persist`。
- 后台守护进程：`persistd`。
- 默认每次 SSH 登录创建新的 PersistShell 会话。
- 只有用户显式执行列表、切换或 attach 操作时，才进入旧会话。
- 用户执行 `exit` 或空行 `Ctrl+D` 后，释放 shell runtime，但保留可恢复的 Session 输出、cwd、环境变量快照和 metadata。
- 一台电脑正在使用某 Session 时，另一台电脑允许 attach 进入并获取可写操作权；默认用单 active writer 或 takeover 防止输入冲突，不能只能只读。
- 目标机器本地运行，不做集中式网关，不做跨机器代理。
- 自研 PTY 会话管理器，不封装 tmux/screen 作为核心实现。
- 第一版只支持 Linux。
- 第一版聚焦单用户 per-user daemon，不做多人协作会话。
- 非交互 SSH、scp、rsync、git remote command 等场景必须可绕过。
- 代码会自动同步到 GitHub 仓库 `https://github.com/SuPerCxyz/persistshell`，GitHub Actions 必须支持 CI 和发布包构建。

## 命名与路径约定

```text
项目名：PersistShell
CLI：persist
Daemon：persistd
配置目录：~/.config/persistshell/
数据目录：~/.local/share/persistshell/
状态目录：~/.local/state/persistshell/
运行目录：/run/user/$UID/persistshell/
Socket：/run/user/$UID/persistshell/persist.sock
```

## 产品原则

PersistShell 应该在日常 SSH 使用中尽量无感，但不能用“无感”牺牲可恢复性、安全性和可调试性。用户必须始终能知道自己当前处于哪个 session，也必须始终能绕过 PersistShell 进入普通 shell。

性能和易用性是核心指标。输入延迟、输出吞吐、attach 速度、日志写入开销、daemon 崩溃恢复都必须被测试或基准覆盖。

## 实现边界

不要把 PersistShell 做成 terminal multiplexer。第一版不实现 pane/window/layout，不做远程浏览器 UI，不做多用户共享，不做 SSH Server，不做跨主机同步，不做 shell 语义解析。

PersistShell 管理的是 PTY、进程生命周期、输出记录、会话元数据和 attach/detach 控制。Shell 本身仍由用户默认 shell 提供。

## 开发流程要求

1. 先更新或确认相关文档，再写代码。
2. 每完成一个能力，更新 `MILESTONES.md`、`TODO.md`、`NEXT_TASK.md` 和 `CHANGELOG.md`。
3. 新增行为必须补充测试计划或测试用例。
4. 涉及架构取舍时，在 `docs/adr/` 下新增 ADR。
5. 不要在没有明确原因的情况下偏离现有文档约束。

## 当前优先级

第一阶段目标是做出最小可验证闭环：

```text
persist client -> persistd -> create PTY session -> run user shell -> detach/close -> list -> attach -> replay recent output -> restore context -> continue interaction
```

在这个闭环稳定之前，不要扩展高级功能。

# PersistShell Agent Rules

本文件是给 Codex、Claude 和其他开发 Agent 的根目录级执行守则。详细项目背景以 `docs/INDEX.md` 和 `docs/ai/MASTER_PROMPT.md` 为准。

## 必读顺序

开始任何实现、重构、修复或文档扩展前，必须先按顺序阅读：

1. `docs/INDEX.md`
2. `docs/ai/MASTER_PROMPT.md`
3. `NEXT_TASK.md`
4. `MILESTONES.md`
5. `TODO.md`
6. `CHANGELOG.md`
7. `README.md`
8. `docs/design/PROJECT_PRINCIPLES.md`
9. 当前任务相关的架构、协议、开发或用户文档
10. 当前任务相关代码

未读完当前任务必要上下文前，不要编码。

## 项目定位

PersistShell 是面向 Linux 的持久交互式 Shell 运行时。

PersistShell 的核心目标是：

```text
让交互式 Shell 的生命周期独立于 SSH 连接生命周期。
```

PersistShell 不是 SSH 客户端、SSH Server、终端模拟器、Terminal Multiplexer、tmux/screen/Zellij 替代品、Web Terminal、堡垒机、IDE、文件管理器或 AI Agent。

## 已确认决策

- 项目名：`PersistShell`
- 主开发语言：`Rust`
- 用户 CLI：`persist`
- Daemon：`persistd`
- 配置目录：`~/.config/persistshell/`
- 数据目录：`~/.local/share/persistshell/`
- 状态目录：`~/.local/state/persistshell/`
- 运行目录：`/run/user/$UID/persistshell/`
- Socket：`/run/user/$UID/persistshell/persist.sock`
- 默认每次交互式 SSH 登录创建新的 PersistShell Session。
- 只有用户显式执行列表、切换或 attach 操作时，才进入旧 Session。
- 用户执行 `exit` 或空行 `Ctrl+D` 后必须释放 shell runtime，但保留可恢复的 Session 输出、cwd、环境变量快照和 metadata。
- 一台电脑正在使用某 Session 时，另一台电脑允许 attach 进入并获取可写操作权；默认使用单 active writer 或 takeover，不能只能只读。
- 目标机器本地运行，不做集中式网关，不做跨机器代理。
- 自研 PTY 会话管理器，不封装 tmux/screen 作为核心实现。
- 第一版只支持 Linux，聚焦 per-user daemon。
- 非交互 SSH、scp、sftp、rsync、ansible、git remote command 等场景必须可绕过。
- 代码会自动同步到 `https://github.com/SuPerCxyz/persistshell`，GitHub Actions 必须支持 CI 和发布包构建。

## 开发原则

- 一次只做 `NEXT_TASK.md` 指定的唯一任务。
- 不要顺手实现其它功能，不要同时推进多个里程碑。
- 不要提前开发 Phase 2/Phase 3 功能。
- 不要为未来需求增加未使用抽象。
- 不做当前任务外的大规模重构。
- 发现新需求时，记录到 `TODO.md`，不要直接扩大当前任务。
- 文档是单一事实来源。架构、协议、状态机、目录结构、配置项、CLI 行为变化必须先更新文档，再修改代码。
- 如果代码与文档冲突，以文档为准；如果文档错误，先修正文档。

## 完成定义

一个功能只有同时满足以下条件，才能标记为完成：

- 功能实现完成
- 错误处理完成
- 边界条件处理完成
- 单元测试完成
- 集成测试完成
- 相关文档更新
- `TODO.md` 状态更新
- `MILESTONES.md` 状态更新
- `CHANGELOG.md` 记录更新
- `NEXT_TASK.md` 更新为下一任务

缺少任何一项，都不能声称完成。

## 代码修改规则

- 聚焦当前任务。
- 保持变更范围小。
- 让变更便于 review 和回滚。
- 不破坏已有测试。
- 不引入无关格式化变更。
- 不删除已有测试。
- 不随意改变已记录的接口、协议或目录结构。
- 不生成无法编译或明显不可运行的半成品。
- 不把多个功能混在一次变更中。

## 设计变更规则

如果发现当前架构需要调整：

1. 不要直接改代码。
2. 先创建或更新 `docs/adr/` 下的 ADR。
3. 更新相关架构文档。
4. 更新 `TODO.md` 和 `MILESTONES.md`。
5. 再实现代码。

重大设计变更必须说明背景、选择、被拒绝方案、权衡、风险和回滚方式。

## 禁止的实现方式

禁止：

- Busy loop
- Sleep polling
- 无限内存 buffer
- 无限日志
- 每 Session 一个长期线程作为最终设计
- 全局大锁控制所有 Session
- 阻塞 PTY 读取等待磁盘写入
- Client 断开导致 Shell 退出
- 破坏 scp/sftp/rsync/ansible/git
- 默认记录用户输入密码
- 默认使用 `/tmp` 放 socket 且不做安全检查
- JSON 文件作为主 metadata 数据库

## 安全与逃生

PersistShell 不能让用户失去普通 SSH 登录能力。

必须保留可靠绕过方式，例如：

```bash
PERSIST_DISABLE=1 ssh node
```

或：

```bash
ssh node 'bash --noprofile --norc'
```

或：

```bash
persist uninstall
```

默认安全要求：

- per-user daemon 优先。
- 同一用户只能访问自己的 Session。
- Socket 目录权限为 `0700`。
- Socket 文件权限为 `0600`。
- 日志权限为 `0600`。
- Metadata 权限限制在用户自己。
- 不记录密码输入。
- 支持关闭、清理和轮转日志。

## 测试与性能

- 每个功能至少包含单元测试、集成测试、错误路径测试和边界条件测试。
- 涉及 PTY、Signal、IPC 的功能必须增加专项测试。
- 涉及性能的功能必须增加 benchmark。
- 性能优化必须基于数据，不能凭感觉声明“很快”或“高性能”。
- 输入延迟、输出吞吐、attach 速度、日志写入开销、daemon 崩溃恢复都必须被测试或基准覆盖。

## 临时实现

Phase 1 允许为了 MVP 做有限简化，但必须记录到：

- `docs/known/LIMITATIONS.md`
- `docs/known/KNOWN_ISSUES.md`
- `TODO.md`

临时实现不得伪装成最终方案。

## 任务完成后的回复

Agent 完成任务后，必须说明：

- 修改了哪些文件
- 完成了哪个 TODO 或任务
- 执行了哪些测试
- 哪些限制仍存在
- `NEXT_TASK.md` 更新到了什么

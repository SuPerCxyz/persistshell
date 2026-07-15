# 已完成里程碑实现审计

本文件记录对标记为完成的 M01-M36 的源码、测试、文档与真实运行入口的核验。
状态含义：`通过` 表示已有实现与自动化证据；`修复中` 表示已定位不一致且正在处理；
`待核验` 表示尚未完成端到端核验。不得仅以里程碑标记作为实现证据。

## 核验顺序

1. M01-M06：构建、配置、错误、IPC、daemon runtime。
2. M07-M14：PTY、attach、信号、Session、metadata、ring buffer、日志、退出恢复。
3. M15-M22：多客户端、SSH 接管、兼容性、CLI、压力与信号。
4. M23-M36：易用性、恢复和观测增强。

## 已发现事项

| 范围 | 状态 | 证据与处理 |
|---|---|---|
| M06 daemon runtime | 通过 | `persistd foreground` 已有 PID/socket/accept loop/关闭路径；本地进程集成与 2026-07-15 `test` 主机 E2E 均覆盖启动、创建、列表、关闭和清理。 |
| M06 CLI 启动目录 | 通过 | `persist daemon start` 先创建并收紧 runtime 目录权限，再创建 daemon log。 |
| M10/M11 新建 Session metadata | 通过 | `NEW_SESSION` 成功创建 PTY 后同步写入 metadata，失败时回收 runtime。 |
| M10 Session ID 重启分配 | 通过 | daemon 从 metadata 最大 Session ID 推导下一 ID，进程集成测试覆盖重启。 |
| M10 Close/Kill 状态同步 | 通过 | Close/Kill 释放 runtime，并持久化 closed 状态、退出码和恢复上下文。 |
| M18 Rename | 通过 | Rename 同步校验 runtime、更新内存名称与 metadata，并返回实际错误。 |
| M14 Closed Session 列表 | 通过 | `LIST_SESSIONS` 合并 metadata 中无 runtime 的 closed 记录。 |
| M14 自然退出与恢复快照 | 通过 | 自然 exit 释放 runtime、持久化退出码/cwd/安全启动环境；closed attach 创建新的可写 PTY。 |
| M14 部分恢复上下文捕获 | 通过 | 逐字段合并 cwd 与环境快照，避免后续不完整 `/proc` 捕获覆盖先前值；M50 连续八次集成回归通过。 |
| M16 SSH 自动接管 | 通过 | hook 检查 `SSH_TTY` 与 `PERSIST_DISABLE`，再执行 `persist daemon start` 和 `persist attach`；安装器单元测试覆盖该 hook 内容。 |
| M19 Shell completion | 通过 | M48 补齐 bash，并新增 zsh/fish；定向脚本验证静态命令、Session ID 候选及无 daemon 副作用。 |
| M23 里程碑状态 | 通过 | `MILESTONES.md` 的重复 M23 已统一为已完成；现有单元测试覆盖 shell/cwd 命名。 |

## 证据要求

每个里程碑至少记录：对应代码入口、直接测试、真实 daemon 验证结果和文档状态。
无法满足时，必须修复或将里程碑降为未完成，不得保留错误的完成标记。

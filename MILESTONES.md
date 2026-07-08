# PersistShell Milestones

本文件是项目里程碑列表。

每完成一个里程碑，必须同步更新：

- TODO.md
- CHANGELOG.md
- NEXT_TASK.md
- 相关设计文档
- 测试文档

状态标记：

```text
☐ 未开始
◐ 进行中
☑ 已完成
✗ 已取消
```

---

## Phase 0：项目准备

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M00 | 文档体系初始化 | ☐ 未开始 | 建立 README、原则、路线图、架构文档、开发规范 |
| M01 | 工程初始化 | ☐ 未开始 | 初始化 Rust workspace、构建系统、目录结构、GitHub Actions CI、测试框架 |

---

## Phase 1：MVP Core

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M02 | 基础配置系统 | ☐ 未开始 | 加载全局配置和用户配置，提供默认配置 |
| M03 | 基础日志框架 | ☐ 未开始 | Daemon/Client 内部日志，不是 Session 输出日志 |
| M04 | 错误处理框架 | ☐ 未开始 | 统一错误类型、错误码、用户友好提示 |
| M05 | Unix Socket IPC 雏形 | ☐ 未开始 | Client 与 Daemon 可以通信 |
| M06 | Daemon 基础生命周期 | ☐ 未开始 | daemon start/stop/status，单用户运行 |
| M07 | PTY Engine MVP | ☐ 未开始 | openpty、fork、exec 默认 shell |
| M08 | Client Attach MVP | ☐ 未开始 | Client 可以 attach 到一个 PTY Shell |
| M09 | Raw Mode 与基础 I/O 转发 | ☐ 未开始 | 本地输入输出正常转发 |
| M10 | Window Resize | ☐ 未开始 | 支持 SIGWINCH 和 TIOCSWINSZ |
| M11 | Signal 转发 | ☐ 未开始 | Ctrl+C、Ctrl+Z、Ctrl+\、EOF |
| M12 | Session Manager MVP | ☐ 未开始 | create/list/attach/detach/close/kill |
| M13 | Metadata Store MVP | ☐ 未开始 | SQLite/BoltDB 存储 Session 元数据 |
| M14 | Ring Buffer MVP | ☐ 未开始 | 固定大小输出缓存和 attach 回放 |
| M15 | Session 输出日志 | ☐ 未开始 | 异步写日志、基础轮转 |
| M16 | Closed Session 恢复 | ☐ 未开始 | exit/Ctrl-D 后释放 runtime，attach 时恢复输出、cwd、env |
| M17 | 多电脑可写 attach | ☐ 未开始 | active writer lease 与 takeover |
| M18 | SSH 自动接管 | ☐ 未开始 | 只接管交互式 SSH |
| M19 | 非交互兼容 | ☐ 未开始 | scp/sftp/rsync/ansible/git 不受影响 |
| M20 | CLI 基础命令 | ☐ 未开始 | persist ls/new/attach/kill/rename/doctor |
| M21 | 安装与卸载 | ☐ 未开始 | install/uninstall/bypass/doctor |
| M22 | 基础兼容性测试 | ☐ 未开始 | bash/zsh/fish/vim/top/less 等 |
| M23 | 基础压力测试 | ☐ 未开始 | 多 Session、大输出、频繁 attach/detach |
| M24 | v0.1 MVP 验收 | ☐ 未开始 | 验证核心场景全部可用 |

---

## Phase 2：易用性增强

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M25 | 自动 Session 命名 | ☐ 未开始 | 根据时间、cwd、前台进程生成可读名称 |
| M26 | Session Notes | ☐ 未开始 | 添加备注 |
| M27 | Session Tags | ☐ 未开始 | 标签和筛选 |
| M28 | Pin Session | ☐ 未开始 | 收藏 Session，避免 GC |
| M29 | 日志搜索 | ☐ 未开始 | search/grep |
| M30 | 日志导出 | ☐ 未开始 | export/tail/log |
| M31 | 独立 History | ☐ 未开始 | 每 Session 独立历史文件 |
| M32 | Idle Detection | ☐ 未开始 | 空闲时间显示和清理策略 |
| M33 | 更完善 GC | ☐ 未开始 | 按时间、状态、大小清理 |
| M34 | 更完善 doctor | ☐ 未开始 | 自动诊断常见问题 |

---

## Phase 3：恢复和观测增强

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M35 | Replay Mode | ☐ 未开始 | 历史输出回放 |
| M36 | Read-only Attach | ☐ 未开始 | 只读查看，不允许输入 |
| M37 | 多 active writer 协作 | ☐ 未开始 | 明确冲突处理后再允许同时写入 |
| M38 | Session Lock | ☐ 未开始 | 锁定重要 Session |
| M39 | Foreground Process Tracking | ☐ 未开始 | 识别当前前台进程 |
| M40 | Process Tree View | ☐ 未开始 | 查看子进程树 |
| M41 | Resource Monitor | ☐ 未开始 | CPU/MEM/IO 信息 |
| M42 | SSH Agent Sync | ☐ 未开始 | 同步 SSH_AUTH_SOCK |
| M43 | Snapshot | ☐ 未开始 | Session 快照 |
| M44 | Metrics | ☐ 未开始 | 基础 metrics |

---

## Phase 4：稳定性和发布

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M45 | 性能 Benchmark | ☐ 未开始 | 100/500/1000 Session 测试 |
| M46 | 安全审查 | ☐ 未开始 | Socket、日志、权限、注入风险 |
| M47 | 兼容性矩阵 | ☐ 未开始 | 主流发行版、Shell、Terminal |
| M48 | 打包 | ☐ 未开始 | GitHub Actions 构建 tarball/deb/rpm 和 checksums |
| M49 | Man Page | ☐ 未开始 | 命令手册 |
| M50 | Shell Completion | ☐ 未开始 | bash/zsh/fish completion |
| M51 | v1.0 文档完善 | ☐ 未开始 | 用户文档、FAQ、故障排查 |
| M52 | v1.0 Release | ☐ 未开始 | 生产可用版本 |

---

## 里程碑规则

1. 不允许跳过 Phase 1 直接开发 Phase 2。
2. 不允许同时开发多个里程碑。
3. 每个里程碑必须有完成标准。
4. 完成标准必须包含测试和文档。
5. 如果里程碑设计变化，必须更新本文件。
6. 如果发现新需求，先写入 TODO.md，不得直接实现。

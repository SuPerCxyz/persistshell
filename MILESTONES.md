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
| M00 | 文档体系初始化 | ☑ 已完成 | 建立 README、原则、路线图、架构文档、开发规范 |
| M01 | 工程初始化 | ☑ 已完成 | 初始化 Rust workspace、构建系统、目录结构、GitHub Actions CI、测试框架 |

---

## Phase 1：MVP Core

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M02 | 基础配置系统 | ☑ 已完成 | 加载系统配置和用户配置，提供默认配置、校验和配置查看命令 |
| M03 | 基础日志框架 | ☑ 已完成 | Daemon/Client 内部日志配置、文件写入、级别过滤和权限基础，不是 Session 输出日志 |
| M04 | 错误处理框架 | ☑ 已完成 | 统一错误类型、错误码、用户友好提示 |
| M05 | Unix Socket IPC 雏形 | ☑ 已完成 | Client 与 Daemon 可以通信 |
| M06 | Daemon 基础生命周期 | ☑ 已完成 | daemon start/stop/status，单用户运行 |
| M07 | PTY Engine MVP | ☑ 已完成 | openpty、fork、exec 默认 shell |
| M08 | Client Attach MVP | ☑ 已完成 | Client 可以 attach 到一个 PTY Shell |
| M09 | Signal & Resize MVP | ☑ 已完成 | SIGPIPE忽略、SIGWINCH转发、Ctrl+C/Z/\通过raw mode+STDIN字节自动处理 |
| M10 | Session Manager CLI | ☑ 已完成 | persist new/ls/close/kill 命令 + Session close/kill IPC |
| M11 | Metadata Store MVP | ☑ 已完成 | SQLite 存储 Session 元数据 |
| M12 | Ring Buffer MVP | ☑ 已完成 | 固定大小输出缓存和 attach 回放 |
| M13 | Session 输出日志 | ☑ 已完成 | 异步写日志、基础轮转、CLI persist log |
| M14 | Closed Session 恢复 | ☑ 已完成 | 释放 runtime、保存 cwd/安全启动环境快照，并可 attach 冷恢复 |
| M15 | 多电脑可写 attach | ☑ 已完成 | pipe 信号 takeover，单 active writer |
| M16 | SSH 自动接管 | ☑ 已完成 | 只接管交互式 SSH，shell hook auto-attach |
| M17 | 非交互兼容 | ☑ 已完成 | hook 只在 SSH_TTY 生效，非交互自动绕过 |
| M18 | CLI 基础命令 | ☑ 已完成 | doctor/rename/attach \<id\>/CLI polish |
| M19 | CLI 补全与改进 | ☑ 已完成 | detach/shell completion/daemon crash prompt/ls formatting |
| M20 | 基础兼容性测试 | ☑ 已完成 | PTY 集成测试框架 + echo/pipe/多命令/重定向 4 个测试 |
| M21 | 基础压力测试 | ☑ 已完成 | 多 Session、大输出、频繁 attach/detach |
| M22 | Signal 处理 | ☑ 已完成 | Ctrl+C Ctrl+Z 信号转发到 PTY 前端进程组 |
| M23 | 自动 Session 命名 | ☑ 已完成 | 根据时间、cwd、前台进程生成可读名称 |
| M24 | Session Notes | ☑ 已完成 | 添加备注 |

---

## Phase 2：易用性增强

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M23 | 自动 Session 命名 | ☑ 已完成 | 根据 shell 和 cwd 生成可读名称 |
| M24 | Session Notes | ☑ 已完成 | 添加备注 |
| M25 | Session Tags | ☑ 已完成 | 标签和筛选 |
| M26 | Pin Session | ☑ 已完成 | 收藏 Session，避免 GC |
| M27 | 日志搜索 | ☑ 已完成 | persist log search \<keyword\> 搜索所有 Session 日志，支持 --session 和 -i |
| M28 | 日志导出 | ☑ 已完成 | persist log export \<session-id\> [--output \<path\>]，支持 stdout 导出 |
| M29 | 独立 History | ☑ 已完成 | 每 Session 独立 HISTFILE 文件，PTY child_setup 中设置环境变量 |
| M30 | Idle Detection | ☑ 已完成 | 空闲时间显示 |
| M31 | Idle GC | ☑ 已完成 | 空闲 Session 自动清理 |
| M32 | 更完善 doctor | ☑ 已完成 | 自动诊断常见问题 |

---

## Phase 3：恢复和观测增强

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M33 | Replay Mode | ☑ 已完成 | 历史输出回放 |
| M34 | Read-only Attach | ☑ 已完成 | 只读查看，不允许输入 |
| M35 | 多 active writer 协作 | ☑ 已完成 | 通知后立即交接，旧 writer 输入隔离 |
| M36 | Session Lock | ☑ 已完成 | 持久化锁定状态，阻止 attach、kill 和 Idle GC |
| M37 | Foreground Process Tracking | ☑ 已完成 | 识别并展示当前前台进程 |
| M38 | Process Tree View | ☑ 已完成 | 有界前台进程树查询 |
| M39 | Resource Monitor | ☑ 已完成 | 前台进程 CPU/RSS/I/O 计数 |
| M40 | SSH Agent Sync | ☑ 已完成 | 仅继承有效 Unix agent socket |
| M41 | Snapshot | ☑ 已完成 | 受限、只读的 Session JSON 快照 |
| M42 | Metrics | ☑ 已完成 | daemon 与 Session 聚合 metrics |

---

## Phase 4：稳定性和发布

| 编号 | 里程碑 | 状态 | 说明 |
|---|---|---|---|
| M43 | 性能 Benchmark | ☑ 已完成 | 本地与 test 主机 100/500/1000 Session 基准 |
| M44 | 安全审查 | ☑ 已完成 | Socket、日志、权限、注入风险审查及修复 |
| M45 | 兼容性矩阵 | ☑ 已完成 | 当前可访问发行版、Shell、Terminal 基线 |
| M46 | 打包 | ☑ 已完成 | 本地 tar/deb、test 原生 rpm、GitHub Actions 构建入口和 checksums |
| M47 | Man Page | ☑ 已完成 | persist/persistd groff 手册、三种包接入和 test RPM 验证 |
| M48 | Shell Completion | ☑ 已完成 | bash/zsh/fish completion、三种包接入和 test RPM 验证 |
| M49 | v1.0 文档完善 | ☑ 已完成 | 用户文档、FAQ、故障排查、三种包文档验证 |
| M50 | v0.1.0 Release | ☑ 已完成 | tag、平台 workflow、artifact 与 test 部署已验证；GitHub Release、签名和 SBOM 暂缓 |
| M51 | 交互式命令历史与完整用户手册 | ☑ 已完成 | `persist ls` 交互选择、实时命令历史和单文件完整用户手册 |
| M52 | Performance dashboard | ◐ 进行中 | Ratatui 全屏界面已完成，当前执行性能、文档与发布验证 |

---

## 里程碑规则

1. 不允许跳过 Phase 1 直接开发 Phase 2。
2. 不允许同时开发多个里程碑。
3. 每个里程碑必须有完成标准。
4. 完成标准必须包含测试和文档。
5. 如果里程碑设计变化，必须更新本文件。
6. 如果发现新需求，先写入 TODO.md，不得直接实现。

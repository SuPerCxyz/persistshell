# PersistShell Documentation Index

本文档是 PersistShell 仓库的文档入口。首次接手项目时，请按顺序阅读。

## 快速入口

- `README.md`：项目概览、目标和基本使用方向。
- `docs/ai/MASTER_PROMPT.md`：给 Codex/大模型/Agent 的项目级提示词。
- `docs/ai/HANDOFF.md`：本轮文档生成的交接说明。
- `NEXT_TASK.md`：下一步任务。
- `MILESTONES.md`：阶段性里程碑。
- `TODO.md`：待办清单。

## 产品与设计

- `docs/design/PROJECT_PRINCIPLES.md`
- `docs/design/PRODUCT_PHILOSOPHY.md`
- `docs/design/NON_GOALS.md`
- `docs/design/LICENSING.md`

## 架构

- `docs/architecture/ARCHITECTURE.md`
- `docs/architecture/COMPONENTS.md`
- `docs/architecture/LIFECYCLE.md`
- `docs/architecture/SESSION_MODEL.md`
- `docs/architecture/PTY_ENGINE.md`
- `docs/architecture/PROCESS_MODEL.md`
- `docs/architecture/SIGNAL_MODEL.md`
- `docs/architecture/IPC_PROTOCOL.md`
- `docs/architecture/RINGBUFFER.md`
- `docs/architecture/LOGGER.md`
- `docs/architecture/METADATA.md`

## 协议

- `docs/protocol/SESSION_PROTOCOL.md`
- `docs/protocol/SOCKET_PROTOCOL.md`
- `docs/protocol/CLIENT_PROTOCOL.md`
- `docs/protocol/HOLDER_PROTOCOL.md`

## 开发规范

- `docs/development/DEVELOPMENT_RULES.md`
- `docs/development/CODING_STYLE.md`
- `docs/development/DIRECTORY_LAYOUT.md`
- `docs/development/ERROR_HANDLING.md`
- `docs/development/LOGGING.md`
- `docs/development/TESTING.md`
- `docs/development/BENCHMARK.md`
- `docs/benchmark/PERFORMANCE.md`
- `docs/development/CI.md`

## 用户文档

- `docs/user/USER_GUIDE.md`：普通用户唯一需要阅读的完整手册。
- `docs/user/INSTALL.md`
- `docs/user/CONFIG.md`
- `docs/user/COMMANDS.md`
- `docs/user/FAQ.md`
- `docs/user/TROUBLESHOOTING.md`
- `docs/man/persist.1`
- `docs/man/persistd.1`

## 维护与社区

- `CONTRIBUTING.md`
- `CHANGELOG.md`
- `SECURITY.md`
- `SUPPORT.md`
- `CODE_OF_CONDUCT.md`
- `docs/known/KNOWN_ISSUES.md`
- `docs/known/LIMITATIONS.md`
- `docs/release/RELEASE_CHECKLIST.md`
- `docs/audit/2026-07-15-m50-release-readiness.md`
- `docs/audit/2026-07-15-m50-platform-package-remote-validation.md`
- `docs/audit/2026-07-16-m52-performance-dashboard-validation.md`
- `docs/audit/2026-07-19-m53-pty-holder-validation.md`
- `docs/audit/2026-07-20-m54-final-shell-state-validation.md`
- `docs/audit/2026-07-20-m55-dynamic-environment-recovery-validation.md`
- `docs/audit/2026-07-20-m56-portable-packages-validation.md`
- `docs/adr/ADR-0000-template.md`
- `docs/adr/ADR-0001-rust-primary-language.md`
- `docs/adr/ADR-0002-closed-session-recovery-context.md`
- `docs/adr/ADR-0003-transient-shell-history-hooks.md`
- `docs/adr/ADR-0004-bounded-performance-dashboard.md`
- `docs/adr/ADR-0005-per-user-pty-holder.md`
- `docs/adr/ADR-0006-final-shell-state-side-channel.md`
- `docs/adr/ADR-0007-safe-dynamic-environment-recovery.md`
- `docs/adr/ADR-0008-portable-linux-packages.md`
- `docs/adr/ADR-0009-bounded-attach-history-replay.md`
- `docs/superpowers/specs/2026-07-20-m54-final-shell-state-side-channel-design.md`
- `docs/superpowers/specs/2026-07-20-m54-final-shell-state-side-channel-implementation-plan.md`
- `docs/superpowers/specs/2026-07-20-m55-dynamic-environment-recovery-design.md`
- `docs/superpowers/specs/2026-07-20-m55-dynamic-environment-recovery-implementation-plan.md`
- `docs/superpowers/specs/2026-07-20-m56-portable-packages-design.md`
- `docs/superpowers/specs/2026-07-23-m57-attach-continuity-design.md`
- `docs/superpowers/specs/2026-07-23-m57-attach-continuity-implementation-plan.md`
- `docs/design/LICENSING.md`

## 当前约束

Rust runtime、PTY、daemon、IPC streaming、SSH 接管、安装器和平台发布包均已完成阶段性验证。
M52 已完成有界 Performance dashboard、本地 100/1000 Session 性能门禁、平台包和 Rocky
test 主机验证；GitHub Release、签名与 SBOM 仍须维护者决策。
M53 已完成单一 per-user PTY Holder、daemon 崩溃接管、metadata 对账、固定路径平台包、
100/500/1000 Session 基准及 Rocky test 故障注入验证。
M54 已完成最终 cwd 私有原子状态文件、Holder 离线退出上下文、metadata-first 对账、
Bash/Zsh/Fish 兼容、双平台包和 Rocky test 验证。
M55 已完成安全动态环境 set/unset、当前连接优先、版本化状态、旧 Holder 降级、性能、
双平台包和 Rocky test 验证。
M56 已完成 GLIBC_2.28 的 x86_64/ARM64 通用 RPM、DEB、tar.xz、体积门禁和
Rocky/CentOS Stream、Ubuntu/Debian 多发行版安装运行验证。

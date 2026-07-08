# PersistShell Changelog

所有重要变更都记录在本文档。

格式参考 Keep a Changelog。

---

## Unreleased

### Added

- 初始化 Rust Cargo workspace，新增 `persist` CLI、`persistd` daemon 骨架和 core/pty/ipc/metadata crate 边界。
- 添加 Rust fmt、clippy、test 验证，以及 GitHub Actions CI/package workflow。
- 添加基础错误、配置路径、日志初始化和 Session 状态模型。
- 添加 GitHub Actions CI 与发布包构建要求，记录 GitHub 镜像仓库 `https://github.com/SuPerCxyz/persistshell`。
- 初始化 PersistShell 项目文档体系。
- 添加 README.md。
- 添加项目原则文档。
- 添加产品哲学文档。
- 添加非目标文档。
- 添加路线图。
- 添加里程碑。
- 添加 TODO。
- 添加 NEXT_TASK。
- 添加架构设计文档。
- 添加开发规范文档。
- 添加协议文档。
- 添加用户文档。
- 添加已知问题和限制文档。
- 添加 ADR 模板。

### Changed

- 将 M00/M01 标记完成，下一任务更新为 M02 基础配置系统。
- 确定 PersistShell 主开发语言为 Rust，并同步 Agent 规则、开发规范和目录结构文档。
- 调整 Session 退出语义：`exit`/`Ctrl-D` 进入 Closed 状态，释放 shell runtime，但保留可恢复的输出、cwd、环境变量快照和 metadata。
- 明确另一台电脑可以 attach 到已有 Session 并获取可写操作权；只读 attach 只是可选模式，不是跨电脑进入会话的唯一方式。

### Fixed

- 无。

### Removed

- 无。

# PersistShell AI Handoff

本文档记录本轮从共享会话提取出的项目上下文，供后续 Agent 在没有原始聊天记录时继续工作。

## 来源

项目文档根据共享会话“SSH任务持久化工具设计”整理生成。会话中的最终命名已确定为 PersistShell，CLI 为 `persist`，daemon 为 `persistd`。

## 当前仓库状态

当前阶段已经建立文档体系和 Rust 工程骨架。

已存在：

- Cargo workspace。
- `persist` CLI 骨架。
- `persistd` daemon 骨架。
- `persist-core`、`persist-pty`、`persist-ipc`、`persist-metadata` crate 边界。
- GitHub Actions CI 和 package workflow。

尚未实现：

- PTY Engine。
- daemon runtime。
- IPC streaming。
- metadata database。
- SSH 自动接管和安装器。

## 后续 Agent 启动顺序

1. 读取 `docs/INDEX.md`。
2. 读取 `docs/ai/MASTER_PROMPT.md`。
3. 读取 `README.md`、`docs/design/PROJECT_PRINCIPLES.md`、`docs/design/NON_GOALS.md`。
4. 根据当前任务读取相关架构、协议、开发文档。
5. 执行任务前更新 `NEXT_TASK.md` 或确认其仍然准确。

## 关键上下文

- PersistShell 运行在目标机器本地，不需要代理层。
- 默认每次 SSH 登录进入新的 PersistShell 会话，避免误复用旧环境。
- 旧会话通过显式命令恢复。
- 必须提供绕过和禁用方式，避免 SSH 登录被工具故障阻断。
- 必须保护同一机器上不同用户之间的 session 隔离。
- 必须谨慎处理环境变量、SSH agent、终端尺寸、信号、日志权限和 crash recovery。

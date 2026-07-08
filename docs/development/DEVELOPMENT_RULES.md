# PersistShell Development Rules

本文件定义 PersistShell 的开发规则。

任何开发者或大模型参与本项目时，都必须遵守本文件。

---

## 总原则

PersistShell 是 Linux 基础设施软件，不是一次性 Demo。

开发目标是：

- 稳定
- 可维护
- 可测试
- 可诊断
- 高性能
- 长期可演进

任何实现都不得牺牲这些目标。

---

## 每次开发前必须阅读

新的开发会话开始时，必须按顺序阅读：

1. `NEXT_TASK.md`
2. `MILESTONES.md`
3. `TODO.md`
4. `CHANGELOG.md`
5. `README.md`
6. `docs/design/PROJECT_PRINCIPLES.md`
7. 当前任务相关架构文档
8. 当前任务相关代码

未阅读前不得编码。

---

## 一次只做一个任务

每次开发只能完成 `NEXT_TASK.md` 中指定的唯一任务。

禁止：

- 顺手实现其它功能
- 同时推进多个里程碑
- 提前开发 Phase 2/Phase 3 功能
- 为未来需求增加未使用抽象
- 在当前任务外做大规模重构

如果发现新需求，只能记录到 `TODO.md`。

---

## 文档先于代码

PersistShell 采用：

```text
Docs as Single Source of Truth
```

任何架构、协议、状态机、目录结构、配置项、CLI 行为变化，必须先更新文档，再修改代码。

如果代码与文档冲突，以文档为准。

如果文档错误，先修正文档，再修代码。

---

## 功能完成定义

一个功能只有同时满足以下条件，才能标记为完成：

- 功能实现完成
- 错误处理完成
- 边界条件处理完成
- 单元测试完成
- 集成测试完成
- 相关文档更新
- TODO.md 状态更新
- MILESTONES.md 状态更新
- CHANGELOG.md 记录更新
- NEXT_TASK.md 更新为下一任务

缺少任何一项，都不能标记为完成。

---

## CI 和包构建规则

PersistShell 代码会自动同步到 GitHub 仓库：

```text
https://github.com/SuPerCxyz/persistshell
```

GitHub Actions 必须支持常规 CI 和发布包构建。

常规 CI 至少包含：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`

包构建 workflow 必须能在 GitHub Actions 中构建 release artifacts，至少包含 Linux tarball 和 checksum；后续发布阶段扩展 `.deb` 和 `.rpm`。

workflow 不得依赖内网 Git 地址、开发者本机路径或私有 SSH 配置。

---

## 代码修改原则

代码修改必须：

- 聚焦当前任务
- 保持变更范围小
- 便于 review
- 便于回滚
- 不破坏已有测试
- 不引入无关格式化变更

禁止一次提交混合多个主题。

---

## 设计变更规则

如果发现当前架构需要调整：

1. 不要直接改代码。
2. 先创建或更新 ADR。
3. 更新相关架构文档。
4. 更新 TODO/MILESTONES。
5. 再实现代码。

重大设计变更必须说明：

- 背景
- 选择
- 被拒绝方案
- 权衡
- 风险
- 回滚方式

---

## 大模型开发规则

大模型参与开发时必须：

- 不猜测已有实现
- 先读文档
- 先读代码
- 不自行扩大需求
- 不随意改变接口
- 不删除已有测试
- 不绕过 TODO/NEXT_TASK
- 不生成无法编译的半成品
- 不把多个功能混在一次变更中

大模型完成任务后，必须输出：

- 修改了哪些文件
- 完成了哪个 TODO
- 执行了哪些测试
- 哪些限制仍存在
- NEXT_TASK 更新到了什么

---

## 不允许的实现方式

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

---

## 允许的临时实现

Phase 1 允许为了 MVP 做有限简化，但必须记录到：

- `docs/known/LIMITATIONS.md`
- `docs/known/KNOWN_ISSUES.md`
- `TODO.md`

临时实现不得伪装成最终方案。

例如：

```text
Phase 1 daemon 崩溃后不保证 PTY 恢复
```

这是允许的，但必须明确记录。

---

## 测试要求

每个功能至少包含：

- 单元测试
- 集成测试
- 错误路径测试
- 边界条件测试

涉及 PTY/Signal/IPC 的功能必须增加专项测试。

涉及性能的功能必须增加 benchmark。

---

## 性能规则

性能优化必须基于数据。

禁止：

- 凭感觉优化
- 未测试就声称高性能
- 为微小性能收益牺牲可读性
- 过早引入复杂 lock-free 结构

必须记录：

- 测试环境
- 测试命令
- 优化前结果
- 优化后结果
- 影响范围

---

## 安全规则

涉及以下内容时必须额外谨慎：

- socket 权限
- metadata 权限
- 日志权限
- 环境变量
- shell profile 注入
- sudo/root 场景
- `/tmp` fallback
- symlink attack
- 用户输入
- 日志脱敏

任何安全相关变更必须更新 `SECURITY.md` 或相关文档。

---

## 兼容性规则

不得破坏：

```bash
ssh node command
scp file node:/tmp/
sftp node
rsync file node:/tmp/
ansible all -m ping
git clone user@node:repo.git
```

SSH 自动接管功能必须有绕过方式。

---

## 发布前规则

发布任何版本前必须完成：

- CHANGELOG.md 更新
- ROADMAP.md 状态更新
- MILESTONES.md 状态更新
- TEST_PLAN.md 更新
- PERFORMANCE.md 更新
- KNOWN_ISSUES.md 更新
- LIMITATIONS.md 更新
- 安装/卸载测试
- 回滚测试
- 基础安全检查

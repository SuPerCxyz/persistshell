# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 0：项目准备

---

## 当前里程碑

M00：文档体系初始化

---

## 当前唯一任务

创建 PersistShell 项目的完整文档体系和基础仓库结构。

本任务只创建文档和目录结构，不实现业务代码。

---

## 需要创建的项目级文件

- README.md
- LICENSE
- CONTRIBUTING.md
- CODE_OF_CONDUCT.md
- SECURITY.md
- SUPPORT.md
- ROADMAP.md
- CHANGELOG.md
- TODO.md
- NEXT_TASK.md
- MILESTONES.md

---

## 需要创建的 docs 文件

```text
docs/
  design/
    PROJECT_PRINCIPLES.md
    PRODUCT_PHILOSOPHY.md
    NON_GOALS.md
    DESIGN.md
    DESIGN_DECISIONS.md
    FUTURE.md

  architecture/
    ARCHITECTURE.md
    COMPONENTS.md
    LIFECYCLE.md
    SESSION_MODEL.md
    PTY_ENGINE.md
    PROCESS_MODEL.md
    SIGNAL_MODEL.md
    IPC_PROTOCOL.md
    RINGBUFFER.md
    LOGGER.md
    METADATA.md

  protocol/
    SESSION_PROTOCOL.md
    SOCKET_PROTOCOL.md
    CLIENT_PROTOCOL.md

  development/
    DEVELOPMENT_RULES.md
    CODING_STYLE.md
    DIRECTORY_LAYOUT.md
    ERROR_HANDLING.md
    LOGGING.md
    TESTING.md
    BENCHMARK.md

  benchmark/
    PERFORMANCE.md
    MEMORY.md
    CPU.md
    STRESS.md

  user/
    INSTALL.md
    CONFIG.md
    COMMANDS.md
    FAQ.md

  known/
    KNOWN_ISSUES.md
    LIMITATIONS.md

  adr/
    ADR-0000-template.md
```

---

## 完成标准

本任务完成时必须满足：

1. 所有文档文件存在。
2. README.md 能清楚说明 PersistShell 是什么和不是什么。
3. PROJECT_PRINCIPLES.md 明确项目原则。
4. NON_GOALS.md 明确非目标。
5. ROADMAP.md 描述版本路线。
6. MILESTONES.md 包含完整里程碑。
7. TODO.md 包含可执行任务列表。
8. NEXT_TASK.md 更新为下一个任务。
9. CHANGELOG.md 记录本次文档初始化。
10. 目录结构清晰。
11. 不实现实际 PTY/Daemon/Client 业务逻辑。

---

## 禁止事项

本任务期间禁止：

- 编写 PTY Engine
- 编写 Daemon
- 编写 Client
- 选择复杂框架
- 提前做 UI
- 提前做 Web
- 提前做插件系统
- 实现 Phase 1 功能

---

## 完成后更新

完成 M00 后：

1. 将 MILESTONES.md 中 M00 标记为已完成。
2. 在 CHANGELOG.md 添加记录。
3. 将 TODO.md 中相关项标记完成。
4. 将本文件更新为：

```text
当前里程碑：M01 工程初始化

当前唯一任务：
初始化 PersistShell 工程，包括构建系统、基础目录、CI、测试框架、代码格式化、静态检查和最小可运行命令骨架。
```

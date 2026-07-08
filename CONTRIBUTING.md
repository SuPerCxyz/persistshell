# Contributing to PersistShell

感谢你愿意参与 PersistShell。

PersistShell 是一个 Linux 基础设施项目，所有贡献都必须以稳定性、可维护性和安全性为前提。

---

## 开始前

贡献代码前必须阅读：

- README.md
- docs/design/PROJECT_PRINCIPLES.md
- docs/design/NON_GOALS.md
- docs/development/DEVELOPMENT_RULES.md
- NEXT_TASK.md
- TODO.md

---

## 开发规则

1. 一次只做一个任务。
2. 不自行扩大范围。
3. 不跳过文档。
4. 不跳过测试。
5. 不提交无法编译代码。
6. 不破坏已有行为。
7. 不引入与项目非目标冲突的功能。

---

## 提交要求

每个提交应聚焦一个主题。

提交前需要：

- 格式化代码
- 运行测试
- 更新文档
- 更新 TODO
- 更新 CHANGELOG

---

## 新功能

新增功能必须先写入 TODO.md。

涉及架构变化时，必须新增 ADR。

---

## Bug 修复

Bug 修复必须增加回归测试。

---

## 性能优化

性能优化必须提供 benchmark 数据。

---

## 安全问题

安全问题不要直接公开提交细节。

请参考 SECURITY.md。

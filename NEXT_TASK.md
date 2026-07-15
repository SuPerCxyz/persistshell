# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 4：发布和长期维护

---

## 当前里程碑

M50：v1.0 Release Readiness

---

## 当前唯一任务

在维护者明确授权后，执行与最终版本一致的 tag、远端同步和 GitHub 发布流程。

### 前置已完成

- M49 已完成用户文档、FAQ、故障排查与三种包文档验证。
- M50 本地/test 发布就绪检查、release checklist 与审计记录已完成。
- 维护者已确认发布版本为 `0.1.0`，对应 tag 为 `v0.1.0`。
- 发布实现已推送并同步到 GitHub，分支 CI run `29413709266` 通过。

---

## 任务范围

- 提交并推送最终 release metadata；创建与版本一致的 tag。
- 检查 GitHub CI/package workflow，独立复核下载 artifact 的 checksum 与内容。
- 按维护者决定创建 GitHub Release、发布说明和可选签名。

---

## 完成标准

1. 远端 tag 与最终版本一致，GitHub mirror 中的 CI/package workflow 全部成功。
2. 下载的 release artifact 均能独立校验 checksum，内容符合 `docs/release/RELEASE_CHECKLIST.md`。
3. GitHub Release、发布说明、签名和依赖许可证审查状态均按维护者决定记录。
4. 公开发布后，CHANGELOG、release checklist 与审计记录补充实际 tag、日期和 workflow 证据。

---

## 禁止事项

执行前必须取得维护者对最终版本、远端推送、tag、GitHub Release 与签名策略的明确授权；未授权时
不得执行任何远端写操作，也不得为未验证的平台、Shell 或 TUI 行为给出支持承诺。

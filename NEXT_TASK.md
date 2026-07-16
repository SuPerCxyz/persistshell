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

提交并推送平台打包与远程验证修复，触发 GitHub workflow 实测。

### 前置已完成

- M49 已完成用户文档、FAQ、故障排查与三种包文档验证。
- M50 本地/test 发布就绪检查、release checklist 与审计记录已完成。
- 维护者已确认发布版本为 `0.1.0`，对应 tag 为 `v0.1.0`。
- 发布实现已推送并同步到 GitHub，分支 CI run `29413709266` 通过。
- `v0.1.0` 已同步到 GitHub，tag CI run `29414016648` 和 Package run `29414016642` 通过。

---

## 任务范围

- 提交当前已通过本地与 test 验证的代码和文档。
- 经维护者确认后推送到自建 Git，并等待同步到 GitHub mirror。
- 手动触发 Package workflow，复核两个平台 job 与 artifact。
- 暂不创建 GitHub Release，不改写 `v0.1.0` tag。

---

## 完成标准

1. 自建 Git 与 GitHub mirror 指向同一已验证提交。
2. Ubuntu 26.04 和 RHEL 9 Package job 均通过。
3. 两个平台 artifact 可下载并独立校验 checksum、内容与运行时 ABI。
4. 审计记录补充 GitHub run ID 和 artifact digest。

---

## 禁止事项

执行前必须取得维护者对最终版本、远端推送、tag、GitHub Release 与签名策略的明确授权；未授权时
不得执行任何远端写操作，也不得为未验证的平台、Shell 或 TUI 行为给出支持承诺。

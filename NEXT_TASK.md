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

由维护者确定 GitHub Release、artifact 附件、签名与 SBOM 策略。

### 前置已完成

- M49 已完成用户文档、FAQ、故障排查与三种包文档验证。
- M50 本地/test 发布就绪检查、release checklist 与审计记录已完成。
- 维护者已确认发布版本为 `0.1.0`，对应 tag 为 `v0.1.0`。
- 发布实现已推送并同步到 GitHub，分支 CI run `29413709266` 通过。
- `v0.1.0` 已同步到 GitHub，tag CI run `29414016648` 和 Package run `29414016642` 通过。
- tag 后平台修复已同步到提交 `3cbe15d`；Package run `29464594020` 的 Ubuntu 26.04 与
  RHEL 9 job 均通过。
- 两个平台 artifact 已下载并独立复核，RHEL 9 RPM 已部署到 test 完成核心回归。

---

## 任务范围

- 决定是否为现有 `v0.1.0` 创建 GitHub Release，或等待后续修复版本。
- 若创建 Release，明确附加历史 tag 产物还是 tag 后平台产物；不得把历史 Ubuntu RPM 标记为 RHEL 9 兼容。
- 决定是否对 artifact 签名、生成 SBOM 或补充 `NOTICE`。
- 不移动或改写现有 `v0.1.0` tag。

---

## 完成标准

1. 发布或暂缓策略得到维护者明确确认。
2. artifact 来源、兼容性标签和附件集合没有歧义。
3. 签名、SBOM 与 `NOTICE` 的执行或暂缓状态写入发布清单。
4. 若创建 Release，发布结果和最终 digest 写入审计；若暂缓，记录下一次评估条件。

---

## 禁止事项

执行前必须取得维护者对最终版本、远端推送、tag、GitHub Release 与签名策略的明确授权；未授权时
不得执行任何远端写操作，也不得为未验证的平台、Shell 或 TUI 行为给出支持承诺。

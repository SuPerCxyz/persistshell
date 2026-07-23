# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 4：稳定性和发布

---

## 当前里程碑

M57：时间化日志与完整 Replay。

---

## 当前唯一任务

实现 M57 第一阶段：Running/Closed Attach 历史连续性。

### 前置已完成

- M56 通用 Linux 多架构发布包已完成。
- Attach 历史连续性设计、ADR 和实施计划已确认。
- 当前 Session 日志是无时间信息的原始字节流。
- `persist replay --speed` 与 `--follow` 仅完成参数解析，尚无实际效果。

---

## 任务范围

- Running Session 继续使用 Holder Ring 回放最近输出。
- Closed Session 恢复前安全读取轮转日志最近 `replay_bytes`。
- 保证旧历史、新 prompt、实时输出的稳定顺序。
- 日志缺失或不安全时降级继续恢复，不跟随 symlink。
- 补齐真实 public attach、exit、Ctrl+D 和配置边界测试。
- 更新文档、构建修复包并在 `test` 主机验证。

---

## 完成标准

1. Running 和 Closed 两条路径均回放最近 `replay_bytes`。
2. Closed 输出顺序为旧历史、新 prompt、实时输出。
3. 轮转、截断、关闭日志和不安全文件测试通过。
4. 不修改 wire、metadata schema 或日志格式。
5. workspace 完整门禁和性能边界通过。
6. 修复包在 `test` 完成真实 SSH 验证。

---

## 禁止事项

不修改日志格式、metadata schema 或 public/private wire，不提前实现 `--speed`/`--follow`，
不引入终端模拟、集中式日志服务或无限历史。

# M38 前台进程树视图设计

## 目标

为运行中的 Session 提供前台进程组的有界进程树视图，帮助用户确认 Shell 当前启动的
命令及其子进程。该能力只读 Linux procfs，不改变进程、信号或 Session 生命周期。

## CLI 与 IPC

新增命令：

```bash
persist ps <session-id>
```

新增 `PROCESS_TREE` / `PROCESS_TREE_RESP` IPC 消息。响应根节点为 M37 获得的前台
进程组 leader；每个节点包含 PID、父 PID、进程名和命令行摘要。

closed Session、没有前台进程组或根进程已退出时返回成功的空树，CLI 显示
`(no foreground process)`。

## procfs 读取

对子节点读取 `/proc/<pid>/task/<pid>/children`，对子进程读取 `comm` 和 `cmdline`。
递归限制为最多 64 个节点、最大深度 8；达到限制时停止展开而不报错。读失败、权限
不足或进程在读取期间退出时跳过该节点或其子节点。

命令行最多 160 字节，NUL 转为空格；无命令行时保留 `comm`。

## 非目标

- 不读取 CPU、内存、I/O 或 namespace 信息。
- 不枚举整个系统进程表。
- 不持久化进程树。
- 不改变 M37 的列表字段或 M14/M36 行为。

## 验证计划

M38 实现后与后续里程碑统一执行：协议编解码、procfs 解析、daemon IPC、CLI 输出及
`test` 主机端到端验证。

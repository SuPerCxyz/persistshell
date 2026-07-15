# M42 Metrics 设计

## 目标

提供 `persist metrics`，用于即时查看 daemon 与 Session 的基础聚合指标。该命令只读，
不启动 metrics server，不写入采样历史，也不创建长期采集线程。

## 输出

命令输出最多 16 KiB 的 JSON：

```json
{
  "daemon": { "pid": 1234 },
  "sessions": {
    "total": 4,
    "running": 2,
    "closed": 2,
    "locked": 1,
    "pinned": 1,
    "runtime": 2,
    "active_writers": 1,
    "readonly_clients": 0
  }
}
```

`total`、`running`、`closed`、`locked` 与 `pinned` 来自 metadata；`runtime`、
`active_writers` 与 `readonly_clients` 来自 daemon 内存状态。指标不含环境变量、输入内容、
Session 输出、note/tag 内容或 SSH agent 路径。

## IPC 与错误处理

新增无 payload 的 `METRICS` / `METRICS_RESP`。响应使用受限 JSON 文本；metadata 不可用
或响应超限时返回带 `error` 字段的 JSON，CLI 映射为非零退出。

## 非目标

- 不支持 Prometheus、OpenTelemetry、HTTP endpoint 或外部 exporter。
- 不计算 CPU 百分比、吞吐率、延迟分位数或时间序列。
- 不修改 M39 的单前台进程资源统计语义。

## 验证计划

补充命令解析、协议消息映射、正常聚合、metadata 不可用和 JSON 长度边界测试；按后续
统一测试里程碑执行。

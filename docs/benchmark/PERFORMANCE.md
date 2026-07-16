# PersistShell Performance Results

本文档保存可重复的性能回归结果。不同硬件、内核或 Rust 版本的数据不能直接横向比较。

## 2026-07-16 M52 Dashboard

环境：

- 当前提交：`c002c26`，另含不影响二进制的 benchmark 脚本工作树变更
- baseline：`f28d67e`（M52 worker 接入前）
- OS：Ubuntu，Linux 7.0.0-27-generic x86_64
- CPU：Intel Core i5-12400
- 内存：11,717,336 KiB
- Rust：1.96.1
- 配置：1 KiB ring buffer、关闭 Session 输出日志
- 每组预热 10 秒，采样 30 秒

命令：

```bash
PERSISTD_BASELINE_BIN=/tmp/persistd-f28d67e \
PERSISTD_DASHBOARD_BIN=target/release/persistd \
PERSIST_BIN=target/release/persist \
scripts/benchmark-dashboard.sh
```

原始结果：

| 模式 | Session | CPU milli-percent | 平均 RSS KiB | 峰值 RSS KiB | 线程 | 指标字节 |
|---|---:|---:|---:|---:|---:|---:|
| baseline | 100 | 0 | 6,300 | 6,300 | 1 | 0 |
| dashboard | 100 | 398 | 6,956 | 6,976 | 3 | 111 |
| baseline | 1000 | 33 | 5,360 | 5,360 | 1 | 0 |
| dashboard | 1000 | 663 | 10,105 | 10,436 | 3 | 122,706 |

100 Session 的附加 CPU 为单核 `0.398%`，低于设计上限 `1%`。1000 Session 用例完成后 daemon
仍可响应 `persist ls --plain`，采样线程数保持固定，没有按 Session 增加线程。

## 容量审计

- `production_history_starts_within_memory_limit` 与淘汰测试覆盖 64 MiB、1 小时、720 帧上限。
- `rotation_keeps_newest_segments_and_honors_total_size` 覆盖 24 分段和 128 MiB 磁盘上限。
- `trigger_queue_coalesces_without_reentrant_scans` 覆盖容量 1 队列和过载合并。
- `full_writer_queue_drops_batch_and_updates_status` 覆盖容量 2 writer 队列和显式丢弃状态。
- Dashboard IPC 编解码测试覆盖 128 Session/页、240 趋势点和控制帧上限。

上述容量是实现硬上限；进程总 RSS 还包含 Session manager、SQLite 和 Rust runtime，不能把
daemon RSS 直接解释为指标历史内存。

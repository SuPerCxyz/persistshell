# PersistShell Benchmark Guide

本文档定义 PersistShell 的性能测试规范。

---

## Benchmark 原则

任何性能优化必须基于 benchmark。

禁止凭感觉优化。

每次 benchmark 必须记录：

- 测试时间
- git commit
- OS
- kernel
- CPU
- memory
- disk
- filesystem
- 编译参数
- 配置文件
- 测试命令
- 测试结果

---

## 核心指标

PersistShell 关注以下指标：

- attach latency
- detach latency
- session create latency
- daemon idle CPU
- daemon memory
- per-session overhead
- PTY throughput
- IPC throughput
- ring buffer throughput
- log writer throughput
- metadata query latency
- 100/500/1000 session scalability
- Dashboard 采样 CPU、RSS、线程和指标磁盘占用

## Dashboard Benchmark

`scripts/benchmark-dashboard.sh` 比较 M52 worker 接入前的 baseline daemon 与当前 daemon。
baseline 必须是独立二进制，不能通过修改正式配置或采样语义模拟。

```bash
cargo build --workspace --release --locked
PERSISTD_BASELINE_BIN=/path/to/baseline/persistd \
PERSISTD_DASHBOARD_BIN=target/release/persistd \
PERSIST_BIN=target/release/persist \
scripts/benchmark-dashboard.sh
```

可通过 `PERSIST_DASHBOARD_BENCH_COUNTS`、`PERSIST_DASHBOARD_BENCH_WARMUP_SECONDS` 和
`PERSIST_DASHBOARD_BENCH_DURATION_SECONDS` 调整用例。正式 M52 门禁使用 100/1000 Session、
10 秒预热和 30 秒采样，100 Session 的附加 CPU 不得超过单核 1000 milli-percent（1%）。

每个用例使用隔离 XDG 目录并验证 daemon socket 仍可响应。原始结果和容量审计保存在
`docs/benchmark/PERFORMANCE.md`。

---

## 目标值

Phase 1 初始目标：

```text
attach latency: < 20ms
daemon idle CPU: ≈ 0%
base per-session overhead: < 500KB，不含 ring buffer 和 shell
100 sessions: stable
1000 sessions: target
```

注意：

Ring Buffer 内存单独计算。

例如：

```text
1000 sessions × 8MB ring buffer = 8GB
```

这不是基础管理开销。

---

## Attach Latency

测试：

```text
client attach request start
到
用户看到首屏输出
```

需要分别测试：

- 无 replay
- 512KB replay
- 1MB replay
- Session idle
- Session 大量输出中

---

## Session Create Latency

测试：

```text
NewSession request
到
shell ready
```

包括：

- metadata create
- openpty
- fork
- exec shell
- attach

---

## Daemon Idle CPU

场景：

- daemon running
- 0 session
- 10 idle sessions
- 100 idle sessions
- 1000 idle sessions

要求：

- 无 busy loop
- 无 sleep polling 高 CPU

---

## Memory

分别统计：

- daemon base memory
- per-session runtime overhead
- ring buffer memory
- metadata cache memory
- client memory

必须区分：

```text
基础管理开销
与
配置型 buffer 开销
```

---

## PTY Throughput

测试命令：

```bash
seq 1 1000000
yes | head -n 1000000
cat large-file
```

统计：

- bytes/sec
- daemon CPU
- client CPU
- memory growth
- dropped output，若有

---

## Slow Client

模拟 client 读取很慢。

验证：

- PTY reader 不被阻塞
- daemon 不无限缓存
- 慢 client 被断开或丢弃旧输出
- 其它 session 不受影响

---

## Logging Benchmark

测试：

- session log on
- session log off
- rotation on
- slow disk
- log directory full

指标：

- PTY throughput 下降比例
- log writer queue size
- flush latency
- dropped log count，若有

---

## Metadata Benchmark

测试：

- list 10 sessions
- list 100 sessions
- list 1000 sessions
- update status
- query by id
- GC scan

---

## IPC Benchmark

测试：

- request/response latency
- streaming throughput
- frame encode/decode cost
- resize event overhead

---

## Stress Benchmark

场景：

```text
100 sessions each producing output
1000 sessions idle
frequent attach/detach
frequent resize
frequent kill/create
```

需要观察：

- fd 数
- goroutine/thread 数，若适用
- memory growth
- CPU
- error count
- zombie process
- database lock

---

## Regression

每次优化后必须保留历史结果。

`docs/benchmark/PERFORMANCE.md` 应记录结果。

如果性能退化，需要说明原因。

---

## 不允许的 Benchmark 结论

禁止写：

```text
性能很好
很快
基本没问题
```

必须写具体数据。

---

## Benchmark 输出格式

推荐：

```text
Test: attach latency
Commit:
OS:
Kernel:
CPU:
Memory:
Config:
Sessions:
Replay:
Result p50:
Result p95:
Result p99:
Notes:
```

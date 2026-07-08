# PersistShell Ring Buffer

本文档描述 PersistShell Ring Buffer 设计。

Ring Buffer 用于保存每个 Session 最近的 PTY 输出。

---

## 目标

Ring Buffer 的目标：

- attach 时快速回放最近输出
- SSH 断开期间保留最近输出
- 避免 attach 时读取磁盘日志
- 吸收短时间输出峰值
- 限制内存使用

---

## 非目标

Ring Buffer 不是完整日志系统。

它不保证保存所有历史输出。

完整历史由异步日志系统负责。

---

## 基本模型

每个 Session 拥有一个固定大小 Ring Buffer。

例如：

```text
default_ring_buffer_size = 64MB
```

当输出超过容量时，覆盖最旧数据。

---

## 数据路径

```text
PTY output
    ↓
Ring Buffer
    ├── attached clients
    └── async logger
```

---

## 固定大小

Ring Buffer 必须固定大小。

禁止：

- 无限增长
- 按输出自动扩容
- 无限制缓存所有历史

原因：

一个 `yes` 命令就可能打爆内存。

---

## 字节流存储

Phase 1 Ring Buffer 按字节流存储。

不解析：

- ANSI
- UTF-8 字符宽度
- 行
- 屏幕状态

后续可以增加索引。

---

## Attach 回放

Client attach 后，Daemon 应回放 Ring Buffer 中最近输出。

默认回放量可以配置：

```text
replay_bytes = 1MB
```

或者：

```text
replay_lines = 200
```

Phase 1 推荐按字节实现，按行显示后续增强。

---

## 回放策略

可配置：

```text
replay_on_attach = true
replay_bytes = 1048576
```

用户可选择：

```bash
persist attach <id> --no-replay
```

Phase 1 可以暂不实现参数，但架构需预留。

---

## 并发

Ring Buffer 会被：

- PTY reader 写入
- attach client 读取
- logger 消费
- search/replay 后续读取

必须保证并发安全。

优先设计为：

- 单 PTY output writer
- 多 reader

---

## 慢客户端

如果 client 读取太慢：

- 不得阻塞 PTY reader。
- 不得阻塞 daemon event loop。
- 不得导致无限队列。

策略：

```text
client output queue max size
超过后断开 client 或丢弃旧输出
```

---

## 日志关系

Ring Buffer 不应等待日志写入。

日志写入异步执行。

如果日志写入失败：

- 记录内部错误
- 标记 Session log error
- 不影响 PTY 和 client live output

---

## 输出风暴

必须防止：

```bash
yes
cat huge.log
journalctl -f
dd if=/dev/zero
```

拖垮 daemon。

需要：

- 批量读取
- 批量写入
- 固定 buffer
- client 队列上限
- 日志 flush 批量
- 可选 per-session 输出限速

Phase 1 可以先实现基本保护。

---

## 内存预算

假设：

```text
1000 sessions
64MB ring buffer
```

总内存会达到 64GB，不可接受。

因此默认值必须谨慎。

建议 Phase 1 默认：

```text
ring_buffer_size = 4MB 或 8MB
```

用户可配置更大。

文档中的性能目标 `<500KB per session` 与大 Ring Buffer 冲突，因此需要区分：

```text
基础管理开销 <500KB
Ring Buffer 按配置额外占用
```

---

## 配置项

建议：

```toml
[ring_buffer]
default_size = "8MB"
max_size = "128MB"
replay_on_attach = true
replay_bytes = "512KB"
```

---

## UTF-8 边界

Ring Buffer 按字节覆盖可能截断 UTF-8 字符。

显示时应尽量容错。

Phase 1 可以允许 replacement character。

后续可以在回放边界做 UTF-8 对齐。

---

## ANSI 边界

Ring Buffer 可能从 ANSI escape sequence 中间开始回放。

可能导致显示异常。

Phase 1 可接受，但应尽量在 attach 前重置终端状态：

```text
ESC c 或适当 reset
```

需要谨慎，避免破坏应用状态。

更好的方案放后续 terminal state cache。

---

## 测试

必须测试：

- 小 buffer 覆盖
- 大量输出
- attach 回放
- 多次 attach
- 慢 client
- 日志失败
- UTF-8 输出
- ANSI 彩色输出
- top/vim/less 场景

---

## 不变量

1. Ring Buffer 不无限增长。
2. PTY 输出写入 Ring Buffer 不应被磁盘 I/O 阻塞。
3. 慢 client 不应阻塞整个 Session。
4. Ring Buffer 覆盖旧数据是允许行为。
5. Ring Buffer 不是完整审计日志。

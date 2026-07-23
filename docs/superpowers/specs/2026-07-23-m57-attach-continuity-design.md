# M57 Attach 历史连续性设计

## 文档状态

- 日期：2026-07-23
- 里程碑：M57
- 状态：已确认
- 决策：`docs/adr/ADR-0009-bounded-attach-history-replay.md`

## 背景

用户重新进入旧 Session 时应先看到离开前的历史输出，再继续交互。当前 Running Session
由 Holder Ring Buffer 回放最近输出；Closed Session 恢复时会创建新的 Shell runtime，
但不会把退出前的持久日志回放给 attach client，因此只看到新 prompt。

`test` 主机当前运行 0.1.0，配置已启用 `replay_on_attach`，`replay_bytes` 为 512 KiB，
Session 日志也已启用。现有 Session 均为 Closed，日志仍在磁盘，证明数据存在但恢复路径
没有消费它。该缺口在 0.2.0 代码中仍然存在。

## 目标

- Running Session 重新 attach 时回放断连期间最近输出。
- Closed Session 重新 attach 时先回放退出前历史，再显示恢复后的新 Shell 输出。
- 两条路径统一受 `ring_buffer.replay_on_attach` 和 `ring_buffer.replay_bytes` 控制。
- 默认最多回放 512 KiB，保持有界内存、I/O 和 IPC frame。
- 日志异常时保持 attach 可用，不降低路径安全要求。

## 非目标

- 不保存或模拟完整终端屏幕状态。
- 不复活 Closed Session 中已经退出的进程。
- 不无界回放所有历史日志。
- 本阶段不实现 replay `--speed`、时间化日志或事件驱动 `--follow`。
- 不改变 public IPC 和 Holder private IPC wire format。

## 用户语义

### Running Session

SSH 断开、窗口关闭或显式 detach 不结束 Shell。重新 attach 后，Holder 先发送 Ring Buffer
最近 `replay_bytes`，再发送实时输出。前台进程、cwd 和环境保持原样。

### Closed Session

`exit` 或空行 `Ctrl+D` 释放旧 Shell runtime。重新 attach 时：

1. 读取退出前日志最近 `replay_bytes`。
2. 使用已保存 cwd 和环境恢复新的 Shell runtime。
3. attach client 先收到旧历史。
4. 随后收到新 runtime Ring 中的 prompt 和实时输出。

旧进程不会复活，但用户能够看到离开前上下文并在恢复后的 Shell 中继续操作。

## 方案

采用 Daemon 分层回放：

- Running Session 仅使用 Holder Ring，不读取磁盘。
- Closed Session 在创建新 runtime 前读取持久日志尾部。
- Daemon 发送成功 `AttachResp` 后，以标准 stdout frame 分片发送历史。
- 历史发送完成后进入现有 Holder public attach proxy。

不把旧历史写入新 Holder Ring，也不重新写入 Session 日志。这样无需新增 private protocol，
不会让旧历史在每次恢复后重复膨胀。

## Closed Attach 数据流

```text
Client        Daemon             Log files          Holder
  | Attach      |                    |                 |
  |------------>|                    |                 |
  |             | bounded tail read  |                 |
  |             |------------------->|                 |
  |             |<-------------------|                 |
  |             | restore runtime                      |
  |             |------------------------------------->|
  |             | holder attach + new runtime replay   |
  |             |<------------------------------------>|
  | AttachResp  |                    |                 |
  |<------------|                    |                 |
  | old history |                    |                 |
  |<------------|                    |                 |
  | new prompt / live output                           |
  |<---------------------------------------------------|
```

日志快照必须在创建新 runtime 前完成，因此快照只包含旧 runtime 输出。Holder Ring 只包含
新 runtime 输出，两者按顺序拼接，不会重复，也不存在新 prompt 插入旧历史中间的竞态。

## 日志尾部读取

日志按时间从旧到新排列：

```text
<id>.log.<max_files> ... <id>.log.2 <id>.log.1 <id>.log
```

读取器从当前文件向旧轮转文件反向收集，达到 `replay_bytes` 后停止，再恢复为时间正序。
每个文件只读取所需尾部，不把完整轮转日志载入内存。输出保持原始字节，不要求 UTF-8，
允许 ANSI 控制序列和二进制输出。

读取器必须：

- 只接受配置生成的固定 Session 日志路径。
- 使用不跟随 symlink 的打开方式。
- 验证普通文件、当前 UID owner 和 `0600` 权限。
- 对文件数量、单次读取量和总返回量执行配置上限。
- 使用至多 64 KiB 的 stdout frame 分片。

如果截断点位于一个 ANSI 序列或多字节字符中，行为与现有字节 Ring 截断一致；本阶段不解析
终端内容。

## Running Attach 验证

Running 路径不改变实现，但当前缺少覆盖 public client、daemon 和 Holder 的完整回归测试。
新增真实测试必须证明：

1. 创建 Session 并输出唯一标记。
2. client 断开但 Shell 保持运行。
3. 断连期间再输出一个唯一标记。
4. 第二次 public attach 在实时 prompt 前收到 Ring replay。
5. `replay_on_attach=false` 或 `replay_bytes=0` 时不回放。

如果该测试失败，应修复现有 Holder/public proxy 路径，不能改用磁盘掩盖 Running Ring 缺陷。

## 错误处理

- 日志关闭、文件不存在或内容为空：Closed attach 正常恢复，不发送历史。
- 权限、owner、文件类型或 symlink 不安全：拒绝读取并记录内部安全告警，attach 降级继续。
- 读取中发生 I/O 错误：丢弃不完整历史快照，attach 降级继续。
- runtime 恢复失败：维持现有失败语义，不发送成功响应或部分历史。
- client 在历史发送期间断开：停止发送并按现有 attach 清理，不影响恢复后的 runtime。
- 历史帧进入有界 public 输出队列；慢客户端不得导致无限内存或阻塞 PTY 日志写入。

用户可通过 `persist log <id>` 或 `persist replay <id>` 单独检查完整保留日志。自动 attach
回放失败不能伪装成日志丢失。

## 配置兼容

不新增配置项：

```toml
[ring_buffer]
replay_on_attach = true
replay_bytes = "512KB"
```

`replay_on_attach` 同时控制 Running Ring 和 Closed 日志尾部。`replay_bytes` 是两条路径共同
上限。现有配置、public protocol、Holder protocol 和 metadata schema 保持兼容。

## 测试

- 单元测试：跨轮转文件尾读、精确边界、空文件、短文件、二进制内容和顺序。
- 安全测试：symlink、非普通文件、错误 owner/mode、替换竞态和超限。
- Daemon 集成测试：Closed attach 的历史、恢复 prompt 和实时输出顺序。
- 生命周期测试：`exit`、空行 `Ctrl+D`、SSH 断开和显式 detach。
- 配置测试：关闭回放、零上限、512 KiB 截断和最大允许上限。
- 回归测试：Running public attach 继续使用 Ring，不读取磁盘。
- 性能门禁：512 KiB 多轮转尾读和 attach 首字节延迟，内存峰值受上限约束。
- 平台验证：本地完整 workspace 门禁，随后在 `test` 安装修复包验证两条路径。

## 发布与回滚

`test` 当前是 0.1.0，不能用来证明 0.2.0 或后续修复已部署。实现通过后应生成新的 patch
版本包，不移动已有 `v0.2.0` tag。远端验证前记录旧版本，安装后确认 `persist`、`persistd`
和包 metadata 版本一致。

若 Closed 日志回放引发回归，可回滚 Daemon 的历史前缀发送，Running Holder Ring 行为不受
影响。不得通过删除用户日志或降低文件安全校验回滚。

## 完成标准

- Running 和 Closed 两条 attach 路径均有真实 public IPC 回归测试。
- Closed attach 输出顺序稳定为旧历史、新 prompt、实时输出。
- 自动回放严格受现有开关和字节上限约束。
- 日志缺失或不安全不会阻止上下文恢复，也不会跟随不可信路径。
- 相关架构、用户、限制、TODO、CHANGELOG 和测试文档同步更新。
- 本地、GitHub CI、双架构包及 `test` 主机验证通过。

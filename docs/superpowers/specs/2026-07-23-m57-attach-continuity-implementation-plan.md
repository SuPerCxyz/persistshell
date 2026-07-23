# M57 Attach 历史连续性实施计划

> 依据：
> `docs/superpowers/specs/2026-07-23-m57-attach-continuity-design.md`
> 和 `docs/adr/ADR-0009-bounded-attach-history-replay.md`

状态：已确认

**目标：** Running Session 使用 Holder Ring 回放，Closed Session 在恢复新 runtime 前读取
有界日志尾部，使重新 attach 按旧历史、新 prompt、实时输出顺序继续交互。

**方法：** 新增安全日志尾读模块；Daemon 仅在 Closed restore 前构造字节快照，并通过现有
public stdout frame 和有界 pending queue 发送。Running 路径不读磁盘，不修改任何 wire。

**质量等级：** Level 2 TDD + Level 3 Review + Level 4 Completion Verification。

## 阶段总览

- [ ] 阶段 1：安全轮转日志有界尾读
- [ ] 阶段 2：Closed attach 历史前缀接入
- [ ] 阶段 3：Running/Closed public attach 回归
- [ ] 阶段 4：性能、安全和文档收尾
- [ ] 阶段 5：版本包、GitHub CI 和 `test` 验证

## 全局硬边界

- 自动回放只使用现有 `replay_on_attach` 和 `replay_bytes`。
- Running attach 只使用 Holder Ring，不以磁盘掩盖 Ring/proxy 缺陷。
- Closed 历史必须在新 runtime 创建前快照，禁止重复写回 Ring 或日志。
- 不跟随 symlink，不接受非普通文件、错误 owner 或宽松权限。
- 日志失败不阻止 cwd、环境和新 Shell 恢复。
- 内存、文件数、单次读取和 public pending bytes 必须有硬上限。
- 不修改 public/private protocol、metadata schema、日志格式或历史 tag。
- 不实现 `--speed`、`--follow` 或终端屏幕模拟。
- 不提交未跟踪的 `TEST_PLAN.md`。

## 主要文件边界

- `crates/persistd/src/attach_history.rs`：安全轮转日志尾读和单元测试。
- `crates/persistd/src/main.rs`：模块注册。
- `crates/persistd/src/public_attach.rs`：有界历史前缀发送与顺序测试。
- `crates/persistd/src/server.rs`：Closed restore 前快照和 attach 接入。
- `crates/persistd/tests/`：真实 Running/Closed public IPC 生命周期测试。
- `docs/architecture/`、`docs/user/`、`docs/known/`：行为和限制。
- `TODO.md`、`MILESTONES.md`、`CHANGELOG.md`、`NEXT_TASK.md`：项目状态。

## 阶段 1：安全轮转日志有界尾读

### 红灯测试

- [ ] 当前日志短于、等于和长于字节上限。
- [ ] `.log.N` 到当前 `.log` 按时间正序拼接。
- [ ] 只读取达到上限所需的文件尾部。
- [ ] 空文件、缺失文件和 `replay_bytes=0` 返回空。
- [ ] 非 UTF-8 和 ANSI 字节原样保留。
- [ ] symlink、目录、FIFO、错误 owner/mode 和替换竞态不被读取。

运行：

```bash
cargo test -p persistd attach_history
```

### 实现

- [ ] 使用固定 `logs_dir/<id>.log[.N]` 构造路径。
- [ ] `O_NOFOLLOW | O_CLOEXEC` 打开后使用 fd metadata 验证。
- [ ] 通过 seek/read_exact 读取所需尾部，不读取完整文件。
- [ ] 从新到旧收集、从旧到新拼接，总长度不超过配置上限。
- [ ] 将缺失与安全/I/O 降级状态区分，禁止记录文件正文。

### 阶段门禁

```bash
cargo fmt --all -- --check
cargo test -p persistd attach_history
cargo clippy -p persistd --all-targets -- -D warnings
git diff --check
```

## 阶段 2：Closed attach 历史前缀接入

### 红灯测试

- [ ] Closed restore 前读取日志，恢复后才发送成功响应。
- [ ] 输出顺序为旧历史、新 runtime Ring/prompt、实时输出。
- [ ] 日志关闭、缺失或不安全时 attach 仍成功且无历史。
- [ ] runtime 恢复失败时不发送成功或部分历史。
- [ ] client 在历史发送期间断开不影响新 runtime。

### 实现

- [ ] Attach handler 只对真实 Closed record 构造历史快照。
- [ ] `public_attach` 将历史编码为标准 `Stdout` frame。
- [ ] 复用 `PendingWrites` 容量检查和非阻塞 flush，不直接阻塞写 socket。
- [ ] 先排入历史，再消费 Holder 已排队的新 runtime replay。
- [ ] Running、locked、lost 和不存在 Session 保持现有语义。

## 阶段 3：Running/Closed public attach 回归

- [ ] Running：输出标记、断开、离线输出、重新 public attach 可见 Ring replay。
- [ ] Running：关闭回放或零上限时不出现历史。
- [ ] Closed：`exit` 后重新 attach 可见旧标记和新 prompt。
- [ ] Closed：空行 `Ctrl+D` 路径与 `exit` 一致。
- [ ] Closed：轮转历史跨文件回放且最多 `replay_bytes`。
- [ ] Closed：新输出只记录一次，第二次恢复不重复膨胀旧历史。
- [ ] Takeover、readonly、resize、signal 和 SessionExited 回归通过。

运行：

```bash
cargo test -p persistd --bin persistd
cargo test -p persistd --test persistd
cargo test -p persistd --test reconciliation
```

## 阶段 4：性能、安全和文档收尾

- [ ] 采样 512 KiB、多个轮转文件的尾读耗时和内存边界。
- [ ] 验证历史分片不超过 public frame 上限和 pending queue 容量。
- [ ] 更新架构 Ring/Logger/Session lifecycle 文档。
- [ ] 更新完整用户手册、配置、命令、FAQ 和已知限制。
- [ ] 更新 TODO、MILESTONES、CHANGELOG 和 NEXT_TASK。
- [ ] 记录本地验证审计，不把未执行结果写成通过。

完整门禁：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

## 阶段 5：版本包和远程验证

- [ ] 根据现有 tag 不可移动原则选择新的 patch 版本。
- [ ] 构建 x86_64 RPM/tar.xz，核对 checksum、版本、GLIBC 和体积。
- [ ] 在 `test` 记录旧版本并安装修复 RPM。
- [ ] 普通用户启动 daemon，验证 Running SSH 断开重连。
- [ ] 验证 `exit` 和空行 `Ctrl+D` 后 Closed attach 历史连续性。
- [ ] 验证 512 KiB 截断、日志关闭和重启后行为。
- [ ] 运行远端 doctor、状态和清理检查。
- [ ] 经授权后提交/push；若创建新 tag 或 Release 必须单独遵循发布确认。

## 完成定义

- 两条 attach 路径行为与设计一致。
- 错误、安全、边界、单元、集成和真实 PTY 测试完成。
- 完整本地门禁通过。
- 文档和项目状态同步。
- 修复包在 `test` 安装并通过真实 SSH 验证。
- 未解决限制明确记录，`NEXT_TASK.md` 只保留下一唯一任务。

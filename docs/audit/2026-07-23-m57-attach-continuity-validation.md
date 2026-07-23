# M57 Attach 历史连续性验证

日期：2026-07-23

## 范围

- Running Session 断线后的 Holder Ring 回放。
- Closed Session 在 `exit`、空行 `Ctrl+D` 和 daemon 重启后的日志尾部回放。
- 旧历史、新 runtime 输出和实时输出顺序。
- 512 KiB 边界、日志关闭、安全文件与 SSH PTY 慢输出。
- Rocky 9.7 x86_64 RPM 安装验证。

## 本地验证

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

结果：

- workspace 测试通过。
- 两个既有 ignored 测试保持忽略：手工 Shell 状态 benchmark 和系统 zshrc 不兼容用例。
- `persistd` 单元测试 156 通过、1 ignored。
- `persistd` foreground 5 通过，reconciliation 15 通过。
- Attach 历史读取覆盖轮转正序、512 KiB 截断、非 UTF-8、缺失、零上限、
  symlink、FIFO、owner、mode 和文件数量上限。
- CLI 覆盖 partial stdout write、closed output 和 stdin poll failure。
- GitHub 首轮 CI 暴露 daemon crash 测试在新 PID 写入后立即连接 socket 的竞态；测试改为
  等待新 daemon 实际 accept，并连续定向验证 3 次。
- workspace 高并发负载下 Ctrl+D 生命周期可能超过原 8 秒测试 deadline；门禁只将等待预算
  调整为 20 秒，Closed 状态、exit code 和 cwd 断言保持不变。

512 KiB 多轮转 release 测试进程采样为 0.06 秒、40,496 KiB RSS；RSS 包含 Cargo
测试进程，功能断言确认返回 Vec 严格为 524,288 字节。

## 包验证

Rocky 8 用户空间构建：

```text
persist:        GLIBC_2.28, 1,268,056 bytes
persistd:       GLIBC_2.28, 2,306,952 bytes
persist-holder: GLIBC_2.28,   877,368 bytes
```

产物：

```text
persistshell-0.2.1-1.x86_64.rpm              1,409,368 bytes
persistshell-v0.2.1-linux-x86_64.tar.xz      1,400,420 bytes
```

两个产物的 SHA-256 sidecar 校验均通过，体积门禁通过。

## test 主机

环境：

```text
Rocky Linux 9.7
x86_64
persistshell-0.2.1-1.x86_64
```

验证结果：

- Session 13：SSH 客户端断开后离线输出继续产生，重新 attach 可见 Ring 回放。
- Session 13：`exit` 后 Closed attach 可见旧标记，旧标记先于新 runtime 输出。
- Session 14：空行 `Ctrl+D` 后释放 runtime，Closed attach 可见退出前标记。
- Session 14：daemon 重启后仍可从持久日志回放 Closed 历史。
- Session 16：实际收到 525,004 字节；尾部新标记存在，超过 512 KiB 的旧标记不存在，
  SSH PTY 未发生 partial write 丢失或 EAGAIN panic。
- Session 17：`logging.session_log=false` 时未创建 Session 日志，Closed attach 正常恢复且
  不回放旧标记；测试后恢复默认配置。
- `rpm -V persistshell` 无差异，daemon/Holder connected，`log_degraded=0`、`lost=0`。
- 所有测试 Session 最终为 Closed，无测试进程残留。

`persist doctor` 除“未安装 SSH shell hook”外均正常；该主机原本未安装 hook，本次验证未擅自
修改用户 profile。

## 结论

M57 第一阶段 Attach 历史连续性满足验收标准。当前仍不提供时间化日志、终端屏幕快照、
`replay --speed` 或事件驱动 `--follow`，这些是 M57 后续唯一任务。

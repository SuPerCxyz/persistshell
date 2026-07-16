# M52 Performance Dashboard 验证审计

## 结论

M52 的有界采样、趋势存储、Dashboard IPC 和 `persist top` 已完成本地及 Rocky Linux 9.7
验证。100 Session 附加 CPU 为单核 `0.398%`，满足不超过 `1%` 的验收标准。

## 自动化验证

- `cargo test --workspace --locked`：360 passed，0 failed。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo fmt --all -- --check`、`cargo check --workspace --locked`：通过。
- `persist-cli` PTY 测试覆盖 panic unwind 后 raw mode 恢复。
- App/渲染测试覆盖排序、稳定选择、Session 消失、常规和紧凑尺寸。

容量与错误路径由 history、storage、worker、writer、procfs、IPC 单元和真实 socket 集成测试
覆盖。详细数据见 `docs/benchmark/PERFORMANCE.md`。

## 发布包

- Ubuntu 26.04 tar/deb：checksum、`persist top` help 和完整用户手册通过。
- Rocky Linux 9.7：原生 release 构建并生成 `persistshell-0.1.0-1.el9.x86_64.rpm`。
- RHEL 9 tar/RPM checksum、RPM 文件清单、包内二进制和 `USER_GUIDE.md` 通过。
- 未创建 tag、GitHub Release，也未执行远端 Git push。

## test 主机

目标为 `ssh test`，Rocky Linux 9.7 x86_64。RPM 已安装，当前二进制同步安装到
`/usr/local/bin/persist` 和 `/usr/local/bin/persistd`，daemon 正常运行。

真实 PTY 验证通过：

- Session 表、详情视图和 15m/1h/24h 范围切换。
- `q` 与 `Ctrl+C` 退出后 `stty -g` 保持一致。
- daemon 下线时显示 disconnected，重启后自动恢复 connected。
- daemon 重启前后指标分段哈希一致，重启后 24h 查询成功。
- `exit` 后 runtime 释放并保留 closed Session；超过采样周期的 cwd 可在再次 attach 时恢复。

## 保留限制

- `cd` 后在下一次 cwd 采样前立即 `exit` 仍可能恢复上一次目录，已记录在限制文档和 TODO。
- 24h 趋势由分钟分段提供，当前分钟尚未落盘时可能暂时没有点。
- test 主机只验证 Rocky/bash；zsh/fish 兼容性沿用既有本地验证证据。

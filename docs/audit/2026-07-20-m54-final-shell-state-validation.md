# M54 最终 Shell 状态验证审计

## 范围与环境

- 日期：2026-07-20
- 版本：0.1.0
- 本地：Ubuntu 26.04 x86_64
- 远端：`ssh test`，Rocky Linux 9.7 x86_64
- Rust：`rustc 1.96.1 (31fca3adb 2026-06-26)`
- Cargo：`cargo 1.96.1 (356927216 2026-06-26)`
- 远端既有 daemon：PID 107264，`/usr/local/bin/persistd foreground`
- 验证期间未终止或替换既有 daemon；所有运行场景使用独立 XDG 目录。

## 正确性门禁

本地执行：

```bash
bash -n scripts/test-final-shell-state.sh scripts/test-holder-recovery.sh
scripts/test-final-shell-state.sh
cargo test -p persist-metadata
cargo test -p persistd
cargo check --workspace --all-targets
cargo clippy -p persist-metadata -p persistd --all-targets -- -D warnings
git diff --check
```

结果：

- Shell state：13 passed，1 个手工 benchmark ignored。
- Holder 协议：15 passed。
- Holder runtime/lifecycle/PTY：16 passed。
- 隐藏 helper：3 passed。
- Bash/Zsh/Fish hook：9 passed。
- persist-metadata：38 passed。
- persistd：132 passed、1 个既有 zsh PTY 测试 ignored。
- persistd 进程测试：5 passed。
- reconciliation 进程测试：11 passed。
- workspace check、定向 Clippy、fmt 和 diff check 均退出 0。

真实场景覆盖：

- `quick_cd_exit_restores_final_cwd`
- `ctrl_d_restores_final_cwd`
- `daemon_offline_exit_preserves_final_cwd`
- `metadata_failure_keeps_exited_holder_context`
- `restart_after_metadata_before_retire_is_idempotent`
- `invalid_state_file_falls_back_without_blocking_exit`
- metadata 前 crash window 的 cwd 恢复
- Closed attach 后 `pwd`
- Bash EXIT trap 保留和 subshell 隔离

## 性能采样

命令：

```bash
cargo test -p persist-core shell_state_commit_benchmark -- --ignored --nocapture
```

同一私有目录连续原子写 1000 次，最终 sequence 均为 1000：

| 环境 | total_us | mean_us | max_us |
|---|---:|---:|---:|
| Ubuntu 26.04 本地 | 19155 | 19 | 92 |
| Rocky Linux 9.7 test | 826875 | 826 | 11039 |

该数据只记录当前文件系统和主机采样，不设未经基线验证的性能阈值。

## 构建与包

Ubuntu 原生构建：

```bash
cargo build --workspace --release --locked
PERSIST_PACKAGE_PLATFORM=ubuntu-26.04 \
PERSIST_PACKAGE_DIST=dist/m54-ubuntu \
scripts/package-release.sh tarball deb
```

Rocky Linux 9.7 原生构建：

```bash
cargo build --workspace --release --locked
PERSIST_PACKAGE_PLATFORM=rhel-9 \
PERSIST_PACKAGE_DIST=dist/m54-rhel9 \
scripts/package-release.sh tarball rpm
```

Artifacts：

| Artifact | SHA256 |
|---|---|
| Ubuntu tar.gz | `dc841d1a771e2f2bfbdea1c659f59e5f8a0e44ee9a5b8871671bfa33fce89204` |
| Ubuntu deb | `e95409f20c6d0f4dd1cda6c231a20f2dd94d6902d156d4368ef3545526519c06` |
| RHEL 9 tar.gz | `41643116816d5939b76044bb09e0c21469668c75712f568fce64d786545f0936` |
| RHEL 9 RPM | `723bb9b3f1d092e54ee1fb58ef54fb70155de71bc2abddc042ff9790c2d4ccf3` |

tar/deb/RPM 均包含 `persist`、`persistd` 和固定路径
`/usr/libexec/persistshell/persist-holder`。Rocky 原生三个二进制的最大 GLIBC symbol 均为
2.34。

## Rocky 安装验证

RPM 使用 `rpm -Uvh --oldpackage --replacepkgs` 安装。原因是主机已有同版本 Release
`1.el9`，本次验证包 Release `1` 的 EVR 较低；首次普通安装被 RPM 安全拒绝，未产生改动。

已安装二进制的隔离 XDG 场景结果：

- 快速 `cd; exit 23`：metadata 为 Closed/23/最终 cwd。
- Closed attach：`pwd` 输出最终 cwd。
- Ctrl+D：metadata 为 Closed/0/最终 cwd。
- daemon SIGKILL 后 Shell 离线 `exit 33`：重启后恢复最终 cwd 并 retire。
- 已有 Bash EXIT trap：marker 为 `preserved`，Session 正常 Closed/9。
- 显式 daemon stop：测试 socket、Holder socket和 state files 均清理。
- 验证前后既有 daemon PID 均为 107264。

远端源码临时目录和隔离测试目录已清理；RPM 保持安装状态。

## 发现与修复

- 修正旧 Close 即删除 Exited runtime 的行为，改为 metadata 成功后显式 retire。
- 修正 fake helper 混淆 history 与 state 两条数据通道的问题。
- 修正 crash 测试把 stale socket inode 当作新 daemon ready 的竞态。
- 修正 Ctrl+D 测试在 prompt hook 完成前发送 EOT 的竞态。
- 手工安装测试从 `script` 管道改为真实 PTY，避免 stdin EOF 被误判为用户退出。

## 已知限制

- M54 只恢复最终 cwd；动态环境变量恢复属于 M55。
- 用户已有 Bash EXIT trap 时不替换 trap，依赖实时 prompt 提交并记录 `exit-conflict`。
- Holder 被 SIGKILL 时仍受 M53 `lost` 边界限制。
- 非 UTF-8、超限、损坏或不可信状态安全降级，不提供强制 cwd 保证。

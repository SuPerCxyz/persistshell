# M56 通用 Linux 多架构发布包验证

日期：2026-07-20

状态：本地 x86_64 完成，ARM64 GitHub 原生验证待执行

## 实现范围

- glibc 2.28 单一 ABI 的 x86_64/aarch64 构建矩阵。
- 不绑定发行版版本的 RPM、DEB、tar.xz 与 SHA-256。
- release `opt-level=z`、fat LTO、单 codegen unit 和 symbol stripping。
- RPM/DEB 3 MiB、tar.xz 3.5 MiB 硬门禁。
- pidfd 不可用时的 PID、procfs start time 和 zombie 状态 fallback。
- Rocky/CentOS Stream 与 Ubuntu/Debian 安装和 Session smoke。

## Rust 门禁

执行：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

结果：fmt、clippy 通过；493 个测试通过，2 个既有测试 ignored。新增 5 个进程监控测试覆盖
复杂 proc stat、进程身份变化、无 pidfd 子进程退出、有界 timeout 和 fallback 错误分类。

## EL8 ABI 与体积

在干净 `rockylinux/rockylinux:8` x86_64 container 中从当前源码原生构建：

| 产物 | 结果 |
|---|---:|
| `persist` | GLIBC_2.28，1,263,200 bytes |
| `persistd` | GLIBC_2.28，2,290,424 bytes |
| `persist-holder` | GLIBC_2.28，873,960 bytes |
| RPM | 1,397,532 bytes |
| tar.xz | 1,387,192 bytes |
| DEB | 1,385,264 bytes |

三个包均低于硬门禁，checksum、架构 metadata、固定 Holder 路径、man page、completion 和用户
文档检查通过。额外使用 6 MiB 不可压缩假二进制验证 DEB 被 3 MiB 门禁拒绝。

## x86_64 多发行版

同一份 EL8 构建二进制完成安装、`persist --version`、foreground daemon、Holder socket、
Session new/list/close 和清理：

- Rocky Linux 8、9、10。
- CentOS Stream 9、10。
- Ubuntu 22.04、24.04、26.04。
- Debian 11、12、13。

所有环境均通过。容器共享宿主内核，不能替代真实 4.18 内核；pidfd fallback 由强制 procfs
路径测试验证。

## Actions 与 ARM64

`actionlint` 通过。workflow 使用 `ubuntu-24.04-arm` 原生 runner，Rocky 8/10、Ubuntu
22.04/26.04 和 Debian 11/13 镜像均确认存在 ARM64 manifest，不使用 QEMU 正式产物。

当前改动尚未 commit/push，GitHub ARM64 workflow 未执行。因此 M56 保持进行中，不能把
ARM64 构建、包体积和运行 smoke 记录为已通过。推送后必须检查全部 package jobs 和 artifacts，
再更新本审计、TODO、MILESTONES、CHANGELOG 与 NEXT_TASK。

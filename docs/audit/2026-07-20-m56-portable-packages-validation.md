# M56 通用 Linux 多架构发布包验证

日期：2026-07-20

状态：完成

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

GitHub Package workflow
[`29733941624`](https://github.com/SuPerCxyz/persistshell/actions/runs/29733941624)
最终通过 21 个 job：

- x86_64 与 aarch64 原生构建、RPM 和通用 tar.xz。
- amd64 与 arm64 DEB 构建。
- Rocky 8/9/10、CentOS Stream 9/10 RPM 安装和 Session smoke。
- Ubuntu 22.04/24.04/26.04、Debian 11/12/13 DEB 安装和 Session smoke。

ARM 原生门禁发现并修复了 `libc::c_char` 在 aarch64 为 `u8` 的可移植性问题。旧版
Debian/Ubuntu 容器还发现默认 `sh` 不支持 `pipefail`，DEB smoke 已显式使用 Bash。
CentOS Stream 9 首次运行在 Holder 启动读取处出现一次 EAGAIN，原样重跑通过；其余
20 个 job 首次通过。

## 最终产物

| 产物 | x86_64/amd64 | aarch64/arm64 |
|---|---:|---:|
| `persist` | 1,263,632 bytes | 1,123,080 bytes |
| `persistd` | 2,290,696 bytes | 2,186,216 bytes |
| `persist-holder` | 874,200 bytes | 795,216 bytes |
| RPM | 1,396,268 bytes | 1,348,044 bytes |
| DEB | 1,385,548 bytes | 1,337,928 bytes |
| tar.xz | 1,389,520 bytes | 1,340,236 bytes |

两种架构 ELF 的最高 symbol 均为 GLIBC_2.28。下载后的 6 个正式包 checksum 全部通过，
RPM metadata 分别为 x86_64/aarch64，DEB metadata 分别为 amd64/arm64，所有包均低于
体积门禁。

## CI 回归

普通 CI 连续暴露 `proven_stale_socket_is_replaced` 对已删除 socket inode 立即复用的错误
假设。等待判据改为设备号、inode 和纳秒 ctime 组成的身份；定向用例连续 20 次及完整
Holder lifecycle 通过。该修改只影响测试判据，不放宽生产 stale socket 检查。

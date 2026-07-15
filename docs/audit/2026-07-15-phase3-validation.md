# Phase 3 验证记录

## 自动化验证

2026-07-15 本地执行以下命令，均以退出码 0 完成：

```bash
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
git diff --check
```

测试套件包含 M38/M39/M41/M42 命令、协议和 daemon 覆盖，以及 M40 有效 Unix socket
校验。一个既有 zsh PTY 用例因系统 zshrc 被显式忽略。

## 本地 M43 基准

隔离 XDG 环境、1 KiB ring buffer、关闭 session log、release 二进制：

| Sessions | Create ms | List ms | Close ms |
|---:|---:|---:|---:|
| 100 | 2648 | 28 | 2987 |
| 500 | 65790 | 1254 | 8030 |
| 1000 | 223217 | 856 | 7779 |

基准期间发现 ClientSocket 将握手 5 秒超时继承到长操作，已在收到 `HELLO_ACK` 后清除并以
1000 Session 重测通过。

## test 主机验证

`test`（Rocky Linux 9.7）使用原生 Rust 1.96 构建并部署 `/usr/local/bin/persist` 与
`/usr/local/bin/persistd`。隔离端到端验证通过：daemon 启动、new、snapshot、metrics、
close 及 socket 清理。

同配置远端基准结果：

| Sessions | Create ms | List ms | Close ms |
|---:|---:|---:|---:|
| 100 | 813 | 16 | 235 |
| 500 | 4260 | 30 | 1233 |
| 1000 | 8794 | 55 | 2499 |

这些数据是环境基线，不应与不同硬件、内核或 ring buffer 配置直接比较。

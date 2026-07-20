# M53 PTY Holder 验证审计

## 结论

M53 单一 per-user PTY Holder 已完成本地、Ubuntu 26.04、Rocky Linux 9 和 `ssh test`
验证。daemon 异常退出不再终止 Holder、Shell 或前台任务；daemon 重启后使用同一 Holder
instance 对账并恢复 attach。显式 stop 仍级联关闭 Holder 和全部 Shell。

## 自动化门禁

- `scripts/test-holder-recovery.sh`：协议拒绝、权限、生命周期、真实 PTY、大输出、daemon
  SIGKILL、重连操作和 metadata 对账全部通过。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：全部通过；一个既有系统 zshrc PTY 用例 ignored。
- package/benchmark/recovery 脚本 `bash -n`、两份 man page groff 和 `git diff --check`：通过。

## 九项故障场景

1. 前台任务运行时 SIGKILL daemon：Holder 与 Shell PID 均存活。
2. daemon 离线期间输出 1 MiB：任务完成，Holder 持续排空 PTY，重连 replay 和日志均有标记。
3. daemon 重启：Holder PID/instance 不变，inventory 对账后 list 和 attach 可用。
4. 重连操作：41x101 resize、SIGTSTP/SIGCONT/SIGINT、readonly 和 writer takeover 通过。
5. daemon 离线期间 Shell exit：closed、exit code、日志及重复对账自动测试通过。
6. 显式 stop 与 SIGKILL：只有显式 stop 关闭 Holder/Shell 并清理 socket。
7. 身份、owner、权限、symlink、协议版本和超限帧均有拒绝测试。
8. 100/500/1000 Session 基准完成，记录 daemon/Holder CPU、RSS 和 attach 延迟。
9. Ubuntu 26.04 tar/deb、RHEL 9 tar/RPM 及 Rocky 9.7 test 端到端通过。

## 性能

本地 Ubuntu 24.04 容器使用 release 固定路径、1 KiB replay、关闭 Session 日志：

| Sessions | Create ms | List ms | Close ms | Attach ms | Daemon RSS KiB | Holder RSS KiB |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 369 | 5 | 236 | 1 | 6124 | 3160 |
| 500 | 1895 | 19 | 1632 | 1 | 6632 | 3824 |
| 1000 | 3894 | 32 | 5319 | 2 | 7540 | 4488 |

Rocky Linux 9.7 `ssh test` 使用最终 RPM：

| Sessions | Create ms | List ms | Close ms | Attach ms | Daemon RSS KiB | Holder RSS KiB |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 963 | 10 | 294 | 2 | 5788 | 3080 |
| 500 | 5820 | 21 | 1852 | 2 | 7044 | 3736 |
| 1000 | 11625 | 43 | 4259 | 2 | 8904 | 4496 |

1000 Session 下仍为单一 Holder，无每 Session helper 进程或长期线程。与 M50/M52 同主机基线
相比创建、列表和关闭没有回归，创建耗时明显下降；不同容器硬件数据不交叉比较。

## 平台包

- Ubuntu 26.04 原生 stable Rust 构建 tar/deb，checksum、版本、固定 Holder 路径和 0755 通过。
- Rocky 9 原生构建 tar/RPM，三个二进制最高 GLIBC 均为 2.34。
- RPM 安装路径为 `/usr/libexec/persistshell/persist-holder`，owner 为 root:root，mode 为 0755。
- RPM remove 后隔离用户的 metadata 和 4 个日志文件均保留，重新安装后可继续使用。

## test 主机

最终 RPM 安装在 Rocky Linux 9.7。隔离 XDG 环境验证 status/doctor、1 MiB 离线输出、
SIGKILL 接管、readonly replay、双 writer takeover、resize、SIGINT、exit code、显式 stop、
Holder SIGKILL 后 lost/attach 拒绝及卸载保留数据。既有 `/usr/local/bin` daemon 全程未停止。

## 验证中修复

- Metrics/Dashboard active writer 合并 Holder inventory 与当前 public attach，消除瞬时漏计。
- CLI 非阻塞读取不再把 `EAGAIN` 当 EOF。
- CLI 和 daemon 忽略 0x0 terminal resize，避免 Holder 拒绝空 resize payload 后断开 data socket。
- benchmark 增加协议级 attach 探针及 daemon/Holder RSS、CPU ticks 记录。

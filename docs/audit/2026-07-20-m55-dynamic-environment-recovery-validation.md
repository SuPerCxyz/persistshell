# M55 动态环境恢复验证审计

## 范围与状态

- 验证日期：2026-07-20
- 提交基线：`d9c5a05b12659b31ae9cd162b799a703f49fc97c`
- 验证状态：基线之上的 M53-M55 累计工作树；最终提交在全部门禁后创建
- 平台：Ubuntu 26.04 LTS / x86_64 / GLIBC 2.43
- 工具链：rustc 1.96.1，cargo 1.96.1

## 本地功能与安全

以下命令退出 0：

```text
scripts/test-dynamic-environment-recovery.sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

workspace 中 M55 reconciliation 为 14 项且无 skip。既有 ignored 项为 core 的手工特殊权限测试
和系统 zshrc 不兼容的 pipe 基线，与 M55 场景无关。

真实 PTY 覆盖：

- export、Closed、恢复、unset、再次恢复。
- client B 当前 `TERM` 覆盖 client A，保存的普通变量仍恢复。
- daemon 离线退出和 metadata 前/后两个崩溃窗口。
- 旧 Holder capability 探测断线后同 instance 降级重连。
- 虚构敏感环境名和值未出现在 state、SQLite、Session log 或 runtime 普通文件。

## 性能

release example `environment_benchmark` 每组运行 1000 次：

| 场景 | total | mean | max | 文件 | 失败 |
|---|---:|---:|---:|---:|---:|
| cwd-only 原子提交 | 9.495 ms | 9.495 us | 70.625 us | 111 B | 0 |
| 16 变量原子提交 | 16.748 ms | 16.748 us | 168.485 us | 1,242 B | 0 |
| 57,701 B 快照提交 | 132.842 ms | 132.842 us | 279.404 us | 57,701 B | 0 |
| 恢复合并 | 3.240 ms | 3.240 us | - | - | 0 |

这些是同机相对采样，不包含编译和 Shell 启动，不外推到其它文件系统。

## 本地发布包

- tarball SHA-256：
  `00a2ff2ce0edfeb218da6d4eb3eeb34e81df97991300fbe0820ca8613fe6c7ea`
- deb SHA-256：
  `4523399babced806ff3ef2658df4003b76458bec5537f7e137e9989d929b68c7`

tar/deb 均包含 `persist`、`persistd`、固定 libexec `persist-holder`、完整用户手册、man page
和 bash/zsh/fish completion。release 二进制忽略开发 Holder/helper 覆盖。

## Rocky test

- 主机：`ssh test`
- 系统：Rocky Linux 9.7，GLIBC 2.34，x86_64
- 工具链：rustc/cargo 1.96.1
- 安装验证 Shell：bash、zsh 5.8、fish 3.7.1
- 原生 tar SHA-256：
  `1b561fc38edaf2121f23562b5dc5891cea8b0b6bdd1e1380a47c1c1e9458bed1`
- 原生 RPM SHA-256：
  `e655482e2fab52dbfa9d52d5eccc24d156d0ccf35f30dd39c04bbc19b80ff211`

远程专项脚本和 workspace 全量测试退出 0。RPM `--replacepkgs` 后，隔离 HOME/XDG 中完成
doctor、new、ls、动态 set/restore/unset、敏感值扫描、显式 stop 和 socket 清理。bash/zsh/fish
真实 PTY 与 hook 测试均通过。主机既有 `/usr/local/bin/persistd foreground` PID 在验证前后
均为 107264；测试创建的 daemon/Holder 已全部清理。

## 失败与修正

- 计划误写 `package-release.sh tar`，脚本实际参数为 `tarball`；已修正文档。
- 性能 example 首次未显式映射 `io::Error`；补充稳定操作名后通过。
- Rocky 缺少 zsh/fish；经维护者授权安装后完成三 Shell 验证。
- 远程全量发现 Shell 缺失和宿主 LANG 假设；测试改为可移植的可执行检查和 metadata 期望。
- Holder 接管测试在慢调度下抢跑 public socket；增加有界 socket 等待。
- 安装 smoke 初次在 write grant 前发送输入；增加有界 attach 就绪等待和失败清理。
- 最终 workspace 的 Ctrl+D 测试在 prompt 切换窗口偶发超时；发送 Ctrl+U 后再 Ctrl+D，
  明确空行语义并消除编辑缓冲竞态。

## 已知边界

未导出变量无法恢复。SIGKILL、exec、hook 删除或状态损坏回退上一可信快照。旧 Holder
没有环境 capability 时保持 cwd-only。当前连接变量不持久化，敏感禁区不能由 include 绕过。

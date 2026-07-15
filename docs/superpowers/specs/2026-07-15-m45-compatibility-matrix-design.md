# M45 兼容性矩阵设计

## 目标

在当前可访问环境记录 Linux 发行版、可执行 Shell 和终端类型，并验证每个可用 Shell 的
daemon/new/list/close 基线。缺失的 Shell 记录为跳过，不安装额外依赖。

## 运行方式

新增 `scripts/compatibility-matrix.sh`。它在每个 Shell 组合中创建独立 XDG 目录，设置
`SHELL` 和 `TERM`，启动 `persistd foreground`，调用 `persist new`、`persist ls` 与
`persist close`，最后清理 daemon 和临时目录。

默认检测 `bash zsh fish`；可用 `PERSIST_COMPAT_SHELLS`、`PERSIST_BIN`、`PERSISTD_BIN`
和 `PERSIST_COMPAT_TERM` 覆盖。输出 CSV，缺失命令输出 `skipped`。

## 非目标

- 不模拟真实 SSH、终端 resize、attach 交互或发行版容器。
- 不安装缺失 Shell，不声明未验证发行版支持。
- 不替代 M20 的 PTY 交互测试。

## 验证计划

本机运行所有已安装 Shell，`test` 主机运行其可用 Shell。记录 OS、TERM、通过/跳过原因。

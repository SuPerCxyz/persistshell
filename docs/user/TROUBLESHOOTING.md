# PersistShell Troubleshooting

本文档只覆盖当前已实现的 Linux per-user daemon 行为。运行任何清理命令前，先确认当前用户
和目标 Session；不要删除其他用户的 runtime 或数据目录。

## daemon 未运行或找不到 socket

症状通常是 `E_SOCKET_MISSING`、`E_DAEMON_NOT_RUNNING` 或 `persist ls` 无法连接。

```bash
persist daemon status
persist daemon start
persist doctor
```

默认 socket 是 `/run/user/$UID/persistshell/persist.sock`。普通 `persist new`、`persist ls`
和 `persist attach <id>` 不会自动启动 daemon；SSH hook 会先尝试 `persist daemon start`。

## socket 或目录权限错误

运行目录必须为 0700，socket 必须为 0600。先停止当前用户的 daemon，再重新启动：

```bash
persist daemon stop
persist daemon start
persist doctor
```

不要用 `/tmp` 替换 runtime socket 目录，也不要修改其他用户的 `/run/user/<uid>/persistshell/`。

## SSH 登录没有自动进入 Session

确认 hook 已安装且为交互 SSH：

```bash
persist install
persist doctor
```

安装器当前只管理 bash 和 zsh profile。fish 需要手动配置 hook。非交互命令、scp、sftp、
rsync、ansible 和 Git remote command 应绕过 hook。

## 需要立刻绕过 PersistShell

```bash
PERSIST_DISABLE=1 ssh node
```

也可使用不加载 profile 的 shell：

```bash
ssh node 'bash --noprofile --norc'
```

确认需要移除 hook 时执行 `persist uninstall`；`persist uninstall --purge` 还会删除当前用户的
PersistShell 配置、数据和状态目录。

## exit 或 Ctrl+D 后 Session 显示 closed

这是预期行为：shell runtime 已释放，不会继续占用资源。先查看记录，再恢复：

```bash
persist ls
persist log <id>
persist attach <id>
```

恢复会创建新的可写 PTY，仅恢复 cwd 与 `TERM`、`COLORTERM`、`LANG`、`LC_*` 启动环境。

## 另一台电脑无法继续输入

一个 Session 同时只有一个 active writer。用新电脑执行可写 attach 可接管输入：

```bash
persist attach <id>
```

旧 writer 会被撤销。若只需观察输出，使用 `persist attach --readonly <id>`。

## 日志或输出不完整

先区分实时 ring buffer 与持久日志。查看日志：

```bash
persist log <id>
persist log search <keyword> --session <id>
persist log export <id> --output session.log
```

`logging.session_log = false` 时不会写入 Session 输出日志。输出包含的密码或 token 不会被自动
识别和删除，处理日志时应遵守本机安全要求。

## `persist ls` 看不到最新命令历史

先确认是在真实终端中运行；脚本、管道和重定向会自动使用纯表格模式。可直接指定 Session：

```bash
persist ls 2
```

实时历史只记录被 Shell 原生 history 接受的命令。如果用户配置禁用 history、过滤了该命令，
或临时 hook 安装失败，记录不会出现。该故障不影响 Shell、attach 或输出日志；终端输出仍可用：

```bash
persist log 2
persist replay 2
```

PersistShell 不会为了修复实时历史而修改用户 Shell 配置。记录文件权限应为 `0600`：

```bash
ls -l ~/.local/share/persistshell/history/2.commands
```

## `persist top` 无法打开或显示 disconnected

`persist top` 只支持真实终端，管道、重定向和非交互 SSH 会返回明确错误。先检查 daemon：

```bash
persist doctor
persist ls --plain
```

daemon 短暂重启时，界面保留最后一次数据并显示 disconnected，随后按有界退避自动重连。
长时间不恢复时退出界面，检查 runtime socket 和 daemon 日志：

```bash
ls -ld /run/user/$UID/persistshell
ls -l /run/user/$UID/persistshell/persist.sock
tail -100 /run/user/$UID/persistshell/daemon.log
```

24h 趋势按分钟写入分段，当前分钟或刚启动 daemon 时暂时没有趋势点是正常现象。Dashboard
故障不会停止 Session、attach 或日志路径。

## 直接运行 persistd

前台诊断用法：

```bash
persistd help
persistd foreground --idle-timeout 30m
```

不要使用 `persistd foreground --help`；当前会进入 foreground daemon。该问题记录在
`docs/known/KNOWN_ISSUES.md`。

# PersistShell 完整用户手册

本手册是 PersistShell 的完整用户入口。只阅读本文件即可完成安装、首次使用、Session 管理、
命令历史查看、跨电脑接管、恢复、日志查看、故障排查和卸载。

## 1. PersistShell 是什么

PersistShell 是 Linux 上的持久交互式 Shell 运行时。它让 Shell 和 SSH 连接拥有不同的生命周期：

```text
SSH 断开 != Shell 退出
```

网络中断或本地电脑关闭时，SSH client 会离开，但 PersistShell daemon 继续持有 Shell、PTY、
前台任务和输出。之后可以从同一台或另一台电脑重新连接并继续操作。

PersistShell 不是 tmux、screen 或 Zellij，不提供窗口、pane、layout 或 prefix key。它也不是
SSH Server、堡垒机、Web Terminal 或远程桌面。

## 2. 核心概念

- **Session**：一个由 PersistShell 管理的 Shell 工作环境，使用数字 ID 标识。
- **daemon**：当前 Linux 用户的 `persistd` 进程，管理所有 Session。
- **attach**：把当前终端连接到 Session，可查看输出并输入命令。
- **detach**：只断开当前终端，Shell 和任务继续运行。
- **Closed Session**：Shell runtime 已释放，但输出、cwd、有限环境快照和 metadata 仍保留。
- **writer takeover**：另一台电脑 attach 后接管唯一写权限，旧 writer 不再发送输入。

每次交互式 SSH 登录默认创建新 Session。PersistShell 不会自动进入旧 Session；恢复旧工作必须
显式选择或 attach。

## 3. 安装

发行包安装后应提供两个命令：

```bash
persist --version
persistd --version
```

启动当前用户 daemon：

```bash
persist daemon start
persist daemon status
```

运行诊断：

```bash
persist doctor
```

若要让交互式 SSH 自动进入 PersistShell：

```bash
persist install
```

退出当前 SSH 后重新登录，hook 才会按新的 profile 生效。当前安装器管理 bash 和 zsh profile；
fish completion 随发行包安装，但 SSH 自动接管 profile 仍需按本机策略配置。

## 4. 五分钟快速开始

创建一个 Session：

```bash
persist new
```

查看并选择 Session：

```bash
persist ls
```

直接进入指定 Session：

```bash
persist attach 2
```

从本机直接通过 SSH 进入远端 Session 2：

```bash
ssh -t node 'persist attach 2'
```

SSH 意外断开后，重新执行相同 attach 命令即可继续。若要主动离开但保留 Shell runtime，可关闭
SSH client，或在另一个终端执行：

```bash
persist detach 2
```

## 5. 正确理解退出行为

以下操作含义不同：

| 操作 | Shell runtime | Session 记录 | 后续 attach |
|---|---|---|---|
| SSH 断开 | 继续运行 | 保留 | 回到同一 runtime |
| detach | 继续运行 | 保留 | 回到同一 runtime |
| `exit` | 释放 | 保留 | 启动新 runtime 并恢复有限上下文 |
| 空行按 `Ctrl+D` | 释放 | 保留 | 启动新 runtime 并恢复有限上下文 |
| `persist close <id>` | 优雅关闭 | 保留 | 可冷恢复 |
| `persist kill <id>` | 强制终止 | 保留关闭结果 | 可按状态处理 |

`exit` 和 Shell 空行上的 `Ctrl+D` 不会让 Session 在后台继续占用 Shell 和 PTY。Closed Session
再次 attach 时会启动新的 Shell runtime；已退出的前台进程不会复活。

## 6. 使用 `persist ls` 选择 Session

在真实终端中执行：

```bash
persist ls
```

命令先显示 Session 表格，再提示输入 Session ID。选择后显示操作菜单：

```text
[h] 查看命令历史
[a] attach 进入会话
[b] 返回 Session 列表
[q] 退出
```

直接打开 Session 2 的菜单：

```bash
persist ls 2
```

脚本、管道、重定向和 shell completion 中，`persist ls` 只输出表格，不进入交互。TTY 中也可
强制使用纯表格模式：

```bash
persist ls --plain
```

按标签筛选：

```bash
persist ls --tag work
```

## 7. 查看实时命令历史

在 Session 菜单中输入 `h`。历史默认每页显示 50 条，顺序为最新命令优先：

```text
[105] 2026-07-16 15:42:31  cargo test --workspace
[104] 2026-07-16 15:40:08  git status
[103] 2026-07-16 15:38:22  cd /srv/persistshell
```

下一页查看更早记录，上一页返回较新记录。多行命令作为一条记录显示。查看结束后仍回到 Session
菜单，可以 attach 当前 Session，也可以返回列表选择其他 Session。

实时命令历史只镜像已被 Shell 原生 history 接受的命令，不读取 PTY 原始输入。bash 的
`HISTCONTROL` 和 `HISTIGNORE` 继续生效。检测到自定义 `zshaddhistory`、zsh history 过滤选项或
`fish_should_add_to_history` 时，PersistShell 为避免绕过或重复执行用户过滤逻辑，会停用该
Session 的实时镜像，并在历史视图中明确提示；原生 history、用户 hook 和 attach 不受影响。
密码程序直接从终端读取的密码不会记录；写在普通命令行中的 token 或密码仍应通过 Shell 自身
history 规则排除。

PersistShell 不修改 `.bashrc`、`.zshrc` 或 `config.fish`。实时 hook 失败时 Shell 继续可用，
但历史视图会提示实时记录不可用。历史文件默认最多 10,000 条或 4 MiB。

## 8. Attach 和多电脑接管

可写 attach：

```bash
persist attach 2
```

只读 attach：

```bash
persist attach 2 --readonly
```

同一 Session 默认只有一个 active writer。另一台电脑可正常 attach 并接管写权限，不是只能
只读查看。接管后旧 writer 会收到撤销通知，其后输入不再发送到 PTY。

Session 被 lock 时不能 attach、kill 或被 Idle GC 清理。先解锁：

```bash
persist unlock 2
```

## 9. SSH 自动接管和绕过

安装 hook：

```bash
persist install
```

hook 只处理带 `SSH_TTY` 的交互登录。以下非交互场景应保持普通 SSH 行为：

```bash
ssh node hostname
scp file node:/tmp/
sftp node
rsync file node:/tmp/
git clone user@node:repo.git
```

临时绕过 PersistShell：

```bash
PERSIST_DISABLE=1 ssh node
```

远程执行干净 Shell：

```bash
ssh node 'bash --noprofile --norc'
```

删除 SSH hook 但保留数据：

```bash
persist uninstall
```

## 10. Session 管理命令

重命名：

```bash
persist rename 2 build-kernel
```

查看或设置备注，空文本用于清除：

```bash
persist note 2
persist note 2 '等待测试结果'
persist note 2 ''
```

管理标签：

```bash
persist tag 2 add work
persist tag 2 list
persist tag 2 remove work
```

pin 可防止 Idle GC 清理：

```bash
persist pin 2
persist unpin 2
```

锁定和解锁：

```bash
persist lock 2
persist unlock 2
```

关闭或强制终止：

```bash
persist close 2
persist kill 2
```

## 11. 输出日志和 Replay

命令历史只包含 Shell 命令；程序输出使用 Session 日志：

```bash
persist log 2
```

搜索所有日志：

```bash
persist log search error
persist log search error --session 2
persist log search error --session 2 -i
```

导出日志：

```bash
persist log export 2 --output session-2.log
persist log export 2
```

回放完整日志或选择头尾字节：

```bash
persist replay 2
persist replay 2 --tail 4096
persist replay 2 --head 4096
```

当前 `--speed` 和 `--follow` 可以解析但尚未改变行为，不能依赖它们做实时跟随或按原始节奏回放。

## 12. 进程和资源观测

查看前台进程树：

```bash
persist ps 2
```

查看前台进程 CPU ticks、RSS 和 I/O 累计值：

```bash
persist stats 2
```

查看不包含敏感内容的有界 JSON 快照：

```bash
persist snapshot 2
```

查看 daemon 和 Session 聚合指标：

```bash
persist metrics
```

打开性能仪表盘入口：

```bash
persist top
```

`persist top` 要求 stdin/stdout 均为 TTY。当前阶段已接入有界分页和趋势数据客户端；全屏图表
和键盘交互将在后续阶段完成。`snapshot` 与 `metrics` 仍是一次性命令。

## 13. Daemon 管理

```bash
persist daemon start
persist daemon status
persist daemon stop
```

`persist new`、`persist ls` 和手动 `persist attach` 不保证自动启动 daemon。连接失败时先执行
`persist daemon start`。SSH hook 会尝试启动 daemon。

当前主要使用 per-user daemon，不提供 systemd user unit。不要同时运行多个 `persistd` 实例；
PID 文件和锁会拒绝重复启动。

## 14. 配置

查看有效配置：

```bash
persist config show
```

配置加载顺序为默认值、系统配置、用户配置：

```text
/etc/persistshell/config.toml
~/.config/persistshell/config.toml
```

常用配置包括 Socket 路径、ring buffer、Session 输出日志、日志轮转、daemon Idle GC 和内部日志。
配置错误会阻止对应命令启动，并显示稳定错误码和建议。

## 15. 文件和目录

```text
~/.config/persistshell/                 用户配置
~/.local/share/persistshell/            metadata、Session 日志和 history
~/.local/state/persistshell/            client/daemon 状态日志
/run/user/$UID/persistshell/            PID、Socket 和运行时文件
```

关键权限：

```text
运行目录和 history 目录  0700
Socket、日志和命令记录   0600
metadata                  仅当前用户可访问
```

## 16. Closed Session 恢复边界

Closed Session attach 会启动新的 Shell runtime，并尝试恢复：

- 最后成功采样的 cwd
- `TERM`、`COLORTERM`、`LANG`、`LC_*` 等受限启动环境
- Session 独立 history
- 输出日志和 metadata

不会恢复：

- 已经退出的前台进程
- 普通子进程的内存状态
- 完整终端画面状态
- Shell 运行期间任意动态 `export` 的变量
- 失效的旧 `SSH_AUTH_SOCK`

有效的当前 SSH agent Unix socket 可在新 Session 启动时同步。普通文件、相对路径和失效 socket
会被拒绝。

快速执行 `cd /path; exit` 可能早于下一次 `/proc` cwd 采样，从而保留上一次 cwd。需要可靠恢复
时，在退出前让 Shell 回到提示符，或避免在同一条命令中立即退出。

## 17. 升级和卸载

升级发行包前可先停止 daemon：

```bash
persist daemon stop
```

安装新包后重新启动并检查：

```bash
persist daemon start
persist doctor
persist --version
```

只移除 SSH hook并保留数据：

```bash
persist uninstall
```

移除 hook、配置、数据和状态：

```bash
persist uninstall --purge
```

`--purge` 会删除当前用户的 Session metadata、日志和历史，执行前应确认不再需要这些内容。

## 18. 常见故障

### daemon 未运行

```bash
persist daemon start
persist daemon status
```

### SSH 没有自动进入 PersistShell

```bash
persist doctor
persist install
```

确认是交互 SSH，并重新登录使 profile 生效。

### 无法 attach

先查看状态：

```bash
persist ls --plain
```

若 Session 已锁定，执行 `persist unlock <id>`。另一台电脑已连接时，正常可写 attach 会请求
takeover；只想观察时使用 `--readonly`。

### 实时命令历史为空

确认使用真实 TTY，并检查 Shell 是否禁用了 history 或过滤了命令。history hook 失败不影响
Session；程序输出仍可通过 `persist log` 和 `persist replay` 查看。

### Socket 权限错误

```bash
persist doctor
ls -ld /run/user/$UID/persistshell
ls -l /run/user/$UID/persistshell/persist.sock
```

期望目录 `0700`、Socket `0600`。不要把 Socket 放在不受保护的共享目录。

## 19. 命令速查

```text
persist help                              显示帮助
persist version                           显示版本
persist doctor                            诊断配置、目录、daemon 和 Socket
persist config show                       显示有效配置
persist daemon start|stop|status          管理 daemon
persist new                               创建 Session
persist ls                                列表并在 TTY 中交互选择
persist ls <id>                           打开指定 Session 菜单
persist ls --plain                        只输出表格
persist ls --tag <tag>                    按标签筛选
persist attach [<id>]                     可写 attach；省略 ID 时新建
persist attach <id> --readonly            只读 attach
persist detach <id>                       断开 active client
persist close <id>                        优雅关闭 runtime
persist kill <id>                         强制终止 runtime
persist rename <id> <name>                重命名
persist note <id> [text]                  查看、设置或清除备注
persist tag <id> add|remove|list [tag]     管理标签
persist pin|unpin <id>                    设置或取消 pin
persist lock|unlock <id>                  锁定或解锁
persist log <id>                          查看输出日志
persist log search <keyword> [options]    搜索输出日志
persist log export <id> [--output path]   导出输出日志
persist replay <id> [--head n|--tail n]   回放输出
persist ps <id>                           查看前台进程树
persist stats <id>                        查看前台资源计数
persist snapshot <id>                     查看受限 JSON 快照
persist metrics                           查看聚合指标
persist top                              打开性能仪表盘
persist install                           安装 SSH 自动接管 hook
persist uninstall [--purge]               移除 hook 和可选数据
```

## 20. 安全使用建议

- 不要在普通命令行参数中写入密码或长期 token。
- 对敏感命令配置 Shell 原生 history 排除规则。
- Session 输出可能包含程序主动打印的敏感信息；必要时关闭 Session 日志。
- 定期清理不再需要的日志和历史。
- 不共享当前用户的 runtime、数据目录或 Socket。
- 始终保留 `PERSIST_DISABLE=1` 和干净 Shell 逃生方式。

## 21. 当前支持边界

- 仅支持 Linux x86_64 的已验证发行包基线。
- bash、zsh、fish 需要分别通过实时 history 兼容验证；自定义复杂插件可能触发安全降级。
- 不提供 pane、window、layout、Web UI 或多人共享权限模型。
- daemon 崩溃后的 PTY 存活和全部 TUI 画面恢复尚未承诺。
- `replay --speed` 与 `--follow` 当前未实现实际效果。
- 非交互 SSH、scp、sftp、rsync、Ansible 和 Git remote command 不应被自动接管。

遇到问题时先执行：

```bash
persist doctor
persist daemon status
persist ls --plain
```

需要立刻恢复普通 SSH 时：

```bash
PERSIST_DISABLE=1 ssh node
```

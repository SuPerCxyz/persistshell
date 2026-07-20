# PersistShell Commands

本文档描述 PersistShell CLI 命令。

---

## 命令总览

```bash
persist
persist version
persist new
persist ls
persist ls <id>
persist ls --plain
persist ps <id>
persist stats <id>
persist snapshot <id>
persist metrics
persist attach [<id>] [--readonly]
persist detach <id>
persist kill <id>
persist lock <id>
persist unlock <id>
persist rename <id> <name>
persist note <id> [<text>]
persist tag <id> <add|remove|list> [<tag>]
persist pin <id>
persist unpin <id>
persist log <id>
persist log search <keyword> [--session <id>] [-i]
persist log export <id> [--output <path>]
persist replay <id> [--tail <n>] [--head <n>] [--speed <f>] [--follow]
persist daemon start
persist daemon stop
persist daemon status
persist doctor
persist install
persist uninstall
persist config show
```

---

## persist

无参数执行时显示帮助。SSH hook 使用 `persist attach`；省略 Session ID 时，
`persist attach` 会创建并 attach 一个新 Session。

---

## persist version

显示版本。

```bash
persist version
```

输出：

```text
persist 0.2.0
```

---

## persist new

创建新 Session。

```bash
persist new
```

---

## persist ls

列出并选择 Sessions。

```bash
persist ls
persist ls 2
persist ls --plain
```

stdin 和 stdout 都连接真实终端时，`persist ls` 显示列表后进入交互选择。输入 Session ID 后，
可以查看该 Session 的 Shell 命令历史、attach、返回列表或退出。`persist ls <id>` 直接打开
指定 Session 菜单。

```text
[h] 查看命令历史
[a] attach 进入会话
[b] 返回 Session 列表
[q] 退出
```

命令历史默认每页显示最近 50 条，最新执行的命令优先。查看后仍回到菜单，可进入当前 Session
或返回选择其他 Session。Closed Session 也可以查看历史和 attach 冷恢复。

管道、重定向、completion 和非交互 SSH 中，`persist ls` 保持当前纯表格输出并立即退出。
`--plain` 可在 TTY 中强制使用该行为。

`FOREGROUND` 列显示运行中 Session 的前台进程命令摘要；没有前台进程或无法读取
`/proc` 时显示 `-`。

示例：

```text
ID   NAME              STATUS     AGE    LAST     CWD              CMD
1    ssh-153011        running    10m    1m       /root            bash
2    make-build        detached   2h     5m       /usr/src/linux   make -j64
```

---

## persist ps

查看运行中 Session 的前台进程组树。

```bash
persist ps 2
```

进程读取失败、Session 已关闭或无前台进程时，命令输出：

```text
(no foreground process)
```

---

## persist stats

查看前台进程的瞬时资源计数。

```bash
persist stats 2
```

输出包括 CPU user/system ticks、RSS KiB 和读写累计字节。它不计算 CPU 百分比，
也不保存采样历史。

---

## persist snapshot

输出 Session 的受限、只读 JSON 快照。

```bash
persist snapshot 2
```

快照包含 metadata 状态、关闭时间、cwd、shell、锁定/pin/note/tag 标记、当前
writer 状态、输出日志路径及前台进程摘要。它不包含 note 或 tag 的实际内容、环境变量、
用户输入和 SSH agent 路径；输出上限为 16 KiB。不存在的 Session 会返回错误。

---

## persist metrics

输出 daemon 与 Session 的基础聚合指标。

```bash
persist metrics
```

输出为受限 JSON，包含 daemon PID、Session 总数、running/closed/locked/pinned 数量、
runtime 数量、active writer 数量和只读客户端数量。它不启动 metrics server、不保存采样历史，
也不输出 Session 内容或敏感数据。

---

## persist top

打开交互式性能仪表盘入口。

```bash
persist top
```

该命令仅允许在 stdin/stdout 都是 TTY 时运行。主视图显示 daemon 状态和 Session 表格，默认
按 CPU 降序；`j/k` 或方向键移动，`s` 切换 CPU/RSS/I/O/进程数/ID 排序，`Enter` 打开详情，
`r` 切换 15 分钟/1 小时/24 小时趋势，`Esc` 返回，`q` 或 `Ctrl+C` 退出。

指标每 5 秒刷新；断线时保留最后数据并显示状态，随后有界重连。窄小终端会隐藏次要列或退化
为数值摘要。正常、错误和按键退出都会恢复终端模式。脚本继续使用 `persist metrics`。

---

## persist attach

Attach 到已有 Session。省略 ID 时创建一个新 Session 后 attach。

```bash
persist attach 2
persist attach --readonly 2
```

默认 attach 可写；另一台电脑可 attach 并接管唯一的 active writer，之前的 writer 会被撤销。
`--readonly` 只能接收输出，不能发送输入。

---

## persist detach

Detach 当前 Session。

```bash
persist detach 2
```

---

## persist kill

终止 Session。

```bash
persist kill 2
```

---

## persist lock / unlock

锁定重要 Session，避免误 attach、误 kill 和 Idle GC 清理。锁定后，
`persist ls` 的 `LOCK` 列会显示锁定标记；先 `unlock` 才能重新 attach 或 kill。

```bash
persist lock 2
persist unlock 2
```

---

## persist rename

重命名 Session。

```bash
persist rename 2 make-build
```

---

## persist note / tag / pin

为 Session 保存简短 note、管理 tag，或标记为 pinned。`note` 省略文本时读取当前值；传入
空文本会清除 note。pinned Session 不会被 Idle GC 清理。

```bash
persist note 2 "waiting for CI"
persist tag 2 add release
persist tag 2 list
persist pin 2
persist unpin 2
```

---

## persist log

查看 Session 日志。

```bash
persist log 2
```

搜索 Session 日志：

```bash
persist log search build --session 2 --ignore-case
```

导出 Session 日志：

```bash
persist log export 2 --output session-2.log
```

---

## persist replay

回放已记录的输出。当前 `--tail` 与 `--head` 已实现；`--speed` 和 `--follow` 可解析但尚未
改变输出行为，详见已知限制。

```bash
persist replay 2 --tail 200
persist replay 2 --follow
```

---

## persist daemon start

启动 daemon。

```bash
persist daemon start
```

---

## persist daemon stop

停止 daemon。

```bash
persist daemon stop
```

停止 daemon 会终止其运行循环；不要把它当作保留 running Session 的安全 detach 操作。

---

## persist daemon status

查看 daemon 状态。

```bash
persist daemon status
```

---

## persist doctor

诊断环境。

```bash
persist doctor
```

它检查配置、运行目录、socket、PTY、hook 和 daemon 连通性，并报告各项结果。

---

## persist install

安装 SSH 自动接管。

```bash
persist install
```

---

## persist uninstall

卸载 SSH 自动接管。

```bash
persist uninstall
```

完全清理：

```bash
persist uninstall --purge
```

---

## persist config show

显示当前配置。

```bash
persist config show
```

---

## 退出码

`persist` 成功时返回 0；错误按类别返回：

```text
0  success
1  invalid argument or configuration validation error
2  environment or configuration read/parse error
3  I/O or system call error
4  protocol error
5  internal error
```

错误输出包含稳定错误码和可行的修复建议。命令输出面向人类；当前没有 `persist ls --json`
接口，`snapshot` 与 `metrics` 是受限 JSON 输出。

---

## 命令设计原则

1. 常用命令要短。
2. 危险命令要明确。
3. 错误提示要有修复建议。
4. 输出默认面向人类。
5. 后续支持 JSON 面向脚本。

# ADR-0003：使用临时 Shell Hook 记录实时命令历史

## 状态

Accepted

## 背景

PersistShell 已为每个 Session 设置独立 `HISTFILE`，但 bash、zsh 和 fish 的落盘时机不同，
Running Session 的最新命令不一定可见。用户要求从 `persist ls` 查看实时命令历史，同时把不
影响用户 Shell 配置作为第一优先级。

PTY 输入不是可靠命令边界，也可能包含密码程序读取的敏感内容。直接记录 stdin 会破坏安全
原则。修改 `.bashrc`、`.zshrc` 或 `config.fish` 会污染用户环境，也不可接受。

## 决策

PersistShell 为其启动的根 Shell 安装仅在当前进程生效的临时 hook：

- 先加载用户原配置，再组合安装命名隔离 hook。
- bash 组合已有 `PROMPT_COMMAND`。
- zsh 使用 `add-zsh-hook precmd`，不修改 history options。
- fish 使用进程级初始化和事件 hook，不写配置文件。
- hook 只同步已被 Shell 原生 history 接受的命令。

hook 将命令通过 stdin 传给 `persist` 的隐藏受限入口。入口根据当前配置和 Session ID 计算固定
路径，拒绝任意路径和超限记录。它不连接 PTY、不解释命令，也不把命令放入进程参数。

记录写入独立结构化文件：

```text
~/.local/share/persistshell/history/<session-id>.commands
```

文件保存序号、完成时间、Shell 类型和多行命令，权限为 `0600`，父目录为 `0700`。默认最多
10,000 条或 4 MiB，超过任一限制时保留最新记录。

## 配置兼容规则

- 不编辑任何用户 dotfile。
- 不覆盖已有 prompt 或 hook；新增 hook 必须组合执行。
- 用户的 history 过滤规则继续决定命令是否可记录。
- hook 不改变上一条用户命令的退出状态。
- hook 或记录写入失败不得阻止 Shell 启动或显示 prompt。
- 用户配置显式禁用 history 时，不创建替代的输入记录。
- fish 存在无法安全复用的自定义 history 过滤时，降级为实时历史不可用。

## 被拒绝方案

### 记录 PTY stdin

无法可靠区分命令、编辑按键和密码输入，违反默认不记录密码的安全要求。

### 修改用户 Shell 配置

可获得稳定 hook，但会持久污染用户环境，并可能覆盖用户已有配置。

### 只读取原生 history 文件

实现简单且兼容，但 Running Session 的最新命令可能尚未落盘，不满足实时查看要求。

### 修改 metadata SQLite schema

可集中查询，但命令内容不属于 metadata，增加迁移、锁竞争和敏感数据面，没有必要。

## 影响

每次完成交互命令会启动一个短生命周期 `persist` helper，带来有限 prompt 开销。实现必须增加
延迟测试，并保证 helper 失败静默降级。命令历史是用户 Shell history 的受限镜像，不等同于
Session 输出日志；终端输出继续由 `persist log` 和 `persist replay` 提供。

## 验证

测试覆盖三种 Shell 的原配置执行、已有 hook 保留、history 过滤、退出状态、多行命令、实时
可见性、权限、容量和失败降级。还必须验证 PTY stdin 与密码提示内容不会被该机制直接记录。

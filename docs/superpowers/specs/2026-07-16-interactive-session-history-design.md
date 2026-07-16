# 交互式 Session 命令历史设计

## 背景
当前 `persist ls` 只输出 Session 表格。用户必须另外执行 `persist log`、`persist replay` 或
`persist attach <id>`，无法在列表中选择 Session、查看其 Shell 命令历史，再决定进入当前
Session 或返回选择其他 Session。现有独立 `HISTFILE` 也依赖 Shell 自身退出或刷新时机，不能
保证 Running Session 的最新命令已经落盘。

## 目标
- 终端中的 `persist ls` 支持交互选择 Session。
- `persist ls <id>` 直接打开指定 Session 的操作菜单。
- 用户可查看实时 Shell 命令历史、attach、返回列表或退出。
- 命令历史倒序显示，最新执行的命令始终优先。
- 不修改、不覆盖且不持久改变用户 Shell 配置。
- 不记录原始终端输入、密码提示输入或被 Shell history 规则拒绝的命令。

## CLI 行为
```text
persist ls                         TTY 中列出并进入交互选择
persist ls <id>                    直接打开指定 Session 菜单
persist ls --plain                 强制只输出表格
persist ls --tag <tag>             按 tag 过滤后进入相同流程
```

stdin 和 stdout 都连接 TTY 时，`persist ls` 默认进入交互模式。管道、重定向或非交互 SSH 中
保持当前表格输出并退出，避免破坏脚本。选择 Session 后显示：

```text
[h] 查看命令历史
[a] attach 进入会话
[b] 返回 Session 列表
[q] 退出
```

历史页和 attach 返回后重新显示菜单。Session 状态变化时重新读取列表；已不存在的 Session
显示明确错误并返回列表。

## 历史显示
- 默认每页 50 条，按记录序号倒序排列。
- 第一页从最新命令开始；下一页查看更早记录，上一页返回较新记录。
- 多行命令作为一条记录保存和显示，不按文本行拆分。
- Running 和 Closed Session 使用相同排序及分页语义。
- 空历史显示明确提示，不回退到终端输出日志。

## 实时记录模型
PersistShell 使用独立、结构化的命令记录文件，不解析 PTY stdin，也不依赖终端输出：

```text
~/.local/share/persistshell/history/<session-id>.commands
```

每条记录包含单调递增序号、完成时间、Shell 类型和完整命令。写入采用长度边界格式，支持多行
命令并拒绝损坏或超限记录。目录权限为 `0700`，文件权限为 `0600`。默认最多保留 10,000 条
或 4 MiB；达到任一上限后压缩掉最旧记录，禁止无限增长。

## Shell 集成
集成只存在于 PersistShell 启动的 Shell 进程内，先加载用户原配置，再追加命名隔离 hook：

- bash：组合现有 `PROMPT_COMMAND`，不覆盖 scalar 或 array 中已有命令。
- zsh：使用 `add-zsh-hook precmd` 追加独立函数，不改变 history options。
- fish：使用进程级初始化命令和原生 history 接口，不写入 `config.fish`。

hook 只读取已被 Shell 原生 history 接受的最新命令，再调用受限内部写入入口。bash 的
`HISTCONTROL` 和 `HISTIGNORE` 继续生效。自定义 `zshaddhistory`、zsh history 过滤选项和
`fish_should_add_to_history` 不能在不重复执行用户逻辑的前提下安全复用，因此检测到时停用实时
镜像并标记不可用。密码程序从终端读取的内容不会进入 Shell history，因此不会记录。包含内联
secret 的普通命令仍遵循用户 Shell 自身的 history 策略，手册必须提示该边界。

## 配置兼容和降级
- 禁止编辑任何用户 dotfile。
- 禁止替换用户 prompt、history 文件或 hook 函数。
- 临时启动层必须以用户原配置为输入，并在安装 hook 后保持原变量和函数可用。
- 用户配置加载或 hook 安装失败时，Shell 必须正常启动；Session 标记实时历史不可用。
- 未支持的 Shell 继续提供 Session 列表和 attach，但不承诺实时命令历史。
- 内部 hook 写入失败不得阻塞提示符、改变上一条命令退出码或中断 Shell。

## 安全边界

内部写入入口只接受当前用户、当前 Session runtime 生成的记录，校验 Session ID、记录大小和
目标路径，不接受任意输出路径。记录文件不作为 shell 脚本执行。显示时对控制字符做安全处理，
但保留合法多行命令结构。

## 文档先行

实现前先更新 ADR、Session 模型、进程模型、命令文档、完整用户手册、FAQ、故障排查、已知
限制和测试计划。文档明确交互/非交互分流、最新优先分页、Shell 支持矩阵、隐私边界和降级行为。

## 验证

- 解析测试覆盖 `ls`、`ls <id>`、`--plain`、`--tag` 及冲突参数。
- 交互测试覆盖查看历史、倒序分页、attach 后返回、返回列表和退出。
- bash、zsh、fish 分别验证用户配置和已有 hook 仍执行，命令实时出现且过滤规则有效。
- 覆盖多行命令、空历史、损坏记录、容量轮转、权限和并发读取。
- 非 TTY、SSH 非交互、管道和重定向保持原表格行为。
- hook 失败时 Shell 可用，命令退出码和 prompt 行为不受影响。

## 完成标准

用户可从 `persist ls` 或 `persist ls <id>` 查看最新优先的实时 Shell 命令历史，并在查看后进入
Session、返回其他 Session 或退出；用户配置文件和既有 hook 未被修改或覆盖，三种受支持 Shell
通过兼容性和隐私测试，文档与发布包同步更新。

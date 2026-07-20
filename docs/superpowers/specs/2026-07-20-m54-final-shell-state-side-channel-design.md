# M54 最终 Shell 状态 Side Channel 设计

## 状态

- 日期：2026-07-20
- 状态：已确认
- 里程碑：M54
- ADR：`docs/adr/ADR-0006-final-shell-state-side-channel.md`

## 背景

当前 daemon 在 attach 活跃期间和 Session 关闭时读取 `/proc/<shell-pid>/cwd`。Shell 执行
`cd /path; exit` 后可能在下一次采样前变为 zombie，最终 cwd 因而不可读。M53 Holder 能在
daemon 崩溃期间继续持有 PTY，但现有退出事件只携带 exit code，无法消除该竞态。

## 目标

- 正常 `exit`、空行 Ctrl+D 和快速 `cd; exit` 保存最终 cwd。
- daemon 离线时 Shell 仍能提交状态，重启后可完成 metadata 对账。
- 不解析用户命令，不修改用户配置文件，不覆盖已有 Shell hook。
- 状态写入和读取具有严格 owner、mode、大小、身份和 symlink 边界。
- metadata 成功前不 retire Holder 中的 exited runtime。
- 保留当前 `/proc` 采样和 metadata cwd 作为故障降级。

## 非目标

- 不采集或恢复动态环境变量；该策略属于 M55。
- 不解决 holder 自身崩溃、系统重启或 `SIGKILL` Shell 后的最终 cwd。
- 不支持 Bash 已有 `EXIT` trap 的自动解析、合并或重写。
- 不改变公共 `persist` CLI、公共 SessionExited payload 或用户配置 schema。
- 不实现时间化 replay；该功能属于 M56。

## 总体架构

```text
Shell hook
   | stdin: PWD
   v
persist __state-commit
   | atomic state file
   v
persist-holder
   | SessionExited / GetExitContext
   v
persistd
   | metadata first
   v
SQLite -> Holder retire
```

daemon 创建或冷恢复 runtime 时生成新的 128-bit incarnation，并建立该 runtime 的私有状态
路径。创建请求把路径和 incarnation 交给 holder；Shell 启动环境把相同信息交给内部 helper。
incarnation 只用于同一 Session ID 的 runtime 身份隔离，不作为跨 UID 鉴权秘密。

## 状态目录与 envelope

状态目录为：

```text
/run/user/$UID/persistshell/session-state/
```

目录必须由当前 UID 所有且 mode 为 `0700`。每个 runtime 使用与 Session ID 和 incarnation
绑定的正式文件；文件为普通文件、mode `0600`。正式文件名由 daemon 生成并作为有界绝对路径
传递，helper 和 holder 不根据不可信 cwd 拼接路径。

版本 1 envelope 包含：

```text
version
session_id
incarnation
sequence
cwd
```

envelope 使用 UTF-8 JSON object，字段名固定且拒绝未知字段，最大 8 KiB。`version`、
`session_id` 和 `sequence` 为无符号整数，`incarnation` 为固定 32 位小写十六进制字符串。
`cwd` 必须是绝对路径、有效 UTF-8、无 NUL，最大 4096 bytes。M54 不加入 env 字段；后续
版本如扩展，也必须由协议版本和 M55 策略显式启用。

## 原子提交

内部命令 `persist __state-commit` 不出现在 help、man page 或补全中。它不接收 cwd、状态路径
或 incarnation 命令行参数：

- cwd 从 stdin 读取，最多 4096 bytes。
- Session ID、incarnation、sequence 和目标路径来自受控启动环境。
- helper 无用户可见输出，错误返回非零但 Shell hook 忽略该状态。

helper 先以 `O_DIRECTORY | O_NOFOLLOW` 打开并验证目标目录，后续临时文件创建和替换都通过
该目录 fd 的 `openat`/`renameat` 语义完成。临时文件使用随机名称、`create_new` 和 mode
`0600`，完整写入并 `fsync` 后原子替换正式文件，最后 `fsync` 目录。任何路径越界、父目录
校验失败、symlink、权限错误或超限输入都拒绝提交。失败不能截断上一份有效状态。

## Holder 读取与保留

holder 回收 Shell 时先排空当前 PTY 输出，再读取状态文件。读取使用 `O_NOFOLLOW`，随后通过
`fstat` 验证 owner、mode、普通文件和大小。只有 version、Session ID 和 incarnation 全部匹配，
且 sequence 不小于已接收值时才接受 cwd。

holder 将已验证 cwd 保存在 Session runtime 中，直到 daemon 明确 retire。状态文件无效或不
存在时，holder 仍发布退出结果，不把路径内容写入日志，也不阻止 Shell 回收。

内部 Holder 协议增加：

- `SessionExited` 的可选最终 cwd。
- Inventory entry 的 `exit_context_available`，不批量传输 cwd。
- 有界 `GetExitContext(session_id)` 请求和响应，只允许查询 exited Session。

协议字符串沿用 4096-byte 路径上限。版本不兼容、尾随数据和无效字段按现有握手及帧错误策略
拒绝。公共 client 仍只接收 Session ID 和 exit code。

## 退出与 metadata 顺序

所有自然退出和显式 close 使用相同顺序：

1. Shell hook 原子提交最终 cwd。
2. holder 排空输出、读取状态、记录 exit code 和最终 cwd。
3. holder 发布退出事件，但保留 exited runtime 和状态文件。
4. daemon 通过事件或 `GetExitContext` 取得退出上下文。
5. daemon 先调用 `close_session_with_context` 提交 metadata。
6. metadata 成功后，daemon 才发送 retire 并清理状态文件。

metadata 写入失败时，holder 保留事实供重试。daemon 在步骤 5 后、步骤 6 前崩溃时，重启后
根据 inventory 发现 exited runtime，查询上下文并幂等写入 metadata，再完成 retire。

最终 cwd 优先级为：

1. holder 已验证的 side-channel cwd。
2. daemon 最后一次成功缓存的 `/proc/<shell-pid>/cwd`。
3. metadata 中原有 cwd。

## Bash 集成

用户 `.bashrc` 先加载，PersistShell 再注册临时 hook。prompt 提交沿用现有
`PROMPT_COMMAND` 的数组追加或字符串追加方式。若不存在用户 `EXIT` trap，再安装最终提交
trap；若已经存在，则不解析 `trap -p` 的内容、不 `eval`、不替换，只保留 prompt 提交。

hook 保存主 Bash PID，并拒绝 subshell、命令替换或继承 trap 的进程提交。退出 hook 必须保留
原退出状态。若用户之后替换 hook，PersistShell 不阻止，最终 cwd 降级为最后一次有效提交。

## Zsh 集成

用户配置先加载，再通过 `add-zsh-hook` 追加 `precmd` 和 `zshexit`。PersistShell 不替换同名
用户函数、不改写 `ZDOTDIR` 最终值，也不包装 `cd`、`exit` 或 prompt。hook API 无法安全使用
时，只保留已成功注册的采样点。

## Fish 集成

Fish 初始化后先提交一次 cwd，再注册独立 `fish_postexec` 和 `fish_exit` event handler。
PersistShell 不包装 `fish_prompt`、`cd` 或 `exit`，也不覆盖用户 event handler。

## 共同 Hook 规则

- 私有函数和变量使用稳定的 PersistShell 前缀。
- sequence 在主 Shell 中单调递增，每次 helper 调用只提交一个完整状态。
- helper 同步执行，但只做本地有界文件操作；失败不改变 prompt 或退出流程。
- 用户配置完整性优先于最终 cwd 强保证。
- `SIGKILL`、Shell `exec` 替换、hook 冲突和用户主动删除 hook 使用降级路径。
- 冷恢复创建新的 incarnation，旧状态文件不能覆盖新 runtime。

## 清理与诊断

正常 retire、显式 daemon stop 和安全 GC 只删除当前 Session 已验证的状态文件。daemon 普通
重启不能清理仍由 holder 持有的文件。stale 清理必须同时验证 owner、文件名结构和 holder
inventory，不得递归删除未知文件。

hook 冲突和状态校验失败只记录不含 cwd、token 或环境内容的原因码。M54 不增加公共配置；
现有 `doctor` 可报告通用的最终状态降级，但不能展示状态文件内容。

## 测试策略

单元测试覆盖：

- envelope round-trip、版本、字段、大小、UTF-8 和绝对路径约束。
- incarnation、Session ID 和 sequence 匹配。
- `create_new`、原子替换、权限、owner、普通文件和 symlink 拒绝。
- Holder 退出上下文事件、查询、inventory 标志和协议错误。
- metadata 成功前不 retire，失败后可重试。

真实 Shell 集成测试覆盖：

1. Bash `cd /path; exit` 保存最终 cwd。
2. Bash `cd /path` 后空行 Ctrl+D 保存最终 cwd。
3. Bash 已有 `EXIT` trap 保持原定义和行为，PersistShell 安全降级。
4. Bash subshell 不覆盖主 Shell 状态。
5. Zsh 用户 `precmd`/`zshexit` 与 PersistShell hook 同时执行。
6. Fish 用户 postexec/exit handler 与 PersistShell handler 同时执行。
7. daemon 离线期间 Shell 退出，重启后查询并保存最终 cwd。
8. metadata 写入前后分别注入 daemon 崩溃，重启对账结果一致。
9. 状态缺失、损坏、超限、symlink 和错误 incarnation 时回退有效。

性能测试记录 helper 单次提交和 prompt 路径延迟，避免明显交互回归。最终门禁包括 workspace
fmt、clippy、test、Ubuntu 26.04/RHEL 9 原生构建，以及 Rocky `test` 主机上的退出、daemon
重启、Closed attach 和 cwd 恢复。

## 完成标准

- bash、zsh、fish 默认配置下的正常退出和快速 `cd; exit` 保存最终 cwd。
- 已有用户 hook 不被覆盖；冲突路径有测试和明确降级。
- daemon 离线退出、metadata 失败和崩溃窗口均可幂等恢复。
- 文件和协议全部执行批准的权限、身份、大小和 symlink 校验。
- 公共 CLI 和 M55 环境恢复行为不变。
- 相关 ADR、架构、协议、限制、用户手册、TODO、里程碑和 CHANGELOG 同步。

## 后续边界

M55 可以在新的版本化状态机制上单独设计动态导出环境，但必须重新确认允许变量、删除语义、
敏感值过滤、容量和用户配置兼容策略。M54 不预先采集、存储或恢复任何动态环境内容。

# ADR-0006：使用私有原子状态文件提交最终 Shell cwd

状态：Accepted

日期：2026-07-20

---

## 背景

Closed Session 当前通过 `/proc/<shell-pid>/cwd` 获取恢复目录。Shell 在两次采样之间执行
`cd /path; exit` 时会先变为 zombie，daemon 和 holder 随后无法再从 `/proc` 读取最终 cwd。
M53 已把 PTY 生命周期迁移到 `persist-holder`，但退出事件仍只包含 exit code。

最终状态提交不能解析用户命令、依赖 daemon 始终在线、覆盖用户 Shell hook，或把敏感环境
写入日志。M54 只解决最终 cwd；动态环境采集和恢复策略属于 M55。

## 决策

为每次 Session runtime incarnation 创建私有状态文件。bash、zsh 和 fish 的临时 hook 调用
`persist` 内部 helper，把当前 `PWD` 通过 stdin 提交。helper 写入有界、版本化 JSON
envelope，经私有临时文件和 `fsync` 后原子替换正式文件并同步父目录。

状态文件位于：

```text
/run/user/$UID/persistshell/session-state/
```

holder 在回收 Shell 前读取并校验状态，将最终 cwd 保留到 Session retire。实时
`SessionExited` 事件携带可选 cwd；daemon 离线错过事件时，通过 inventory 状态和
`GetExitContext` 查询恢复。daemon 必须先写 metadata，再 retire holder runtime。

现有 `/proc/<shell-pid>/cwd` 采样继续作为降级路径。

## 原因

- 文件提交不依赖 daemon 在线，符合 M53 的控制面崩溃隔离。
- 原子替换使 holder 不会读取半写状态。
- helper 集中实现权限、symlink、大小和结构校验，Shell 不直接拼 JSON。
- incarnation 防止相同 Session ID 的旧 runtime 状态串用。
- holder 保留退出上下文后，daemon 可在任意提交步骤崩溃并幂等恢复。
- 不增加每 Session 长期线程、进程或常驻连接。

## 被考虑的方案

### 方案 A：私有原子状态文件

实现和故障边界清晰，可跨 daemon 重启保留最终状态。需要处理文件权限、原子写和清理。
选择该方案。

### 方案 B：helper 直连 holder socket

不产生状态文件，但需要新增认证连接角色和时序处理。holder 短暂不可用时提交会直接丢失，
实现复杂度和协议攻击面更大。

### 方案 C：继承专用 fd

提交开销最低，但 fd 会进入 Shell 子进程和插件，难以控制继承、阻塞、关闭和错误恢复，
不符合最小暴露原则。

## 安全约束

- 状态目录 owner 为当前 UID、mode `0700`；状态文件为普通文件、mode `0600`。
- helper 验证目录 fd 后使用随机临时文件、`create_new`、`openat`/`renameat` 和目录
  `fsync`；holder 使用 `O_NOFOLLOW` 和 `fstat` 复核 owner、mode、类型与大小。
- envelope 最大 8 KiB；cwd 最大 4096 bytes，必须是绝对、无 NUL、有效 UTF-8 路径。
- 每次 runtime 使用新的 128-bit incarnation；错误 Session、incarnation、版本或 sequence
  一律丢弃。
- incarnation 不写 metadata、日志或用户可见诊断；cwd 内容不进入内部错误日志。
- 无效状态只触发降级，不能阻止 Shell 退出。

## Shell 兼容策略

- Bash 在无现有 `EXIT` trap 时安装最终提交；已有 trap 时不解析、不替换，只保留 prompt
  提交并明确降级。
- Zsh 使用 `add-zsh-hook` 追加 `precmd` 和 `zshexit`。
- Fish 使用独立 `fish_postexec` 和 `fish_exit` event handler。
- 所有 hook 在加载用户配置后安装，不修改用户配置文件，不包装 `cd`、`exit` 或 prompt。
- hook 注册冲突、`SIGKILL`、Shell `exec` 替换和用户主动删除 hook 时允许回退。

用户配置完整性优先于强行捕获最终 cwd。

## 影响

### 正面影响

- 正常 `exit`、空行 Ctrl+D 和快速 `cd; exit` 可以保存最终 cwd。
- daemon 离线期间退出的 Session 可在重启后正确关闭和恢复。
- metadata 提交与 holder retire 顺序变为可重试、幂等流程。

### 负面影响

- 每个 prompt 和退出边界会执行一次短生命周期本地 helper。
- holder 协议增加退出上下文查询和可选 cwd 字段。
- 运行目录增加每 runtime 一个有界状态文件。

### 风险

- 用户 Shell hook 冲突可能导致最终状态降级到上一次 prompt。
- helper 延迟可能增加 prompt 开销，必须通过基准约束。
- metadata 成功但 retire 失败会暂时保留 exited runtime，需要重启对账清理。
- 非 UTF-8 cwd 仍只能使用现有回退值。

## 回滚方案

停止注入状态 hook，并让 daemon 忽略新增可选退出上下文，即可恢复 `/proc` 尽力采样。
协议 major 不兼容时仍按现有握手拒绝错误组合。回滚前应 retire 已退出 runtime 和清理经过
身份验证的状态文件；不得批量删除运行目录中的未知文件。

## 后续任务

- [x] 编写并确认 M54 实施计划。
- [x] 实现状态 envelope、原子 helper 和安全读取。
- [x] 扩展 holder 退出上下文协议与离线对账。
- [x] 实现 bash、zsh、fish 兼容 hook。
- [x] 完成单元、进程集成、性能、平台构建和远程验证。
- [ ] M55 单独设计动态环境变量采集和恢复策略。

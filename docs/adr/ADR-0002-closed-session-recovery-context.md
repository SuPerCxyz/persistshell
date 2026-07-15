# ADR-0002: 已关闭会话的恢复上下文

状态：Accepted

日期：2026-07-15

---

## 背景

PersistShell 在用户执行 `exit` 或 `Ctrl+D` 后必须释放 PTY 与 Shell
runtime，避免关闭会话继续消耗系统资源。同时，用户再次进入同一 Session 时应
恢复此前的工作目录、可安全恢复的终端环境和输出记录，并获得可写操作权。

现有 metadata 仅能保留 closed 状态、输出日志、cwd 和 shell，无法保存运行时
环境，也无法把 closed metadata 重新转换为运行中的 PTY Session。

---

## 决策

在 SQLite `sessions` 表中增加 `env_snapshot` JSON 文本列。会话关闭前从
Shell 进程的 `/proc/<pid>/cwd` 与 `/proc/<pid>/environ` 尽力采集恢复上下文。
再次 attach closed Session 时，创建新的 PTY Shell，使用保存的 cwd 和环境快照，
并把同一 metadata 记录恢复为 `running`。

环境白名单仅包括 `TERM`、`COLORTERM`、`LANG` 与 `LC_*`。不保存 `PATH`、
`HOME`、`USER`、`SHELL`、SSH/显示相关变量，以及变量名包含 `TOKEN`、
`SECRET`、`PASSWORD`、`KEY` 的值。

---

## 原因

- Shell runtime 已退出，不能真正续跑原进程；显式新建 runtime 才符合资源释放目标。
- SQLite migration 保持旧用户 metadata 可升级且可查询。
- 严格白名单能保留终端语言和颜色行为，同时降低凭据与机器相关路径泄露风险。
- 使用原 Session ID 可保留 note、tag、pin、lock、历史日志和会话语义。

---

## 被考虑的方案

### 方案 A：保存完整环境

恢复效果最接近原进程，但可能保存认证令牌、代理配置和路径相关状态，安全边界不
可接受。

### 方案 B：只保存 cwd

实现简单，但无法满足用户要求的环境变量恢复。

### 方案 C：白名单环境快照

保存交互终端最需要的变量，并排除敏感与宿主相关变量。选择此方案。

---

## 被拒绝的方案

- 保留已退出的 PTY：与用户退出后释放 runtime 的要求冲突。
- 通过 shell history 重放命令：会产生副作用，不能保证恢复语义。
- 在 metadata 中保存加密凭据：不属于 PersistShell 的职责，且增加密钥管理风险。

---

## 影响

### 正面影响

- closed Session 可重新 attach 并继续可写操作。
- 退出的 Shell 不再在后台占用 PTY/runtime。
- 恢复行为与 metadata 和输出保留策略一致。

### 负面影响

- 恢复后的 shell 是新进程，无法保留内存变量、前台作业或未提交的 shell 内部状态。
- `/proc` 不可读或进程已退出时，只能使用已有 cwd 或 daemon 当前目录回退。

### 风险

- cwd 可能在进程退出前已不可读；恢复必须容忍缺失快照。
- 新 schema 需要 migration 测试，避免升级破坏已有 metadata。

---

## 回滚方案

恢复逻辑可停止读取 `env_snapshot`，旧列保留但不使用。数据库中的追加列不影响旧
版本读取已有字段。

---

## 后续任务

- [x] 更新相关文档
- [ ] 更新代码
- [ ] 更新测试
- [ ] 更新 CHANGELOG

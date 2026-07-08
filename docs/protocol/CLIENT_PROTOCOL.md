# PersistShell Client Protocol

本文档定义 PersistShell CLI 与用户交互的行为规范。

---

## 命令入口

统一命令：

```bash
persist
```

---

## 默认行为

当 `persist` 在交互式 SSH hook 中被自动执行时：

```bash
persist
```

应默认：

1. 连接或启动 daemon。
2. 创建新 Session。
3. Attach 到该新 Session。

---

## 子命令

Phase 1 需要支持：

```bash
persist version
persist new
persist ls
persist attach <id>
persist detach
persist kill <id>
persist rename <id> <name>
persist log <id>
persist tail <id>
persist daemon start
persist daemon stop
persist daemon status
persist doctor
persist install
persist uninstall
persist config
```

---

## persist new

创建新 Session。

```bash
persist new
persist new --name debug
```

行为：

- 创建 Session
- 默认 attach
- 如果指定 `--no-attach`，只创建不进入

Phase 1 可以先默认 attach。

---

## persist ls

列出 Sessions。

示例输出：

```text
ID   NAME              STATUS     AGE    LAST     CWD              CMD
1    ssh-153011        running    10m    1m       /root            bash
2    make-build        detached   2h     5m       /usr/src/linux   make -j64
3    fio-test          closed     1d     1d       /data            exit=0
```

要求：

- 输出简洁
- 默认隐藏 archived
- 状态清晰
- ID 易复制

后续支持：

```bash
persist ls --all
persist ls --json
persist ls --tag prod
```

---

## persist attach

Attach 到旧 Session。

```bash
persist attach 2
```

如果 Session busy：

```text
错误：Session 2 当前已有 active writer。
建议：使用 --takeover 接管写入权，或使用 --read-only 仅查看。
```

跨电脑进入已有会话不能只能是只读。默认应支持可写 attach 或可写接管，并通过单 active writer 策略避免输入冲突。

---

## persist detach

从当前 Session detach。

在 attach 模式中，detach 应：

- 断开 client
- 保持 Session alive
- 恢复本地终端
- 返回普通 shell 或退出

具体体验后续实现时确定。

---

## persist kill

终止 Session。

```bash
persist kill 2
persist kill 2 --force
```

要求：

- 必须明确这是终止 Session。
- 对 running session 可以提示确认。
- 脚本模式可用 `--yes`。

---

## persist rename

```bash
persist rename 2 make-build
```

要求：

- 名称可读
- 禁止控制字符
- 长度有限制

---

## persist log

查看 Session 日志。

```bash
persist log 2
```

Phase 1 可以：

- 打印日志路径
- 或直接输出日志内容

---

## persist tail

查看日志尾部。

```bash
persist tail 2
persist tail 2 -n 200
```

---

## persist doctor

诊断当前环境。

检查：

- daemon 状态
- socket 权限
- runtime dir 权限
- metadata 权限
- log 权限
- profile hook
- config
- stale socket
- zombie session

输出应包含修复建议。

---

## persist install

安装 SSH 自动接管。

行为：

1. 检测用户 shell。
2. 备份 profile 文件。
3. 注入 hook。
4. 创建配置目录。
5. 创建数据目录。
6. 验证安装。

要求：

- 幂等
- 可重复执行
- 不重复注入
- 可回滚

---

## persist uninstall

卸载 SSH 自动接管。

默认：

- 移除 profile hook
- 保留 metadata 和 logs

如果用户指定：

```bash
persist uninstall --purge
```

才删除数据。

---

## persist daemon

Daemon 管理命令：

```bash
persist daemon start
persist daemon stop
persist daemon status
```

如果存在 running Session，`stop` 默认拒绝。

---

## persist config

配置查看和修改。

Phase 1 可以只支持：

```bash
persist config show
```

后续支持 set/get。

---

## 自动 SSH Hook 行为

Hook 中执行 persist 时必须检测：

- 是否 SSH
- 是否交互式
- 是否有 bypass
- 是否已有 PersistShell 环境
- 是否非交互命令

只有满足条件才接管。

---

## Bypass

必须支持绕过：

```bash
PERSIST_DISABLE=1 ssh node
```

也可以兼容：

```bash
SH_DISABLE=1 ssh node
```

---

## 输出风格

CLI 输出要求：

- 错误清楚
- 表格整齐
- 不输出无意义调试信息
- 默认人类可读
- 后续支持 JSON

---

## Exit Code

命令应返回合理 exit code。

建议：

```text
0 success
1 general error
2 invalid usage
3 daemon error
4 permission denied
5 session not found
6 session busy
```

---

## 不变量

1. `persist` 默认不应破坏非交互 SSH。
2. `persist ls` 不应需要 attach。
3. `persist kill` 必须显式。
4. `persist uninstall` 必须可回滚。
5. 用户必须能 bypass。

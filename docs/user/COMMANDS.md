# PersistShell Commands

本文档描述 PersistShell CLI 命令。

---

## 命令总览

```bash
persist
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
persist config show
```

---

## persist

无参数执行时：

- 在 SSH hook 场景下：创建新 Session 并 attach。
- 在普通命令行中：可以显示帮助或创建新 Session，具体行为由实现决定。

Phase 1 推荐：

```bash
persist
```

等价于：

```bash
persist new
```

---

## persist version

显示版本。

```bash
persist version
```

输出：

```text
PersistShell v0.1.0
protocol: 1
```

---

## persist new

创建新 Session。

```bash
persist new
persist new --name debug
```

---

## persist ls

列出 Sessions。

```bash
persist ls
```

示例：

```text
ID   NAME              STATUS     AGE    LAST     CWD              CMD
1    ssh-153011        running    10m    1m       /root            bash
2    make-build        detached   2h     5m       /usr/src/linux   make -j64
```

---

## persist attach

Attach 到已有 Session。

```bash
persist attach 2
```

---

## persist detach

Detach 当前 Session。

```bash
persist detach
```

Phase 1 可以通过快捷键或命令实现，具体实现需在后续设计中确定。

---

## persist kill

终止 Session。

```bash
persist kill 2
persist kill 2 --force
```

---

## persist rename

重命名 Session。

```bash
persist rename 2 make-build
```

---

## persist log

查看 Session 日志。

```bash
persist log 2
```

---

## persist tail

查看 Session 日志尾部。

```bash
persist tail 2
persist tail 2 -n 200
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

如果仍有 running session，默认应拒绝停止。

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

M03 起会显示配置有效状态和 client 内部日志路径。

后续可支持：

```bash
persist doctor --fix
```

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

## JSON 输出

后续可支持：

```bash
persist ls --json
```

Phase 1 不强制。

---

## 退出码

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

## 命令设计原则

1. 常用命令要短。
2. 危险命令要明确。
3. 错误提示要有修复建议。
4. 输出默认面向人类。
5. 后续支持 JSON 面向脚本。

# PersistShell Install Guide

本文档描述 PersistShell 的安装、卸载和绕过方式。

---

## 安装目标

安装 PersistShell 后，用户执行：

```bash
ssh node
```

应自动进入一个新的 PersistShell Session。

---

## 安装方式

```bash
persist install
```

当前 `persist install` 会识别 bash 或 zsh profile，在其中追加带标记的 SSH hook。重复安装会
拒绝重复插入。目录、socket、metadata 和日志会在 daemon 或相关命令首次运行时创建。

---

## 目录

配置目录：

```text
~/.config/persistshell/
```

数据目录：

```text
~/.local/share/persistshell/
```

状态目录：

```text
~/.local/state/persistshell/
```

运行目录：

```text
/run/user/$UID/persistshell/
```

---

## Shell Hook

安装器需要根据用户 shell 注入 hook。

当前 hook 安装器支持 bash 和 zsh。fish 有 completion，但 `persist install` 尚不管理 fish
profile；可手动按当前 hook 逻辑配置。

---

## Bash Hook 示例

示例：

```bash
if [ -n "$SSH_TTY" ] && [ -z "${PERSIST_DISABLE:-}" ] && command -v persist >/dev/null 2>&1; then
  persist daemon start >/dev/null 2>&1 || true
  persist attach 2>/dev/null
fi
```

实际 hook 应由 install 命令生成和管理。

---

## Shell Completion

Shell completion 与 SSH hook 独立。`persist install` 不安装 completion；发行版包安装后，
bash、zsh 和 fish 会从各自的标准目录发现补全文件。

从 tarball 或源码目录使用时，可按 shell 手动启用：

```bash
# bash
source completions/persist.bash
```

```bash
# zsh
fpath=("$PWD/completions" $fpath)
autoload -Uz compinit && compinit
```

```fish
# fish
source completions/persist.fish
```

Session ID 候选只读取 `persist ls`。daemon 未运行、命令不可用或列表失败时，补全静默返回
空候选，不会启动 daemon 或创建 Session。

---

## 幂等安装

重复执行：

```bash
persist install
```

不得重复插入 hook。

必须识别已有 hook。

---

## 备份

当前安装器不创建 profile 备份。首次执行前应由用户自行备份对应的 `.bashrc`、`.bash_profile`
或 `.zshrc`；这是当前已知限制。

---

## 卸载

执行：

```bash
persist uninstall
```

应移除 hook。

默认保留：

- metadata
- logs
- config

完全清理：

```bash
persist uninstall --purge
```

---

## 绕过 PersistShell

临时绕过：

```bash
PERSIST_DISABLE=1 ssh node
```

也可以执行非交互命令：

```bash
ssh node 'bash --noprofile --norc'
```

---

## 验证安装

执行：

```bash
persist doctor
```

检查：

- hook 是否存在
- daemon 是否可启动
- socket 权限是否正确
- 数据目录是否正确
- 配置是否有效

---

## Daemon 管理

当前不提供 systemd user unit。SSH hook 会尝试启动 daemon；手动使用时先执行：

```bash
persist daemon start
persist daemon status
```

普通 `persist new`、`persist ls` 或 `persist attach <id>` 不会代替用户启动 daemon。

---

## 注意事项

安装 SSH 自动接管工具有风险。

PersistShell 必须始终保留绕过方式。

如果安装后 SSH 登录异常，使用：

```bash
PERSIST_DISABLE=1 ssh node
```

然后执行：

```bash
persist uninstall
persist doctor
```

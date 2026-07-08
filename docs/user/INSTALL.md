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

## Phase 1 安装方式

Phase 1 推荐提供：

```bash
persist install
```

安装内容：

- 创建配置目录
- 创建数据目录
- 创建状态目录
- 创建 runtime 目录
- 注入 shell profile hook
- 检查权限
- 验证 daemon 可启动

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

支持：

- bash
- zsh
- fish

Phase 1 至少支持 bash。

---

## Bash Hook 示例

示例：

```bash
# PersistShell hook
if command -v persist >/dev/null 2>&1; then
  if [ -n "$SSH_CONNECTION" ] &&
     [ -t 0 ] &&
     [ -t 1 ] &&
     [ -z "$SSH_ORIGINAL_COMMAND" ] &&
     [ -z "$PERSIST_DISABLE" ] &&
     [ -z "$SH_DISABLE" ] &&
     [ -z "$PERSIST_ACTIVE" ]; then
    export PERSIST_ACTIVE=1
    exec persist
  fi
fi
```

实际 hook 应由 install 命令生成和管理。

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

安装前必须备份被修改的 profile 文件。

例如：

```text
~/.bashrc.persistshell.bak
```

或者记录 patch block。

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

或：

```bash
SH_DISABLE=1 ssh node
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

## systemd user service

后续版本可以支持 systemd user service。

Phase 1 可以按需启动 daemon。

注意：

没有 linger 时，systemd user service 可能在用户完全退出后停止。

---

## 非 systemd 环境

PersistShell 必须支持非 systemd 环境。

Fallback：

- client auto-spawn daemon
- lock file
- runtime dir fallback

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

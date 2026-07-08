# PersistShell Security Policy

PersistShell 涉及 Shell、PTY、日志、Socket 和用户会话，安全非常重要。

---

## 安全原则

- per-user daemon
- 最小权限
- 默认不记录用户输入
- socket 权限安全
- 日志权限安全
- metadata 权限安全
- 支持绕过和卸载

---

## 权限要求

```text
runtime dir: 0700
socket:      0600
data dir:    0700
metadata:    0600
logs:        0600
```

---

## 不记录用户输入

默认不记录用户输入。

原因：

- sudo 密码
- token
- secret
- 私钥
- 命令中的敏感参数

---

## Session 输出日志风险

即使不记录输入，程序输出仍可能包含敏感信息。

用户可以关闭 Session 日志。

---

## Socket 安全

Daemon 必须校验 peer credentials。

同一用户只能访问自己的 daemon 和 session。

---

## /tmp fallback

默认不应使用 `/tmp` 存 socket。

如必须 fallback，必须防止：

- symlink attack
- 权限错误
- socket hijack
- stale socket

---

## 安全漏洞报告

如果发现安全问题，请不要公开 issue。

请通过项目维护者指定的安全渠道报告。

当前项目初始化阶段，安全报告流程后续完善。

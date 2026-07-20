# PersistShell Next Task

本文件永远只记录下一步唯一任务。

任何新的开发会话开始时，必须首先阅读本文件。

不得在未完成当前任务前开始其它任务。

---

## 当前阶段

Phase 4：发布和长期维护

---

## 当前里程碑

M56：通用 Linux 多架构发布包。

---

## 当前唯一任务

实现 M56：glibc 2.28 基线的 x86_64/ARM64 通用 Linux 发布包。

### 前置已完成

- M55 Closed Session 动态环境恢复已完成。
- M56 设计和 ADR 已由维护者确认。
- 当前 x86_64 包分别绑定 Ubuntu 26.04 和 RHEL 9，尚无 ARM64 产物。

---

## 任务范围

- 以 glibc 2.28 为统一 ABI，按 x86_64/aarch64 各构建一次。
- 生成不绑定发行版名称的 RPM、DEB、tar.xz 和 SHA-256。
- 增加 release 体积优化及 RPM/DEB 3 MiB、tar.xz 3.5 MiB 门禁。
- 为不支持 `pidfd_open` 的内核实现有界、安全的进程身份 fallback。
- 扩展 GitHub Actions 原生双架构构建与多发行版安装/运行验证。
- 更新安装、CI、兼容范围、限制和发布文档。

---

## 完成标准

1. 六类主产物名称、架构 metadata、checksum 和内容验证通过。
2. 所有 ELF 的最高 GLIBC symbol 不超过 2.28。
3. 两种架构通过构建，代表性 RPM/DEB 发行版完成安装和运行 smoke。
4. pidfd 正常路径和强制 fallback 路径测试通过。
5. workspace fmt、clippy、test 与体积门禁通过。
6. M56 审计和项目状态文档更新完成。

---

## 禁止事项

不实现 M57 时间化日志、`--speed` 或 `--follow`，不修改 metadata schema，
不增加 i686、ARMv7、musl 或未经验证的发行版认证。

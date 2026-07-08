# PersistShell Roadmap

本文件描述 PersistShell 的版本路线图。

路线图不是固定承诺，而是项目阶段规划。

任何路线调整都必须同步更新 MILESTONES.md、TODO.md 和 NEXT_TASK.md。

---

## v0.1：MVP Core

目标：

实现最小可用的持久 Shell 运行时。

v0.1 必须证明核心假设成立：

```text
SSH 断开后，Shell 和任务继续运行。
重新 SSH 登录后，可以 attach 到旧 Session。
用户执行 exit/Ctrl-D 后释放 shell runtime，但仍可恢复该 Session 的输出、cwd 和环境快照。
另一台电脑可以 attach 到已有 Session 并获取可写操作权。
```

范围：

- 项目初始化
- PTY Engine
- Daemon
- Client
- Session Manager
- Ring Buffer
- 基础日志
- Metadata Store
- Unix Socket IPC
- attach/detach/list/new/kill/rename
- Closed Session 恢复
- 多电脑可写 attach / writer takeover
- Signal 转发
- Window Resize
- SSH 交互式自动接管
- 非交互式 SSH 兼容
- install/uninstall/doctor/bypass
- 基础测试
- 压力测试雏形

不包含：

- Web UI
- REST API
- Pane
- Window
- Replay 高级模式
- 插件系统

---

## v0.2：Usability

目标：

提升用户查找、理解和管理 Session 的体验。

范围：

- 自动 Session 命名
- Session notes
- Session tags
- Pin session
- Idle detection
- Better `persist ls`
- 日志搜索
- 日志导出
- 更完善的配置文件
- 独立 history 文件
- 更完善的 GC 策略
- 更好的错误提示
- 更完善的 doctor

---

## v0.3：Recovery & Observation

目标：

增强恢复体验和可观测性。

范围：

- Replay mode
- Read-only attach
- 多 active writer 协作
- Session lock
- Resource monitor
- Foreground process tracking
- Process tree view
- Session snapshot
- SSH Agent synchronization
- 环境变量同步策略
- Metrics 初步支持

---

## v0.4：Hardening

目标：

提高稳定性、安全性、兼容性和性能。

范围：

- 大规模压力测试
- 100/500/1000 Session Benchmark
- 输出风暴防护
- 慢客户端处理
- 日志脱敏
- 加密日志可选
- Metadata migration
- 协议版本升级
- 崩溃恢复策略
- systemd user service 集成
- 非 systemd fallback
- GitHub Actions CI
- GitHub Actions package build
- GitHub Release artifacts

---

## v0.5：Beta

目标：

进入较稳定的 Beta 版本，适合高级用户试用。

范围：

- 完整文档
- 安装脚本
- 包管理初步支持
- GitHub Actions 构建发布包
- Debian/RPM 打包
- Man page
- Shell completion
- FAQ
- 故障排查文档
- 兼容性矩阵
- 安全审查
- 性能回归测试

---

## v1.0：Production Ready

目标：

达到生产可用标准。

要求：

- 核心功能稳定
- 断线恢复可靠
- 日志和资源控制可靠
- 安装/卸载可靠
- 非交互式 SSH 不受影响
- 主流 Linux 发行版兼容
- 主流 Shell 兼容
- 主流终端兼容
- Benchmark 达标
- 文档完整
- 安全边界清晰
- 已知限制清晰

---

## v1.x：Advanced

v1.0 之后可以考虑：

- Web 只读面板
- REST API
- 更强 Replay
- Full-text index
- Timeline
- Plugin system
- Cluster mode
- Central management
- Enterprise audit mode

这些功能必须保持可选，不得污染核心路径。

---

## 长期原则

任何版本都不得引入：

- Pane
- Window
- Prefix Key
- Terminal Emulator
- SSH Server
- 复杂 UI
- 与持久 Shell 无关的功能

PersistShell 始终保持小而专注。

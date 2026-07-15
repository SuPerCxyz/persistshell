# M06 Runtime 恢复设计

## 目标

在审计后续里程碑前，恢复文档中定义的 M06 daemon runtime。`persistd
foreground` 必须持有用户级 PID 文件和 Unix socket，接受 IPC 连接，将每个连接
交给既有的 `handle_client`，并在关闭时可靠清理资源。本工作只让既有 M05-M36
处理逻辑能够由安装后的二进制实际执行，不改变其协议语义。

## 范围

- 加载有效配置，使用其 runtime、data、state 与 socket 路径，不再使用 `/tmp`。
- 获取 `daemon.pid`，绑定 `persist.sock`，注册关闭信号，并持续接受客户端连接。
- 对每个连接在短生命周期线程中调用现有 `handle_client`，共享
  `SessionManager` 与 `MetadataStore`。
- 收到关闭信号后停止 accept loop 和 GC worker；通过释放既有所有权类型删除
  PID 文件和 socket。
- 增加真实进程级测试：foreground 启动、HELLO、重复 daemon 阻止、SIGTERM
  关闭与资源清理。

## 非目标

- 不重新设计 IPC，不修改 writer 策略，不实现进程树，也不集成系统级 service
  manager。
- 不恢复 daemon 崩溃后的存活 PTY；该项仍是已记录限制。

## 运行模型

1. `persist daemon start` 按既有方式启动 `persistd foreground`。
2. daemon 校验配置、创建需要的目录、打开 SQLite metadata，并获取 PID 锁。
3. `DaemonSocket::bind` 在配置的用户级路径创建 socket。accept loop 使用有限的
   socket 超时，以便及时观察 SIGTERM，且不使用 sleep polling。
4. 每个已接受连接运行在短生命周期线程。共享 manager 与 metadata store 继续使用
   既有 mutex 保护。
5. SIGTERM 让 accept loop 与 GC worker 退出；socket 和 pidfile 被 drop 后删除其
   文件系统产物。

## 错误与兼容性

- 已持有的 PID 锁返回 `DaemonAlreadyRunning`。
- socket 绑定失败继续遵守现有的安全 socket 检查。
- metadata 打开或配置校验失败时，daemon 不会暴露 socket。
- `PERSIST_DISABLE`、非交互 SSH 绕过和 M35 active-writer 协议保持不变。

## 验证

- 单元测试覆盖 lifecycle helper 和关闭状态重置。
- 集成测试启动真实 `persistd`，等待配置的 socket，完成 HELLO 和 Session 请求，
  发送 SIGTERM 后断言资源清理。
- 最终在本地和 `test` 上执行 fmt、Clippy（warnings denied）、workspace tests 与
  release build；通过后才安装二进制。

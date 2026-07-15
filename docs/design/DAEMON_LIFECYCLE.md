# Daemon 基础生命周期 — 设计文档

## 概述

本文档描述 PersistShell Daemon 的基础生命周期管理（M06 里程碑），包括 daemon 进程的后台启动、停止、状态查询和信号处理。

## 设计原则

1. Daemon 本身以**前台进程**模式运行，不执行传统 double-fork daemonization。
2. `persist daemon start` 通过子进程方式启动 daemon，CLI 负责后台化。
3. PID 文件 + `flock` 实现单实例锁。
4. 复用 M05 的 `DaemonSocket` 进行 socket 监听。

## 文件结构

```
crates/persistd/src/
├── main.rs           # 不变
├── server.rs         # 扩展：添加 foreground 子命令实现
└── lifecycle.rs      # 新增：daemon 进程管理（PID 文件、信号处理、accept 循环）

crates/persist-cli/src/
├── main.rs           # 不变
├── cli.rs            # 修改：daemon start/stop/status 从 not_implemented 改为真实实现
├── command.rs        # 不变
└── daemon.rs         # 新增：persist daemon 命令的具体实现
```

## 组件设计

### 1. persistd foreground

server.rs 新增 `foreground` 处理：

1. 加载配置（`ConfigLoadOptions::from_environment()`）
2. 初始化内部日志
3. 创建 PID 文件并加锁（`libc::flock(LOCK_EX | LOCK_NB)`）
4. `DaemonSocket::bind(config.paths.socket_path)`
5. 注册 SIGTERM 信号处理
6. 进入 accept 循环（当前仅日志记录，未来连接 session manager）
7. 收到 SIGTERM 后：退出 accept 循环 → 清理 socket → 释放 PID 锁并删除文件 → exit(0)

### 2. PID 文件管理

路径：`$runtime_dir/daemon.pid`（与 socket 同目录）

```
struct PidFile {
    path: PathBuf,
    file: File,
}

impl PidFile {
    fn create(path, expected_uid) -> Result<Self>;
    // - 创建或打开文件
    // - flock(LOCK_EX | LOCK_NB)
    // - 如果文件已存在且锁失败 → DaemonAlreadyRunning
    // - 如果锁成功但文件非空 → 检查 /proc/$PID，如果进程不存在则为 stale，允许覆盖
    // - 写入当前 PID（覆盖已存在的 stale PID）
    // - ftruncate + write
    fn read_pid(path) -> Result<Option<u32>>;
    // - 读文件内容，解析十进制 PID
    fn is_process_alive(pid: u32) -> bool;
    // - 检查 /proc/$pid 是否存在
    fn release(self);
    // - 关闭文件（自动释放锁）
    fn remove(&self);
    // - 删除 PID 文件
}
```

### 3. 信号处理

- 使用 `signal_hook` 或简单的 `libc::sigaction` 注册信号处理。
- SIGTERM → 设置 atomic flag → accept 循环检查 flag 退出。
- SIGINT/SIGHUP/SIGQUIT → 忽略（daemon 不被终端信号影响）。

### 4. persist daemon start（CLI）

```
fn daemon_start(config, options) -> Result<()>:
    1. 检查 socket 是否存在
       - 若存在且可连接 → DaemonAlreadyRunning
    2. 检查 PID 文件
       - 若存在且 pid 存活 → DaemonAlreadyRunning
    3. 清理 stale socket（如有）
    4. 启动 persistd 子进程：
       Command::new("persistd")
           .arg("foreground")
           .stdout(std::fs::File::create(&log_path)?)  // daemon stdout → log
           .stderr(std::fs::File::create(&log_path)?)  // daemon stderr → log
           .stdin(std::process::Stdio::null())
           .spawn()
    5. 记录子进程 PID（与 persistd 自身的 PID file 是同一个）
    6. 轮询 socket 文件出现（最多 2s，每 100ms 一次）
    7. 若超时 → 读取子进程 stderr → 报告启动失败
    8. 若成功 → 打印 "daemon started (pid N)"
```

### 5. persist daemon stop（CLI）

```
fn daemon_stop(config) -> Result<()>:
    1. 读取 PID 文件
    2. 若不存在 → DaemonNotRunning
    3. 验证进程存在
       - 若不存在 → 清理 stale PID 文件和 socket → 报告 "daemon was not running"
    4. 发送 SIGTERM
    5. 轮询 PID 文件删除（最多 5s，每 200ms 一次）
    6. 若超时 → 发送 SIGKILL → 报告 "daemon forcefully killed"
    7. 清理 socket（保险）
    8. 打印 "daemon stopped"
```

### 6. persist daemon status（CLI）

```
fn daemon_status(config) -> Result<()>:
    1. 读取 PID 文件
    2. 若无 PID 文件 → "daemon: stopped"
    3. 验证进程存活
       - 若不存在 → "daemon: stopped (stale pidfile)" + 清理提示
    4. 检查 socket 存在
    5. 读取 /proc/$PID/stat 计算运行时间
    6. 输出：
       daemon: running
       pid: N
       uptime: Xm Ys
       socket: /run/user/UID/persistshell/persist.sock
       socket_status: listening
```

## 依赖

在 `persistd` 的 `Cargo.toml` 中新增 `libc = "0.2"` 直接依赖（用于信号处理和 PID 文件 flock）。

在 `persist-cli` 的 `Cargo.toml` 中新增 `libc = "0.2"` 依赖（用于 `kill(2)` 和进程检查）。

信号处理使用 `libc::sigaction` + `std::sync::atomic::AtomicBool`，不引入 `signal-hook` 依赖。

## Daemon 二进制路径

`persist daemon start` 通过 `/proc/self/exe` 解析同目录下的 `persistd` 二进制路径：

```
let daemon_path = std::env::current_exe()?.parent()?.join("persistd");
```

测试时通过 `env!("CARGO_BIN_EXE_persistd")` 获取集成测试中的二进制路径。

## 错误处理

| 场景 | 错误码 | Exit Code |
|------|--------|-----------|
| daemon start 时 PID 文件已锁定 | DaemonAlreadyRunning | 2 |
| daemon start 超时 | Internal | 5 |
| daemon stop 时未运行 | DaemonNotRunning | 2 |
| daemon status 时 PID 文件无效 | DaemonNotRunning | 2 |
| PID 文件 I/O 错误 | Io | 3 |

## 测试

### 单元测试

- `PidFile::create` 成功创建并锁定
- 第二次 `PidFile::create` 返回 `DaemonAlreadyRunning`
- `PidFile` 释放后可以再次锁定
- Stale PID 文件检测
- `daemon_status` 在不同状态下的输出

### 集成测试（tests/persistd.rs）

- `persist daemon start` + `persist daemon status` + `persist daemon stop` 全流程
- 重复 `persist daemon start` → 错误
- `persist daemon stop` 未运行 → 错误
- `persist daemon status` 未运行 → 正确输出
- SIGTERM 后 daemon 优雅退出

## 未包含（延后）

- Daemon 空闲自动退出（idle_exit 功能在 M06 之后）
- Daemon 崩溃自动重启
- 日志轮转
- Daemon 监控和 metrics

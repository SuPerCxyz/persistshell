# PTY Engine MVP — 设计文档

## 概述

本文档描述 PersistShell PTY Engine 的最小可用实现（M07 里程碑），包括伪终端创建、Shell 进程 fork/exec、PTY I/O 和生命周期管理。

## 文件结构

```
crates/persist-pty/src/
├── lib.rs        # 重写：PtyEngine + PtySession 公开 API
├── platform.rs   # 不变
├── process.rs    # 不变
├── termios.rs    # 不变
├── signal.rs     # 不变
└── pty.rs        # 新增：底层 PTY 操作（posix_openpt, grantpt, unlockpt, ptsname）
```

## PtyEngine API

```rust
pub struct PtyEngine;

impl PtyEngine {
    pub fn new() -> Self;
    pub fn open_session(&self) -> Result<PtySession>;
}

pub struct PtySession {
    master_fd: RawFd,
    child_pid: pid_t,
    shell: String,
    exit_status: Option<i32>,
}

impl PtySession {
    pub fn read_output(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    pub fn write_input(&mut self, buf: &[u8]) -> io::Result<usize>;
    pub fn child_pid(&self) -> u32;
    pub fn is_alive(&self) -> bool;
    pub fn poll_output(&self, timeout: Duration) -> io::Result<bool>;
    pub fn wait_exit(&mut self) -> Result<i32>;
}

impl Drop for PtySession;
```

## PTY 创建流程

1. `posix_openpt(O_RDWR | O_NOCTTY)` → master_fd
2. `grantpt(master_fd)` — 修改 slave 权限
3. `unlockpt(master_fd)` — 允许 slave 打开
4. `ptsname_r(master_fd)` → slave 设备路径
5. fork()
   - 子进程：`setsid()` → `open(slave_path, O_RDWR)` → `ioctl(TIOCSCTTY)` → `dup2(slave_fd, 0/1/2)` → `close(master_fd)` → `execvp(shell, args)`
   - 父进程：`close(slave_fd)` → 设置 master 为非阻塞 → 返回 PtySession

## Shell 检测

1. `libc::getpwuid_r(libc::getuid())` → `pw_shell`
2. fallback: 环境变量 `SHELL`
3. fallback: `/bin/sh`

## 生命周期

- `PtySession::drop()`:
  1. 若子进程存活 → `kill(child_pid, SIGHUP)`
  2. 轮询 `waitpid(WNOHANG)`，最多 3s
  3. 若仍存活 → `SIGKILL` + `waitpid`
  4. `close(master_fd)`

## 错误处理

| 场景 | 错误码 | Exit Code |
|------|--------|-----------|
| posix_openpt 失败 | PtyOpenFailed | 3 |
| grantpt/unlockpt 失败 | IoctlFailed | 3 |
| fork 失败 | ForkFailed | 3 |
| child setsid/dup2/exec 失败 | ExecFailed | 3 |
| waitpid 异常 | Io | 3 |

## 测试

- `pty_shell_echo` — 创建 PTY → 写 "echo hello\n" → 读输出包含 "hello"
- `pty_exit_code` — "exit 42" → wait_exit → 42
- `pty_drop_cleans_up` — drop 后子进程退出
- `pty_shell_detection` — 检测到的 shell 路径存在
- `pty_write_and_read_large_output` — 写长命令读回完整输出

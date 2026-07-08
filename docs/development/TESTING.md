# PersistShell Testing Guide

本文档定义 PersistShell 测试策略。

PersistShell 涉及 PTY、信号、进程、IPC、文件权限和 SSH 接管，必须高度重视测试。

---

## 测试分层

测试分为：

1. 单元测试
2. 集成测试
3. E2E 测试
4. 兼容性测试
5. 压力测试
6. 性能测试
7. 安全测试
8. 回归测试

---

## 单元测试

适合测试：

- config parser
- error type
- ring buffer
- metadata schema
- CLI parser
- session state machine
- protocol frame encode/decode
- path resolver
- permission checker

---

## 集成测试

适合测试：

- client ↔ daemon IPC
- daemon create session
- attach/detach
- metadata update
- log write
- resize message
- kill session
- daemon restart behavior

---

## E2E 测试

模拟真实用户行为：

```bash
persist new
persist ls
persist attach <id>
```

以及：

```bash
ssh node
```

自动接管场景可以在容器或测试 VM 中验证。

---

## PTY 测试

必须测试：

```bash
echo hello
pwd
cd /tmp
whoami
exit
```

`exit` 后应验证 Session 进入 Closed 状态：PTY 和 shell 进程已释放，但再次 attach 可以恢复输出、cwd 和允许持久化的环境变量。

交互程序：

```bash
bash
zsh
fish
vim
nano
less
top
htop
watch
python
mysql
```

---

## Signal 测试

必须测试：

```bash
sleep 100
Ctrl+C
```

```bash
vim
Ctrl+Z
fg
```

```bash
cat
Ctrl+D
```

在 shell 空行按 `Ctrl+D` 时，应与 `exit` 一样进入 Closed；在 `cat` 等程序中，`Ctrl+D` 只表示 EOF 输入并由程序自行处理。

```bash
top
resize
```

---

## Detach/Reattach 测试

场景：

1. 创建 Session。
2. 运行长任务。
3. Client 断开。
4. 确认任务仍运行。
5. 重新 attach。
6. 确认可继续查看输出和输入命令。

示例：

```bash
for i in $(seq 1 1000); do echo $i; sleep 1; done
```

---

## SSH 兼容测试

必须确保不破坏：

```bash
ssh node hostname
scp file node:/tmp/
sftp node
rsync file node:/tmp/
ansible all -m ping
git clone user@node:repo.git
```

---

## 大输出测试

测试命令：

```bash
yes | head -n 1000000
seq 1 1000000
journalctl -f
cat large-file
```

验证：

- daemon 不崩溃
- 内存不无限增长
- client 慢时不拖垮 session
- 日志轮转正常
- ring buffer 覆盖正常

---

## 多 Session 测试

测试：

- 10 sessions
- 100 sessions
- 500 sessions
- 1000 sessions

验证：

- 创建速度
- attach 延迟
- daemon CPU
- daemon memory
- metadata 查询速度
- GC 行为

---

## 权限测试

测试：

- socket 目录权限错误
- socket 文件权限错误
- metadata 权限错误
- log 目录不可写
- 非 owner attach
- `/tmp` symlink attack 防护

---

## 安装/卸载测试

测试：

```bash
persist install
persist uninstall
```

验证：

- shell profile 正确注入
- 重复 install 不重复注入
- uninstall 可恢复
- bypass 可用
- 失败时不破坏用户 shell

---

## doctor 测试

构造错误环境：

- daemon 未运行
- stale socket
- 权限错误
- 数据库损坏
- profile hook 缺失
- 日志目录不可写

验证 doctor 给出明确诊断。

---

## 回归测试

每个 bug 修复必须增加回归测试。

回归测试名称应能说明 bug。

---

## 测试环境

至少覆盖：

- Ubuntu
- Debian
- Rocky Linux
- Fedora
- Arch/Manjaro

Shell：

- bash
- zsh
- fish

Terminal：

- xterm
- GNOME Terminal
- Konsole
- Windows Terminal over SSH
- iTerm2
- Alacritty
- WezTerm

---

## 自动化要求

GitHub Actions CI 至少运行：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- basic build

GitHub Actions package workflow 至少验证：

- release tarball 可构建
- artifact 可上传
- SHA256 checksum 可生成

PTY/E2E 测试可以分为 privileged 或 nightly。

---

## 测试数据记录

性能和压力测试必须记录：

- OS
- kernel version
- CPU
- memory
- filesystem
- terminal
- shell
- command
- result

---

## 完成标准

没有测试的功能不算完成。

没有回归测试的 bug 修复不算完成。

没有压力测试数据的性能优化不算完成。

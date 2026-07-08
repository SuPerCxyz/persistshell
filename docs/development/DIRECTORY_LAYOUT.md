# PersistShell Directory Layout

本文档定义 PersistShell 推荐仓库目录结构。

PersistShell 的主开发语言是 Rust。目录结构应围绕 Cargo workspace 组织，但核心模块边界应保持稳定。

---

## 顶层结构

```text
PersistShell/
  README.md
  LICENSE
  CONTRIBUTING.md
  CODE_OF_CONDUCT.md
  SECURITY.md
  SUPPORT.md
  ROADMAP.md
  CHANGELOG.md
  TODO.md
  NEXT_TASK.md
  MILESTONES.md
  Cargo.toml
  Cargo.lock

  docs/
  .github/
  crates/
  tests/
  scripts/
  packaging/
  examples/
```

---

## .github/

GitHub Actions workflow 目录。

代码会自动同步到 GitHub 仓库：

```text
https://github.com/SuPerCxyz/persistshell
```

GitHub Actions 在该仓库运行。后续应创建：

```text
.github/
  workflows/
    ci.yml
    package.yml
```

`ci.yml` 负责 fmt、clippy、test。

`package.yml` 负责构建 tarball、deb/rpm 和 checksum，并上传 artifacts。

---

## docs/

项目文档目录。

docs 是单一事实来源。

```text
docs/
  design/
  architecture/
  protocol/
  development/
  benchmark/
  user/
  known/
  adr/
```

---

## crates/

Rust workspace crates。

推荐：

```text
crates/
  persist-cli/
  persistd/
  persist-core/
  persist-pty/
  persist-ipc/
  persist-metadata/
```

`persist-cli` 生成用户命令 `persist`。

`persistd` 生成 per-user daemon `persistd`。

`persist-core` 保存共享领域模型、错误类型、配置和状态机。

`persist-pty` 封装 PTY、termios、process group、signal 和 Linux syscall。

`persist-ipc` 封装 Unix socket 协议。

`persist-metadata` 封装 SQLite metadata store。

---

## CLI

子命令包括：

```text
persist new
persist ls
persist attach
persist kill
persist daemon start
persist doctor
persist install
```

---

## persistd crate

Daemon 二进制 crate。

包含：

```text
crates/persistd/
  src/main.rs
  src/server.rs
  src/lifecycle.rs
  src/event_loop.rs
  src/gc.rs
  src/runtime.rs
```

Daemon 负责：

- IPC server
- session runtime
- PTY owner
- event loop
- GC
- metadata coordination

---

## persist-cli crate

Client CLI 二进制 crate。

包含：

```text
crates/persist-cli/
  src/main.rs
  src/cli.rs
  src/attach.rs
  src/terminal.rs
  src/command.rs
```

Client 负责：

- CLI 参数
- 终端 raw mode
- attach 数据转发
- resize
- 用户输出

---

## library crates

共享库 crate。

推荐：

```text
crates/persist-core/
  src/config.rs
  src/error.rs
  src/session.rs
  src/state.rs

crates/persist-pty/
  src/lib.rs
  src/termios.rs
  src/process.rs
  src/signal.rs
  src/platform.rs

crates/persist-ipc/
  src/lib.rs
  src/protocol.rs
  src/socket.rs

crates/persist-metadata/
  src/lib.rs
  src/schema.rs
  src/migration.rs
```

---

## public API policy

Phase 1 尽量少暴露公开 API。

只有真正稳定并可被外部使用的 API 才公开到 crate public surface。

不要过早公开 API。crate 内部优先使用 `pub(crate)`。

---

## tests/

集成测试和测试工具。

```text
tests/
  integration/
  e2e/
  fixtures/
  stress/
  compat/
```

---

## scripts/

开发脚本。

例如：

```text
scripts/
  test.sh
  lint.sh
  fmt.sh
  bench.sh
  install-dev.sh
```

---

## packaging/

打包相关。

```text
packaging/
  deb/
  rpm/
  systemd/
```

Phase 1 可先为空。

---

## examples/

示例配置和使用示例。

```text
examples/
  config.toml
  shell-hook.bash
  shell-hook.zsh
```

---

## XDG 运行目录

运行时目录推荐：

```text
$XDG_RUNTIME_DIR/persistshell/
```

或：

```text
/run/user/$UID/persistshell/
```

包含：

```text
persist.sock
daemon.pid
daemon.lock
```

权限：

```text
0700
```

---

## XDG 数据目录

数据目录推荐：

```text
$XDG_DATA_HOME/persistshell/
```

默认：

```text
~/.local/share/persistshell/
```

包含：

```text
persist.db
logs/
history/
```

---

## XDG 状态目录

状态目录推荐：

```text
$XDG_STATE_HOME/persistshell/
```

默认：

```text
~/.local/state/persistshell/
```

包含：

```text
daemon.log
client.log
```

---

## XDG 配置目录

配置目录推荐：

```text
$XDG_CONFIG_HOME/persistshell/
```

默认：

```text
~/.config/persistshell/
```

包含：

```text
config.toml
```

---

## 系统配置

系统级配置：

```text
/etc/persistshell/config.toml
```

Phase 1 可选。

---

## 文件权限

必须遵守：

```text
runtime dir: 0700
config dir:  0700 或 0755，视内容而定
data dir:    0700
state dir:   0700
socket:      0600
database:    0600
logs:        0600
```

---

## 目录创建规则

程序启动时可以自动创建必要目录。

但必须：

- 检查 owner
- 检查权限
- 防止 symlink attack
- 不覆盖用户文件
- 错误时给出清晰提示

---

## 不推荐路径

不推荐默认使用：

```text
/tmp/persistshell.sock
```

除非作为 fallback，并且必须使用安全目录：

```text
/tmp/persistshell-$UID/
```

目录必须：

```text
0700
owner = current uid
not symlink
```

---

## 目录结构变更规则

目录结构变化必须更新：

- 本文件
- README
- INSTALL
- CONFIG
- TODO
- CHANGELOG

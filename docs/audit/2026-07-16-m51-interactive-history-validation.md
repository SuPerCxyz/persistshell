# M51 交互式命令历史验证

## 范围

本次验证覆盖 `persist ls` TTY 交互、按 ID 菜单、最新优先命令历史、Shell 临时 hook、用户配置
保护、有界存储、Ubuntu/RHEL 包和 Rocky Linux 9.7 真实部署。

## 本地自动化

环境：Ubuntu 26.04，Rust 1.96.0。

通过命令：

```bash
cargo test --workspace --all-targets --all-features --no-fail-fast
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release --workspace --locked
```

结果：全部测试通过；`persistd` 仍有一个仓库既有的系统 zshrc PTY 用例保持 ignored。严格 Clippy
零警告。

定向覆盖包括：

- 二进制历史倒序、分页、多行、权限、4 MiB 压缩、并发追加、损坏文件和符号链接拒绝。
- `persist ls` 列表选择、未知 ID、历史返回、attach、退出和第 51 条跨页。
- bash、zsh、fish 真实 PTY 配置加载和既有 prompt/postexec hook 保留。
- bash 原生 history 过滤；zsh/fish 自定义过滤器的安全降级和 `0600` 状态标记。
- attach 返回后 stdin 文件状态 flags 恢复。

## Ubuntu 包

构建并验证：

```text
persistshell-v0.1.0-ubuntu-26.04-x86_64-unknown-linux-gnu.tar.gz
persistshell_0.1.0_amd64.deb
```

两个 checksum 均通过。tar 的 `docs/user/USER_GUIDE.md` 和 deb 的
`/usr/share/doc/persistshell/USER_GUIDE.md` 均存在。

## Rocky Linux 9.7

在 `ssh test` 原生 release 构建并生成 `persistshell-0.1.0-1.el9.x86_64.rpm`，checksum、版本和
`/usr/share/doc/persistshell/USER_GUIDE.md` 通过。使用 `--replacepkgs` 安装后 daemon 正常监听。

真实 TTY 验证：

- `persist ls` 显示表格并接受 Session ID。
- `persist ls 8` 直接打开菜单。
- 菜单选择 `a` attach，执行 `echo menu-return-fixed` 后 `exit`，可返回原菜单。
- 返回后立即选择 `h`，历史显示 `[1] echo menu-return-fixed`。
- `persist ls 8` 非 TTY 只输出 Session 8 的表格行。
- `/root/.bashrc` 验证前后 SHA-256 一致。
- 命令历史和状态文件权限均为 `0600`，hook 目录为 `0700`。

test 主机没有 zsh 和 fish；两者使用本地真实 PTY 自动化验证，不声明 Rocky 端到端覆盖。

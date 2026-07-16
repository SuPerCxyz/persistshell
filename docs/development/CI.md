# PersistShell CI And Package Build

本文档定义 PersistShell 的 CI、GitHub Actions 和发布包构建要求。

## Repository Topology

PersistShell 的代码会自动同步到 GitHub 仓库：

```text
https://github.com/SuPerCxyz/persistshell
```

GitHub Actions 运行在该 GitHub 仓库上。因此 CI 和发布包构建 workflow 必须满足：

- 可以在 GitHub 托管 runner 上运行。
- 不依赖内网 Git 地址。
- 不依赖开发者本机路径。
- 不要求访问自建 Git 服务才能完成普通构建。
- 不把密钥、token、私有 SSH 配置写入 workflow。

## Workflow Location

后续实现 CI 时，workflow 文件应放在：

```text
.github/workflows/
```

建议至少包含：

```text
.github/workflows/ci.yml
.github/workflows/package.yml
```

## CI Workflow

`ci.yml` 用于常规质量检查。

触发条件：

- push
- pull_request
- workflow_dispatch

Rust 项目至少运行：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

如果后续存在需要 Linux capability、PTY、真实 SSH 或 systemd 的测试，应拆分为单独的 integration job，避免阻塞普通单元测试。

## Package Workflow

`package.yml` 用于构建可分发包。

触发条件：

- tag push，例如 `v*`
- workflow_dispatch

当前发布包构建必须按运行时 ABI 分开生成：

- Ubuntu 26.04 x86_64 tarball 与 Debian `.deb`
- RHEL 9 x86_64 tarball 与 `.el9` RPM
- SHA256 checksums

后续发布阶段应支持：

- release notes
- artifact upload
- 可选签名

本地先在目标用户空间构建 release binaries，再执行对应格式：

```text
cargo build --release --workspace --locked
PERSIST_PACKAGE_PLATFORM=ubuntu-26.04 scripts/package-release.sh tarball deb
PERSIST_PACKAGE_PLATFORM=rhel9 PERSIST_PACKAGE_RPM_RELEASE=1.el9 \
  scripts/package-release.sh tarball rpm
```

GitHub `ubuntu-26.04` runner 原生构建 Ubuntu 包。RHEL 9 包在 `rockylinux:9` job container
中原生构建，并拒绝最高 GLIBC 需求超过 2.34 的二进制。禁止在 Ubuntu 上构建二进制后再包装
成 RHEL RPM。

GitHub Actions 构建出的包必须作为 workflow artifacts 上传，tag release 时可进一步附加到 GitHub Release。

## Target Platforms

Phase 1 当前支持：

```text
Ubuntu 26.04 x86_64
RHEL 9 compatible x86_64
```

后续再扩展：

```text
aarch64-unknown-linux-gnu
x86_64-unknown-linux-musl
aarch64-unknown-linux-musl
```

跨平台构建不得影响 Linux PTY 语义测试。不能为了跨平台牺牲 Linux 行为正确性。

## Package Contents

发布包至少包含：

- `persist`
- `persistd`
- README
- LICENSE
- CHANGELOG
- user documentation
- bash/zsh/fish shell completion
- man page

不得把测试 fixture、构建缓存、私有配置或开发者本机路径打进发布包。

## Release Artifact Rules

所有 release artifact 必须：

- 文件名包含项目名、版本、目标平台。
- 生成 SHA256 checksum。
- 可从干净 runner 复现构建。
- 不依赖未提交文件。
- 不包含 secret。

推荐命名：

```text
persistshell-v0.1.0-ubuntu-26.04-x86_64-unknown-linux-gnu.tar.gz
persistshell-v0.1.0-rhel9-x86_64-unknown-linux-gnu.tar.gz
persistshell_0.1.0_amd64.deb
persistshell-0.1.0-1.el9.x86_64.rpm
```

## GitHub Mirror Rule

GitHub Actions 只负责在 GitHub 镜像仓库中执行构建、测试和打包。

不要在 workflow 中做反向同步，不要把 GitHub Actions 设计成修改自建 Git 仓库的工具。

如果需要同步状态，应由外部同步机制负责，而不是 PersistShell 的 CI workflow。

## Completion Criteria

CI 和包构建能力完成时，必须满足：

1. GitHub Actions 在 GitHub 镜像仓库可见。
2. `ci.yml` 能运行 fmt、clippy、test。
3. `package.yml` 能构建 tarball、`.deb` 与 `.rpm`。
4. workflow artifacts 可下载。
5. artifact 有 checksum。
6. 文档说明如何触发 workflow。
7. TODO、MILESTONES、CHANGELOG 已更新。

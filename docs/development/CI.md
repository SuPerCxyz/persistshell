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

当前发布包使用 glibc 2.28 单一 ABI，每个架构只构建一次：

- x86_64 与 aarch64 release binaries
- 通用 Linux tar.xz、Debian `.deb` 与 RPM `.rpm`
- SHA256 checksums
- RPM/DEB 3 MiB、tar.xz 3.5 MiB 体积门禁

后续发布阶段应支持：

- release notes
- artifact upload
- 可选签名

本地先在 EL8 级用户空间构建 release binaries，再执行对应格式：

```text
cargo build --release --workspace --locked \
  --target x86_64-unknown-linux-gnu
scripts/check-linux-release.sh x86_64-unknown-linux-gnu
PERSIST_PACKAGE_TARGET=x86_64-unknown-linux-gnu \
  scripts/package-release.sh tarball deb rpm
```

GitHub 使用 x64/ARM64 原生 runner，在 `rockylinux:8` container 中构建，并拒绝最高
GLIBC 需求超过 2.28、架构错误或保留 debug section 的二进制。同一架构的 RPM、DEB 和
tar.xz 必须复用这次构建，禁止在新用户空间重新编译后伪装成通用包。

GitHub Actions 构建出的包必须作为 workflow artifacts 上传，tag release 时可进一步附加到 GitHub Release。

## Target Platforms

当前发布架构和用户空间：

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
glibc 2.28+
```

不支持：

```text
i686 / ARMv7
EL7
musl/Alpine
```

Package workflow 在 Rocky 8/9/10、CentOS Stream 9/10、Ubuntu 22.04/24.04/26.04 和
Debian 11/12/13 执行安装 smoke；两种架构都覆盖最低和最高代表版本。跨平台构建不得
影响 Linux PTY 语义测试，不能为了跨平台牺牲 Linux 行为正确性。

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

固定命名：

```text
persistshell-v{version}-linux-x86_64.tar.xz
persistshell-v{version}-linux-aarch64.tar.xz
persistshell_{version}_amd64.deb
persistshell_{version}_arm64.deb
persistshell-{version}-1.x86_64.rpm
persistshell-{version}-1.aarch64.rpm
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

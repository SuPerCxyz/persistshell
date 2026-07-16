# M46 发布包验证

> 历史记录：本审计中的 Ubuntu 构建 RPM 方案已被 RHEL 9 ABI 实测证明不兼容。
> 当前方案与证据见 `docs/audit/2026-07-15-m50-platform-package-remote-validation.md`。

打包入口为 `scripts/package-release.sh`。它从 workspace manifest 读取版本，生成
Linux tarball、Debian `.deb`、RPM `.rpm` 及对应 SHA-256 文件；GitHub Actions
`package.yml` 复用此入口。

| 环境 | 产物 | 验证 | 结果 |
|---|---|---|---|
| 本机 Ubuntu 26.04 | tarball | 内容列表、解包后 `persist --version`、checksum | 通过 |
| 本机 Ubuntu 26.04 | `.deb` | `dpkg-deb --contents`、解包执行、checksum | 通过 |
| test Rocky Linux 9.7 | `.rpm` | 原生 release 编译、`rpmbuild`、`rpm -qpl`、checksum | 通过 |

YAML 已由本机 Python YAML parser 解析。`actionlint` 在本机不可用，且本任务禁止
push、tag 和发布，所以 GitHub hosted runner 尚未实际触发。workflow 明确在
`ubuntu-latest` 安装 `rpm` 后构建并上传三种包与 checksum；其真实执行结果必须由
后续 mirror 同步触发的 workflow run 记录。

本验证没有在主机上安装 `.deb` 或 `.rpm`，以免覆盖 test 上已部署的服务。签名、GitHub
Release、systemd unit、shell hook 与其他 CPU 架构不属于 M46。

M47 验证中修复了 checksum 记录 `dist/...` 相对路径的问题。现在每个 checksum 只记录
artifact 文件名，可在下载后的 artifact 目录中通过 `sha256sum --check *.sha256` 校验。

# M46 打包设计

## 目标

统一生成带平台标识的 Linux tarball、Debian `.deb`、RPM `.rpm` 和 SHA-256 checksum。
打包逻辑位于 `scripts/package-release.sh`，GitHub Actions 分别准备 Ubuntu 26.04 与 EL9
用户空间并上传独立 artifact。

## 内容

包内包含 `persist`、`persistd`、README、LICENSE、CHANGELOG 与用户文档。deb 安装到
`/usr/bin` 和 `/usr/share/doc/persistshell`；rpm 使用相同文件布局。包版本来自 workspace
manifest。

## 验证

Ubuntu 26.04 构建 tarball/deb；`rockylinux:9` 构建 RHEL 9 tarball 与 `.el9` RPM，并校验
二进制最高 GLIBC 需求不超过 2.34。每个平台在上传前检查版本、包内容和 checksum。

## 非目标

- 不发布 GitHub Release、不创建 tag 或签名。
- 不构建 macOS、Windows、ARM 或容器镜像。
- 不安装 systemd unit 或 shell hook。

# M46 打包设计

## 目标

统一生成 Linux tarball、Debian `.deb`、RPM `.rpm` 和 SHA-256 checksum。打包逻辑位于
`scripts/package-release.sh`，GitHub Actions 只负责准备工具链和上传 artifact。

## 内容

包内包含 `persist`、`persistd`、README、LICENSE、CHANGELOG 与用户文档。deb 安装到
`/usr/bin` 和 `/usr/share/doc/persistshell`；rpm 使用相同文件布局。包版本来自 workspace
manifest。

## 验证

本机构建 tarball、deb 与 checksum，使用 `dpkg-deb --info/--contents` 校验。若缺少
`rpmbuild`，本机记录跳过；GitHub Actions 在 Ubuntu runner 安装 `rpm` 后构建并上传 rpm。

## 非目标

- 不发布 GitHub Release、不创建 tag 或签名。
- 不构建 macOS、Windows、ARM 或容器镜像。
- 不安装 systemd unit 或 shell hook。

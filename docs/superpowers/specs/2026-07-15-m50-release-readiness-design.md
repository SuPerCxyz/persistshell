# M50 v1.0 Release Readiness 设计

## 目标

在不创建 tag、不 push、不发布 GitHub Release、不上传 artifact、不签名的前提下，完成 v1.0
本地和 test 主机的发布就绪验证，并形成维护者可执行的 release checklist。

## 验证范围

本机执行 Rust fmt、clippy、workspace tests、completion、groff/man、release build、tarball、
deb、checksum 和 diff 检查。test 主机以原生 Rust 构建 rpm，检查包内容和 checksum，安装最新
`persist`/`persistd` 到既有 `/usr/local/lib/persistshell/0.1.0` 目录后，用隔离 XDG 环境验证
daemon、new、list、close 与清理。

## 发布边界

根目录 MIT `LICENSE` 是当前许可事实，release 包必须包含它。GitHub Actions workflow 已定义但
未实际触发；tag、镜像同步、GitHub Release、artifact 上传、签名、依赖许可证审查和最终发布
均需要维护者明确授权或外部状态，不能在本任务中伪报完成。

## 交付

新增 release checklist 与 M50 审计记录，分别列出已完成的本地/test 证据、已知限制和待维护者
执行项。不会改变 CLI、daemon、协议、配置或发布版本号。

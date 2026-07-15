# M47 Man Page 设计

## 目标

为已经实现的 `persist` 和 `persistd` 提供可离线阅读的 Unix manual page，并将其作为
release artifact 的一部分。手册只描述当前代码可执行的参数和行为。

## 方案

采用两份直接维护的 groff man source：`docs/man/persist.1` 与
`docs/man/persistd.1`。不引入文档生成器或新的构建依赖。`persist(1)` 聚焦 session
管理、日志、标记与安装命令；`persistd(1)` 聚焦 `foreground`、`--idle-timeout`、运行
目录和信号行为。

每份手册包含 NAME、SYNOPSIS、DESCRIPTION、COMMANDS 或 OPTIONS、ENVIRONMENT、FILES、
EXIT STATUS、SECURITY、SEE ALSO。内容与 CLI parser、配置路径和用户文档交叉核对；不把
规划功能或过时命令写入手册。

## 打包与验证

tarball 放入 `docs/man/`；deb 与 rpm 安装到 `/usr/share/man/man1/`。本机用
`groff -man -Tutf8` 渲染两个文件，并重新构建 tarball/deb 检查 man page 路径和 checksum。
RPM 将在有 `rpmbuild` 的 test 主机复用既有打包验证入口。不会安装任何生成的包。

## 边界

不实现 CLI help 改进、不增加 shell completion、不发布 release、不压缩 man page，亦不为
未来功能编写手册。与当前 CLI 冲突的用户命令总览会在本任务中作最小事实校正。

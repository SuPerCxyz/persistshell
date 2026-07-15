# M47 Man Page 验证

新增 `docs/man/persist.1` 与 `docs/man/persistd.1`。手册按当前 CLI parser、daemon
入口和配置路径编写，未描述未实现的命令或选项。

| 环境 | 验证 | 结果 |
|---|---|---|
| 本机 Ubuntu 26.04 | `groff -man -Tutf8` 渲染两个手册 | 通过 |
| 本机 Ubuntu 26.04 | `man --local-file docs/man/persist.1` | 通过 |
| 本机 Ubuntu 26.04 | tarball/deb 包含 man page、`dist/` 内 checksum | 通过 |
| test Rocky Linux 9.7 | 原生 RPM 构建、man 路径和 checksum | 通过 |

tarball 将 source 放在 `docs/man/`，deb 放在 `/usr/share/man/man1/`。Rocky RPM 的
`brp-compress` 按发行版规范将文件压缩为 `persist.1.gz` 和 `persistd.1.gz`。

`persistd foreground --help` 目前会运行 foreground daemon，故手册要求使用
`persistd help`；该可用性问题已记录到 `docs/known/KNOWN_ISSUES.md`。

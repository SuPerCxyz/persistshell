# M49 v1.0 用户文档验证

M49 以当前 CLI parser、daemon 入口、配置默认值、安装器和 package script 为依据，修正用户
文档中的旧设计描述，新增 `docs/user/TROUBLESHOOTING.md`。

已移除作为可执行示例的 `SH_DISABLE`、`persist tail`、`persist kill --force`、
`persist new --name`、`persist doctor --fix` 和 `persist ls --json`。文档现在明确：

- SSH hook 固定使用 `PERSIST_DISABLE`，且当前安装器只管理 bash/zsh profile。
- Closed Session attach 会创建新的可写 PTY，只恢复受限环境快照。
- `SSH_AUTH_SOCK` 仅在创建 PTY 时安全继承绝对 Unix socket。
- 单 active writer、readonly attach、daemon/socket 排障及发布包使用方式。

| 环境 | 验证 | 结果 |
|---|---|---|
| 本机 Ubuntu 26.04 | 用户文档旧可执行示例扫描、文档清单 | 通过 |
| 本机 Ubuntu 26.04 | `persist --help`、`persistd help` 对照 | 通过 |
| 本机 Ubuntu 26.04 | tarball/deb 包含 `TROUBLESHOOTING.md`、checksum | 通过 |
| test Rocky Linux 9.7 | 原生 RPM 包含 `TROUBLESHOOTING.md`、checksum | 通过 |

GitHub hosted runner 仍未在镜像仓库触发；该外部 workflow 运行不在本次文档验证中伪报完成。

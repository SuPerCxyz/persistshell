# M48 Shell Completion 验证

新增 `completions/_persist`（zsh）和 `completions/persist.fish`，并修正
`completions/persist.bash`。三者覆盖当前顶层命令、daemon/config/log 子命令、Session ID、
`ls --tag`、`attach --readonly`、replay 和 uninstall 选项。

`scripts/test-completions.sh` 使用 mock `persist ls` 验证 bash 的顶层和 Session ID 候选；
fish 通过 `complete -C` 验证静态候选；zsh 完成语法和无执行自动加载检查。该脚本不运行
真实 daemon，也不会创建或修改 Session。

| 环境 | 验证 | 结果 |
|---|---|---|
| 本机 Ubuntu 26.04 | `scripts/test-completions.sh` | 通过 |
| 本机 Ubuntu 26.04 | tarball/deb completion 路径、`dist/` checksum | 通过 |
| test Rocky Linux 9.7 | 原生 RPM completion 路径、checksum | 通过 |

tarball 包含 `completions/`；deb/rpm 分别安装 bash、zsh、fish 的标准 completion 路径。
test 主机未安装 zsh 或 fish，因此其运行时补全行为仅在本机验证，RPM 只验证文件内容。

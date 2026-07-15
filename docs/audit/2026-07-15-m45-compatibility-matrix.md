# M45 兼容性矩阵

基线命令为 `scripts/compatibility-matrix.sh`：每个组合以隔离 XDG 目录验证
`persistd foreground`、`persist new`、`persist ls` 和 `persist close`。

| 环境 | Shell | TERM | 结果 |
|---|---|---|---|
| Ubuntu 26.04（本机） | bash | xterm-256color | 通过 |
| Ubuntu 26.04（本机） | zsh | xterm-256color | 通过 |
| Ubuntu 26.04（本机） | fish | xterm-256color | 通过 |
| Rocky Linux 9.7（test） | bash | dumb | 通过 |
| Rocky Linux 9.7（test） | zsh | dumb | 跳过：未安装 |
| Rocky Linux 9.7（test） | fish | dumb | 跳过：未安装 |

该矩阵只证明当前环境的非 attach 基线。真实 SSH、交互 attach、resize 和未安装的发行版/终端
仍需在相应环境中验证，不能由本记录推断为已支持。

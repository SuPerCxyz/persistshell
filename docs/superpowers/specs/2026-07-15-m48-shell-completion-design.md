# M48 Shell Completion 设计

## 目标

提供与当前 `persist` parser 一致的 bash、zsh、fish 命令补全，并将三个文件纳入 tarball、
deb 与 rpm。补全不能创建、attach、关闭或修改 Session。

## 方案

保留并修正 `completions/persist.bash`，新增 zsh 的 `completions/_persist` 和 fish 的
`completions/persist.fish`。三者维护相同的静态顶层命令、子命令和选项；仅在需要
Session ID 时调用 `persist ls 2>/dev/null`，提取数据行首列。命令不可用、daemon 未运行或
列表失败时返回空候选，不启动 daemon。

zsh 使用 `_arguments` 和 `_describe`，fish 使用 `complete`，bash 继续使用
`bash-completion` 的 `_init_completion`。不生成 completion，不增加 CLI 参数，也不调用
`persist attach`、`new`、`daemon start` 等有副作用命令。

## 打包与用户安装

tarball 保存 `completions/`。deb/rpm 分别安装到
`/usr/share/bash-completion/completions/persist`、
`/usr/share/zsh/site-functions/_persist`、
`/usr/share/fish/vendor_completions.d/persist.fish`。用户文档说明从源码目录 source 的方式；
不改变 `persist install` 的 shell hook 职责。

## 验证

新增 `scripts/test-completions.sh`：检查三种语法，使用 mock `persist ls` 验证 bash 的
命令与 Session ID 候选，使用 fish `complete -C` 验证顶层候选，并检查 zsh 文件可加载。
之后重新构建 tarball/deb，并在 test Rocky 主机验证 RPM 文件路径与 checksum。

## 边界

不做 Session 名称、tag 值、文件路径或远程主机补全；不执行真实 daemon 测试；不安装 shell
配置，不创建 GitHub Release，也不改动已存在的 SSH hook。

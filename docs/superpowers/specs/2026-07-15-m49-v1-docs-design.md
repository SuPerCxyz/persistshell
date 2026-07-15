# M49 v1.0 文档完善设计

## 目标

使 README、用户安装/配置/命令/FAQ、限制和已知问题与当前 Rust CLI、daemon、配置和发布包
保持一致，并新增面向用户的故障排查文档。文档不得把设计目标或未实现命令写成可执行行为。

## 信息来源与范围

命令以 `crates/persist-cli/src/command.rs` 和实际 `persist --help` 为准；daemon 以
`crates/persistd/src/server.rs` 为准；路径与配置以 `persist-core` 配置定义为准；打包内容
以 `scripts/package-release.sh` 为准。只改用户可见文档和相关索引/限制记录，不改 CLI、
daemon、协议或配置语义。

## 处理方式

1. 命令文档列出全部当前 parser 支持的命令、参数与副作用，移除 `tail`、`--force`、
   `new --name`、`doctor --fix` 等不存在的示例。
2. 安装与 FAQ 使用实际 hook 条件和唯一的 `PERSIST_DISABLE` 绕过变量；说明包、man page
   和 completion 的安装位置。
3. 配置文档区分已生效字段与当前只解析/保留的字段，补齐 GC 配置和安全限制。
4. 新增故障排查，覆盖 daemon/socket、权限、SSH hook、Closed Session 恢复、日志和
   逃生路径。
5. 限制和已知问题只保留当前事实，特别是安全的 `SSH_AUTH_SOCK` 继承与 `persistd`
   子命令帮助问题。

## 验证

对照实际 help、parser 和配置默认值做定向审阅；使用 `rg` 阻止已移除的错误示例；检查所有
用户文档链接存在，并运行 `git diff --check`。文档变更不需要启动 daemon 或安装发布包。

## 非目标

不承诺 GitHub workflow 已运行，不为未验证发行版、TUI 或 root 跨用户访问背书，不修复
`persistd foreground --help` 行为，也不改动 shell hook 或安装器代码。

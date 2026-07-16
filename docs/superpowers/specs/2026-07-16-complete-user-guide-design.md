# PersistShell 完整用户手册设计

## 背景

PersistShell 已有 README、命令、安装、配置、FAQ 和故障排查等专题文档，但普通用户需要在多个
文件之间查找信息，难以仅凭一个入口完成安装、使用、恢复和排障。项目需要一份按实际使用顺序
组织的中文完整用户手册，同时保留现有专题文档作为维护参考。

## 目标

- 新增一个普通用户唯一需要阅读的完整入口。
- 用户仅凭该手册即可安装、启用、日常使用、跨电脑接管、恢复、排障和卸载 PersistShell。
- 全部已实现 CLI 命令、重要参数、状态语义和安全逃生方式都有可执行示例。
- 手册随 tar、deb 和 rpm 安装，不要求访问源码仓库或互联网。
- 内容与当前代码、协议、已知限制和项目原则一致，不承诺尚未实现的功能。

## 非目标

- 不删除或合并现有专题文档。
- 不修改 CLI、协议、配置、打包逻辑或运行时行为。
- 不编写开发者架构、内部协议或贡献指南。
- 不把 GitHub Release、签名或 SBOM 决策混入用户操作手册。

## 文件和入口

主文件为 `docs/user/USER_GUIDE.md`。现有 `scripts/package-release.sh` 使用 `docs/user/*.md`
收集用户文档，因此无需改代码，安装后路径为：

```text
/usr/share/doc/persistshell/USER_GUIDE.md
```

同步更新 `README.md`、`docs/INDEX.md` 和 `docs/user/INSTALL.md`，将该文件标记为首选用户入口。
发布清单增加 tar、deb、rpm 均携带该文件的检查项。

## 内容结构

手册按用户完成任务的顺序组织：

1. 产品定位、适用范围和 Session 核心概念。
2. 安装、验证版本、启动 daemon 和运行 doctor。
3. 五分钟快速开始：创建、列出、attach、断开和恢复。
4. 生命周期语义：SSH 断开、detach、`exit`、`Ctrl+D`、close 和 kill 的区别。
5. 通过 `persist ls` 交互选择、查看最新优先的实时命令历史、进入或返回其他 Session。
6. 指定 Session、Closed Session 冷恢复、多电脑 writer takeover 和 readonly attach。
7. SSH 自动接管、非交互兼容、临时绕过和永久卸载。
8. 全部 CLI 命令及常用参数，按任务分组并提供示例。
9. 日志查看、搜索、导出、replay 及日志边界。
10. rename、note、tag、pin、lock 等 Session 管理能力。
11. ps、stats、snapshot、metrics 等观测能力。
12. 配置、目录、权限、环境变量恢复和 SSH agent 同步边界。
13. daemon 管理、升级、卸载、保留或清理数据。
14. 常见工作流、故障排查、已知限制和命令速查表。

## 内容规则

- 默认示例使用 `persist`、`persistd` 和 `ssh test` 形式，不依赖特定生产主机名。
- 首次出现命令时说明预期输出或状态变化，不只列语法。
- 明确区分“SSH 断开后 runtime 继续”和“`exit`/`Ctrl+D` 后 runtime 释放”。
- 明确默认 attach 可写，新连接可 takeover；readonly 不是跨电脑访问的唯一方式。
- 明确 Closed Session 恢复会启动新 shell runtime，只恢复已记录的有限上下文。
- `replay --speed`、`replay --follow`、快速 `cd; exit` cwd 竞态等限制必须如实说明。
- 安全章节必须包含 `PERSIST_DISABLE=1`、干净 shell 和 `persist uninstall` 逃生方式。
- 避免要求用户理解 PTY、IPC、SQLite schema 等内部实现细节。

## 验证

- 将 `persist help` 和命令解析测试中的命令集合与手册逐项对照。
- 对照 `CONFIG.md`、`COMMANDS.md`、`FAQ.md`、`TROUBLESHOOTING.md` 和已知限制检查一致性。
- 检查 Markdown 链接、标题层级、代码块和未完成内容。
- 本地生成 tar 和 deb，检查安装内容包含 `USER_GUIDE.md`。
- 在 Rocky Linux 9 容器生成 rpm，检查 RPM 文件列表包含 `USER_GUIDE.md`。

## 完成标准

用户无需阅读其他文档，即可依据手册完成安装、首次启动、Session 日常操作、跨电脑接管、
Closed Session 恢复、日志与观测、SSH 接管与绕过、daemon 管理、卸载和常见故障处理。相关入口、
任务状态、发布检查和变更记录同步更新，三种包的手册路径验证通过。

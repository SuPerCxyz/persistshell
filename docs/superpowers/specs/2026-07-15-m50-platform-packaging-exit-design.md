# M50 平台打包与退出通知修复设计

## 背景

`v0.1.0` 的 GitHub Package workflow 在 Ubuntu runner 上构建同一组二进制，再同时生成
tarball、deb 和 rpm。远程 Rocky Linux 9.7 验证发现，该 tarball 依赖 `GLIBC_2.39`，无法在
RHEL 9 ABI 环境运行。自然执行 `exit` 的交互测试还发现 daemon 已关闭 runtime 和保存恢复
上下文，但没有向 attach 客户端发送既有协议中的 `SessionExited`，客户端因此持续等待。

## 目标

- GitHub Actions 分别生成 Ubuntu 26.04 和 RHEL 9 x86_64 二进制包。
- RHEL 9 包必须在 EL9 用户空间原生构建，最高 GLIBC 需求不得超过 2.34。
- 包文件名和 artifact 名包含平台，禁止将 Ubuntu 二进制包装成 RHEL rpm。
- shell 自然退出后，writer 和 read-only 客户端收到最终输出及 `SessionExited`。
- 不创建 GitHub Release，不移动或改写现有 `v0.1.0` tag。

## 构建架构

Package workflow 使用两个独立 job：

1. `ubuntu-26.04` 使用 GitHub 官方同名 x64 runner，构建 tarball 和 deb。
2. `rhel9` 在 GitHub Ubuntu runner 内使用 `rockylinux:9` 容器，构建 tarball 和
   `.el9.x86_64.rpm`。Rocky Linux 9 作为可公开重复的 RHEL 9 ABI 基线。

打包脚本接受显式平台标识，并将其写入 tarball、deb 和 rpm 文件名。每个平台独立上传
artifact 及相邻 `.sha256`。workflow 对二进制执行版本检查、包内容检查和 GLIBC 上限检查；
任一检查失败则不上传该平台产物。Ubuntu 26.04 runner 当前处于 GitHub Preview，因此固定
使用版本标签，不使用 `ubuntu-latest` 隐式迁移。

## 退出通知

PTY 自然退出后，daemon 先排空当前可读输出并写入 ring buffer，再取得 exit code、保存 cwd、
环境变量快照和 metadata。随后编码现有 `SessionExitedPayload`，发送给 active writer 和该
Session 的全部 read-only 客户端，最后释放连接所有权。客户端断开只清理失效 fd，不影响
metadata 收尾。

协议编号和 payload 不变，本修复恢复已经记录但 daemon 漏发的协议行为，不引入协议版本
变化。回归测试覆盖非零退出码、writer 通知、read-only 广播和断开客户端错误路径。

## 验证

- 本地执行 fmt、clippy、workspace tests 和打包脚本定向测试。
- 在对应构建环境检查 tar/deb/rpm 内容、checksum、版本及 GLIBC 符号版本。
- 将 EL9 原生产物安装到 `ssh test` 的 Rocky Linux 9.7，复测自然退出、恢复上下文、writer
  takeover、read-only、日志、信号、管理命令、SSH 绕过和安装卸载。
- 发现的问题先修复并重复相关回归，再更新 M50 审计、里程碑、TODO、CHANGELOG 和下一任务。

## 边界

本任务不新增架构、CPU 类型、musl 包、自托管 runner、签名或 GitHub Release。既有
`v0.1.0` artifact 保留为历史证据，新修复进入 `master`，由后续版本发布。

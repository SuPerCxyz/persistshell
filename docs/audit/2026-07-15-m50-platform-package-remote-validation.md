# M50 平台包与 test 全功能验证审计

## 结论

Ubuntu 26.04 与 RHEL 9 x86_64 打包流程已在本机、全新 `rockylinux:9` 容器和 GitHub hosted
runner 完成验证。GitHub 下载产物已独立复核，RHEL 9 RPM 已安装到 test 的 Rocky Linux 9.7，
文档功能和 CI artifact 部署均完成黑盒验证。

## 修复项

- shell 自然退出未发送 `SessionExited`，attach 客户端持续等待。
- read-only stdout 使用裸字节而非 IPC frame，且未收到退出通知。
- daemon 忽略的 SIGINT 等 disposition 被 PTY child 继承，Ctrl+C 无法终止前台进程。
- attach 主 I/O 路径未写 Session 日志，日志文件始终为空。
- Idle GC 移除 runtime 后未关闭 SQLite metadata，留下 running 幽灵记录。
- attach ring replay 未接入，stdin 反而被写入 ring；大 replay 也未按 64 KiB frame 分片。
- CLI help 漏列 lock、unlock 和 log export。
- `PERSIST_PACKAGE_DIST` 使用绝对路径时，RPM topdir 被错误拼接，导致产物定位失败。
- RHEL 9 build 命令的局部双引号使 workflow YAML 无法解析，GitHub 拒绝 dispatch。
- 原 Package workflow 使用 Ubuntu 二进制生成 RPM，产物要求 `GLIBC_2.39`，不能在 RHEL 9
  运行；checksum 验证还错误地从仓库根目录解析 basename。

## 本地和容器证据

- fmt、Clippy warnings deny、workspace 全测试通过；一个既有 zsh PTY 用例 ignored。
- Ubuntu 平台 tarball/deb 的 checksum、内容、版本通过。
- 全新 Rocky Linux 9 容器原生构建 tarball 与 `persistshell-0.1.0-1.el9.x86_64.rpm`；
  `persist` 和 `persistd` 最高 GLIBC 均为 2.34，checksum 和包内容通过。
- 使用绝对 `PERSIST_PACKAGE_DIST` 重建 RHEL 9 tarball/RPM 通过，确认自定义输出目录可用。
- bash/zsh/fish compatibility matrix、三种 completion 与两份 man page 通过。
- 100/500/1000 Session benchmark 的创建/列表/关闭耗时分别为：
  `2432/117/1899ms`、`67710/1056/8069ms`、`201673/343/14699ms`。

## GitHub hosted runner 证据

- `master` 提交 `3cbe15d` 的 Package run `29464594020` 通过。
- Ubuntu 26.04 job `87514972356` 与 RHEL 9 job `87514972366` 分别通过全部构建、校验和上传步骤。
- artifact `persistshell-ubuntu-26.04-x86_64`（ID `8362474107`）digest 为
  `sha256:b8a499e2185df3b8829ab5c4c5a4e22bf52727ee7ec62df25902ec439f4172af`。
- artifact `persistshell-rhel9-x86_64`（ID `8362478750`）digest 为
  `sha256:cf4028fbde09042481703e08fd3f197c51401d8e9d8a627975ef949f0772a362`。
- 两个 artifact 已下载；四个相邻 checksum、版本、架构、License、man page、三种 completion、
  用户文档和包内容均独立复核通过。EL9 二进制最高 GLIBC 为 2.34。

## test Rocky Linux 9.7 证据

- 安装 `.el9` RPM 后，daemon 状态、配置、doctor、new/list、rename、note、tag、pin、
  lock/unlock、ps/stats、snapshot、metrics、detach、close、kill 和权限检查通过。
- `exit 7`、Ctrl+D、Closed Session cwd/受限环境恢复、客户端断开后继续运行通过。
- 双 writer takeover、旧 writer 撤销、read-only 实时输出和退出通知通过。
- Ctrl+C、less、vim、top 基础交互通过。
- Session log 查看、搜索、导出、head/tail replay 和 0600 权限通过。
- install、重复安装拒绝、uninstall、purge、daemon start/stop 和 `PERSIST_DISABLE` 通过。
- 自定义 socket、配置错误、Idle GC 的 pinned/locked 排除和 metadata closed 收尾通过。
- ring replay 对 writer/read-only 生效；有效 SSH agent socket 被继承，普通文件被过滤。
- 非交互 SSH、scp、sftp、rsync 和 git-over-SSH 通过；本机没有 ansible，未执行真实 ansible。
- GitHub run `29464594020` 下载的 `.el9` RPM 已用 `--replacepkgs` 安装；自然退出通知、日志和
  replay、Closed Session cwd 冷恢复、runtime 释放、daemon 状态及 0700/0600 权限通过。

## 已知限制

- 快速 `cd; exit` 可能在下一次 `/proc` 采样前退出并保留旧 cwd，已记录 KI-0007。
- replay `--speed` 与 `--follow` 尚未改变行为，已记录 KI-0008 和 TODO。
- test 未安装 zsh/fish，Rocky 远程只验证 bash；本地 Ubuntu compatibility matrix 已验证三者。
- GitHub `ubuntu-26.04` 当前是 Preview runner；run `29464594020` 已通过，但 Preview 不提供稳定 SLA。

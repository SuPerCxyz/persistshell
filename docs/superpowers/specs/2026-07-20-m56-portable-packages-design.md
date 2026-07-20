# M56 通用 Linux 多架构发布包设计

## 文档状态

- 日期：2026-07-20
- 里程碑：M56
- 状态：已确认
- 决策：`docs/adr/ADR-0008-portable-linux-packages.md`

## 目标

- 每个 CPU 架构只构建一套 glibc 2.28 基线二进制。
- 生成通用 RPM、DEB 和 tar.xz，不绑定具体发行版版本。
- 支持 x86_64 与 ARM64，不支持 32 位架构。
- 安装包保持小体积，并由 CI 阻止体积回归。
- 在不支持 pidfd 的 EL8 级内核上保持 daemon/Holder 生命周期正确。

## 产物

每个版本固定生成六个主产物及各自 SHA-256：

```text
persistshell-<version>-1.x86_64.rpm
persistshell-<version>-1.aarch64.rpm
persistshell_<version>_amd64.deb
persistshell_<version>_arm64.deb
persistshell-v<version>-linux-x86_64.tar.xz
persistshell-v<version>-linux-aarch64.tar.xz
```

RPM release 不带 `.el8`、`.el9` 等 suffix。包 metadata 声明最低 libc ABI，但不声明未经
验证的发行版认证。三个程序始终来自同一次对应架构的 build。

## 构建

正式 build 在 EL8 glibc 2.28 用户空间执行。x86_64 和 aarch64 使用原生 runner；不以
QEMU 产物作为正式发布包。构建后逐个 ELF 检查架构、动态链接器和最高 GLIBC symbol，
任何二进制超过 2.28 都失败。

使用专用 release profile执行 LTO、单 codegen unit 和 strip。保留 bundled SQLite，
不使用 UPX。打包前后的大小都记录到 job summary：

- RPM/DEB：每个不超过 3 MiB。
- tar.xz：每个不超过 3.5 MiB。

## 旧内核进程监控

首选 `pidfd_open` 和 `poll`。收到 `ENOSYS`、`EINVAL` 或 `EPERM` 时降级：

1. 读取 `/proc/<pid>/stat` 的 start time，连同 PID 固定进程身份。
2. `has_exited` 再读 stat；文件消失或 start time 改变即视为退出。
3. 读取失败但进程仍可能存在时返回错误，不猜测为退出。
4. 显式 shutdown 使用 timerfd 驱动的短间隔、有界身份检查，最长三秒。
5. startup 继续由 inotify socket 事件和 `Child::try_wait` 决定，不要求 pidfd。

fallback 不改变 Holder crash 后 metadata 标记 lost、显式 stop 或 daemon 重连语义。

## 验证矩阵

构建验证覆盖两种架构。安装和运行 smoke 至少覆盖：

- RPM：Rocky 8、9、10。
- DEB：Ubuntu 22.04、24.04、26.04。
- 扩展兼容：CentOS Stream 9/10、Debian 11/12/13。

每个环境验证 checksum、包查询、安装、ELF 架构、`--version`。代表性最低/最高版本还要
验证 foreground daemon、socket、Session new/list/close、Holder 启动和清理。容器无法
证明真实旧内核行为，pidfd fallback 必须有强制 fault path 的自动化测试。

## 完成标准

- 六类产物可重复生成，名称和 metadata 不绑定发行版。
- x86_64/aarch64 ABI 与体积门禁通过。
- pidfd 和 procfs fallback 的单元、错误及集成测试通过。
- 本地 workspace 验证通过，GitHub workflow 语法和脚本验证通过。
- 用户安装、CI、限制、TODO、里程碑、变更日志和下一任务同步更新。

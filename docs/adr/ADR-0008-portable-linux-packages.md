# ADR-0008：通用 Linux 多架构发布包

状态：Accepted

日期：2026-07-20

## 背景

现有 workflow 分别在 Ubuntu 26.04 和 Rocky Linux 9 构建 x86_64 包，产物绑定发行版
名称，且不支持 ARM64。为每个发行版版本重复编译会扩大产物和维护矩阵；完全静态 musl
会改变 libc、DNS/NSS 和 PTY 运行边界，也不一定得到更小的包。

PersistShell 当前还直接依赖 Linux 5.3 引入的 `pidfd_open`。EL8 的基线内核早于该版本，
仅降低 glibc ABI 不能构成真实运行兼容。

## 决策

发布二进制统一使用 GNU libc 动态链接，以 glibc 2.28 为最低 ABI。每个架构只编译一次，
再封装为不包含发行版名称和版本的 RPM、DEB 与 tar.xz：

- 架构仅支持 `x86_64` 和 `aarch64`。
- RPM 面向 RHEL、Rocky、AlmaLinux 8/9/10 与 CentOS Stream 9/10。
- DEB 面向 Ubuntu 22.04/24.04/26.04 与 Debian 11/12/13。
- 不支持 i686、ARMv7、EL7 和更旧用户空间。

RPM/DEB 的架构字段由构建目标映射生成，不在脚本中固定为 x86_64/amd64。产物名称只包含
项目、版本、包格式和架构。tarball 使用 xz，校验文件继续使用 SHA-256。

release 构建启用 LTO、单 codegen unit 和 symbol stripping；`opt-level` 在实测后选择。
bundled SQLite 保留，以避免发行版 SQLite ABI 差异。不使用 UPX，不发布包含两个架构的
fat package，也不把调试符号放入安装包。

体积硬门禁：

- 单个 RPM 或 DEB 不超过 3 MiB。
- 单个 tar.xz 不超过 3.5 MiB。

`pidfd_open` 不可用或被运行环境禁止时，使用 PID 加 `/proc/<pid>/stat` start time 验证
进程身份，防止 PID 复用误判。显式 shutdown 的 fallback 等待必须有固定期限，不允许
无限或 sleep polling。

## 选择理由

glibc 2.28 覆盖仍受维护的主流企业 Linux，同时比按发行版分别构建更容易审计。动态链接
加 strip 能维持小包；同一架构复用二进制可避免 RPM 与 DEB 行为漂移。原生 ARM64 runner
可以避免 QEMU 隐藏架构问题。

## 被拒绝方案

- 每个发行版和版本单独构建：重复产物多，测试成本高，实际 ABI 差异有限。
- musl 全静态通用包：libc 行为边界扩大，包不一定更小。
- 单一多架构包：RPM/DEB 生态不采用该分发方式，下载体积翻倍。
- 继续要求 pidfd：会让 EL8 安装成功但 daemon 无法启动。

## 风险与回滚

glibc 兼容不等于厂商认证；无真实 RHEL runner 时只能声明 RHEL ABI compatible。容器共享
宿主内核，旧内核 fallback 还需专项测试。ARM64 hosted runner 可用性变化时允许改用
self-hosted ARM64，但不得用未执行验证的交叉产物替代正式包。

回滚时可恢复原 RHEL 9/Ubuntu 26.04 workflow；不得删除已经发布的通用包或伪造兼容状态。

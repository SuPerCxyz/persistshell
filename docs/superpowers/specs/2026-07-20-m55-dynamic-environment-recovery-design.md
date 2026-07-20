# M55 Closed Session 动态环境恢复设计

## 文档状态

- 日期：2026-07-20
- 里程碑：M55
- 状态：已确认
- 前置：M14 Closed Session 恢复、M53 PTY Holder、M54 最终 cwd side channel

## 背景

M14 仅从 `/proc/<shell-pid>/environ` 尽力保存 `TERM`、`COLORTERM`、`LANG` 和
`LC_*`。该接口不能可靠反映 Shell 运行期间的动态 `export`，Shell 退出后也不可读。
M54 已建立不修改用户 rc 的 bash/zsh/fish hook、私有原子状态文件、Holder 离线退出
上下文和 metadata-first retire 顺序。M55 在该机制上恢复安全的动态导出环境。

## 目标

- 恢复受支持 Shell 中允许持久化的动态 `export`、修改和 `unset`。
- 普通变量使用 Session 快照，连接变量使用当前 attach 客户端环境。
- 不持久化凭据、系统身份、基础路径或 PersistShell 内部变量。
- 用户配置完整性优先；失败不阻塞 prompt、退出或 Closed attach。
- daemon 离线和 metadata/retire 崩溃窗口保持可重试、幂等。

## 非目标

- 不恢复未导出的 Shell 局部变量、函数、alias、option、job 或进程内存。
- 不保存完整环境，不提供绕过敏感变量禁区的开关。
- 不修改用户 dotfile，不包装 `export`、`unset`、`cd` 或 `exit`。
- 不实现 Holder/system reboot 恢复，也不进入 M56 时间化 replay。

## 方案选择

采用扩展 M54 state helper 的方案。helper 进程直接读取从 Shell 继承的 exported
environment，在 Rust 公共策略层过滤后，与 cwd 一起原子提交。

拒绝以下方案：

- Shell 执行 `env -0` 或原生命令再传给 helper：跨 Shell 转义和用户覆盖边界更复杂。
- daemon 采样 `/proc/<pid>/environ`：不能可靠捕获动态 export/unset，且保留退出竞态。
- 保存完整环境再做恢复时过滤：敏感值已经落盘，不满足安全要求。

## 架构

```text
Shell prompt / normal exit
    -> hidden state helper reads inherited exported environment
    -> shared policy filters set/unset and atomically writes envelope v2
    -> Holder validates and retains opaque bounded environment context
    -> SessionExited or GetExitContext
    -> daemon revalidates policy and commits metadata first
    -> RetireExited
    -> Closed attach merges saved and current connection environment
    -> Holder starts a new Shell runtime
```

Holder 不决定变量是否安全，只验证版本、identity、sequence、编码和容量。策略由共享
Rust 模块定义，helper 写入和 daemon 恢复必须调用同一实现。恢复入口仍需重新过滤，
不能信任状态文件、IPC 或 metadata。

daemon 在创建 runtime 时把已验证 include 规则和 policy fingerprint 作为私有 hook
上下文传给 helper。Shell 可以修改继承变量，因此 helper 只把它作为采集提示；硬禁区
始终由编译进二进制的共享策略执行，daemon 按当前配置再次过滤后才允许持久化或恢复。
任何被篡改的 hook 上下文最多导致本次环境降级，不能扩大恢复权限。

## 变量分类与优先级

### 保存的普通变量

默认只保存 `LANG`、`LC_*` 和用户显式扩展的名称。Closed attach 时，保存的普通变量
优先于 daemon 环境；随后加载的用户 rc 仍可按用户配置修改它们。

用户扩展支持精确名称和仅尾部 `*` 的前缀规则，不支持任意 glob 或正则。配置不能
放宽硬禁区。

### 当前连接变量

`TERM`、`COLORTERM`、`SSH_AUTH_SOCK`、`SSH_CLIENT`、`SSH_CONNECTION`、`SSH_TTY`、
`DISPLAY` 和 `WAYLAND_DISPLAY` 不进入持久快照。它们由每次 attach 的 `persist`
客户端发送，并覆盖旧 Session 或 daemon 环境。

### 重新计算和永久禁止变量

`HOME`、`USER`、`LOGNAME`、`SHELL`、`PATH`、`PWD`、`OLDPWD`、`SHLVL`、`XDG_*`
和 `PERSIST_*` 不允许由快照控制。身份和基础环境由当前系统、daemon 和 Shell 启动
流程重新计算。

名称包含凭据语义的变量永久拒绝，包括 `TOKEN`、`SECRET`、`PASSWORD`、`PASSWD`、
`CREDENTIAL`、`API_KEY`、`ACCESS_KEY`、`PRIVATE_KEY` 和 `COOKIE`。匹配不区分
大小写，用户扩展不能覆盖。实现前的计划必须列出精确匹配规则和误报测试。

策略不检查或猜测值是否“看起来像密钥”，避免误删普通数据。用户显式 include 一个名称
即授权其值写入私有状态和 metadata；用户文档必须警告不要 include 可能承载凭据的自定义
变量。硬保证仅覆盖内置禁区和名称匹配规则。

## 精确 unset 语义

环境上下文包含 `env_set` 和 `env_unset`。helper 将当前允许集合与上一次可信集合求差：

- 当前存在的变量写入 `env_set`。
- 上次已跟踪、当前不存在的变量写入 `env_unset`。
- 用户配置中的精确名称即使首次缺失，也可写入 `env_unset`。
- 前缀规则只对曾观察到的名称生成 unset，不枚举不存在的变量空间。

恢复时先应用 set，再显式删除 unset。禁区和系统基础变量不记录 set 或 unset。

## 数据模型与边界

Shell state envelope 升级到 v2，保留 M54 identity、sequence 和 cwd，并增加：

```text
environment:
  format_version
  policy_version
  policy_fingerprint
  env_set
  env_unset
  capture_status
```

metadata 继续使用现有 `env_snapshot` 列，内容改为可辨识的版本化 JSON。旧版普通 JSON
map 作为 M14 legacy 格式读取，不需要新增数据库列。新格式写入前必须通过统一策略验证。

硬上限：

- 变量数 128。
- 名称 128 bytes。
- 单值 8 KiB。
- 环境快照编码后 64 KiB。
- 完整 state envelope 72 KiB。
- cwd 4096 bytes。
- public/Holder control frame 继续受 1 MiB 总上限约束。

只接受有效 UTF-8，拒绝 NUL、重复名称、set/unset 重叠、非法变量名、未知字段、错误版本、
非单调 sequence 和不匹配 identity。序列化必须确定性排序，以便 fingerprint、测试和审计。

## 配置

新增恢复环境配置，仅包含：

```toml
[recovery.environment]
include = ["EDITOR", "MY_PROJECT_*"]
max_variables = 128
max_bytes = "64KiB"
```

用户只能收紧资源上限，不能超过硬上限。非法规则导致配置校验失败，不静默放宽策略。
policy fingerprint 由有效规则、硬禁区版本和边界共同计算，不包含环境值。

## Public IPC 与当前连接环境

public protocol minor 递增，Attach 增加可选、有界的 `connection_env`。新版 decoder 接受
旧 4-byte Attach；旧 daemon 继续忽略它不理解的尾部，不破坏 attach。daemon 只接受连接
变量固定 allowlist，并再次验证 socket、路径、UTF-8 和容量。

旧客户端不发送连接环境时，daemon 明确降级到当前 M14 行为。当前 SSH agent socket 必须
来自 attach 客户端进程，不能使用长期 daemon 启动时继承的旧 socket。

## Holder 协议兼容

Holder protocol 增加环境退出上下文能力。首次控制握手使用旧 minor 可理解的基线帧，
随后协商最高共同 minor/capability；新 daemon 可以接管旧 Holder，但只获得 cwd，
新 Holder 也必须接受旧 daemon 的基线操作。协商前不得发送新字段。

若共同能力不含环境上下文，Session 继续按 M54/M14 降级，不能丢失 runtime、cwd、
exit code 或既有 metadata。升级不得要求终止仍在运行的旧 Holder Session。

## 恢复合并顺序

Closed attach 创建 runtime 时按固定顺序构造环境：

1. 从当前系统和 daemon 建立身份与基础环境。
2. 重新过滤并应用保存的 `env_set`。
3. 应用允许变量的 `env_unset`。
4. 使用 attach 客户端的当前连接变量覆盖。
5. 注入本次 runtime 的 PersistShell 私有变量和 hook。
6. exec Shell；用户 rc 最后按 Shell 原生顺序生效。

不得让 Session 快照覆盖当前连接变量、基础身份或内部 hook 变量。

## 失败处理

环境采集采用全有或全无语义，不写部分快照。非法 UTF-8、数量/容量超限、策略错误或
序列化失败时：

- cwd 仍独立更新。
- helper 通过安全读取现有 identity/sequence envelope 保留上一次可信环境；没有可信值时
  标记 unavailable。
- helper 返回不影响 prompt/退出的降级结果。
- 日志和状态只记录原因码、策略版本和变量数量，不记录名称或值。

Holder 读取失败时仍保留真实 exit code 和可用 cwd。daemon 必须先成功写 metadata，
再发送 `RetireExited`；metadata 失败、daemon 崩溃或 retire 失败均可在重启后幂等重试。

配置收紧或 fingerprint 改变后，恢复入口按当前策略重新过滤旧快照。被取消授权的变量
不再注入；快照不因策略变化而获得新增权限。

## 测试与验收

### 单元和协议

- 策略分类、大小写敏感变量识别、精确/前缀 include 和硬禁区不可覆盖。
- set/unset 差异、确定性排序、policy fingerprint 和配置收紧。
- envelope v1/v2、legacy metadata、未知字段、identity、sequence、UTF-8 和容量边界。
- public Attach 与 Holder capability 的新旧双向兼容和损坏帧。

### Shell 与进程集成

- bash、zsh、fish 的 export、修改、unset、快速 exit 和空行 Ctrl+D。
- 用户 rc、prompt/history hook、已有 Bash EXIT trap 和嵌套 Shell 降级。
- daemon 离线退出、重连查询、metadata 前崩溃和 metadata 后 retire 前崩溃。
- 旧 Holder 被新 daemon 接管时 runtime 不退出，环境能力明确降级。

### 端到端与安全

- Closed attach 恢复普通变量和 unset，当前连接变量覆盖旧值。
- 第二台电脑 takeover 使用当前 SSH agent、终端和显示上下文并保持可写。
- 状态文件、metadata、日志、错误和 diagnostics 中不出现被禁止的敏感值。
- symlink、owner/mode、损坏文件、恶意 metadata 和配置绕过均被拒绝。
- 环境失败不阻塞退出、cwd 恢复或 attach。

### 性能与平台

- 1000 次状态提交，与 M54 cwd-only 基线比较并记录 mean/max。
- 测试最小、典型和 64 KiB 上限快照，确认无 busy loop、sleep polling 或无限 buffer。
- Ubuntu 26.04 与 RHEL 9 原生包构建。
- Rocky `ssh test` 使用隔离 XDG 验证真实 Shell、离线 daemon、跨连接恢复和包安装。

## 安全日志规则

禁止记录环境名称和值、序列化正文、metadata JSON 或状态文件正文。允许记录 Session ID、
runtime identity 的非敏感标识、策略版本、fingerprint、变量数量、字节数和固定原因码。
测试夹具中的敏感值必须是虚构值，并验证它们不会出现在任何持久文件或命令输出。

## 回滚

关闭 M55 环境写入后，M54 cwd side channel 继续工作。daemon 可停止读取 v2 环境部分，
保留 metadata 列和旧快照但不应用。协议能力协商降级到 M54，不能因此 retire 或关闭
活动 Holder runtime。回滚不批量删除用户 metadata 或未知运行时文件。

## 后续实施边界

实施计划应依次覆盖共享策略和 envelope、public connection context、Holder capability、
metadata-first 恢复、三 Shell 兼容、故障/性能/平台验证和文档收尾。M55 完成前不得将
M56 replay 或完整 Shell 状态序列化并入本任务。

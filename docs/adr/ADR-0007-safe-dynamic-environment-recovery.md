# ADR-0007：安全的动态环境恢复

状态：Accepted

日期：2026-07-20

## 背景

M14 的 `/proc/<pid>/environ` 白名单快照不能可靠观察 Shell 运行期间的动态
`export` 和 `unset`。保存完整环境会把 token、password、agent、显示连接和内部变量
写入磁盘，违反 PersistShell 的安全边界。M54 已提供不修改用户配置的 Shell hook、
原子状态文件和 daemon 离线退出对账，可作为受限环境提交通道。

变量值本身没有可靠的通用凭据识别方式。安全保证基于保守默认白名单和不可绕过的名称
禁区；用户显式 include 自定义名称时，视为授权持久化其值。

## 决策

扩展 M54 隐藏 state helper。helper 从自身继承的 exported environment 读取当前状态，
在 Rust 共享策略层完成过滤和 set/unset 差异计算，再与 cwd 一起原子提交版本化状态。

默认只持久化 `LANG` 和 `LC_*`。用户可配置精确名称或仅尾部 `*` 的前缀规则，但不能
绕过以下边界：

- 当前连接变量不持久化，由 attach 客户端提供并优先：终端、SSH、agent 和显示变量。
- 系统身份、基础路径、`XDG_*` 和 `PERSIST_*` 不允许快照控制。
- 名称匹配 token、secret、password、passwd、credential、api_key、access_key、
  private_key、cookie 等凭据标记时永久拒绝。

允许变量同时记录 `env_set` 和 `env_unset`。Closed attach 按“系统基础环境、保存快照、
unset、当前连接变量、PersistShell 内部变量、用户 rc”顺序合并。

Holder 只验证和保留有界退出上下文，不决定变量策略。daemon 在 metadata 写入和恢复时
再次执行同一策略，metadata 成功后才 retire Holder runtime。

## 数据与兼容

- Shell state 使用 envelope v2，环境快照最多 128 个变量、64 KiB，完整 envelope
  最多 72 KiB。
- 名称最多 128 bytes，单值最多 8 KiB，只接受有效 UTF-8。
- metadata 复用 `env_snapshot` 列，使用可区分 M14 legacy map 的版本化 JSON。
- public Attach 增加可选当前连接环境，旧 4-byte payload 继续兼容。
- Holder 使用基线握手协商环境 capability，新 daemon 接管旧 Holder 时降级但不结束 runtime。
- policy fingerprint 不含环境值；配置收紧后恢复入口重新过滤旧快照。

## 失败语义

环境采集不允许部分成功。非法、超限或策略失败时更新 cwd，但保留上一次可信环境；
没有可信值时标记 unavailable。失败不能改变 prompt、命令退出状态、Shell 退出或 attach。
任何日志、错误和 diagnostics 都不能包含环境名称或值。

## 选择理由

- helper 继承的环境能可靠反映动态 exported variables，不依赖 `/proc` 采样。
- Rust 单一策略实现避免 bash、zsh、fish 各自过滤产生差异。
- 默认白名单加用户扩展比完整环境安全，也比纯用户白名单提供更合理的默认行为。
- 当前连接变量单独传递可避免复用长期 daemon 的旧 SSH agent 或终端环境。
- 精确 unset 使恢复语义不会因 daemon 继承环境而重新引入已删除变量。

## 被拒绝方案

- 保存完整环境后恢复时过滤：敏感值已经落盘。
- Shell 执行 `env -0` 再解析：增加跨 Shell 转义和用户覆盖风险。
- daemon 继续采样 `/proc/environ`：无法可靠捕获动态修改和最终退出状态。
- 只允许用户白名单：默认行为不足，不能延续 M14 已有语言环境恢复。
- 仅使用敏感名称 denylist：未知命名的凭据仍可能被保存。

## 影响

正面影响：Closed Session 可安全恢复允许的动态 export/unset，跨电脑 attach 使用当前
连接上下文，daemon 离线退出仍可恢复。

负面影响：每次 prompt/退出提交的数据增大；用户必须显式扩展非默认变量；未导出变量和
硬禁区变量不会恢复。

风险：敏感名称规则可能误报，配置前缀可能扩大采集范围，旧 Holder 不具备环境能力。
通过不可绕过禁区、恢复时二次过滤、容量边界、capability 降级和安全测试控制。

## 回滚

停止写入和应用 v2 环境部分即可回退到 M14/M54；cwd、Holder runtime 和旧 metadata
继续可用。回滚不得关闭活动 Session 或批量删除用户数据。

## 后续任务

- [x] 编写 M55 分阶段实施计划。
- [x] 实现共享策略、envelope v2 和配置校验。
- [x] 实现 current connection context 与 Holder capability。
- [x] 实现 metadata-first 环境恢复和精确 unset。
- [x] 完成跨 Shell、安全、故障、性能、平台和远程验证。

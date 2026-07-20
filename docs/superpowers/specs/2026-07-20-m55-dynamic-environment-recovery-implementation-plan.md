# M55 Closed Session 动态环境恢复实施计划

> 依据：
> `docs/superpowers/specs/2026-07-20-m55-dynamic-environment-recovery-design.md`
> 和 `docs/adr/ADR-0007-safe-dynamic-environment-recovery.md`

状态：已确认

**目标：** 在默认不保存已识别敏感变量、不破坏用户配置的前提下，让 Closed Session
恢复允许的动态 `export`、修改和 `unset`，并使用当前 attach 客户端的连接环境。

**方法：** 扩展 M54 state helper 和版本化 envelope；共享 Rust 策略在采集、metadata
提交和恢复三处执行。public IPC 传递当前连接上下文，Holder 通过向后兼容 capability
传递退出环境，daemon 保持 metadata-first/retire 顺序。

**质量等级：** Level 2 TDD + Level 3 Review + Level 4 Completion Verification。

## 阶段总览

- [x] 阶段 1：共享环境策略、配置与 envelope v2
- [x] 阶段 2：隐藏 helper 动态环境采集
- [x] 阶段 3：Public Attach 当前连接上下文
- [x] 阶段 4：PTY 环境 set/unset 启动模型
- [x] 阶段 5：Holder capability 与退出环境上下文
- [x] 阶段 6：Daemon metadata-first 动态环境恢复
- [x] 阶段 7：跨 Shell、兼容、故障和安全验证
- [x] 阶段 8：性能、平台包、远程验证和文档收尾

## 全局硬边界

- M55 只恢复 exported environment，不恢复局部变量、函数、alias、option、job 或进程。
- 默认只保存 `LANG`、`LC_*`；用户 include 不能绕过敏感、身份、基础和内部变量禁区。
- 当前连接变量不持久化，必须来自当前 `persist` 客户端。
- 环境失败不阻塞 cwd 提交、prompt、exit、Ctrl+D 或 attach。
- 不记录环境名称、值、JSON 正文或状态文件正文。
- 新 daemon 必须接管旧 Holder；能力降级不能终止 runtime。
- 每阶段只修改列出的 ownership 范围；发现 shared contract 变化先更新设计和 ADR。
- 不提前实现 M56，不创建 release、tag、commit、push 或非测试部署。

## 主要文件边界

- `crates/persist-core/src/config/{sections,tests}.rs`：恢复环境配置。
- `crates/persist-core/src/shell_state.rs` 及子模块：策略、snapshot、envelope v2、安全 I/O。
- `crates/persist-cli/src/shell_state.rs`：隐藏 helper 采集。
- `crates/persist-cli/src/attach.rs`：当前连接环境。
- `crates/persist-ipc/src/protocol.rs`：public Attach 可选扩展。
- `crates/persist-ipc/src/holder/`：capability 与退出环境编码。
- `crates/persist-pty/src/lib.rs`：启动环境 set/unset。
- `crates/persist-holder/src/runtime/state.rs`：验证并保留环境上下文。
- `crates/persistd/src/{server,shell_history}.rs`：策略注入、提交、合并和恢复。
- `crates/persist-metadata/src/store.rs`：legacy/v2 快照持久化验证。
- `docs/protocol/`、用户/架构/已知限制文档：协议和行为事实来源。

---

## 阶段 1：共享环境策略、配置与 envelope v2

### 目标

建立唯一安全策略和版本化数据模型，后续模块只能调用该实现，不自行复制过滤规则。

### 步骤 1：先写配置和策略失败测试

- [x] 在 `persist-core` 写测试覆盖默认 `LANG`/`LC_*`、精确 include、尾部前缀 include。
- [x] 覆盖非法变量名、任意 glob/regex、重复规则和超过硬上限的配置。
- [x] 覆盖大小写不敏感的敏感标记、`XDG_*`、`PERSIST_*`、身份/基础变量永久拒绝。
- [x] 证明用户 include 不能覆盖硬禁区。

运行：

```bash
cargo test -p persist-core config::tests::recovery_environment
cargo test -p persist-core shell_state::environment_policy
```

预期：新增测试先因配置和策略类型不存在而失败。

### 步骤 2：实现配置和共享策略

- [x] 新增 `RecoveryEnvironmentConfig`，默认无用户扩展，资源值只能收紧。
- [x] 新增确定性的变量分类、include 解析、硬禁区和 policy fingerprint。
- [x] 错误只报告规则索引/原因码，不回显环境值。
- [x] 更新默认配置、示例配置和配置验证测试。

### 步骤 3：先写 envelope v2 和差异测试

- [x] 覆盖 legacy cwd-only v1、v2 set/unset round-trip 和确定性排序。
- [x] 覆盖上一次集合到当前集合的 set/update/unset 差异。
- [x] 覆盖重复名、set/unset 重叠、未知字段、错误版本、fingerprint 和 sequence。
- [x] 覆盖 128/129 变量、128/129-byte 名称、8 KiB/超限值、64 KiB 环境和 72 KiB envelope。
- [x] 覆盖非法 UTF-8、NUL、symlink、owner/mode 和原子替换失败保留。

### 步骤 4：实现 snapshot 和 envelope v2

- [x] 增加版本化 `EnvironmentSnapshot`、capture status 和全有或全无验证。
- [x] v1 继续读取为 cwd-only；v2 严格拒绝未知或不一致字段。
- [x] 安全读取旧 envelope 后计算 unset；失败时保留最后可信环境并允许 cwd 更新。
- [x] 将状态文件硬上限提升到 72 KiB，保持 dirfd、`O_NOFOLLOW`、`0700`/`0600`。

### 步骤 5：运行阶段门禁

```bash
cargo fmt --all -- --check
cargo test -p persist-core
cargo clippy -p persist-core --all-targets -- -D warnings
git diff --check
```

验收：策略只有一个实现；legacy v1 可读；任何非法或超限环境都不产生部分快照。

阶段结果（2026-07-20）：配置/策略和 envelope 测试先因缺少 API 编译失败；实现后
`persist-core` 72 passed、1 个手工 benchmark ignored，workspace fmt、全 targets Clippy、
全量测试和 `git diff --check` 均通过。额外修复了硬禁区前缀配置、跨架构 fingerprint、
矛盾容量配置和 JSON 重复环境名静默覆盖。

---

## 阶段 2：隐藏 helper 动态环境采集

### 目标

让 helper 从自身继承环境采集 exported variables，同时保持无 stdout、无日志值和 cwd
独立提交语义。

### 步骤 1：先写 helper 红灯测试

- [x] 动态 export、修改和 unset 产生正确 set/unset。
- [x] `TERM`、SSH/显示、基础、敏感和 `PERSIST_*` 不进入快照。
- [x] 用户精确/前缀 include 生效，篡改 hook include 不能绕过 daemon 二次过滤边界。
- [x] 非 UTF-8、129 变量、超限值和序列化失败保留旧环境但更新 cwd。
- [x] stdin cwd、exit status、stdout/stderr 和无配置兼容行为保持 M54 契约。

运行：

```bash
cargo test -p persist-cli shell_state
```

预期：测试先因 helper 不支持环境采集而失败。

### 步骤 2：实现 helper 采集

- [x] helper 使用 `vars_os` 读取继承环境，不执行 Shell 命令，不读取用户 rc。
- [x] 从已验证策略提示构造候选集合，再调用 `persist-core` 硬策略。
- [x] 安全读取上次 envelope，生成 set/unset；失败不回显名称或值。
- [x] 原子写入 cwd 和环境；环境失败时携带旧可信环境或 unavailable status。

### 步骤 3：扩展 ShellLaunch 私有策略上下文

- [x] daemon 将有效 include、边界和 fingerprint 注入本次 runtime 的 `PERSIST_*` 私有上下文。
- [x] bash/zsh/fish 继续调用同一 `__state-commit`，不增加 Shell 原生环境解析。
- [x] 已有 Bash EXIT trap、用户 prompt/history hook 和用户主动删除 hook 继续按 M54 降级。
- [x] 私有策略上下文不进入恢复快照。

### 步骤 4：运行跨 Shell 定向门禁

```bash
cargo test -p persist-cli shell_state
cargo test -p persistd shell_history_tests
cargo clippy -p persist-cli -p persistd --all-targets -- -D warnings
```

验收：三种 Shell 的 hook 文本不解析环境、不修改 dotfile，helper 失败不改变用户命令语义。

阶段结果（2026-07-20）：helper 红灯测试先因缺少动态环境采集失败；实现后使用
`vars_os` 和共享策略采集 exported environment，并在策略提示缺失、被篡改、非 UTF-8
或容量超限时更新 cwd、保留同策略的最后可信环境或标记 unavailable。Bash、Zsh、Fish
继续复用 M54 hook，私有提示由硬禁区排除。CLI helper 7 项、Shell history 10 项定向测试、
全 targets Clippy、workspace 全量测试和 `git diff --check` 均通过。

---

## 阶段 3：Public Attach 当前连接上下文

### 目标

由当前 `persist` 客户端提供终端、SSH agent 和显示上下文，旧客户端/daemon 继续 attach。

### 步骤 1：先写 public IPC 兼容测试

- [x] 旧 4-byte Attach 与新版可选 connection context 均 round-trip。
- [x] 新 decoder 拒绝未知变量、重复项、非法 UTF-8、NUL、超限名称/值/数量和尾随垃圾。
- [x] 旧 payload 在新 daemon 降级；新 payload 在旧 decoder 下只使用 Session ID。
- [x] protocol minor 协商和 capability 不因同 major 的旧客户端失败。

### 步骤 2：扩展协议与客户端

- [x] public protocol minor 递增，Attach 增加明确长度的可选 connection context。
- [x] CLI 仅采集固定 allowlist，不读取任意环境。
- [x] `SSH_AUTH_SOCK` 必须是当前用户拥有的有效 Unix socket；普通文件、symlink 和失效路径拒绝。
- [x] 终端/SSH/显示值执行独立容量和控制字符验证。
- [x] 所有 attach 入口复用同一 encoder，不能只修改交互主路径。

### 步骤 3：daemon 接收并隔离连接上下文

- [x] connection context 只绑定当前 attach 请求，不写 metadata、状态文件或日志。
- [x] running Session attach 不改变现有 runtime 环境。
- [x] closed Session restore 才把上下文交给恢复合并器。
- [x] 缺失或单项非法时采用明确错误/降级规则，不读取 daemon 的旧 SSH agent。

### 步骤 4：运行阶段门禁

```bash
cargo test -p persist-ipc protocol::tests
cargo test -p persist-cli attach
cargo test -p persistd --bin persistd
cargo clippy -p persist-ipc -p persist-cli -p persistd --all-targets -- -D warnings
```

验收：新旧 4-byte Attach 保持可用；当前连接变量不持久化且不会污染 running Session。

阶段结果（2026-07-20）：public protocol 从 `0.1` 递增为 `0.2`，保留 legacy 4-byte
Session ID，并增加严格长度前缀的可选 connection context。CLI 主 attach、交互列表入口和
benchmark 共用 minor-aware encoder；Client 与 Daemon 双重验证 agent socket。Daemon 只在
Closed restore 边界传递请求级上下文，Running Session、metadata、状态文件和日志均不修改。
IPC 19 项、CLI attach 11 项、persistd 134 passed/1 个既有 ignored、全 targets Clippy、
workspace 全量测试和格式检查均通过。原计划的 `--lib` 已按实际 bin target 修正。

---

## 阶段 4：PTY 环境 set/unset 启动模型

### 目标

在 exec 前明确应用保存变量、unset、当前连接和内部变量，避免继承 daemon 环境破坏语义。

### 步骤 1：先写 PTY 环境层测试

- [x] 保存普通变量覆盖 daemon 同名值。
- [x] `env_unset` 从 child 环境中明确删除变量。
- [x] 当前连接变量覆盖保存值。
- [x] `HOME`/`USER`/`SHELL`/`PATH` 和 `PERSIST_*` 不能被快照覆盖或删除。
- [x] 重复项、set/unset 冲突、NUL 和超限输入在 fork 前失败。

### 步骤 2：引入结构化启动环境

- [x] 用命名结构替代裸 `&[(String, String)]`，分别持有 saved set、saved unset、
  connection override 和 private environment。
- [x] 在 parent 侧完成全部验证和 CString 转换，child 侧只执行有界 setenv/unsetenv。
- [x] 保持 signal disposition、cwd、job control 和 exec 错误管道现有顺序。
- [x] 测试确认用户 rc 在 exec 后仍可覆盖普通变量。

### 步骤 3：同步 Holder Create 边界

- [x] Holder Create 请求携带已经合并前的结构化启动上下文，而不是不分来源的扁平列表。
- [x] 对旧 capability 保持当前扁平环境兼容路径。
- [x] 所有来源共享 128 变量和 control frame 总边界，禁止重复扩容。

### 步骤 4：运行阶段门禁

```bash
cargo test -p persist-pty
cargo test -p persist-ipc holder::tests::create
cargo test -p persist-holder runtime
cargo clippy -p persist-pty -p persist-ipc -p persist-holder --all-targets -- -D warnings
```

验收：unset 是真实 child 环境删除，不依赖空字符串；系统和内部变量始终由当前 runtime 控制。

阶段结果（2026-07-20）：新增共享 `ShellLaunchEnvironment`，在 fork 前统一验证
saved set/unset、connection 和 private 四层的名称、值、冲突、128 项与 64 KiB 边界。
PTY child 按固定顺序执行真实 setenv/unsetenv，测试同时证明 Shell 启动代码仍可覆盖普通变量。
Holder Create request 改为结构化字段，minor 1 codec 明确降级为 set 层，v2 codec 精确保留四层，
但在 Stage 5 capability 前不发送。首次全量测试发现 legacy TERM/COLORTERM 被错误归入 saved
source，修复为只从旧快照接收 LANG/LC_*；随后 workspace 全量测试、fmt、五 crate 全 targets
Clippy 和 `git diff --check` 均通过。一次全量并发中的 Ctrl+D 超时经单测、reconciliation 全组
和最终全量重跑均通过，未复现。

---

## 阶段 5：Holder capability 与退出环境上下文

### 目标

让新 Holder 保留有界环境快照，同时确保新 daemon 可接管只支持 minor 1 的旧 Holder。

### 步骤 1：先写协议版本和能力红灯测试

- [x] 新 frame decoder 接受同 major 的 minor 1 和 minor 2，拒绝 0、未来 minor 和错误 major。
- [x] 控制连接先以 minor 1 完成现有握手，再以 minor 2 frame 执行 capability 请求。
- [x] 新 Holder 返回 environment-exit-context capability；旧 Holder 断开控制连接后 runtime
  保持，daemon 以同 instance/minor 1 重连并缓存 legacy。
- [x] capability 结果绑定 Holder instance，instance 变化后必须重新探测。
- [x] data connection 使用控制面选定 minor，不能自行猜测。

### 步骤 2：实现有状态协议版本

- [x] frame encode/decode 接受显式 negotiated minor，不再全局硬编码单一 minor。
- [x] 新增固定编号 CapabilityRequest/Response，严格 request ID、nonce、instance 和尾部验证。
- [x] daemon 只探测一次；探测断线必须走现有有界重连，不 busy loop 或 sleep polling。
- [x] legacy cache 只存在内存，不能把旧 Holder 永久标记 lost。

### 步骤 3：先写环境退出上下文测试

- [x] SessionExited/GetExitContext 在 capability 存在时携带版本化环境 snapshot。
- [x] legacy minor 1 保持原 cwd-only 精确 wire 格式。
- [x] 拒绝超限、未知格式、重复名、set/unset 冲突和尾随数据。
- [x] daemon 离线后环境上下文仍保留到 RetireExited。

### 步骤 4：扩展 Holder runtime

- [x] Shell 退出后读取 envelope v2，保留 cwd 和环境；失败仍保留 exit code/cwd。
- [x] Holder 不解释 allowlist，只执行共享结构验证、identity、sequence 和容量检查。
- [x] environment capability 不存在时不发送新字段。
- [x] Retire 后清理经过 identity 验证的状态文件和有界退出上下文。

### 步骤 5：运行阶段门禁

```bash
cargo test -p persist-ipc holder
cargo test -p persist-holder
cargo test -p persistd holder
cargo clippy -p persist-ipc -p persist-holder -p persistd --all-targets -- -D warnings
```

验收：旧 Holder 的 PTY 在能力探测和重连期间持续运行；新 Holder 离线退出环境可查询并 retire。

阶段结果：minor 1 基线握手和 minor 2 capability 协商已实现；旧 Holder 在探测断线后由
daemon 使用同一 instance 重新接管，runtime 不退出。新协议精确保留 envelope v2 环境，
legacy wire 保持 cwd-only。定向测试、全 workspace 测试、fmt 和全 targets Clippy 均通过。

---

## 阶段 6：Daemon metadata-first 动态环境恢复

### 目标

统一在线事件、周期对账、启动对账和 Closed attach 的环境提交与恢复，不升级数据库 schema。

### 步骤 1：先写 metadata legacy/v2 测试

- [x] M14 legacy JSON map 可读并只产生原白名单值。
- [x] v2 set/unset、policy version/fingerprint 和 capture status round-trip。
- [x] 恶意 JSON、超限字段、禁区变量和当前策略收紧在写入/读取两端拒绝或过滤。
- [x] metadata 环境失败不覆盖上一可信快照，cwd/exit code 仍可提交。

### 步骤 2：实现版本化 metadata codec

- [x] `env_snapshot` 列保持不变，新增独立 codec 模块，禁止 server 内散落 JSON 解析。
- [x] legacy map 只读兼容；新写入统一使用 v2。
- [x] codec 输出确定性、有界，不记录原始解析错误正文。
- [x] schema version 不变化，并增加旧数据库打开/恢复测试。

### 步骤 3：先写统一 finalizer 红灯测试

- [x] 在线 SessionExited、周期 reconcile、启动 reconcile 都优先 side-channel 环境。
- [x] metadata 成功前不得 RetireExited。
- [x] metadata 前崩溃保留 Holder 环境；metadata 后 retire 前崩溃幂等清理。
- [x] 无 environment capability、状态损坏和策略不匹配回退 M14/上次可信值。

### 步骤 4：扩展统一退出完成结构

- [x] `ExitContext`/`RecoveryContext` 使用结构化环境，不在 daemon 内传递裸 JSON。
- [x] helper/Holder 数据进入 finalizer 前按当前 config 重新过滤。
- [x] env capture unavailable 只回退环境，不降低有效最终 cwd。
- [x] metadata 更新成功后才调用 retire；失败原因不含名称或值。

### 步骤 5：先写 Closed attach 合并测试

- [x] 基础环境不能被 snapshot set/unset 控制。
- [x] 普通保存变量覆盖 daemon 环境，保存 unset 显式删除。
- [x] 当前 connection context 覆盖旧终端/SSH/显示值。
- [x] private hook 变量最后注入，用户 rc 随后可按原生语义修改普通变量。
- [x] 第二次 close/attach 使用上一 runtime 的新快照，不累积已 unset 值。

### 步骤 6：实现恢复合并器

- [x] 新增纯函数构造结构化 PTY launch environment，固定六层优先级。
- [x] public attach handler 不再直接 decode legacy map 后传裸 Vec。
- [x] restore 成功后 metadata reopen 和 Holder binding 保持现有事务顺序。
- [x] 当前连接 context 生命周期限定为本次请求，完成或失败后释放。

### 步骤 7：运行阶段门禁

```bash
cargo test -p persist-metadata
cargo test -p persistd
cargo test -p persistd --test reconciliation
cargo clippy -p persist-metadata -p persistd --all-targets -- -D warnings
```

验收：三条退出路径共用一个 metadata-first finalizer；Closed attach 精确恢复 set/unset 和当前连接。

阶段结果：metadata schema 保持 v7，legacy map 只读兼容，新写入统一为确定性 v2。
在线事件、周期和启动对账均在 retire 前提交环境；无效环境保留旧值且不丢失 cwd/exit code。
真实 PTY 已验证 set、恢复、unset、再次恢复。阶段定向门禁、全 workspace、Clippy、fmt 和
diff 检查均通过。

---

## 阶段 7：跨 Shell、兼容、故障和安全验证

### 目标

用真实 PTY/进程证明用户配置完整性、跨版本接管、敏感值不落盘和所有降级路径。

### 步骤 1：扩展真实 Shell 测试

- [x] bash/zsh/fish：export、修改、unset 后正常 exit。
- [x] 快速 `export X=v; exit` 和空行 Ctrl+D。
- [x] 用户 rc、prompt、history、zsh hook、fish event 和已有 Bash EXIT trap 保持。
- [x] 嵌套/unsupported Shell、删除 hook、`exec` 和 SIGKILL 明确降级。
- [x] 非 UTF-8 与容量失败更新 cwd 但保留可信环境。

### 步骤 2：增加跨连接和兼容进程测试

- [x] client A 关闭 Session，client B 使用不同 TERM/agent/display 恢复并可写。
- [x] 普通 Session 变量来自 A，连接变量来自 B，禁区变量来自当前系统。
- [x] 新 daemon 探测旧 Holder 后控制重连，Shell PID、输出和 writer 数据面不丢失。
- [x] 新 Holder/新 daemon 使用 minor 2 环境上下文；旧客户端仍可 attach。
- [x] daemon 离线退出后，新 daemon 查询环境并恢复。

### 步骤 3：增加故障注入

- [x] helper 读取旧状态前、cwd 写入前、原子 rename 前后。
- [x] Holder 读取环境前后、SessionExited 发送失败、GetExitContext 重试。
- [x] daemon 在 metadata 前、metadata 后/retire 前、retire 响应丢失。
- [x] 配置在 capture 和 restore 之间收紧，旧变量不得恢复。
- [x] 所有故障可重复对账，无 busy loop、sleep polling、无限 buffer 或提前 retire。

### 步骤 4：敏感值泄漏扫描

- [x] 使用虚构 token/password/cookie/private key 值覆盖环境和恶意 metadata。
- [x] 检查 state、SQLite、Session log、daemon/Holder/client log、doctor 和错误输出。
- [x] 断言禁区值和名称不出现；只允许固定 reason code、数量、字节和 fingerprint。
- [x] 用户显式 include 的自定义名称按授权持久化，文档警告不得 include 凭据变量。
- [x] 测试失败输出也不能打印夹具敏感值。

### 步骤 5：编写 M55 专项脚本

新增 `scripts/test-dynamic-environment-recovery.sh`，顺序执行：

```text
policy/config/envelope
helper and three-shell compatibility
public current connection context
PTY set/unset
Holder minor 1/2 compatibility and offline retention
metadata-first crash windows
cross-client restore
sensitive leak scan
```

脚本必须 `set -euo pipefail`、使用隔离 XDG、设置超时并清理自身进程，不触碰已有 daemon。

### 步骤 6：运行阶段门禁

```bash
bash -n scripts/*.sh
scripts/test-dynamic-environment-recovery.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

验收：专项和全量测试全部退出 0；忽略项必须是已有且有明确原因，M55 场景不得 skip。

阶段结果：新增跨客户端连接覆盖和持久化泄漏扫描，专项脚本串联策略、三 Shell hook、
public/Holder 兼容、metadata 崩溃窗口、动态 set/unset 与敏感值验证。专项脚本、全
workspace、Clippy、fmt 和 diff 检查均通过；M55 场景无 skip。

---

## 阶段 8：性能、平台包、远程验证和文档收尾

### 目标

用本地基准、双平台原生包和 Rocky test 证明资源边界，再更新单一事实来源。

### 步骤 1：扩展稳定性能采样

- [x] benchmark 1000 次 cwd-only、典型 16 变量和接近 64 KiB 三组原子提交。
- [x] 记录 total/mean/max、文件大小和 helper 失败率，与 M54 cwd-only 基线比较。
- [x] 测量 Closed attach merge/restore 1000 次，不把编译时间或 Shell 启动混入纯函数基准。
- [x] 性能门禁基于同机相对回归和绝对资源上限，不宣称未测量的延迟。

### 步骤 2：本地发布包

```bash
cargo build --workspace --release
scripts/package-release.sh tarball
scripts/package-release.sh deb
```

- [x] Ubuntu 26.04 tar/deb 包含三个二进制、配置、man page、completion 和新文档。
- [x] 解包校验固定 Holder 路径、权限和 checksum。
- [x] release 二进制禁止测试 helper/Holder 路径覆盖。

### 步骤 3：RHEL 9 与 `ssh test`

- [x] 在 Rocky/RHEL 9 原生构建 tar/RPM，记录 rustc、OS、GLIBC 和 checksum。
- [x] 使用隔离 XDG 安装测试，不终止或替换主机既有 daemon。
- [x] 验证三 Shell export/unset、快速 exit、Ctrl+D、daemon SIGKILL、跨连接变量覆盖。
- [x] 验证旧 Holder capability 降级、显式 stop 清理和敏感值不落盘。
- [x] 测试后只清理本任务创建的隔离目录和进程。

### 步骤 4：写验证审计

新增 `docs/audit/YYYY-MM-DD-m55-dynamic-environment-recovery-validation.md`，记录：

- commit/worktree 状态、工具链、平台和包 checksum。
- 本地专项/全量/Clippy/性能结果。
- 远程命令、场景结果、既有 daemon PID 前后不变证据。
- 失败尝试和测试驱动修正，不删除失败证据。
- 未覆盖项和真实限制。

### 步骤 5：更新架构、用户和协议文档

- [x] 更新 Session/Process/Lifecycle、public IPC、Holder protocol 和 metadata。
- [x] 更新完整用户手册、配置、命令、Troubleshooting、man page。
- [x] 更新限制和已知问题，保留未导出变量、hook 冲突、SIGKILL、旧 Holder 降级等边界。
- [x] 明确当前连接变量不持久化，敏感禁区不可配置绕过。

### 步骤 6：关闭状态并切换下一任务

只有代码、错误、边界、测试、性能、双平台包和远程验证全部有证据后：

- [x] 勾选 M55 实现 TODO，将 M55 标记完成。
- [x] 更新 CHANGELOG、ADR 后续任务和本计划全部阶段。
- [x] `NEXT_TASK.md` 只指向 M56 设计，不提前实现。
- [x] 运行文档检索，不能把所有历史 `[ ]` 当成真实缺口。

### 步骤 7：最终门禁

```bash
rg -n "M55|动态环境|env_unset|connection_env|ADR-0007" \
    NEXT_TASK.md TODO.md MILESTONES.md CHANGELOG.md docs
bash -n scripts/*.sh
groff -man -Tutf8 docs/man/persist.1 >/dev/null
groff -man -Tutf8 docs/man/persistd.1 >/dev/null
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/test-dynamic-environment-recovery.sh
git diff --check
```

预期：全部退出 0；M55 不再是缺口，M56 仍明确未完成。

## 阶段检查点

每阶段结束必须记录：

```text
阶段：
修改文件：
红灯证据：
通过证据：
安全/兼容结论：
已知限制：
下一阶段：
```

一次只执行一个阶段。阶段门禁失败时保持当前阶段为唯一任务，不跳过到后续工作。

## 完成判定

M55 只有同时满足以下事实才能完成：

1. 三种 Shell 的动态 export/修改/unset 和配置保护有真实测试。
2. 普通变量、当前连接变量、基础环境和内部变量优先级符合设计。
3. 敏感禁区在 helper、daemon、metadata 恢复三处不可绕过且无持久泄漏。
4. 新 daemon 接管旧 Holder 不结束 PTY，新旧 public Attach 保持兼容。
5. daemon 离线和两个 metadata/retire 崩溃窗口幂等恢复。
6. workspace、专项、性能、双平台包和 Rocky test 有审计证据。
7. 用户与状态文档准确，`NEXT_TASK.md` 只指向 M56 设计。

未得到明确授权前不执行 git commit、push、tag、release 或非测试环境部署。

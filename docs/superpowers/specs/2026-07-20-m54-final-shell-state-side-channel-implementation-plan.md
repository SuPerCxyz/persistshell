# M54 最终 Shell 状态 Side Channel 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement
> this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标：** 让 bash、zsh、fish 在正常退出前通过私有原子状态文件提交最终 cwd，并让 Holder
在 daemon 离线和崩溃窗口中可靠保留该状态，直到 metadata 成功落盘。

**架构：** `persist __state-commit` 把 stdin 中的 `PWD` 写入版本化 JSON envelope；
`persist-holder` 在 Shell 退出边界安全读取并保留 cwd，通过实时事件或 `GetExitContext`
交给 daemon。daemon 先提交 SQLite，再显式 retire Holder runtime。

**技术栈：** Rust 1.80、Serde/serde_json、Linux `openat`/`renameat`/`fsync`、现有 Holder
二进制协议、bash/zsh/fish 临时 hook、SQLite metadata。

---

## 进度

- [x] 阶段 1：共享 Shell 状态 envelope 与安全文件 I/O
- [x] 阶段 2：Holder 退出上下文协议
- [x] 阶段 3：Holder 最终 cwd 读取、保留与 retire
- [x] 阶段 4：隐藏 helper 与 bash/zsh/fish hook
- [x] 阶段 5：Daemon metadata-first 关闭与离线对账
- [x] 阶段 6：故障注入、性能与平台验证
- [x] 阶段 7：文档、状态和完成门禁

## 约束

- 采用 Level 2 TDD：先写失败测试，再做最小实现。
- 不采集、写入或恢复动态环境变量，不进入 M55。
- 不改变公共 CLI、公共 IPC 或用户配置 schema。
- 不覆盖用户 Shell hook；冲突时宁可降级。
- 不引入 Tokio、轮询线程、每 Session 常驻进程或无限缓冲。
- 当前未授权 commit、push、tag、release 或远端部署；各阶段只保留可验证检查点。
- 共享协议、root dependency、Holder runtime 和 daemon 关闭顺序由主 Agent 串行修改。

## 文件职责

新增：

- `crates/persist-core/src/shell_state.rs`：envelope、identity、随机 incarnation、安全读写与清理。
- `crates/persist-cli/src/shell_state.rs`：隐藏 helper 的环境和 stdin 边界。
- `scripts/test-final-shell-state.sh`：真实 Shell、daemon 离线和崩溃恢复验收。

主要修改：

- `crates/persist-ipc/src/holder/{mod,session,inventory,operation}.rs`：状态身份、退出上下文和 retire。
- `crates/persist-holder/src/{runtime,server,server_handlers}.rs`：读取、保留和发布最终 cwd。
- `crates/persistd/src/holder/{client,mod,reconcile}.rs`：查询退出上下文和 metadata-first 编排。
- `crates/persistd/src/{shell_history,shell_history_tests,public_attach,server}.rs`：hook 注入和关闭顺序。
- `crates/persist-cli/src/{main,command,cli}.rs`：隐藏命令解析与执行。
- `docs/protocol/HOLDER_PROTOCOL.md` 及生命周期、Session、安全、用户和状态文档。

## 阶段 1：共享状态 envelope 与安全文件 I/O

**文件：**

- 新增：`crates/persist-core/src/shell_state.rs`
- 修改：`crates/persist-core/src/lib.rs`
- 修改：`crates/persist-core/Cargo.toml`

- [x] **步骤 1：先写 envelope 和身份校验失败测试**

测试必须构造合法状态，并覆盖未知字段、相对 cwd、4097-byte cwd、零 Session ID、错误
incarnation、倒退 sequence、损坏 JSON 和超过 8 KiB 文件。

```rust
let identity = ShellStateIdentity::for_test(7, [0x11; 16], root.join("7-state.json"));
let valid = ShellStateEnvelope::new(7, [0x11; 16], 3, "/srv/work".into())?;
assert_eq!(validate_envelope(&identity, 2, valid)?.cwd, "/srv/work");
assert!(decode_and_validate(&identity, 3, br#"{"version":1,"extra":1}"#).is_err());
assert!(ShellStateEnvelope::new(7, [0x11; 16], 4, "relative".into()).is_err());
```

- [x] **步骤 2：运行测试并确认先失败**

运行：`cargo test -p persist-core shell_state -- --nocapture`

预期：因 `shell_state` 模块和类型尚不存在而编译失败。

- [x] **步骤 3：定义固定数据模型和 JSON 边界**

```rust
pub const SHELL_STATE_VERSION: u32 = 1;
pub const MAX_SHELL_STATE_BYTES: usize = 8 * 1024;
pub const MAX_SHELL_CWD_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellStateEnvelope {
    pub version: u32,
    pub session_id: u32,
    pub incarnation: String,
    pub sequence: u64,
    pub cwd: String,
}
```

`ShellStateIdentity` 保存非零 Session ID、`[u8; 16]` incarnation 和绝对状态文件路径。
编码必须输出 32 位小写十六进制 incarnation；解码拒绝版本错误、NUL、相对路径、超限和
sequence 小于调用方已接受值的数据。

在 `persist-core/Cargo.toml` 使用已有 workspace `serde_json` 依赖，并从
`persist-core/src/lib.rs` 导出 `shell_state`；不得新增第三方随机数或文件系统 crate。

- [x] **步骤 4：补安全文件测试并确认失败**

使用 `tempfile` 覆盖目录 `0700`、文件 `0600`、原子替换保留旧状态、target symlink、
parent symlink、非普通文件、错误 owner/mode、临时文件冲突和安全清理。

```rust
write_atomic(&identity, &ShellStateEnvelope::new(7, identity.incarnation(), 1, "/a".into())?)?;
write_atomic(&identity, &ShellStateEnvelope::new(7, identity.incarnation(), 2, "/b".into())?)?;
assert_eq!(read_validated(&identity, 1)?.unwrap().cwd, "/b");
symlink(root.join("outside"), identity.path())?;
assert!(write_atomic(&identity, &state).is_err());
```

- [x] **步骤 5：实现 dirfd 相对的安全读写**

实现以下稳定接口：

```rust
pub fn create_identity(runtime_dir: &Path, session_id: u32) -> Result<ShellStateIdentity>;
pub fn identity_from_parts(
    session_id: u32,
    incarnation: [u8; 16],
    state_file: PathBuf,
) -> Result<ShellStateIdentity>;
pub fn write_atomic(identity: &ShellStateIdentity, state: &ShellStateEnvelope) -> Result<()>;
pub fn read_validated(
    identity: &ShellStateIdentity,
    minimum_sequence: u64,
) -> Result<Option<ShellStateEnvelope>>;
pub fn remove_validated(identity: &ShellStateIdentity) -> Result<()>;
```

`ShellStateIdentity` 提供 `session_id()`、`incarnation()`、`incarnation_hex()`、`path()` 和
`path_string()` 只读访问器。`create_identity` 从 `/dev/urandom` 完整读取 16 bytes，拒绝
全零结果，并使用 `<session_id>-<incarnation>.json` 作为单个 basename。

创建目录时验证当前 UID 和 mode `0700`。通过已验证目录 fd 执行 `openat`、`renameat` 和
`unlinkat`；临时文件使用 `O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC` 和 `0600`。写入后
依次 `fsync(temp_fd)`、`renameat`、`fsync(dir_fd)`。读取使用 `O_NOFOLLOW`，并在读取前后
`fstat` 验证普通文件、owner、mode 和不超过 8 KiB。

- [x] **步骤 6：运行 core 定向门禁**

运行：

```bash
cargo fmt --all -- --check
cargo test -p persist-core shell_state
cargo clippy -p persist-core --all-targets -- -D warnings
```

预期：全部退出 0；测试同时证明失败写入不会破坏上一份有效状态。

阶段结果（2026-07-20）：新增 13 个 Shell state 测试；`persist-core` 共 63 个测试通过，
fmt、全 targets Clippy、`umask 000` 权限复测和 diff check 均通过。实现拆分为 envelope、
状态文件流程、Linux syscall 和独立测试四个低于 300 行的文件。

## 阶段 2：Holder 退出上下文协议

**文件：**

- 修改：`crates/persist-ipc/src/holder/mod.rs`
- 修改：`crates/persist-ipc/src/holder/session.rs`
- 修改：`crates/persist-ipc/src/holder/inventory.rs`
- 修改：`crates/persist-ipc/src/holder/operation.rs`
- 修改：`crates/persist-ipc/src/holder/tests.rs`
- 修改：`docs/protocol/HOLDER_PROTOCOL.md`

- [x] **步骤 1：先写协议 round-trip 和损坏输入测试**

覆盖 Create 的状态路径/incarnation、Inventory 的 `exit_context_available`、带可选 cwd 的
SessionExited、GetExitContext 和 RetireExited。测试 4096/4097-byte cwd、running Session
错误携带 context、截断、尾随和非法 UTF-8。

```rust
let context = ExitContext {
    session_id: 7,
    exit_code: 23,
    cwd: Some("/srv/final".into()),
};
assert_eq!(
    decode_exit_context(&encode_exit_context(&context)?)?,
    context
);
```

- [x] **步骤 2：运行测试并确认协议测试失败**

运行：`cargo test -p persist-ipc holder -- --nocapture`

预期：缺少新增消息类型和字段导致编译失败。

- [x] **步骤 3：扩展协议类型并提升 minor 版本**

将 `HOLDER_PROTOCOL_MINOR` 从 `0` 提升为 `1`，分配：

```rust
GetExitContext = 0x001a,
GetExitContextResp = 0x001b,
RetireExited = 0x001c,
RetireExitedResp = 0x001d,
```

新增或修改的数据结构：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitContext {
    pub session_id: u32,
    pub exit_code: i32,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitContextResponse {
    pub session_id: u32,
    pub status: OperationStatus,
    pub exit_code: Option<i32>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HolderSessionEntry {
    pub session_id: u32,
    pub shell_pid: u32,
    pub state: HolderSessionState,
    pub exit_code: Option<i32>,
    pub created_at_ms: u64,
    pub last_active_at_ms: u64,
    pub ring_bytes: u32,
    pub writer_active: bool,
    pub log_state: HolderLogState,
    pub exit_context_available: bool,
}
```

`CreateSessionRequest` 增加必填 `state_file: String` 和 `state_incarnation: [u8; 16]`。
`SessionExitedEvent` 改为与 `ExitContext` 相同的有界字段。GetExitContext 请求复用非零
`OperationRequest`；成功响应必须有 exit code，可选 cwd，NotFound/Conflict/Rejected/Internal
响应不得携带 exit code 或 cwd。RetireExited 使用现有 `OperationResponse`。

- [x] **步骤 4：实现严格 encoder/decoder**

所有可选 cwd 使用现有 `put_optional_string`/`Reader::optional_string`，并验证绝对路径和
`MAX_HOLDER_PATH`。Inventory entry 追加单字节布尔值；只有 Exited entry 可以把
`exit_context_available` 设为 true。Create 的 state file 必须是绝对路径，incarnation
不能全零。

- [x] **步骤 5：同步协议文档并运行门禁**

在 Holder 协议文档记录 minor 1、消息编号、字段、长度、离线查询和 metadata-first retire。

运行：

```bash
cargo fmt --all -- --check
cargo test -p persist-ipc holder
cargo clippy -p persist-ipc --all-targets -- -D warnings
```

预期：全部退出 0；现有 public IPC 编号和测试不变。

阶段结果（2026-07-20）：Holder 私有协议提升到 minor 1，新增 Create 状态身份、
`exit_context_available`、可选最终 cwd、GetExitContext 和 RetireExited；5 个 M54 专项测试、
全部 44 个 `persist-ipc` 测试、fmt 和全 targets Clippy 均通过。

## 阶段 3：Holder 最终 cwd 读取、保留与 retire

**文件：**

- 修改：`crates/persist-holder/src/runtime.rs`
- 修改：`crates/persist-holder/src/server.rs`
- 修改：`crates/persist-holder/src/server_handlers.rs`
- 修改：`crates/persist-holder/tests/runtime.rs`
- 修改：`crates/persist-holder/tests/runtime_advanced.rs`

- [x] **步骤 1：先写 runtime 状态保留失败测试**

测试创建真实 Shell 状态文件，令 Shell 退出后断言：

```rust
assert_eq!(
    runtime.exit_context_response(session_id),
    ExitContextResponse {
        session_id,
        status: OperationStatus::Ok,
        exit_code: Some(17),
        cwd: Some("/srv/final".into()),
    },
);
assert!(runtime.inventory(one).entries[0].exit_context_available);
assert!(runtime.retire_exited(session_id).is_ok());
assert!(runtime.inventory(one).entries.is_empty());
```

另测损坏文件、错误 incarnation、symlink 和状态缺失仍返回 exit code，cwd 为 `None`。

- [x] **步骤 2：运行 Holder runtime 测试并确认失败**

运行：`cargo test -p persist-holder runtime -- --nocapture`

预期：Create 字段尚未进入 runtime，且不存在 exit context/retire API。

- [x] **步骤 3：让 Session 持有 identity 和最终上下文**

在 `Session` 增加：

```rust
state_identity: ShellStateIdentity,
last_state_sequence: u64,
final_cwd: Option<String>,
exit_code: Option<i32>,
```

`Runtime::create` 从 Create 请求构造并验证 identity。`reap_exited` 在 PTY 输出已排空后调用
`read_validated`，把有效 sequence/cwd 保存到 Session，再设置 exit code。读取错误转换为
无 cwd 的安全降级，不中止 child 回收。

`HolderServer::new` 从已加载配置取得
`config.paths.runtime_dir.join("session-state")`，并传给 `Runtime::new(state_dir)`。Create 的
`state_file.parent()` 必须与该目录逐组件相等，basename 必须与 Session ID/incarnation
生成规则一致；仅仅是绝对路径仍不足以通过 Holder 校验。

- [x] **步骤 4：拆分 close 与 retire 语义**

```rust
pub(crate) fn close(&mut self, session_id: u32) -> OperationResponse;
pub(crate) fn exit_context_response(&self, session_id: u32) -> ExitContextResponse;
pub(crate) fn retire_exited(&mut self, session_id: u32) -> OperationResponse;
```

`close` 只向 running Shell 发送 SIGHUP 并标记 closing；即使 Shell 已退出也不得删除 Session。
`retire_exited` 只接受已有 exit code 的 Session，安全删除状态文件后移除 runtime。Running
返回 Conflict，未知 ID 返回 NotFound。

为 `Session` 实现有序清理：先 `take()` 并 drop PTY，使现有 `PtySession` 有界回收 Shell，
随后调用 `remove_validated`。这样显式 ShutdownAll、正常 Holder 终止和 retire 都不会在
Shell EXIT hook 仍可能写入时提前删除状态文件；SIGKILL Holder 仍按 M53 的 lost 限制处理。

- [x] **步骤 5：发布上下文并实现控制请求**

`Runtime::reap_exited` 返回的每项包含完整 `ExitContext`、PTY fd 和 closing 标志；
`server::handle_signal` 将该上下文编码到数据连接和控制连接事件。
`server_handlers::handle_control` 增加 GetExitContext 和 RetireExited；公共 data connection
仍不接受这两种控制消息。

- [x] **步骤 6：验证 daemon 离线后的 Holder 查询**

真实进程测试流程：创建 Session，关闭控制连接，令 Shell `cd` 后退出，重新 claim 同一 Holder，
inventory 必须显示 exited/context available，GetExitContext 返回 cwd，Retire 后 entry 和
状态文件都消失。

- [x] **步骤 7：运行 Holder 阶段门禁**

运行：

```bash
cargo test -p persist-holder
cargo clippy -p persist-holder --all-targets -- -D warnings
```

预期：全部退出 0；M53 daemon 断线保活、输出 drain 和 attach 测试不回归。

阶段结果（2026-07-20）：Holder 严格绑定 runtime 状态目录，在 PTY 排空并回收 child 后读取
最终 cwd，保留 Exited Session 直到显式 retire。3 个状态安全单元测试和 11 个生命周期/真实
PTY 进程测试通过，覆盖 daemon 离线退出、重连查询、缺失/损坏/身份错误/symlink 降级及状态
文件清理；fmt、全 targets Clippy 和定向 diff check 均通过。

## 阶段 4：隐藏 helper 与 Shell hook

**文件：**

- 新增：`crates/persist-cli/src/shell_state.rs`
- 修改：`crates/persist-cli/src/main.rs`
- 修改：`crates/persist-cli/src/command.rs`
- 修改：`crates/persist-cli/src/cli.rs`
- 修改：`crates/persistd/src/shell_history.rs`
- 修改：`crates/persistd/src/shell_history_tests.rs`
- 修改：`crates/persistd/src/server.rs`

- [x] **步骤 1：先写隐藏命令解析和 helper 失败测试**

```rust
assert_eq!(
    parse_command(&["__state-commit".into()])?,
    Command::ShellStateCommit
);
assert!(parse_command(&["__state-commit".into(), "extra".into()]).is_err());
```

helper 测试使用注入的环境 map 和 reader，覆盖缺失变量、非法 Session ID/incarnation/sequence、
stdin 超限、相对状态路径，以及成功写入后 stdout/stderr 为空。

- [x] **步骤 2：运行 CLI 定向测试并确认失败**

运行：`cargo test -p persist-cli shell_state -- --nocapture`

预期：缺少 `ShellStateCommit` 和 helper 模块导致编译失败。

- [x] **步骤 3：实现无配置、无日志的隐藏 helper**

新增命令分支：

```rust
Command::ShellStateCommit => {
    crate::shell_state::commit_from_reader(std::env::vars(), io::stdin().lock())
}
```

该命令必须从 `command_uses_config` 集合排除，并和 history helper 一样禁用内部日志。环境变量
固定为 `PERSIST_STATE_FILE`、`PERSIST_STATE_SESSION_ID`、`PERSIST_STATE_INCARNATION` 和
`PERSIST_STATE_SEQUENCE`。helper 最多读取 4097 bytes，以便可靠识别超限，不读取其它环境。

- [x] **步骤 4：先写 Shell hook 文本和真实配置兼容测试**

测试断言：

- Bash hook 在用户 `.bashrc` 后注册，主 Shell 无 trap 时增加 EXIT 提交。
- Bash 已有 EXIT trap 时 `trap -p EXIT` 的定义和 marker 行为不变。
- Bash subshell 的 cwd 不覆盖主 Shell 状态。
- Zsh 用户 precmd/zshexit marker 和 PersistShell 状态均存在。
- Fish 用户 postexec/exit marker 和 PersistShell 状态均存在。

- [x] **步骤 5：在 daemon 创建一次 runtime identity**

`SessionManager` 增加 `runtime_dir: PathBuf`，生产构造传入 `config.paths.runtime_dir`，测试构造
传入各自隔离临时目录。`holder_create_request` 每次创建或冷恢复 runtime 时只调用一次：

```rust
let identity = create_identity(&self.runtime_dir, id)?;
let launch = shell_history::prepare(shell, id, &self.history_dir, &helper, &identity)?;
```

同一个 identity 同时进入 ShellLaunch 和 Holder Create 请求：

```rust
state_file: identity.path_string(),
state_incarnation: identity.incarnation(),
```

若创建 identity、私有目录或 hook 文件失败，本次 Session 创建必须失败，不能启动一个身份
不完整的 runtime。重建 Closed Session 时必须生成新 identity。

- [x] **步骤 6：扩展 ShellLaunch 状态环境**

`prepare` 接受 `&ShellStateIdentity`，并在三个 Shell 的启动环境中加入：

```rust
("PERSIST_STATE_FILE", identity.path_string()),
("PERSIST_STATE_SESSION_ID", identity.session_id().to_string()),
("PERSIST_STATE_INCARNATION", identity.incarnation_hex()),
("PERSIST_STATE_HELPER", helper.to_string_lossy().into_owned()),
```

unsupported Shell 仍不安装 hook，但 Holder 继续持有 identity 并使用 `/proc` 回退。

- [x] **步骤 7：实现 Bash 可降级 hook**

核心行为固定为：

```bash
__persist_state_sequence=0
__persist_state_shell_pid=${BASHPID:-$$}
__persist_state_commit() {
    [[ ${BASHPID:-$$} == "$__persist_state_shell_pid" ]] || return 0
    ((__persist_state_sequence+=1))
    printf '%s' "$PWD" | PERSIST_STATE_SEQUENCE=$__persist_state_sequence \
        "$PERSIST_STATE_HELPER" __state-commit >/dev/null 2>&1 || :
    return 0
}
```

把 `__persist_state_commit` 追加到现有 prompt capture。仅当 `trap -p EXIT` 为空时安装私有 EXIT
函数；该函数先保存 `$?`、提交状态，再返回原状态。已有 EXIT trap 时写入独立私有
`state-status` 文件，内容为 `exit-conflict`，不得改写现有 history `status` 文件，也不得解析
或替换 trap。

- [x] **步骤 8：实现 Zsh 和 Fish 可组合 hook**

Zsh 用 `add-zsh-hook precmd` 和 `add-zsh-hook zshexit` 注册两个私有函数；zshexit 保存原
退出状态。Fish 初始化时提交一次，并用不同私有函数监听 `fish_postexec` 和 `fish_exit`。
两者都递增私有 sequence，并通过 stdin 调用同一个 helper。

- [x] **步骤 9：运行三 Shell 定向门禁**

运行：

```bash
cargo test -p persist-cli
cargo test -p persistd shell_history -- --nocapture
cargo clippy -p persist-cli -p persistd --all-targets -- -D warnings
```

预期：bash 测试必过；安装了 zsh/fish 的环境执行真实测试，缺少可选 Shell 时测试明确跳过。
用户 marker、history filter 和原 EXIT trap 行为不变。

阶段结果（2026-07-20）：新增无配置、无日志的 `persist __state-commit`，严格校验四个受控
环境变量和 4096-byte cwd；daemon 每次 runtime 只生成一个 identity 并同时传给 Holder 与
Shell。Bash/Zsh/Fish 使用私有可组合 hook，真实测试证明已有 prompt、history filter、EXIT
trap、precmd/postexec 行为不变且 Bash subshell 不覆盖主状态。105 个 CLI 测试、9 个 Shell
兼容测试、identity 专项测试及双 crate Clippy 均通过。

## 阶段 5：Daemon metadata-first 关闭与离线对账

**文件：**

- 修改：`crates/persistd/src/holder/client.rs`
- 修改：`crates/persistd/src/holder/mod.rs`
- 修改：`crates/persistd/src/holder/reconcile.rs`
- 修改：`crates/persistd/src/holder/reconcile/tests.rs`
- 修改：`crates/persistd/src/public_attach.rs`
- 修改：`crates/persistd/src/server.rs`
- 修改：`crates/persist-metadata/src/store.rs`

- [x] **步骤 1：先写 Holder client 上下文与 retire 测试**

fake Holder 必须验证 GetExitContext 的 request ID/generation，返回有界 cwd，并确认
RetireExited 只有在显式调用后发送。

```rust
let context = runtime.exit_context(7)?;
assert_eq!(context.cwd.as_deref(), Some("/srv/final"));
runtime.retire_exited(7)?;
```

- [x] **步骤 2：实现 Holder client API**

```rust
pub(crate) fn exit_context(&self, session_id: u32) -> Result<ExitContext>;
pub(crate) fn close(&self, session_id: u32) -> Result<ExitContext>;
pub(crate) fn retire_exited(&self, session_id: u32) -> Result<()>;
```

`close` 发送 Close 后等待带上下文的 SessionExited，不再自动 refresh 并删除；retire 成功后
才 refresh inventory。事件队列验证必须接受新的 SessionExited payload，仍保持有界。

- [x] **步骤 3：先写 metadata cwd 对账测试**

修改 `reconcile_exited` 测试，验证新 cwd 覆盖旧 cwd、`None` 保留旧 cwd、重复执行幂等，
且不改变 `env_snapshot`。

```rust
store.reconcile_exited(1, 23, Some("/srv/final"), INSTANCE, 8)?;
assert_eq!(store.get_session(1)?.unwrap().cwd.as_deref(), Some("/srv/final"));
```

- [x] **步骤 4：扩展 metadata 对账而不升级 schema**

`MetadataStore::reconcile_exited` 增加 `cwd: Option<&str>`，SQL 使用
`cwd = COALESCE(?3, cwd)`；holder instance、generation 和 Session ID 参数顺延。不得新增
env 字段、migration 或 schema 版本。

- [x] **步骤 5：先写关闭顺序和失败重试测试**

测试用 fake metadata failure 证明：

1. 收到 ExitContext 后先调用 metadata。
2. metadata 失败时没有 RetireExited。
3. metadata 成功后才 retire。
4. retire 失败时 metadata 已 Closed，重启对账可再次 retire。

- [x] **步骤 6：建立统一退出完成结构**

```rust
struct ClosedSession {
    exit_code: i32,
    recovery_context: RecoveryContext,
    holder_retire: bool,
}

pub(crate) struct ProxyOutcome {
    pub(crate) exit_context: Option<ExitContext>,
}
```

side-channel cwd 与现有 `/proc` 缓存合并时必须优先；现有白名单 `env_snapshot` 只来自 M14
路径。public attach 将内部 cwd 留在 daemon，发给 public client 的 SessionExited 仍只有
Session ID 和 exit code。

`SessionManager` 把现有删除式 `close_session` 拆为：

```rust
fn prepare_close(
    &mut self,
    session_id: u32,
    observed_exit: Option<ExitContext>,
) -> Result<Option<ClosedSession>>;
fn finish_close(&mut self, session_id: u32);
```

`prepare_close` 优先使用 public proxy 已观察的 ExitContext；没有事件上下文时发送 Close 或
调用 GetExitContext。它可以合并 `/proc` 回退，但不得删除 session info、writer、只读连接、
活动时间或 recovery cache。只有 metadata 和 retire 均成功后才能调用 `finish_close`。

- [x] **步骤 7：重构统一 metadata-first finalizer**

将当前忽略错误的 `close_runtime_metadata` 改为返回 `Result<()>` 的单一 finalizer：

```rust
fn finalize_runtime_exit(
    session_id: u32,
    observed_exit: Option<ExitContext>,
    sessions: &Arc<Mutex<SessionManager>>,
    metadata: &Arc<Mutex<MetadataStore>>,
) -> Result<()>;
```

finalizer 取得 ClosedSession，执行 `close_session_with_context`，成功后调用
`holder.retire_exited`，最后才从 SessionManager 删除派生状态。所有自然退出、显式 close、
GC 和 public attach 退出路径调用该函数并传播或记录明确错误。

- [x] **步骤 8：修正启动对账顺序**

启动时对每个 `exit_context_available` 的 exited entry 调用 GetExitContext。查询失败时停止
本次接管并保留 Holder runtime；无 context 标志时允许 cwd 为 `None`。`reconcile_metadata`
收到已查询上下文，以 side-channel cwd 调用 `reconcile_exited`；只有 metadata 成功的
Session ID 才进入 retire 列表。orphan、instance conflict 和 metadata 缺失不得 retire。

- [x] **步骤 9：加入两个崩溃点**

保留现有 debug-only crash 注入模式，新增：

```rust
crash_at_test_point("after_exit_context_before_metadata");
crash_at_test_point("after_exit_metadata_before_retire");
```

进程测试验证第一个窗口重启后仍可查询 cwd，第二个窗口重启后 metadata 幂等且 Holder entry
最终被 retire。

- [x] **步骤 10：运行 daemon/metadata 阶段门禁**

运行：

```bash
cargo test -p persist-metadata
cargo test -p persistd holder -- --nocapture
cargo test -p persistd reconciliation -- --nocapture
cargo clippy -p persist-metadata -p persistd --all-targets -- -D warnings
```

预期：全部退出 0；Closed attach 继续使用最终 cwd 和原有允许环境快照。

阶段结果（2026-07-20）：Holder client 支持上下文查询和独立 retire，在线、周期和启动对账
统一执行 metadata-first finalizer；side-channel cwd 优先，缺失时使用既有 `/proc` 回退，env
白名单快照不变。两个真实 crash window 证明 metadata 前后崩溃均可幂等恢复 cwd 并最终清理
Holder 状态。38 个 metadata、132 个 persistd 单元、13 个 daemon/reconciliation 进程测试、
workspace check、fmt、Clippy 和 diff check 均通过。

## 阶段 6：故障注入、性能与平台验证

**文件：**

- 新增：`scripts/test-final-shell-state.sh`
- 修改：`scripts/test-holder-recovery.sh`
- 修改：`crates/persist-core/src/shell_state.rs`
- 修改：`crates/persistd/tests/persistd.rs`
- 修改：`crates/persistd/tests/reconciliation.rs`
- 新增：`docs/audit/2026-07-20-m54-final-shell-state-validation.md`

- [x] **步骤 1：增加稳定 Rust 性能采样**

在 shell_state 测试模块增加 ignored test，使用同一 private tmpfs 目录连续写入 1000 次不同
sequence，输出总耗时、平均和最大耗时；每次写入后最终读取 sequence 必须为 1000。

```rust
#[test]
#[ignore = "manual shell state commit benchmark"]
fn shell_state_commit_benchmark() {
    let samples = run_commit_samples(1_000).expect("benchmark samples");
    assert_eq!(samples.final_sequence, 1_000);
    eprintln!(
        "commits={},total_us={},mean_us={},max_us={}",
        samples.count, samples.total_us, samples.mean_us, samples.max_us
    );
}
```

- [x] **步骤 2：编写里程碑专项脚本**

脚本按顺序运行：

```bash
cargo test -p persist-core shell_state
cargo test -p persist-ipc holder
cargo test -p persist-holder --tests
cargo test -p persist-cli shell_state
cargo test -p persistd shell_history -- --nocapture
cargo test -p persistd --test persistd final_cwd
cargo test -p persistd --test reconciliation final_cwd
```

脚本使用 `set -euo pipefail`，任何 Shell 缺失只允许对应兼容测试明确 skip，daemon/Holder
故障测试不得跳过。`test-holder-recovery.sh` 增加 M54 离线退出和 metadata-first 场景。

- [x] **步骤 3：增加真实端到端场景**

至少实现以下测试名：

```text
quick_cd_exit_restores_final_cwd
ctrl_d_restores_final_cwd
daemon_offline_exit_preserves_final_cwd
metadata_failure_keeps_exited_holder_context
restart_after_metadata_before_retire_is_idempotent
invalid_state_file_falls_back_without_blocking_exit
```

每个测试使用隔离 XDG 目录、真实 `persistd`/Holder/PTY，验证 metadata cwd、状态文件生命周期、
Closed attach 后 `pwd`、原 exit code 和旧日志输出。

- [x] **步骤 4：运行本地专项、全量和性能门禁**

运行：

```bash
bash -n scripts/test-final-shell-state.sh scripts/test-holder-recovery.sh
scripts/test-final-shell-state.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p persist-core shell_state_commit_benchmark -- --ignored --nocapture
git diff --check
```

预期：所有正确性命令退出 0；性能输出写入审计文档，不预设虚构阈值。若相对 M53 的交互路径
出现明显回归，先定位 helper/file I/O，再决定是否调整实现。

- [x] **步骤 5：构建 Ubuntu 26.04 与 RHEL 9 原生包**

在对应原生构建环境执行：

```bash
cargo build --workspace --release --locked
PERSIST_PACKAGE_PLATFORM=ubuntu-26.04 PERSIST_PACKAGE_DIST=dist/m54-ubuntu \
    scripts/package-release.sh tarball deb
PERSIST_PACKAGE_PLATFORM=rhel-9 PERSIST_PACKAGE_DIST=dist/m54-rhel9 \
    scripts/package-release.sh tarball rpm
```

检查 tar/deb/RPM 中 `persist`、`persistd` 和固定路径 Holder，校验 SHA256；RHEL 9 三个二进制
最大 GLIBC symbol 不得超过 2.34。

- [x] **步骤 6：在 `ssh test` 使用隔离 XDG 做 RPM 验证**

安装本地构建的 RHEL 9 RPM 后，使用新的隔离 XDG 路径运行：

```text
快速 cd; exit -> metadata 最终 cwd -> Closed attach pwd
Ctrl+D -> 最终 cwd
SIGKILL daemon -> Shell exit -> daemon 重启 -> GetExitContext -> retire
已有 Bash EXIT trap -> marker 保留且 cwd 安全降级
损坏和 symlink 状态文件 -> 退出不阻塞且回退
```

显式 stop 后确认 Shell、Holder 和状态文件清理。测试前后记录并确认主机原有非测试 daemon
PID 未被终止；结束后删除隔离测试目录。

- [x] **步骤 7：写验证审计**

审计必须记录精确命令、日期、发行版、Rust 版本、包名、checksum、测试数量、性能数据、
远程场景结果、发现并修复的问题，以及未覆盖限制。不得只写“测试通过”。

阶段结果（2026-07-20）：新增 M54 专项与 Holder recovery 脚本、1000 次原子写采样及 6 个
命名端到端场景。本地 workspace 全量测试、Clippy、fmt、diff check 和专项脚本通过；Ubuntu
26.04 tar/deb、Rocky 9.7 tar/RPM 原生构建和 checksum 通过，RHEL 二进制最大 GLIBC 2.34。
`ssh test` 已安装 RPM，隔离 XDG 验证快速 exit、Closed attach、Ctrl+D、daemon 离线恢复、
EXIT trap 与清理，既有 PID 107264 未变化。完整证据见
`docs/audit/2026-07-20-m54-final-shell-state-validation.md`。

## 阶段 7：文档、状态和完成门禁

**文件：**

- 修改：`docs/INDEX.md`
- 修改：`docs/architecture/LIFECYCLE.md`
- 修改：`docs/architecture/SESSION_MODEL.md`
- 修改：`docs/architecture/PROCESS_MODEL.md`
- 修改：`docs/known/KNOWN_ISSUES.md`
- 修改：`docs/known/LIMITATIONS.md`
- 修改：`docs/user/USER_GUIDE.md`
- 修改：`docs/user/TROUBLESHOOTING.md`
- 修改：`docs/adr/ADR-0006-final-shell-state-side-channel.md`
- 修改：`TODO.md`
- 修改：`MILESTONES.md`
- 修改：`CHANGELOG.md`
- 修改：`NEXT_TASK.md`

- [x] **步骤 1：先按实际实现更新架构与协议引用**

生命周期和 Session 模型必须写明 Shell hook、原子文件、Holder 保留、GetExitContext、
metadata-first 和 retire 顺序。不得把 `/proc` 回退描述为强保证，不得声称动态环境已恢复。

- [x] **步骤 2：更新用户文档和限制**

用户手册说明正常 exit/Ctrl+D/快速 cd 的最终 cwd 恢复。Troubleshooting 说明已有 Bash EXIT
trap、SIGKILL、Shell exec、非 UTF-8 cwd 和 hook 被用户删除时会降级。KI-0007 只有在全部
验收完成后才能标记解决；限制文档保留真实例外。

- [x] **步骤 3：更新里程碑单一事实来源**

只有代码、测试、平台包和远程验证均完成后：

- 勾选 `TODO.md` 的 M54 核心缺口。
- 将 `MILESTONES.md` 的 M54 标为已完成。
- 在 `CHANGELOG.md` 记录最终 cwd、用户 hook 保护和 daemon 离线恢复。
- 勾选本计划全部阶段及 ADR 后续任务。
- 将 `NEXT_TASK.md` 的唯一任务更新为 M55 动态环境恢复设计，不提前实现 M55。

- [x] **步骤 4：运行文档与最终门禁**

运行：

```bash
rg -n "M54|最终 cwd|GetExitContext|KI-0007" \
    NEXT_TASK.md TODO.md MILESTONES.md CHANGELOG.md docs
bash -n scripts/*.sh
groff -man -Tutf8 docs/man/persist.1 >/dev/null
groff -man -Tutf8 docs/man/persistd.1 >/dev/null
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/test-final-shell-state.sh
git diff --check
```

预期：全部退出 0；状态文档不再把 M54 列为未实现，同时 M55/M56 仍明确未完成。

阶段结果（2026-07-20）：架构、用户、限制、已知问题和状态文档已与实际实现对齐；
脚本语法、两份 man page、workspace fmt/Clippy/全量测试、M54 专项脚本及 `git diff --check`
全部通过。`NEXT_TASK.md` 已只指向 M55 设计。

## 阶段检查点

每个阶段结束时记录：

```text
阶段：
修改文件：
失败测试证据：
通过测试证据：
已知限制：
下一阶段：
```

未得到明确授权前不执行 git commit、push、tag、release 或非测试环境部署。

## 计划完成判定

只有以下事实同时成立，M54 才能完成：

1. 三类 Shell 默认路径、Bash trap 冲突和 subshell 边界均有真实测试。
2. daemon 离线退出和两个 metadata/retire 崩溃窗口可恢复。
3. 状态文件权限、identity、sequence、大小、UTF-8 和 symlink 边界有错误路径测试。
4. side-channel cwd 优先，`/proc` 与 metadata 回退保持有效。
5. workspace、专项脚本、性能采样、双平台构建和 Rocky test 验证有审计证据。
6. 用户与状态文档准确，`NEXT_TASK.md` 只指向 M55 设计。

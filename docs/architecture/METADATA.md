# PersistShell Metadata Store

本文档描述 PersistShell Metadata Store 设计。

Metadata Store 用于保存 Session 的长期状态。

---

## 目标

Metadata Store 需要支持：

- Session 列表
- Session 状态恢复
- Session 查询
- Session GC
- Session 日志路径
- Exit code
- Tags/Notes 后续扩展
- Schema migration
- Doctor 诊断

---

## 非目标

Metadata Store 不保存完整 PTY 输出。

PTY 输出由：

- Ring Buffer
- Session Log

负责。

---

## 存储选择

Phase 1 推荐 SQLite。

原因：

- 单文件
- 成熟稳定
- 支持事务
- 支持索引
- 支持 migration
- 易于查询
- 适合本地 metadata

不推荐 JSON 作为主数据库。

原因：

- 并发差
- migration 差
- 查询差
- 容易损坏
- 大文件更新成本高

---

## 数据路径

建议：

```text
~/.local/share/persistshell/persist.db
```

或遵循 XDG：

```text
$XDG_DATA_HOME/persistshell/persist.db
```

---

## 权限

数据库文件：

```text
0600
```

数据库目录：

```text
0700
```

Daemon 启动时必须检查。

---

## Schema Version

必须有 schema version。

当前实现使用 schema v7。v7 在既有 `sessions` 表增加：

```text
holder_instance    32 位小写十六进制 Holder instance ID，可为空
holder_generation  最近一次完成对账的 Holder generation，可为空
```

旧数据库通过单向 SQLite migration 升级；重复打开不会重复迁移。`lost` 复用现有 `status` 字段，
不增加独立布尔列。

M55 不升级 schema。既有 `env_snapshot` 文本列继续使用，但读写由独立 codec 管理：

- M14 JSON string map 仅用于旧数据读取，并按当前恢复策略重新过滤。
- 新写入统一使用包含 format/policy version、fingerprint、set/unset 和 capture status 的 v2。
- 编码固定排序且最多 64 KiB；重复键、未知字段、冲突和非法名称拒绝。
- 在线退出、周期对账和启动对账先原子提交 exit code/cwd/environment，再 retire Holder。
- 环境不可用或不符合当前策略时使用 `COALESCE` 保留上一可信快照，cwd 和 exit code 仍提交。

例如：

```sql
CREATE TABLE schema_version (
    version INTEGER NOT NULL
);
```

或使用 migration 表：

```sql
CREATE TABLE migrations (
    id TEXT PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
```

---

## Session 表

示例字段：

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    display_id INTEGER,
    name TEXT NOT NULL,
    owner_uid INTEGER NOT NULL,
    owner_username TEXT NOT NULL,
    hostname TEXT,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_active_at INTEGER,
    last_attached_at INTEGER,
    last_detached_at INTEGER,
    shell_pid INTEGER,
    shell_path TEXT,
    foreground_pid INTEGER,
    foreground_cmd TEXT,
    cwd TEXT,
    rows INTEGER,
    cols INTEGER,
    term TEXT,
    source_ip TEXT,
    source_tty TEXT,
    client_count INTEGER DEFAULT 0,
    log_path TEXT,
    ring_buffer_size INTEGER,
    exit_code INTEGER,
    exit_time INTEGER,
    pinned INTEGER DEFAULT 0,
    archived INTEGER DEFAULT 0,
    updated_at INTEGER NOT NULL
);
```

Phase 1 可减少字段，但应预留 migration。

---

## Tags 表

Phase 2：

```sql
CREATE TABLE session_tags (
    session_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY(session_id, tag)
);
```

---

## Notes 表

Phase 2：

```sql
CREATE TABLE session_notes (
    session_id TEXT PRIMARY KEY,
    note TEXT,
    updated_at INTEGER NOT NULL
);
```

---

## Events 表

可选。

记录 Session 生命周期事件：

```sql
CREATE TABLE session_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    detail TEXT
);
```

事件示例：

- created
- attached
- detached
- closed
- killed
- renamed
- log_rotated
- gc

---

## 事务

Session 创建必须事务化。

例如：

```text
create metadata
create log path
create ring buffer
create pty
update status
```

需要定义失败回滚策略。

---

## Metadata 与 Runtime State

Metadata 是持久状态。

M53 后，Holder inventory 是活动 PTY runtime 的事实来源，SQLite 是长期 Session 记录的事实来源。
daemon 必须在开放 public socket 前取得 generation 稳定的完整 Holder snapshot 并完成以下幂等
对账：

| Metadata | Holder | 结果 |
|---|---|---|
| active | Running，同 instance 或未绑定 | 更新为 `running` 并记录 instance/generation |
| active | Exited，同 instance 或未绑定 | 更新为 `closed`，保存 exit code，再清退 Holder 项 |
| active | 缺失 | 更新为 `lost` |
| 任意记录 | 不同 Holder instance | 活动记录更新为 `lost`，runtime 按 orphan 隔离 |
| 缺失 | 任意 runtime | 标记为 orphan，不允许 public attach |

`closed` 与 `lost` 都会出现在 Session 列表中，但只有 `closed` 可以执行冷恢复。orphan 只用于当前
daemon 的隔离集合，不会伪造 metadata 记录；新 Session ID 必须跳过 orphan 占用的 ID。

运行期间 daemon 每个 Dashboard 采样周期刷新 inventory 并重复相同对账。对账 API 允许重复执行，
相同 instance/generation 不会创建重复 Session。

### 崩溃窗口

Session create 和 SQLite commit 无法组成跨进程事务，因此通过恢复规则闭合窗口：

- Holder create 后、metadata commit 前崩溃：runtime 成为不可 attach 的 orphan。
- metadata commit 后崩溃：新 daemon 按同一 Holder instance 恢复为 running。
- daemon 离线期间 Shell 退出：snapshot 恢复 closed、exit code 和 Holder 已写入的 Session 日志。
- 对账完成后再次崩溃：下一 daemon 重复对账，结果保持一致。

debug 构建的集成测试使用 `PERSIST_TEST_CRASH_POINT` 注入上述窗口；release 构建不执行该测试
控制逻辑。

Daemon 内存中还有 runtime state：

- PTY fd
- attached clients
- ring buffer memory
- write queues
- process handles

这些不能直接存进 SQLite。

Daemon 启动时需要将 metadata 与实际 runtime 对齐。

---

## Daemon 启动恢复

Daemon 启动后：

1. 打开数据库。
2. 读取未结束 Session。
3. 检查 pid 是否存在。
4. 检查是否属于当前用户。
5. 标记异常 Session 为 Zombie 或 Closed。
6. 清理 stale 状态。

Phase 1 因 daemon 崩溃无法恢复 PTY fd，因此应将旧 Running Session 标记为 Zombie 或 Closed，并明确说明限制。

---

## 并发访问

推荐只有 daemon 写 metadata。

Client 不直接写数据库。

Client 查询也通过 daemon。

这样避免锁竞争和权限复杂度。

---

## 索引

建议索引：

```sql
CREATE INDEX idx_sessions_owner ON sessions(owner_uid);
CREATE INDEX idx_sessions_status ON sessions(status);
CREATE INDEX idx_sessions_last_active ON sessions(last_active_at);
CREATE INDEX idx_sessions_created ON sessions(created_at);
```

---

## Migration

每次 schema 修改必须：

1. 新增 migration。
2. 更新 METADATA.md。
3. 更新 CHANGELOG.md。
4. 增加测试。
5. 确保旧数据库可升级。

---

## 损坏处理

如果数据库损坏：

- Daemon 不应静默失败。
- doctor 应明确提示。
- 提供备份和修复建议。
- 不应自动删除数据库。

---

## 备份

后续可以支持：

```bash
persist metadata backup
```

Phase 1 不必须。

---

## 隐私

Metadata 可能包含：

- cwd
- source ip
- command
- username
- hostname

这些不是高度敏感，但仍应限制权限。

---

## 测试

必须测试：

- 创建 Session metadata
- 更新状态
- 查询列表
- exit code
- migration
- 权限错误
- 数据库损坏模拟
- 并发请求
- daemon 重启后的状态修正

---

## 不变量

1. Client 不直接修改 metadata。
2. Metadata 权限必须安全。
3. Schema 必须有版本。
4. Running runtime state 不等于 metadata state。
5. Daemon 启动必须校验 metadata 与实际进程状态。

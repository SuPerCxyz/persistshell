# ADR-0001: Use Rust As The Primary Implementation Language

状态：

```text
Accepted
```

日期：

```text
2026-07-08
```

---

## 背景

PersistShell 是 Linux 基础设施软件，需要长期维护以下能力：

- PTY 管理
- `openpty`
- `fork` / `exec`
- `setsid`
- `TIOCSCTTY`
- process group
- signal
- Unix domain socket
- epoll
- 文件权限
- metadata store
- 高吞吐输出转发

项目需要在系统编程能力、性能、内存安全和长期可维护性之间取得平衡。

---

## 决策

PersistShell 的主开发语言确定为 Rust。

Phase 1 工程应使用 Cargo workspace，并围绕以下 crate 拆分：

- `persist-cli`
- `persistd`
- `persist-core`
- `persist-pty`
- `persist-ipc`
- `persist-metadata`

---

## 原因

Rust 适合 PersistShell 的核心约束：

- 能直接开发 Linux 系统软件。
- 性能足够接近 C/C++。
- 内存安全和类型系统有助于长期维护。
- 适合封装少量高风险 `unsafe` 边界。
- Cargo、rustfmt、clippy 和测试工具链成熟。
- 对 daemon、CLI、Unix socket、SQLite、日志和测试生态支持较好。

---

## 被考虑的方案

### Rust

优点：

- 系统编程能力强。
- 内存安全默认更好。
- 工具链统一。
- 长期维护性好。

缺点：

- 学习成本高于 Go。
- PTY、termios、signal 等底层代码仍需要谨慎处理。
- 少量 Linux syscall 可能需要 `unsafe` 或底层 crate。

### Go

优点：

- 开发速度快。
- CLI、daemon、并发和测试生态成熟。
- 部署简单。

缺点：

- 对 PTY、termios、process group、signal 等底层细节的控制需要更谨慎。
- runtime 和 goroutine 模型可能掩盖一些资源边界。
- 对极低开销、长期 daemon 的控制不如 Rust 直接。

### C/C++

优点：

- Linux 系统调用和 PTY 能力最直接。
- 性能和控制力强。

缺点：

- 内存安全风险高。
- 长期维护成本更高。
- 测试、依赖和构建一致性成本更高。

---

## 被拒绝的方案

Go 和 C/C++ 不作为 Phase 1 主语言。

Go 可作为后续辅助工具语言重新评估，但核心 daemon、PTY、IPC 和 session runtime 不使用 Go。

C/C++ 只在必要的极小底层 helper 或 FFI 边界中作为最后选择，不作为主实现语言。

---

## 影响

### 正面影响

- 提高系统软件长期安全性。
- 提供清晰模块边界和强类型约束。
- 方便建立 cargo workspace、单元测试、集成测试和 benchmark。
- 有利于把 unsafe Linux 边界集中封装。

### 负面影响

- 初期开发速度可能低于 Go。
- 需要为 unsafe 和底层 syscall 设定严格 review 规则。
- 需要选择和约束底层 crate，避免依赖膨胀。

### 风险

- `unsafe` 边界处理不当仍可能产生内存或 fd 生命周期问题。
- crate 选择不慎可能影响稳定性和 MSRV。
- async runtime 是否引入需要单独评估，不能默认增加复杂度。

---

## 回滚方案

如果 Rust 在 Phase 1 验证中证明不合适，需要新增 ADR 说明原因，并重新评估 Go 或 C/C++。

回滚前必须保留协议、状态机、测试计划和文档约束，避免语言切换导致产品语义改变。

---

## 后续任务

- [ ] 初始化 Cargo workspace。
- [ ] 确定 Rust MSRV。
- [ ] 选择 PTY/syscall crate。
- [ ] 选择 SQLite crate。
- [ ] 配置 rustfmt。
- [ ] 配置 clippy。
- [ ] 配置 cargo test。
- [ ] 配置 benchmark 入口。
- [ ] 更新 CI 文档。

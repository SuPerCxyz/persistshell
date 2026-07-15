# M43 Session Benchmark 设计

## 目标

提供可手动执行的端到端基准，覆盖 100、500、1000 个 Session 的创建、列表和关闭路径。
每个规模使用独立的 XDG 临时目录和 daemon，避免污染用户数据或受前一轮数据影响。

## 运行方式

新增 `scripts/benchmark-sessions.sh`。默认依次执行 100、500、1000，可用
`PERSIST_BENCH_COUNTS="100 500"` 缩小范围；`PERSIST_BIN` 与 `PERSISTD_BIN` 可指定
release 二进制路径。默认在隔离配置中使用 `1KB` ring buffer 并关闭 session log，以测量
基础管理开销；可用 `PERSIST_BENCH_RING_SIZE` 调整 buffer 大小。

脚本输出 CSV 表头和每个规模的 create/list/close 毫秒数。创建与关闭使用独立 CLI 进程，
因此测得的是 client IPC、daemon、metadata、PTY 创建/销毁的端到端成本。

## 安全与清理

脚本仅操作 `mktemp` 创建的 XDG 目录。退出时向该脚本启动的 daemon 发送 TERM、等待其
退出并删除临时目录；不停止用户已运行的 daemon，不修改用户配置。

默认失败也会清理临时目录；设置 `PERSIST_BENCH_KEEP_FAILURE=1` 时，失败现场会保留并
在 stderr 打印目录，便于检查 daemon 日志。

基准运行发现握手的 5 秒 socket 超时不能泄漏到后续长操作；ClientSocket 必须在收到
`HELLO_ACK` 后清除该超时，避免高负载建会被误报为 `EAGAIN`。

## 非目标

- 不作为 `cargo test` 或 CI 默认步骤。
- 不把 1000 个 PTY Session 放入单元测试。
- 不测量 attach 延迟、PTY 吞吐、内存或 CPU；这些需要后续专项基准。

## 验证计划

先以 `PERSIST_BENCH_COUNTS="1"` 验证脚本清理和输出，再在具备足够资源的本地与 `test`
主机运行默认规模并归档环境与结果。

# M39 前台进程资源监控设计

## 目标

为运行中 Session 的前台进程组 leader 提供瞬时资源视图：CPU 累计 tick、RSS、
读写累计字节数。它是一次性查询，不创建后台采样线程，不写入 metadata。

## CLI 与 IPC

新增：

```bash
persist stats <session-id>
```

新增 `PROCESS_STATS` / `PROCESS_STATS_RESP` IPC。响应包含 pid、CPU user/system ticks、
RSS KiB、read_bytes 与 write_bytes。closed Session、无前台进程或 `/proc` 不可读时
返回成功的空统计，CLI 输出 `(no foreground process)`。

## Linux 数据来源

- `/proc/<pid>/stat`：utime、stime、RSS pages；RSS 使用 `sysconf(_SC_PAGESIZE)` 换算 KiB。
- `/proc/<pid>/io`：`read_bytes`、`write_bytes`。

进程在读取期间退出或文件格式无法解析时返回空统计，不影响 daemon 或其他 Session。

## 非目标

- 不计算 CPU 百分比，不保存两次采样间隔。
- 不递归汇总 process tree。
- 不增加长期线程、timer 或 metrics server。

## 验证计划

M38/M39 后续统一验证协议编解码、procfs 解析、daemon IPC、CLI 输出和 test 主机
端到端路径。

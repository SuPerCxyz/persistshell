# M50 发布就绪审计

## 结论

M50 的本地与 test 主机发布就绪检查已经完成。维护者已确认发布版本为 `0.1.0`，对应 tag 为
`v0.1.0`。当前可构建并校验该版本的 tarball、deb 和 rpm；发布到远端仍取决于提交、push、tag
授权以及 GitHub mirror 的实际 workflow 结果。

## 本机证据

| 检查 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| workspace clippy（warnings deny） | 通过 |
| `cargo test --workspace --all-features` | 通过；一个既有 zsh PTY 用例 ignored |
| 恢复上下文定向测试连续八次 | 通过 |
| release build、completion、groff/man | 通过 |
| tarball、deb 解包内容和 standalone checksum | 通过 |
| `git diff --check` | 通过 |

闭合 Session 的恢复上下文曾暴露时序缺陷：后续部分 `/proc` 捕获可能只取得 cwd，从而覆盖先前
取得的环境快照。`RecoveryContext` 现在逐字段保留既有值，并在关闭时以完整存储快照补全直接
捕获结果；新增单元测试和重复集成测试覆盖该回归。

## test 主机证据

| 环境 | 检查 | 结果 |
|---|---|---|
| Rocky Linux 9.7 | 原生 release build、rpm build、checksum | 通过 |
| Rocky Linux 9.7 | rpm 中二进制、completion、用户文档、压缩 man page | 通过 |
| Rocky Linux 9.7 | 已安装二进制的隔离 XDG daemon/new/list/close/SIGTERM | 通过 |

## 发布阻塞项

以下不是代码质量失败，而是未授权或依赖外部平台的发布动作：

1. 提交并推送权威仓库，等待 GitHub mirror 同步。
2. 创建并推送 `v0.1.0` tag。
3. 在 GitHub 上实际运行并检查 CI/package workflow。
4. 决定 GitHub Release、artifact 附件、签名和依赖许可证审查策略。

详细操作顺序见 `docs/release/RELEASE_CHECKLIST.md`。本审计不把这些未执行动作标记为完成。

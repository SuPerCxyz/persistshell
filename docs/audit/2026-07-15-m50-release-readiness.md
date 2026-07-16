# M50 发布就绪审计

## 结论

M50 的本地与 test 主机发布就绪检查已经完成。维护者已确认发布版本为 `0.1.0`，对应 tag 为
`v0.1.0`。当前可构建并校验该版本的 tarball、deb 和 rpm；实现已推送并同步到 GitHub mirror，
`v0.1.0` 已发布，分支和 tag CI、tag Package workflow 均已通过。GitHub artifact 已生成，但当前
匿名环境不能下载归档，尚未对 GitHub 下载副本执行独立 checksum。

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

## GitHub CI 证据

首次 run `29413397484` 在 `Test` step 失败。根因是 PTY 兼容性测试直接启动 zsh/fish，而 hosted
runner 未预装这两个 shell；fmt 和 clippy 均已通过。提交 `b7e4cc2` 在 CI 中显式安装 zsh/fish，
后续 run `29413709266` 全部通过。

最终 release metadata 提交 `77e2027` 的分支 CI run `29413877184` 通过。`v0.1.0` tag 指向该
提交，tag CI run `29414016648` 与 Package run `29414016642` 均通过。Package workflow 生成
artifact `persistshell-linux-packages`（ID `8342325886`，6,359,905 字节，archive digest
`sha256:f3319ec37cc32f252f88e5680cd6a5d20e529e8537b6820b355a33281a873a61`）。匿名下载 API 返回
401，因此没有把 GitHub 下载副本的内容与 checksum 标记为已复核。

## 依赖许可证元数据

`cargo metadata --locked` 返回 57 个外部依赖。许可证表达式均提供 MIT、Apache-2.0、Zlib、
Unicode-3.0 或 Unlicense 等宽松许可路径；其中 `r-efi` 的 LGPL-2.1-or-later 是 MIT/Apache-2.0
之外的可选项，不是唯一许可。未发现仅提供强 copyleft 许可证的依赖。该检查不替代法律意见。

## 发布阻塞项

以下不是代码质量失败，而是未授权或依赖外部平台的发布动作：

1. 使用具备 GitHub Actions artifact 读取权限的身份下载并复核 artifact。
2. 决定 GitHub Release、artifact 附件和签名策略。

详细操作顺序见 `docs/release/RELEASE_CHECKLIST.md`。本审计不把这些未执行动作标记为完成。

## Tag 后平台兼容性复核

后续 Rocky Linux 9.7 安装测试证明，历史 `v0.1.0` GitHub workflow 在 Ubuntu 上构建的通用
tarball 依赖 `GLIBC_2.39`，不能作为 RHEL 9 二进制。`master` 已改为 Ubuntu 26.04 与
Rocky Linux 9 独立构建，并修复远程全功能测试发现的 runtime 缺陷。历史 tag 和 artifact
保持不变；新 workflow 尚待推送触发。完整证据见
`docs/audit/2026-07-15-m50-platform-package-remote-validation.md`。

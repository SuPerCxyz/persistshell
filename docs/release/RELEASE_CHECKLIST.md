# 发布检查清单

本清单用于 M50 的维护者发布操作。维护者已确认本次发布版本为 `0.1.0`，对应 tag 为
`v0.1.0`。M50 名称中的 v1.0 是发布就绪里程碑，不代表语义版本 `1.0.0`。

## 已完成的就绪证据

- [x] 根目录 `LICENSE`、Cargo manifest、README 与三种包均使用并携带 MIT License。
- [x] 本机通过 `cargo fmt --all --check`。
- [x] 本机通过 `cargo clippy --workspace --all-targets --all-features -- -D warnings`。
- [x] 本机通过 `cargo test --workspace --all-features`；一个依赖系统 zsh 配置的 PTY 用例按既有
  规则 ignored。
- [x] Closed Session 恢复上下文测试连续运行八次均通过，覆盖部分 `/proc` 捕获不应丢失既有环境快照。
- [x] 本机构建 release 二进制、tarball、deb，验证 SHA-256、可执行文件、man page、fish completion
  和用户故障排查文档。
- [x] `scripts/test-completions.sh` 与 `groff -man` 定向验证通过。
- [x] test Rocky Linux 9.7 原生构建 rpm，验证 checksum、二进制、压缩 man page、三种 completion
  及用户文档路径。
- [x] test 已安装二进制在隔离 XDG 环境完成 daemon、new、list、close、SIGTERM 清理流程。
- [x] M43 benchmark、M44 安全审查、M45 兼容性矩阵已有审计记录。

## 发布前由维护者执行

- [x] 审查工作区改动、许可证与最终 release notes；未发现凭证或未预期构建产物。
- [x] 维护者确认发布版本为 `0.1.0`，tag 为 `v0.1.0`；workspace manifest 无需改版。
- [x] 发布实现和 CI 修复已推送权威仓库，GitHub mirror 同步到 `b7e4cc2`；对应 CI run
  `29413709266` 通过。
- [x] `v0.1.0` 已推送并同步到 GitHub mirror；tag CI run `29414016648` 和 Package run
  `29414016642` 均通过。
- [ ] 下载 workflow artifact，独立执行 `sha256sum --check`，检查 tarball、deb、rpm 的版本、架构、
  许可证、man page、completion 和用户文档。
- [ ] 确定是否创建 GitHub Release、附加哪些 artifact、是否生成 release notes。
- [x] 审查 57 个锁定依赖的 Cargo SPDX 元数据；未发现仅提供强 copyleft 许可证的依赖。
- [ ] 按发布策略决定是否签名、生成 SBOM 或补充 `NOTICE`。
- [ ] 对外发布后记录 tag、workflow run、artifact checksum、发布日期和支持入口。

## 当前未覆盖边界

- GitHub hosted runner 的分支和 tag CI、tag Package workflow 均已通过。
- Artifact `persistshell-linux-packages`（ID `8342325886`）已生成，archive digest 为
  `sha256:f3319ec37cc32f252f88e5680cd6a5d20e529e8537b6820b355a33281a873a61`；匿名下载 API
  返回 401，尚未对下载副本执行独立 checksum。其 Ubuntu 二进制依赖 `GLIBC_2.39`，不得再
  作为 RHEL 9 兼容产物发布。
- `master` 已改为 Ubuntu 26.04 与 RHEL 9 独立构建；本地和 Rocky 容器验证通过，但新的
  GitHub hosted jobs 尚待推送触发。
- 已验证 Linux x86_64 的 Ubuntu 与 Rocky 基线，未验证其他架构、发行版或 macOS。
- test 主机没有 zsh/fish 端到端交互环境；相应补全已作语法和打包路径验证。
- daemon 崩溃后的 PTY 存活、所有全屏 TUI 的画面恢复不属于当前承诺。
- 当前 workflow 上传 artifact，但不自动创建 GitHub Release，也不执行签名。

## 当前尚未执行的操作

尚未下载 GitHub artifact、创建 GitHub Release 或签名。`v0.1.0` 的提交、push、mirror 同步、
分支/tag CI 和 Package workflow 已完成；tag 后平台打包修复尚待提交、推送和触发新 workflow。

依赖许可证检查基于 Cargo metadata 中的 SPDX 表达式，不替代法律意见。

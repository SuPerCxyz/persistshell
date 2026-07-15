# PersistShell Licensing

本文档记录 PersistShell 的许可证决策边界。

## Current Decision

本仓库使用根目录 `LICENSE` 中的 MIT License。Cargo workspace 通过 `license-file = "LICENSE"`
引用该文件；发布包必须携带同一份许可证文本。

## Why This Matters

许可证是法律和社区协作边界的一部分。

PersistShell 属于 Linux 基础设施类软件，后续可能涉及：

- 发行版打包
- 企业环境部署
- 安全审计
- 贡献者协议
- 专利授权边界
- 依赖许可证兼容

许可证已由维护者写入仓库。后续如需更换许可证，必须由维护者明确决策，不能由实现 Agent
自行替换。

## Recommended Options

### Apache-2.0

适合希望提供明确专利授权、面向企业和基础设施场景的项目。

优点：

- 专利授权条款清晰。
- 企业采用阻力较低。
- 适合长期基础设施项目。

注意：

- 文本比 MIT 更长。
- 需要保留 NOTICE 相关约束。

### MIT

适合希望保持最简单、最宽松授权的项目。

优点：

- 简洁。
- 社区熟悉。
- 与多数依赖和项目兼容。

注意：

- 专利授权不如 Apache-2.0 明确。

## Release Rule

在任何公开 release 前，必须完成以下事项：

1. 确认根目录 `LICENSE` 仍为维护者认可的 MIT License。
2. 确认 README 和发布包包含许可证说明与文本。
3. 检查依赖许可证兼容性。
4. 在 `CHANGELOG.md` 记录任何许可证变更。
5. 如需要，补充 `NOTICE` 文件。

## Non-Decision

本文档不是许可证本身；权利和限制以根目录 `LICENSE` 为准。

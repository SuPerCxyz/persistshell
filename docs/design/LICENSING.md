# PersistShell Licensing

本文档记录 PersistShell 的许可证决策边界。

## Current Decision

当前仓库没有选择具体开源许可证，因此不应把本仓库视为已经正式开源发布。

在维护者明确选择许可证之前，不创建根目录 `LICENSE` 文件，不在 README 中声明 MIT、Apache-2.0、GPL 或其他许可证。

## Why This Matters

许可证是法律和社区协作边界的一部分。

PersistShell 属于 Linux 基础设施类软件，后续可能涉及：

- 发行版打包
- 企业环境部署
- 安全审计
- 贡献者协议
- 专利授权边界
- 依赖许可证兼容

因此许可证必须由项目维护者明确选择，而不是由实现 Agent 自动决定。

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

在任何公开发布、打包、接受外部贡献或创建 release artifact 之前，必须完成以下事项：

1. 维护者选择许可证。
2. 创建根目录 `LICENSE` 文件。
3. 更新 `README.md` 的许可证说明。
4. 检查依赖许可证兼容性。
5. 在 `CHANGELOG.md` 记录许可证选择。
6. 如需要，补充 `NOTICE` 文件。

## Non-Decision

本文档不是许可证本身，也不授予任何使用、复制、修改、分发本仓库内容的权利。


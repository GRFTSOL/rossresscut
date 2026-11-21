# 贡献指南

本项目是自用 / 学习性质的个人项目：Issue、Discussions、Wiki 与 PR 均已关闭。
本文件只记录维护者自己的开发流程与环境说明。

先读 `AGENTS.md`。它列出不可违背的硬约束；具体规则在 `docs/` 中。

## 环境

```bash
rustup component add rustfmt clippy
sudo pacman -S --needed ffmpeg mpv
cargo build
```

工具链由 `rust-toolchain.toml` 固定。`libmpv` 与 `ffmpeg` / `ffprobe` 是运行时依赖；项目不自带二进制。

## 提交前

```bash
./scripts/check.sh
```

该脚本包含格式、构建、两种 feature 组合的 clippy、单元/集成测试。

写完代码后再运行 `fuck-u-code-analysis`，查看输出并判断是否合理；合理时按它的建议修正，然后提交。

## 修改流程

1. 保持改动范围单一；不要在功能提交里混入重构、格式化或依赖升级。
2. 先查现有代码、ADR 与文档，再决定是否需要新模块或依赖。
3. 涉及真实取舍（架构、媒体语义、渲染/导出策略、依赖选择）时，先写 ADR 或使用 `architecture-decision` 流程。
4. 外部实现只作为行为参考；不复制 LosslessCut / FilmCraft / EffectCraft 的代码。见 `docs/research.md`。
5. 错误路径、外部输入与取消/清理都要考虑；测试要覆盖新增行为。
6. 更新受影响的文档：`PLANS.md`、`docs/architecture.md`、相关 ADR、`docs/open-questions.md`。

## 代码风格

见 `docs/coding-standards.md`。重点：

- 项目代码禁止 `unsafe`。
- 外部输入不可信；错误必须带上下文。
- 媒体时间使用整数类型。
- UI 不阻塞、不执行业务逻辑。

## 许可证

项目以 **GPL-3.0-or-later** 发布（ADR 0015）。不要引入与现有依赖（尤其
GPL-2.0-or-later 的 `libmpv`）不兼容的代码或资源，也不要在未确认前提交第三方素材。

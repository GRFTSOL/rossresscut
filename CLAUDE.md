# CLAUDE.md

本项目的规则入口是 `AGENTS.md`。先读它，再读与任务相关的 `docs/` 与 ADR。

## 工作循环

1. 读 `AGENTS.md`、`GOALS.md`、`PLANS.md` 和相关专题文档。
2. 查现有代码与 ADR，不猜 API 或 FFmpeg 行为。
3. 需要真实取舍时使用 `architecture-decision` skill。
4. 实现后运行 `./scripts/check.sh`。
5. UI 功能/状态检查优先用 `scripts/ui_a11y.py`（见 `docs/development.md`）；
   截图只用于颜色、布局与视频画面验证。
6. 再运行 `fuck-u-code-analysis`，查看输出并判断是否合理；合理时按输出修正。
7. 不自动 commit、push、reset 或 rebase，除非用户明确要求。

## 关键约束

- 原生 Rust + egui/eframe；不引入浏览器内核或 Web UI 运行时（低占用目标）。
- 项目代码 `unsafe_code = "forbid"`。
- 媒体时间使用整数类型。
- UI 不阻塞、不执行业务逻辑。
- 外部 FFmpeg / FFprobe 参数逐项传递，不拼接 shell 命令。
- 外部输入不可信；错误必须带上下文。
- 默认 `stream copy`；重编码必须说明原因，不能标为无损。
- LosslessCut / FilmCraft / EffectCraft 只作行为与工程参考；不复制代码。

## 详细规则

- `docs/architecture.md`、`docs/media-model.md`
- `docs/ui-guidelines.md`、`docs/media-rules.md`
- `docs/coding-standards.md`、`docs/development.md`
- `docs/research.md`、`docs/open-questions.md`

思维过程和回答使用简体中文；代码、命令、标识符与日志不强行翻译。

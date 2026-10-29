# AGENTS.md

本项目是 **Rust + egui/eframe** 的原生桌面媒体工具，目标是轻量、快速、低资源占用的 LosslessCut 类替代品。媒体处理由外部 FFmpeg / FFprobe 与进程内 libmpv 负责。

本文件只保留**入口、硬约束和文档索引**。具体规则写在专题文档中；专题文档与这里的硬约束冲突时，以本文件为准。

## 文档索引

| 内容 | 文档 |
| --- | --- |
| 产品目标与非目标 | `GOALS.md` |
| 阶段划分与当前任务 | `PLANS.md` |
| 贡献流程 | `CONTRIBUTING.md` |
| 发布流程 | `RELEASING.md` |
| 分层、数据流、模块边界 | `docs/architecture.md` |
| 媒体数据模型与时间系统 | `docs/media-model.md` |
| UI 与桌面集成 | `docs/ui-guidelines.md` |
| FFmpeg / FFprobe / 无损导出规则 | `docs/media-rules.md` |
| 错误处理、Rust 风格、依赖、测试、性能 | `docs/coding-standards.md` |
| 命令、环境、权限边界、Git | `docs/development.md` |
| 外部参考项目使用边界 | `docs/research.md` |
| 第三方组件与参考项目许可证 | `ATTRIBUTION.md` |
| Claude 系 agent 入口 | `CLAUDE.md` |
| 未解决问题 | `docs/open-questions.md` |
| 架构决策 / 调查记录 | `docs/decisions/`、`docs/investigations/` |

## 硬约束

- 保持原生 Rust 架构：UI 与业务逻辑运行在同一个原生进程内，不引入浏览器内核或 Web UI
  运行时（Electron、Tauri、WebView、Node.js 等），以保持低内存占用与快速启动。
- 项目代码保持 `unsafe_code = "forbid"`。FFI 与 `unsafe` 只能存在于依赖内部；GPU 句柄不得越过 `media::preview` 与 UI 渲染适配层。
- 媒体时间必须使用整数 `Timestamp` / `Duration` / `TimeBase`，禁止用 `f32` / `f64` 秒作为唯一表示。
- UI 帧内不得执行 FFmpeg、FFprobe、解码、缩略图/波形生成或阻塞等待。Domain 与 Media 不得依赖 egui。
- 所有领域状态修改必须经由 Application 命令；UI 只构造参数、发送命令、显示结果。
- 导出与探测使用外部 FFmpeg / FFprobe，参数逐项传递；禁止通过 shell 拼接命令。
- 默认 `stream copy`。需要重新编码时必须给出具体原因并告知用户，不得把重编码标记为无损，也不得静默降级。
- 外部输入一律不可信：媒体文件、FFprobe 输出、FFmpeg 输出、路径、索引、时间戳、codec、容器。错误必须带上下文，禁止用 `unwrap` / `expect` / `panic!` 处理用户输入或外部输出。
- 不扩大任务范围；不为未来假想需求引入 trait、泛型、事件总线或新依赖。新增模块必须说明分层归属与依赖关系。
- LosslessCut、FilmCraft、EffectCraft 只作为**行为与工程实践参考**，不复制其代码，也不照搬其架构。边界见 `docs/research.md`。

## Agent 工作方式

1. 修改前先读 `AGENTS.md`、`GOALS.md`、`PLANS.md`、相关 `docs/` 与 ADR，确认现有模块是否已经具备该能力。
2. 不猜测 API、依赖或 FFmpeg 行为。先查代码、官方文档、实际命令输出或最小实验。
3. 涉及真实取舍（架构、媒体语义、渲染/导出策略、依赖选择）时使用 `architecture-decision` skill。
4. 研究外部实现时遵循 `docs/research.md` 的顺序与许可证约束，不把 TypeScript / Electron 实现直接翻译成 Rust。
5. **每次写完代码都用 `fuck-u-code-analysis` 检查一下；查看其输出并判断是否合理。如果合理，依照它的输出进行更正，然后再提交 commit。**
6. 提交前运行 `./scripts/check.sh` 并确保全部通过。禁止声称没有实际运行的检查已经通过。
7. 检查 UI 优先用 `scripts/ui_a11y.py` 读取和点击控件；截图只用于颜色、布局与视频画面。用法见 `docs/development.md`。
8. 不自动 commit、push、reset、rebase 或修改历史，除非用户明确要求。
9. 为节省 token，思维过程和回答使用简体中文；代码、标识符、命令、日志与专有名词不强行翻译。

## 完成标准

见 `docs/development.md` 的「完成标准」。功能、重构、格式化与依赖升级不得混在同一次修改中。

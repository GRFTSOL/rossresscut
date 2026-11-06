# rossresscut

原生 Rust + egui 的轻量媒体剪切工具，目标是提供 LosslessCut 类的工作流：打开媒体、预览、在时间轴上划出片段、无损导出。

- 视频预览：进程内 `libmpv`，优先硬件解码
- 媒体探测：外部 `ffprobe`
- 导出：外部 `ffmpeg`，默认 `stream copy`
- 多段导出：临时片段 + concat demuxer

## 当前状态

Phase 7 已完成：时间轴与片段编辑、单段与多段无损导出、章节、波形、项目文件、
批处理与合并、场景检测与静音切分、字幕与流选择、智能裁切。

Phase 8（发布）进行中：Linux 的 pacman 包与桌面集成已完成；Windows 原生
MSVC 构建、测试与便携包已完成，macOS 未开始。详见 `PLANS.md` 与
`docs/open-questions.md`。

## 已知限制

- 多段导出的拼接偏移按各段真实时间戳计算，拼接处不会额外插入帧间隙；起点
  仍会回退到关键帧，输出不是帧精确的。
- 非关键帧结束点在含 B 帧的素材上可能留下短暂画面停留（stream copy 限制）。
  开启智能裁切后，段尾由编码尾补齐，不再有该停顿。
- 片段起点不是关键帧时，stream copy 会从更早的关键帧开始；智能裁切可让起点
  精确，但头部会重新编码。

## 环境要求

- Rust：由 `rust-toolchain.toml` 固定
- `libmpv`（系统 `mpv` 包）
- `ffmpeg` / `ffprobe`
- Linux 桌面：Wayland 或 X11；文件对话框通过 XDG desktop portal
- Windows：MSVC 工具链（例如 `msvc-kit`）、`MPV_LIBRARY_DIR` 指向的 `mpv.lib`
  导入库，运行期需要 `libmpv-2.dll`；细节见 `docs/development.md`

## 构建与运行

```bash
cargo build --release
./target/release/rossresscut [media-file]
```

Windows 还需要指定 `mpv.lib` 所在目录（其余工具链步骤见 `docs/development.md`）：

```powershell
$env:MPV_LIBRARY_DIR = "C:/data/mpv-msvc"
cargo build --release
```

本地完整检查：

```bash
./scripts/check.sh
```

## 配置

配置文件：`$XDG_CONFIG_HOME/rossresscut/config.toml`（默认 `~/.config/rossresscut/config.toml`）。

## 文档

| 内容 | 文档 |
| --- | --- |
| 硬约束与入口 | `AGENTS.md` |
| 产品目标 | `GOALS.md` |
| 阶段计划 | `PLANS.md` |
| 已完成阶段记录 | `docs/history/completed-phases.md` |
| 贡献流程 | `CONTRIBUTING.md` |
| 发布流程 | `RELEASING.md` |
| 架构 | `docs/architecture.md` |
| 媒体模型 | `docs/media-model.md` |
| UI 规则 | `docs/ui-guidelines.md` |
| 媒体与导出规则 | `docs/media-rules.md` |
| 编码规范 | `docs/coding-standards.md` |
| 开发环境与命令 | `docs/development.md` |
| 参考项目边界 | `docs/research.md` |
| 第三方组件与参考项目 | `ATTRIBUTION.md` |

## 许可证

项目以 **GPL-3.0-or-later** 发布，见 `LICENSE`。

预览主路径动态链接的 `libmpv` 是 `GPL-2.0-or-later`，与 GPLv3 兼容；
外部回退路径的 `cpal` 是 Apache-2.0，同样与 GPLv3 兼容。详见 `ATTRIBUTION.md`。

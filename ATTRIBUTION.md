# 第三方组件与参考项目

本项目以 GPL-3.0-or-later 发布，见 `LICENSE`。以下组件与参考项目的许可证需要在分发前复核。

## 项目自身

| 组件 | 许可证 |
| --- | --- |
| `rossresscut` 项目代码 | GPL-3.0-or-later（`LICENSE`） |

## 直接依赖

| 组件 | 用途 | 许可证 |
| --- | --- | --- |
| `eframe` / `egui` | 桌面窗口与 UI | MIT OR Apache-2.0 |
| `wgpu` | GPU 渲染 / 纹理导入 | MIT OR Apache-2.0 |
| `mpv-engine` | libmpv safe wrapper 与 exported frame | MIT OR Apache-2.0 |
| `libmpv` / `mpv` | 预览解码、音频输出、硬件解码 | GPL-2.0-or-later |
| `fontdb` | 系统字体枚举 | MIT |
| `rfd` | Linux XDG portal / Windows Shell COM 文件对话框 | MIT |
| `cpal` | 外部 FFmpeg 回退路径的音频输出 | Apache-2.0 |
| `serde` / `serde_json` | 数据序列化 | MIT OR Apache-2.0 |
| `toml` | 配置文件 | MIT OR Apache-2.0 |
| `log` / `env_logger` | 日志 | MIT OR Apache-2.0 |

`libmpv` 是运行时动态链接的系统库，不随源码仓库分发。GPL-3.0-or-later 与
`libmpv` 的 GPL-2.0-or-later 兼容；相关决策见 `docs/decisions/0015-project-license.md`。

## 参考项目

| 项目 | 用途 | 许可证 | 使用方式 |
| --- | --- | --- | --- |
| LosslessCut | 产品行为、媒体语义、FFmpeg 工作流参考 | GPL-2.0 | 只阅读与验证行为；不复制代码 |
| FilmCraft | Rust + egui 工程分层参考 | 见项目仓库 | 只参考架构取舍；不复制代码 |
| EffectCraft | engine-first 分层参考 | 见项目仓库 | 只参考架构取舍；不复制代码 |

本仓库不包含上述项目的源代码。

## 素材与字体

- `assets/fonts/JetBrainsMonoNerdFontPropo-Regular.ttf`：JetBrains Mono Nerd Font Propo
  Regular，来自 Nerd Fonts v3.5.1（patched JetBrains Mono），
  许可证 SIL Open Font License 1.1，见 `assets/fonts/OFL.txt`。
  仅作为 UI 图标 glyph 的回退字体，不用于正文排版。
- `tests/fixtures/` 中的媒体文件是使用 FFmpeg `lavfi` 生成的合成素材，不包含第三方内容；重新生成方式见 `tests/fixtures/README.md`。
- 正文 UI 字体来自用户系统，通过 `fontdb` 枚举；项目不分发正文排版字体。
- 若以后加入其他素材，必须记录来源、作者、许可证，并更新本文件。

# rossresscut

[English](README.md) · **简体中文**

用 Rust + [egui](https://github.com/emilk/egui) 写的原生轻量级无损媒体剪切工具，支持 Windows 与
Linux。工作流对齐 LosslessCut：打开媒体、进程内预览、在时间轴上划出片段，然后不重新编码地导出。

> **这是一个 vibe coding 项目。** 代码几乎全部由 AI coding agent 根据简短的人类指令生成，
> 人负责需求、取舍与验收。
> 它是自用与学习性质的个人项目，所以 **Issue、Discussions、Wiki 与 Pull Request 均已关闭**。
> 没有支持承诺，也没有路线图。愿意的话可以在 GPL 下自行 fork。

<img width="1679" height="979" alt="图片" src="https://github.com/user-attachments/assets/eee4e2b2-f37e-4201-8171-d8ec87a25ba8" />

## 功能

### 预览与导航

- **进程内 libmpv** 播放，默认使用硬件解码；解码后端可在 *设置 → 预览* 中选择：
  `auto-safe`、`auto`、`d3d11va`、`nvdec`、`cuda`、`vaapi`、`no`，或回退到外部 FFmpeg 管线。
- 滚轮跳转步长可配置（默认 **6.67 秒**，约等于 30 fps 下的 200 帧）；`Ctrl` + 滚轮固定 ±10 秒。
- 按秒步进（`Alt+←` / `Alt+→`）、按帧步进（`,` / `.`）、跳到片段起止。
- 时间轴背景绘制波形，并用关键帧索引加速拖动预览。
- 播放信息面板显示实际使用的解码器、帧数以及视频 / 音频流信息。

### 剪切

- 设置切点起止、分割、删除、上移下移、排序、打乱、合并重叠片段、反转选区。
- 片段列表支持 Ctrl 多选，片段可复制粘贴，时间轴编辑支持撤销 / 重做。
- 片段可以禁用而不删除；导出顺序只由时间轴顺序决定。

### 导出

- **默认 stream copy**：普通剪切不解码、不重新编码。
- 多片段导出先生成临时片段，再用 concat demuxer 拼接。
- **智能裁切**（默认关闭）只重新编码片段的头尾，让切点落在精确帧上；界面会明确标注这是
  重新编码，不会当作无损。
- 按片段写入章节；字幕按切点裁剪（保留或丢弃跨界字幕）；可只导出选中的视频 / 音频 / 字幕流。
- 批处理队列：对多个文件执行同一套导出，也可以把结果合并成一个文件。
- 工具 → *已执行的 ffmpeg 命令* 会列出本次会话的全部命令及用途，`argv` 随时可复现。

### 分析

- **场景检测**带实时进度条；点取消或关闭窗口会立即杀掉 FFmpeg 子进程。
- **基于音量自动切分**（`Alt+S`）：由波形峰值判断静音段，阈值可调。
- 打开媒体时可以把容器自带的章节转成片段。

### 项目与设置

- 项目文件（`*.rrc`）保存时间轴；自动保存可以写到源文件所在目录。
- 设置是独立窗口，分为 *常规 / 预览 / 导出 / 快捷键* 四个 tab：主题、界面语言（English /
  简体中文）、字体与缩放、预览后端、跳转步长、导出默认值。
- **43 个快捷键动作，全部可改键**。点击绑定开始录制，录制时按 `Backspace` 清空。

### 桌面集成

- **Windows**：无边框窗口 + 自绘标题栏与菜单栏合并（32 px，窗口按钮区无边框），支持拖放和
  原生文件对话框，提供便携 zip。
- **Linux**：Wayland / X11，文件对话框走 XDG desktop portal，提供带 desktop entry 与图标的
  pacman 包。
- 可以直接从命令行打开文件：`rossresscut video.mp4`。

## 默认快捷键

| 动作 | 默认键 | 动作 | 默认键 |
| --- | --- | --- | --- |
| 播放 / 暂停 | `Space` | 撤销 / 重做 | `Ctrl+Z` / `Ctrl+Shift+Z` |
| 设置切点开始 | `Alt+I` | 剪切 / 复制 / 粘贴 | `Ctrl+X` / `Ctrl+C` / `Ctrl+V` |
| 设置切点结束 / 完成 | `Alt+O` | 全选 | `Ctrl+A` |
| 分割片段 | `Alt+B` | 上一个 / 下一个片段 | `↑` / `↓` |
| 删除片段 | `Delete` | 上一秒 / 下一秒 | `Alt+←` / `Alt+→` |
| 清除标记 | `Escape` | 上一帧 / 下一帧 | `,` / `.` |
| 关闭媒体 | `Ctrl+W` | 打开设置 | `Ctrl+,` |
| 基于音量自动切分 | `Alt+S` | | |

菜单里的命令同样可以绑定；没有默认键的动作就是未分配。

## “无损”是怎么做到的

普通导出用 `-c copy` 复制压缩后的数据包，音频和视频完全不解码，所以画质与源完全一致，
导出速度只受磁盘 I/O 限制。

代价是 stream copy 只能从关键帧开始。如果片段起点落在两个关键帧之间，输出会从上一个关键帧
开始（界面会提示）。智能裁切通过只重新编码片段的头尾来避免这一点，并且会明确标注为重新编码。

## 安装

每个 [Release](https://github.com/GRFTSOL/rossresscut/releases) 都会附带构建产物：

| 平台 | 产物 | 说明 |
| --- | --- | --- |
| Windows 10/11 x64 | `rossresscut-<version>-windows-x64.zip` | 解压后直接运行 `rossresscut.exe`；已内置 `libmpv-2.dll`、`ffmpeg.exe`、`ffprobe.exe`。 |
| Arch Linux x86_64 | `rossresscut-<version>-1-x86_64.pkg.tar.zst` | `sudo pacman -U <文件>`；运行时依赖 `mpv`、`ffmpeg`。 |

## 从源码构建

Linux：

```bash
sudo pacman -S --needed rust ffmpeg mpv
cargo build --release
./target/release/rossresscut
```

Windows（原生 MSVC 工具链）：

```powershell
# 官方 Windows 版不提供 mpv.lib，需要用 scripts/mpv-import-lib.sh 从 DLL 生成，
# 然后让构建指向它所在目录。
$env:MPV_LIBRARY_DIR = "C:/data/mpv-msvc"
cargo build --release
```

运行期 `libmpv-2.dll` 必须和 exe 放在同一目录。工具链细节、打包脚本与测试环境见
[`docs/development.md`](docs/development.md)。

完整的本地检查（fmt、两轮 clippy、全部测试、构建）：

```bash
./scripts/check.sh
```

## 配置

`config.toml` 位于 Windows 的 `%APPDATA%\rossresscut\`，其他平台位于
`$XDG_CONFIG_HOME/rossresscut/`（通常是 `~/.config/rossresscut/`）。

```toml
[general]
follow_system_theme = true   # 为 false 时使用下面的 theme
theme = "dark"               # 或 "light"
language = "en"              # 或 "zh_hans"
font_zoom = 1.0

[preview]
seek_step_micros = 6666667   # 滚轮步长，默认 6.67 秒
backend = "auto-safe"        # libmpv 的 hwdec 值

[export]
chapters = false
smart_cut = false

[general.shortcuts]
play_pause = "Space"
add_cut = "Alt+I"
```

## 已知限制

- 未开启智能裁切时，stream copy 的片段起点会落在前一个关键帧。
- 含 B 帧的素材上，落在关键帧之间的结束点可能留下短暂画面停留（容器 / stream copy 的限制）。
  智能裁切通过重新编码尾帧消除它。
- 外部 FFmpeg 回退路径的按帧步进使用标称帧时长，对可变帧率素材只是近似；libmpv 路径使用
  mpv 自己的 frame-step。
- 暂时没有安装器：Windows 提供便携 zip，Linux 提供 pacman 包。

## 许可证

**GPL-3.0-or-later**，见 [`LICENSE`](LICENSE)。

`libmpv` 是 GPL-2.0-or-later，Rust 依赖是 MIT / Apache-2.0，都与 GPLv3 兼容。完整清单（含内置
字体）见 [`ATTRIBUTION.md`](ATTRIBUTION.md)。

## 文档

| 内容 | 文档 |
| --- | --- |
| 硬约束与入口 | [`AGENTS.md`](AGENTS.md) |
| 产品目标 | [`GOALS.md`](GOALS.md) |
| 阶段与当前工作 | [`PLANS.md`](PLANS.md) |
| 架构与数据流 | [`docs/architecture.md`](docs/architecture.md) |
| 媒体模型与整数时间 | [`docs/media-model.md`](docs/media-model.md) |
| UI 约定 | [`docs/ui-guidelines.md`](docs/ui-guidelines.md) |
| FFmpeg / 导出规则 | [`docs/media-rules.md`](docs/media-rules.md) |
| 编码规范与测试 | [`docs/coding-standards.md`](docs/coding-standards.md) |
| 构建、命令、权限 | [`docs/development.md`](docs/development.md) |
| 参考项目与边界 | [`docs/research.md`](docs/research.md) |
| 已完成阶段记录 | [`docs/history/completed-phases.md`](docs/history/completed-phases.md) |
| 架构决策 | [`docs/decisions/`](docs/decisions/) |
| 发布流程 | [`RELEASING.md`](RELEASING.md) |

## 状态

计划中的阶段已全部完成。后续只做功能完善与缺陷修复。

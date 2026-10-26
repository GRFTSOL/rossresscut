# 0009 预览解码与渲染：进程内 libav* + 平台 hwaccel + zero-copy

- 状态：已被 `0010-preview-via-mpv-engine.md` 取代（2026-10-26）；本文保留为 Phase 4B 历史记录
- 日期：2026-10-24
- 阶段：Phase 4B
- 取代：`0003-preview-backend.md` 的视频帧解码部分；`0005-hardware-decoding.md` 的预览解码实现部分

> 2026-10-26 更新：spike 证明 libmpv + 隐藏 GL context + DMA-BUF/IOSurface/
> shared D3D11 导出 + wgpu 导入可以满足 zero-copy，且比自建 libav* + 三平台
> surface 导入的工程量更小。预览后端改为方案 F，见 `0010-preview-via-mpv-engine.md`。
> 本文的方案 D 不再实施；`media/platform` 也不再为预览路径所需。

## 背景

Phase 4A 用真实 4K HEVC 素材测得：

| 路径 | 吞吐 |
| --- | --- |
| VAAPI 全程留在 GPU（`-hwaccel_output_format vaapi`） | 86 fps / 1.44x |
| VAAPI + `hwdownload`（即使已缩到 960x540） | 20 fps / 0.33x |
| VAAPI 默认下载 + CPU scale + rgb24 | 10.3 fps / 0.34x |
| 软件解码 + CPU scale + rgb24 | 45 fps / 1.51x |

application 层测量：软件解码 29.8 fps、`delta ≈ 0`；VAAPI 10.4 fps、40 秒内 `delta` 增长 11.6 秒。纹理上传 0.01 ms，egui `ui_fps` 40+。

结论：瓶颈是 GPU → CPU 下载，不是 egui / wgpu / frame scheduling。外部 FFmpeg 进程通过 `pipe:1` 只能传系统内存，结构上无法交付 GPU-resident 帧。

产品负责人决定：预览必须支持 4K 硬件解码并保持 GPU-resident 到呈现；导出继续使用外部 FFmpeg stream copy。

## 已知约束

- 导出、裁切、合并与探测仍由外部 FFmpeg / FFprobe 进程负责。
- 平台媒体 API（VAAPI / D3D11VA / VideoToolbox）只解码，不解析容器。
- `wgpu 30.0.1` 导入外部纹理只有 `unsafe fn`：`create_texture_from_hal`、各后端 `texture_from_raw`。
- 当前没有成熟的 crate 提供「DMA-BUF / IOSurface / D3D11 shared handle → wgpu」的 safe 封装。
- 项目原有的 `unsafe_code = "forbid"` 无法与 zero-copy 同时成立。

## 候选方案

### 方案 A：外部 FFmpeg 软件解码预览

优点：不引入 FFI，当前已能 29.8 fps。
缺点：4K HEVC CPU 占用高；不满足硬件解码与 zero-copy 要求。

### 方案 B：libmpv 软件 render

优点：实测 67.9 fps（4K HEVC），保留 wgpu；测量程序见 `docs/investigations/mpv-sw-render-probe.c`。
缺点：需要引入 libmpv 与 FFI；仍有一次 GPU → CPU 回读；与自建 media engine 方向不符。

### 方案 C：libmpv OpenGL render + eframe glow

优点：可保持 GPU 帧。
缺点：需要切换渲染栈；egui 与 libmpv 的 GL 状态共享复杂；跨平台仅 OpenGL。

### 方案 D：进程内 libav* + 平台 hwaccel + zero-copy

优点：统一容器与 codec；zero-copy；不依赖播放器运行时。
缺点：引入 FFI；三平台 surface 导入分别实现；与现有外部进程预览实现不兼容。

### 方案 E：完全自写 demux + 平台媒体 API

优点：不依赖 libav*。
缺点：需要实现 MP4 / MKV / TS 等容器解析，工程量接近独立项目。

## 决策

选择：

> 方案 D：进程内 `libavformat` + `libavcodec` hwaccel + 平台 surface 导入 wgpu，zero-copy。

具体边界：

```text
libavformat              demux（进程内，FFI 依赖）
        ↓
libavcodec + hwaccel     VAAPI / D3D11VA / VideoToolbox（进程内，FFI 依赖）
        ↓
hardware surface         DMA-BUF / D3D11 shared handle / IOSurface
        ↓
media/platform           surface → wgpu texture（项目内唯一允许 unsafe 的 crate）
        ↓
wgpu external texture
        ↓
egui
```

规则：

1. 新增 `media/platform` crate，是整个 workspace **唯一**允许 `unsafe_code` 的 crate。
2. 其余 crate 保持 `unsafe_code = "forbid"`，包括 `domain`、`application`、`ui`、`media::ffprobe`。
3. `media/platform` 对外只暴露 safe API；类型不得泄漏裸指针、`wgpu-hal` 句柄或 FFI 类型。
4. 每个 `unsafe` 块必须带 `// SAFETY:` 注释，说明成立的不变量。
5. 导出、裁切、合并与探测继续使用外部 FFmpeg / FFprobe 进程；不得改为链接 libav*。
6. 硬件解码失败必须回退到进程内软件解码，并让回退在 UI 可见。
7. 预览音频也使用进程内 `libavcodec` 解码，PCM 仍由 `cpal` 输出；音频样本时钟继续作为主时钟。
8. 预览解码路径不得影响 Segment、时间轴或导出语义。

## 决策理由

1. Phase 4A 已证明外部进程管线无法利用 GPU-resident 帧，任何参数调优都无法消除同步下载。
2. zero-copy 是产品负责人明确要求的目标。
3. `libavformat` 避免自写容器解析；`libavcodec` 的 hwaccel 统一三个平台，比分别直连平台媒体 API 的工程量低。
4. unsafe 隔离在单一 crate，其余代码保持 `forbid`，与 FilmCraft 的 `crates/platform` 模式一致。
5. 导出仍走外部 FFmpeg，保留无损 stream copy 语义与现有进程管理。

## 后果

### 正面

- 4K HEVC 预览可保持 GPU-resident，避免每帧同步下载。
- 容器与 codec 覆盖沿用 FFmpeg。
- unsafe 边界明确，可通过 crate 级 lint 与代码审查约束。

### 负面

- 运行时依赖系统 `libavformat` / `libavcodec` / `libavutil` 等动态库。
- 三套平台 surface 导入代码：Linux DMA-BUF / Vulkan、Windows D3D11 shared handle / DX12、macOS IOSurface / Metal。
- wgpu 后端必须与平台句柄匹配，`wgpu-hal` 版本与 `wgpu` 强耦合。
- 现有 `media/preview/video.rs` 与 `audio.rs` 的外部 FFmpeg 路径将被替换，相关测试需要重写。
- 音频解码与视频解码同时迁入进程内，`cpal` 只保留 PCM 输出职责。
- 本机只能验证 Linux / VAAPI；Windows 与 macOS 路径在本阶段无法实测。

### 后续影响

- `AGENTS.md` 的 unsafe/FFI 与「FFmpeg 必须直接执行」条款需要按本 ADR 收窄。
- 需要新增测试：
  - 硬件路径与软件路径输出一致性（容差内）。
  - hwaccel 初始化失败时的回退。
  - zero-copy 纹理的生命周期与帧释放。
  - A/V 时钟仍以音频为主时钟。
- 播放信息中的 Decoder 必须报告实际生效的后端，而不是请求值。

## 未解决问题

见 `docs/open-questions.md`。该文档记录本 ADR 未解决的实现问题、处理时机与解除条件。

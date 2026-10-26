# 0010 预览后端：mpv-engine export + wgpu（方案 F）

- 状态：已接受；实现未开始
- 日期：2026-10-26
- 阶段：Phase 4C 修订
- 取代：`0009-preview-decode-zero-copy.md` 的预览解码决策；
  `0004-audio-clock-and-desktop-integration.md` §1 音频主时钟与 §3 快速 scrub
  的主路径

## 背景

ADR 0009 选择方案 D：进程内 `libav*` + 平台 hwaccel + `media/platform` surface
导入。Phase 4C 尚未开始，项目仍是外部 FFmpeg 进程 + RGB24 管道预览。

Phase 4A 已证明外部进程只能交付系统内存帧，4K HEVC VAAPI 下载是瓶颈。
Phase 4B 的候选方案没有覆盖一条后来被验证的路径：

> libmpv 用隐藏 GL context 渲染到可导出 framebuffer，再导出
> DMA-BUF / IOSurface / shared D3D11 texture，由 wgpu 导入。

`mpv-engine 0.2.1` 已经实现了这条路径，并且提供 safe Rust API。实测记录见
`docs/investigations/mpv-engine-spike.md`，关键数据：

| 项 | 结果 |
| --- | --- |
| Linux DMA-BUF → wgpu | 3/3 导入测试通过；字节一致、生命周期安全、帧池可回收 |
| 4K HEVC 预览 | 29–30 fps；CPU 约 18.5%；RSS 约 269 MiB |
| DMA-BUF 参数 | `AR24`、linear modifier、stride 8192 |
| eframe 0.37 集成 | 红 `(253,0,0)`、灰 `(127,128,127)` 正确；无需 CPU 回读 |
| 音频 | mpv `ao` 可用；`position()` 与 `audio-pts` 稳定相差约 11–16 ms |
| 快速 seek | `hr-seek=no` 约 130–190 ms，落在前一个关键帧 |
| 精确 seek | 4K headless 约 50–330 ms（多数 200–330 ms）；eframe + 4K + 音频 365 ms |

## 候选方案

### 方案 A：继续外部 FFmpeg 软件预览

优点：不引入新运行时，现有实现可用。
缺点：4K HEVC CPU 占用高；外部进程 + `pipe:1` 结构上无法交付 GPU 帧。

### 方案 B：libmpv 软件 render

优点：实测吞吐高于外部 FFmpeg。
缺点：仍有 GPU → CPU 回读；不能满足 zero-copy。

### 方案 C：libmpv OpenGL render + eframe glow

优点：GPU-resident。
缺点：跨平台仅 OpenGL；GL 状态与 egui 共享复杂；与项目已选的 wgpu 渲染栈不一致。

### 方案 D：进程内 libav* + 平台 hwaccel + media/platform

优点：不依赖播放器运行时；zero-copy。
缺点：需要自建 libav* 绑定、三平台 surface 导入、音频与 seek 同步；工程量和平台风险最高。

### 方案 E：完全自写 demux + 平台媒体 API

优点：不依赖 libav*。
缺点：相当于独立容器解析项目，范围不可接受。

### 方案 F：libmpv + 隐藏 GL context + 可导出 surface + wgpu

优点：zero-copy；跨平台；音频、seek、容器与 codec 覆盖由 mpv 提供；项目代码不需要 unsafe。
缺点：引入 libmpv 运行时与 GPL 约束；wgpu 版本锁；导出固定 8-bit BGRA。

## 决策

采用方案 F：

- 预览的视频解码、缩放、色彩转换、帧调度、音频输出与 seek 交给
  **libmpv**，通过 **`mpv-engine 0.2.1` 的 `export` + `wgpu` feature** 使用。
- 项目只消费 `ExportedFrame` → `wgpu::Texture`，注册给 egui；
  预览路径不做 CPU 回读。
- 导出、裁切、合并与探测继续使用外部 FFmpeg / FFprobe；
  stream copy 语义不变。
- 现有外部 FFmpeg 预览在迁移期保留为回退路径，直到方案 F 稳定。

本 ADR 取代 ADR 0009 的“自建 `libav*` + `media/platform`”预览方案。
`media/platform` 不再为预览路径所需；如果未来重新选择方案 D，再恢复该计划。

## 决策理由

1. Phase 4A 已证明外部进程只能交付系统内存帧，方案 A 无法满足 4K HEVC 预览。
2. 方案 B 仍有 GPU → CPU 回读，方案 C 绑定 OpenGL 且与 eframe 的 wgpu 渲染栈冲突。
3. 方案 D 的所有正面目标（zero-copy、跨平台、统一 codec）方案 F 都能达到，
   但方案 F 不需要自建 libav* 绑定、三平台 hwaccel 与 surface 导入。
4. spike 已在 Linux RADV 上验证零拷贝、颜色、音频、seek 与 eframe 集成；
   数据见 `docs/investigations/mpv-engine-spike.md`。
5. 产品负责人接受 GPLv2+ 分发，并确认“预览与导出可以分开”。

## 架构与边界

```text
UI (egui)
  ↑ wgpu::Texture / TextureId
Application (commands / state / clock)
  ↑ mpv-engine safe API
media::preview::mpv
  ↑
mpv-engine (export + wgpu)
  ↑
libmpv → hwaccel (VAAPI / D3D11VA / VideoToolbox)
        → 隐藏 GL context → DMA-BUF / IOSurface / shared D3D11
        → wgpu::Texture
```

规则：

1. 新增 `src/media/preview/mpv.rs`（或同层子模块），只在这里依赖
   `mpv-engine`。Domain 与 Application 不直接依赖 `mpv-engine` 类型。
2. `Application` 负责命令、状态、错误与位置；UI 只发送命令并显示结果。
   UI 不构造 mpv 命令，不解析 mpv 属性。
3. `ExportedFrame` / `wgpu::Texture` 是 GPU 句柄，**不得进入 Domain 或
   Application 的命令状态**，不得写入 `Segment`、`Timeline` 或持久化设置。
   只允许在 `media::preview` 与 UI 渲染适配层之间按帧传递。
4. 项目代码继续保持 `unsafe_code = "forbid"`。`mpv-engine` 内部的 FFI 与
   `unsafe` 由第三方依赖承担；本项目不新增 unsafe 代码。
5. 如果以后必须在项目内直接操作 GPU 句柄，才重新引入 `media/platform`；
   方案 F 不要求它。
6. eframe 的 wgpu 设备必须请求
   `wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`（Linux），并在
   `NativeOptions.wgpu_options` 中使用 `WgpuSetupCreateNew.device_descriptor`。
   `instance_descriptor.backends` 固定为 `mpv_engine::WGPU_BACKEND`。
7. `wgpu` / `wgpu-hal` 必须与 eframe 解析到同一版本（当前 30.0.1）。
   升级 eframe 或 mpv-engine 时，两者必须同步升级，否则 `Device::as_hal`
   的类型不匹配。
8. `register_native_texture` 在 egui-wgpu 0.37 可以直接接受 `Bgra8Unorm`；
   文档要求 `Rgba8Unorm` 与实测不一致，必须保留颜色回归测试。

## 音频

采用 mpv 独占音频输出：

- `Engine::video()` 使用默认 `ao`；音频设备通过 mpv 的 `audio-device`
  属性选择。
- UI 的播放位置使用 `Engine::position()`（`time-pos`），不再使用 cpal
  消费样本帧的时钟。
- 暂停与 seek 已接入 `set_paused`、`seek_absolute`；音量与静音由 mpv 的
  `volume` / `mute` 属性承载，UI 尚无控件。
- 主路径移除 `PreviewAudio` 与 cpal 音频输出；cpal 只保留在外部 FFmpeg
  回退路径中，回退路径删除后一并移除。
- 不再同时让 mpv 与 cpal 输出音频，避免双声源和两套时钟。

ADR 0004 §1 的 cpal 主时钟与 §3 的 FFmpeg scrub 设计由本节取代。
音频测量：

- 稳定播放时 `position()` 与 `audio-pts` 相差约 11–16 ms；
  启动阶段约 79 ms 的瞬态。
- 暂停、暂停中 seek、恢复、音量、静音均已在 spike 中验证。
- 多音轨、设备切换、真实扬声器延迟与 10 分钟以上漂移仍是 OQ-11 / OQ-9。

## 快速 scrub 与精确 seek

交互规则：

1. 拖动开始：设置 `hr-seek=no`，发出 seek。
2. 拖动期间：UI 显示 `Engine::position()` 的实际落点；该落点是前一个
   关键帧，不是用户请求的目标时间。
3. 松开：设置 `hr-seek=yes`，发出最终精确 seek。
4. 精确 seek 完成以 `PlaybackRestart` 为准；如果事件没有到达，以
   `position()` 或观察 `time-pos` 兜底。
5. 预览位置只用于视觉反馈，不得用于 Segment 边界、关键帧索引或导出。

实测：

- `hr-seek=no`：播放中 132–193 ms，暂停 128–313 ms；
  测试素材 GOP 约 5 秒，落点误差约 4.9–5.0 秒。
- `hr-seek=yes`：4K headless 约 50–330 ms（多数 200–330 ms），误差 0；
  eframe + 4K + 音频 365 ms，`position=6.039`、`audio-pts=6.015`。
- headless / `vo=null` / `audio=no` 下精确 seek 可能不触发
  `PlaybackRestart`；真实 eframe + 音频下正常触发。实现必须两种路径都支持。

## 许可证

产品负责人已确认接受 GPLv2+，发布时按实际链接的 libmpv 许可证分发。

- `mpv-engine`、`rsmpv`、`rsmpv-sys`：MIT / Apache-2.0 / ISC，宽松许可。
- libmpv：mpv 默认构建为 GPLv2+；只有 `-Dgpl=false` 构建才是 LGPLv2.1+。
- 分发时如果带上 GPL 构建的 libmpv，整个组合作品必须按 GPL 兼容方式分发，
  并提供对应源码与许可证。由于项目依赖中含 Apache-2.0 代码，保守选择
  **GPLv3+**；GPLv2-only 与 Apache-2.0 不兼容。
- 如果只依赖用户系统提供的 libmpv，动态链接 GPL 库仍可能被视为衍生作品；
  不能把“不捆绑”当作许可证规避。
- 外部 `ffmpeg` 进程不受影响：不是链接，不构成衍生作品。
- 分发清单必须包含 `LICENSES` / About 页面中的 libmpv 与 FFmpeg 说明。
  如果未来需要宽松许可分发，必须改用 `-Dgpl=false` 的 LGPL libmpv，
  或放弃方案 F。

许可证细节以实际发行时的审查为准；本节记录的是产品决策，不是法律意见。

## 依赖与平台

新增依赖：

```toml
mpv-engine = { version = "0.2.1", features = ["wgpu"] }
```

`wgpu` feature 隐含 `export`。传递依赖：`rsmpv 0.2.1`、
`wgpu 30.0.1`、`wgpu-hal 30.0.1`。

系统依赖：

- Linux：libmpv（系统包）、libEGL、libgbm；运行需要可读的
  `/dev/dri/renderD*`。
- Windows：libmpv、OpenGL、D3D11、DXGI；导出路径使用
  `WGL_NV_DX_interop2`。
- macOS：libmpv、CGL、IOSurface、Metal。

被否决的替代方案：

- 直接用 `rsmpv` / `libmpv2`：只提供 libmpv 绑定与 GL/software render，
  仍需自己实现 GL context、FBO、DMA-BUF 导出与 wgpu 导入。
- `egui-sharkplayer`：wgpu 路径实际是 GL 渲染 + `glReadPixels` + egui
  texture；且锁 eframe 0.35，与项目 0.37 不兼容。
- `alhazen-core`：Linux 只支持 NVIDIA NVDEC，本机 AMD 无硬件解码；
  输出 CPU BGRA；API 是播放器而非编辑器。
- 继续方案 D：需要自建 libav* 绑定、三平台 surface 导入与同步；
  spike 已证明方案 F 可以用更少代码达到同样目标。

## 回退与错误

- libmpv 不可用、`Engine` 初始化失败、export 后端不可用、
  或 device 缺少 DMA-BUF feature 时，回退到现有外部 FFmpeg 预览。
- mpv-engine 的加载 API 只接受 UTF-8 路径；非 UTF-8 路径必须回退外部 FFmpeg，
  并在错误信息中说明原因。
- 回退必须可见：播放信息显示实际后端（mpv / ffmpeg software / ffmpeg vaapi）。
- 不允许静默把 zero-copy 降级为 CPU 回读；如果使用 `render_sw`，
  必须在 UI 标注。
- 导入失败、驱动拒绝、尺寸不支持必须返回带上下文的错误，不 panic。

## 帧语义与编辑约束

- `ExportedFrame` 不提供 PTS。`Engine::position()` 是 mpv 播放时钟，
  **不是当前纹理对应的源 PTS**。
- 预览位置只用于 UI 显示与 scrub 反馈；Segment 边界、关键帧索引、
  导出参数仍来自 FFprobe / FFmpeg。
- 内部媒体时间继续使用 `Timestamp` / `Duration`；`f64` 秒只出现在
  mpv API 边界，不能成为项目的时间表示。
- 预览固定为显示尺寸；导出与编辑不得依赖预览帧或预览时间。
- ADR 0009 的 OQ-7（预览必须使用源 PTS）在方案 F 下不再适用；
  如果未来需要帧精确预览，另立 ADR。

## HDR / 10-bit

mpv-engine 导出 8-bit BGRA（Linux `AR24`/`XR24`），HDR 素材会被 mpv
tone map 到 SDR。

- 当前产品以 8-bit stream copy 裁切为主，8-bit 预览满足需求。
- stream copy 导出不受预览位深影响。
- smart cut 重编码必须从源流读取 `pix_fmt`、primaries、transfer、matrix，
  保持 10-bit/HDR 元数据；不得把预览的 8-bit 帧作为重编码输入。
- HDR 原样预览、10-bit 输出仍是 OQ-13；如果产品需要，再评估更高位深导出
  或回到 libav* 路径。

## 窗口尺寸

- `set_export_size` 必须使用**物理像素**，不是 egui 的逻辑点。
- eframe 0.37 在 Wayland scale 下逻辑 1280x720 对应物理 1707x960；
  实现需要从 `egui::Context` / `ViewportInfo` 获取像素尺寸。
- resize 时要节流，避免每个事件都重配置 export；原始帧尺寸变化后再更新。
- resize 目前未在 spike 中验证，是 OQ-10 的剩余项。

## 测试与验收

### 单元测试

- `Timestamp` ↔ mpv 秒参数的转换。
- `hr-seek=no/yes` 状态机；拖动开始/结束的幂等性。
- 回退选择：引擎不可用、feature 不可用、导入失败。
- 错误上下文包含路径、目标时间与后端。

### 集成测试（Linux 必需）

- 4K HEVC：导出帧 → wgpu 读回与 `copy_pixels()` 字节一致。
- 纹理生命周期：detach / drop 后仍可读；帧池回收不串帧。
- eframe 红/灰颜色回归：BGRA 通道顺序与 sRGB 不偏移。
- 音频：播放、暂停、暂停中 seek、音量、静音；
  `position()` 与 `audio-pts` 在容差内。
- scrub：`hr-seek=no` 落在关键帧；`hr-seek=yes` 落点误差在 1 帧内；
  延迟记录到报告中。
- resize：物理像素变化后画面比例与清晰度正确。
- 回退：缺少 libmpv / DMA-BUF feature 时回到外部 FFmpeg，并在 UI 显示。

### 平台验收

- Linux：本机 RADV 实测。
- Windows：D3D11VA + DX12 导入，真实机器验证。
- macOS：VideoToolbox + Metal/IOSurface，真实机器验证。
- 未完成前，不得在文档或 UI 声称对应平台已支持。

### 性能目标（本机 4K HEVC）

- 预览 ≥ 30 fps。
- CPU ≤ 25%（单核口径，按 spike 约 18.5%）。
- 快速 seek ≤ 300 ms；精确 seek ≤ 500 ms。
- 预览路径无 CPU 像素回读。

## 后果

### 正面

- 预览保持 GPU-resident，避免 4K VAAPI 下载瓶颈。
- 不再需要自建 libav* 绑定、三平台 hwaccel 与 surface 导入。
- 音频、seek、VFR、容器与 codec 覆盖由 mpv 提供。
- eframe 0.37 / wgpu 30 与 mpv-engine 0.2.1 版本已对齐。
- 项目代码仍无 unsafe；许可证风险由产品决策接受。

### 负面

- 运行时依赖 libmpv；分发受 GPL 约束。
- wgpu 大版本与 mpv-engine 强耦合，升级需要同步。
- 隐藏 GL context 增加平台依赖与调试复杂度。
- 预览不暴露源 PTS；不能用于帧精确编辑。
- 导出固定 8-bit BGRA；HDR 预览受限。
- Windows / macOS 路径尚未实测。
- mpv-engine 0.2.1 很新，维护与升级风险由项目承担。
- 现有 `PreviewAudio`、`PreviewVideo` 与相关测试需要替换或保留为回退。

## Phase 4C 实施步骤

以下步骤都发生在 Phase 4C 内，**不是项目 Phase 1**。Phase 1/2/3 已完成，
不重做媒体分析、FFprobe、时间轴或现有编辑功能。

1. 加入 `mpv-engine` 依赖与平台检测；保留外部 FFmpeg 回退。
2. 实现 `media::preview::mpv`：生命周期、attach export、acquire frame、
   命令、错误与状态。
3. UI 接入：请求 wgpu feature、注册纹理、颜色/生命周期回归。
4. 音频切换：mpv `ao` 成为主路径，移除 cpal 主时钟。
5. scrub 状态机：`hr-seek=no` → `hr-seek=yes`，事件与位置兜底。
6. resize / Wayland scale 的物理像素处理。
7. 回退与播放信息：显示实际后端；错误路径可见。
8. 测试、文档与 AGENTS 条款修订；稳定后再删除旧预览路径。

## 需要同步修订的文档

- `AGENTS.md`：预览解码/音频边界、GPU 句柄传递范围、依赖与许可证条款。
- `docs/architecture.md`：预览数据流。
- `docs/media-model.md`：预览时间与编辑时间的边界（如需要）。
- `PLANS.md`：Phase 4C 任务改为方案 F。
- `docs/open-questions.md`：OQ-3/4/5/6/7 标记为方案 D 历史项；
  OQ-10..13 作为方案 F 的待办。

## 旧预览路径的退出条件

`PreviewVideo` 与 `PreviewAudio`（外部 FFmpeg + cpal）目前同时承担三条回退：

1. `mpv-preview` feature 关闭的构建（无 libmpv）。
2. eframe 没有 wgpu render state。
3. mpv 启动或帧导入失败。

因此它们不能在 Phase 4C 删除。可以删除的前提是：

1. 项目只提供带 libmpv 的构建，或者回退改为不依赖这两条进程路径的实现。
2. Windows 与 macOS 的 mpv 导入路径完成实机验证（目前只有 Linux）。
3. `tests/mpv_gpu_regression.rs` 与 `tests/mpv_preview.rs` 成为发布检查的一部分，
   覆盖颜色、生命周期、seek 与音频时钟。
4. OQ-9（长时漂移）、OQ-11（多音轨 / 设备切换）、OQ-13（HDR / 10-bit）
   已有可接受结论，或明确记录为不阻塞发布。

删除时同步移除 `cpal` 依赖、`PreviewAudio`、`PreviewVideo` 及对应测试，
并更新 `AGENTS.md`、`PLANS.md` 与 `docs/architecture.md`。

## 未解决问题

- OQ-9：10 分钟以上 A/V 漂移。
- OQ-10：resize / Wayland scale 的物理像素处理。
- OQ-11：多音轨、音频设备切换、真实扬声器延迟。
- OQ-12：拖动期间快速 scrub 在真实 eframe + 音频下的复测。
- OQ-13：HDR / 10-bit 预览。
- Windows D3D11VA / macOS VideoToolbox 的实机验证。
- 多 GPU / 多 render node 时 mpv 的设备选择策略。
- libmpv 分发许可证的最终审查与 `LICENSES` 清单。

## 参考

- `docs/investigations/mpv-engine-spike.md`
- `docs/decisions/0009-preview-decode-zero-copy.md`
- `docs/decisions/0004-audio-clock-and-desktop-integration.md`
- mpv-engine README 与 `src/export/wgpu_linux.rs`（设计参考，不复制代码）

# mpv-engine export + wgpu spike

状态：`Linux 独立 wgpu、eframe、音频与 scrub 已实测；resize / HDR / Windows / macOS 未验证`

本文件只记录实际运行的命令与数据。结论中标注为“推断”的部分没有实测。

## 目的

评估 `mpv-engine 0.2.1` 的 `export` + `wgpu` feature 能否替代或补充 ADR 0009 的
“进程内 libav* + 平台 hwaccel + surface 导入”预览方案。

重点回答：

- Linux DMA-BUF → wgpu 导入是否真的零拷贝、生命周期是否安全。
- 4K HEVC 在本机 AMD Vega 8 / RADV 上的实际吞吐与资源占用。
- 暂停 seek 后能否按需产出新帧，位置是否可用。
- 接入 eframe 0.37 的具体风险。

## 环境

| 项 | 值 |
| --- | --- |
| OS | Arch Linux，Wayland（niri） |
| GPU | AMD Radeon Vega 8（RADV / radeonsi） |
| 渲染节点 | `/dev/dri/renderD128` |
| FFmpeg | `n9.0.2` |
| mpv | `1:0.41.0-6`（包 SPDX：`GPL-2.0-or-later AND LGPL-2.1-or-later`） |
| libmpv | `pkg-config --modversion mpv` = `2.5.0` |
| EGL / GBM | `1.5` / `26.2.5-arch1.1` |
| Rust / Cargo | `1.99.1` |
| 测试素材 | `/home/chasel/videos/bilibili/Immich相册部署和使用教程.mp4`（HEVC 3840×2160，59.94 fps） |
| spike 目录 | `target/spike/mpv-engine`（已被 `.gitignore` 忽略） |

## 执行的命令

```bash
git clone --depth 1 https://github.com/ophymx/mpv-engine target/spike/mpv-engine

cd target/spike/mpv-engine
cargo test --release --features wgpu --test export_wgpu -- --nocapture
cargo build --release --features wgpu --example winit_player
cargo build --release --features wgpu --example spike_stats

./target/release/examples/spike_stats '/home/chasel/videos/bilibili/Immich相册部署和使用教程.mp4'
```

`spike_stats` 是 `winit_player` 的本地副本，只增加帧计数、DMA-BUF 参数输出、
`position()` 输出，以及一次暂停 + `seek_absolute(60.0)`。它不进入项目仓库。

## 结果

### 1. wgpu 导入测试

```text
running 3 tests
test imported_texture_matches_frame_pixels ... ok
test texture_outlives_frame_and_detach ... ok
test continuous_import_recycles_buffers_and_stays_correct ... ok
test result: ok. 3 passed; 0 failed; finished in 1.76s
```

覆盖内容：

- 导出 buffer 经 `into_wgpu_texture` 后，通过 wgpu 读回的像素与 `copy_pixels()` 字节一致。
- 纹理在 `detach_render()` 与 `drop(engine)` 之后仍可读，说明 wgpu 拥有底层内存。
- 连续导入超过 pool 容量的帧，buffer 被回收复用且画面不串帧。

本机没有出现 “adapter lacks `VULKAN_EXTERNAL_MEMORY_DMA_BUF`” 的跳过提示，说明 RADV
暴露了该 feature，且 wgpu 成功启用了它。

### 2. 4K HEVC 播放

```text
spike: frame 2017x1508 fourcc=0x34325241 stride=8192 modifier=0x0
spike: t=2.0s frames=52  fps=25.8 paused=false time-pos=1.735
spike: t=4.0s frames=118 fps=29.2 paused=false time-pos=3.754
spike: t=6.1s frames=183 fps=30.2 paused=false time-pos=5.756
```

- `fourcc=0x34325241` 是 `AR24`；`modifier=0x0` 是 linear modifier。
- 导出尺寸 `2017x1508` 是窗口尺寸，mpv 在 GPU 内完成缩放与 letterbox。
- 播放稳定在约 29–30 fps。
- 12 秒时 `ps` 采样：`%CPU 18.5`、`RSS 275452 KB`（约 269 MiB）。
- `Cannot load libcuda.so.1` 来自 wgpu 枚举 NVIDIA 后端，不影响 Vulkan 路径。

### 3. 暂停 + seek

```text
spike: paused + seek 60.0 at frames=183
spike: paused-after-seek frames_delta=4 time-pos=60.010
spike: t=17.0s frames=200 fps=11.7 paused=false time-pos=62.012
```

- 暂停状态下 `seek_absolute(60.0)` 后仍产出了新帧（`frames_delta=4`）。
- `position()`（`time-pos`）为 `60.010`，与目标相差 10 ms。
- 恢复播放后继续正常推进。
- `fps` 字段是“总帧数 / 总墙钟”，暂停期间被拉低，不是瞬时帧率。

### 4. eframe 0.37 端到端

```text
cargo run --release --features wgpu --example eframe_spike -- red.mp4
Cannot load libcuda.so.1
screenshot 1707x960 center=(253, 0, 0, 255)
spike: center pixel is red -> BGRA channel order OK
```

- 窗口逻辑尺寸 1280x720；Wayland scale 下截图是 1707x960（物理像素）。
- 纯红视频中心像素 `(253, 0, 0, 255)`：`Bgra8Unorm` 纹理可以直接交给
  `register_native_texture`，颜色通道顺序正确。
- 50% 灰视频中心像素 `(127, 128, 127, 255)`：未见明显 sRGB 亮度偏移。
- eframe 的 device 通过 `WgpuSetupCreateNew.device_descriptor` 请求
  `mpv_engine::REQUIRED_WGPU_FEATURES`，并把 `instance_descriptor.backends`
  固定为 `mpv_engine::WGPU_BACKEND`（Linux 为 Vulkan）。
- eframe 0.37 的 `App` trait 使用 `ui(&mut self, &mut Ui, &mut Frame)`，不是
  `update`；这与项目当前 eframe 版本一致。

### 5. 音频

命令：

```bash
cargo run --release --example audio_spike -- av.mp4
```

素材：H.264 + AAC，44.1 kHz，单声道，10 秒。引擎为 `Engine::headless()`，
默认 mpv 音频输出，音量 10。

```text
t=1.0s: pos=1.0 audio-pts=0.9659 device=auto format=floatp rate=44100 ch=1 vol=10
t=2.0s: pos=2.0 audio-pts=1.9728
t=6.0s paused: pos=6.0 audio-pts=5.9972
paused + seek 3.0: pos=3.0 audio-pts=2.99998 (PlaybackRestart)
resumed: pos=4.0 audio-pts=3.9849
volume 50 + mute: vol=50
eframe + audio: pos=0.8 audio-pts=0.7208 format=floatp rate=44100 ch=1 vol=5
eframe + 4K: audio 48000 Hz, 2 ch
```

- 音频输出初始化成功：`audio-params/format=floatp`、`samplerate=44100`、
  `channel-count=1`。`audio-device` 读回 `auto`，没有单独查询实际 AO。
- 稳定播放时 `position()` 与 `audio-pts` 相差约 11–16 ms；0.8 秒时相差约
  79 ms，属于启动/输出缓冲的瞬态。
- 暂停、暂停中 seek、恢复、音量、静音均正常；`position()` 在暂停 seek 后为
  目标值。
- 无法由 agent 人工听音；以上依据是音频参数非空、`audio-pts` 持续推进、
  `position()` 与音频 PTS 同步。

### 6. Scrub

命令：

```bash
cargo run --release --example scrub_spike -- <4K HEVC>
```

引擎为 `Engine::headless()`，`audio=no`，`hwdec=auto-safe`，目标是同一 4K HEVC
素材。测量从 `seek_absolute` 发出到 `PlaybackRestart`（`hr-seek=no`）或
`position()` 到达目标 ±0.05s（`hr-seek=yes`）的时间。

| 模式 | 状态 | 延迟 | 落点误差 |
| --- | --- | --- | --- |
| `hr-seek=no` | 播放中 | 132–193 ms | 4.87–4.98 s（前一个关键帧） |
| `hr-seek=no` | 暂停 | 128–313 ms | 4.88–5.00 s（前一个关键帧） |
| `hr-seek=yes` | 播放中 | 46–331 ms | 0.00 s |
| `hr-seek=yes` | 暂停 | 0.4–305 ms | 0.00 s |

- 该素材的关键帧间隔约 5 秒，因此 `hr-seek=no` 会回退到目标前约 5 秒的关键帧。
  这是预期行为，UI 在拖动期间必须显示实际落点，而不是请求的目标时间。
- `hr-seek=yes` 在 4K HEVC 上也只需约 200–330 ms（首次 46 ms），精确 seek
  可以作为松手后的最终 seek。
- `PlaybackRestart` 在 `hr-seek=no` 下会触发；在 `hr-seek=yes` 的 headless /
  `vo=null` / `audio=no` 组合下没有在 20 秒内触发，但 `position()` 已到达目标。
  实现不能只依赖该事件，需要用 `position()` 或观察 `time-pos` 兜底。
- 真实 eframe 渲染 + 音频下复测：4K HEVC + AAC 48 kHz 2ch，`hr-seek=yes`
  到 6.0 用时 365 ms，`position=6.039`，`audio-pts=6.015`，
  `PlaybackRestart=true`；320x240 红片同路径用时 50 ms。
- 4K eframe 截图的中心像素是 `(244,244,244)`，因为该素材 6 秒处不是纯红；
  颜色正确性以 `red.mp4` 的 `(253,0,0)` 和 `gray.mp4` 的 `(127,128,127)` 为准。

### 7. GPU 引擎归属（2026-10-26）

用户观察到预览时 GPU encode engine 有占用。`nvtop -s` 快照显示：

```text
benchmark 期间（4K HEVC 自动播放）:
  /usr/bin/sunshine          encode=35%  gpu_usage=null
  ./target/release/rossresscut encode=null  decode=72%  gpu_usage=77%

空闲基线（未启动 rossresscut）:
  /usr/bin/sunshine          encode=40%
```

- encode engine 的占用来自 **Sunshine 串流编码**，不是 rossresscut 预览。
- rossresscut 的预览路径使用 **decode engine（VAAPI HEVC 解码）** 和
  **GPU 3D/shader**；`encode` 为 `null`。
- `960x540` 导出限制只影响 FBO 尺寸与 GPU 缩放/带宽，不触发视频编码。
- 因此“不缩放直接播放源素材”不会省掉一个编码步骤；它只会把缩放从
  mpv 的 export FBO 移到 egui 绘制阶段，并显著增加导出纹理的显存/带宽
  （4K BGRA 约 33 MB/帧）。当前 8-bit 裁切预览保留 960x540 输出。

## 关键事实

- `ExportedFrame` 的公开 API **没有 PTS**。可用字段是 `width`、`height`、
  `fourcc`、`stride`、`modifier`、`dma_buf_fd`（Linux）、`copy_pixels`。
  播放位置只能通过 `Engine::position()` 读取 `time-pos`。
- `Engine` 提供 `seek_absolute(f64)`、`seek_relative(f64)`、`set_paused`、
  `load_paused`、`render_update`、`acquire_frame`、`position`。
- mpv-engine 的 `wgpu` feature 与项目 eframe 0.37 都解析到 `wgpu 30.0.1` /
  `wgpu-hal 30.0.1`，版本一致。
- Linux 导入使用 `texture_from_raw` + `DropCallback` + `TextureMemory::External`，
  避免 `texture_from_dmabuf_fd` 的 `drop_callback: None` 导致每帧重新分配。
- 导出格式是 8-bit BGRA (`AR24`/`XR24`)，不是 10-bit/HDR。
- mpv 自己输出音频时，`position()` 与 `audio-pts` 稳定相差约 11–16 ms；
  音量、静音、暂停、暂停中 seek 都可用。
- `hr-seek=no` 是快速关键帧 seek（约 5 秒粒度）；`hr-seek=yes` 是精确 seek，
  在 4K HEVC 上约 200–330 ms。
- `nvtop` 的 encode 占用可能来自 Sunshine 串流；判断预览负载要看
  rossresscut 进程的 `decode` 与 `gpu_usage`，以及 `encode` 是否为 `null`。

## eframe 集成风险

以下第 1–4 条已在 Linux 上做过端到端验证；其余仍是待验证项。

1. **device feature**：mpv-engine 的导入要求
   `wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`。eframe 的默认
   `WgpuSetupCreateNew.device_descriptor` 不请求它，需要在
   `NativeOptions.wgpu_options` 里替换 `device_descriptor`。
   spike 已证明 RADV 支持该 feature。
2. **版本一致性**：eframe 0.37 与 mpv-engine 都使用 wgpu 30.0.1；若项目后续升级
   eframe，必须同步升级 mpv-engine，否则 `Device::as_hal` 的类型不匹配。
3. **egui 纹理格式**：`register_native_texture` 的文档要求 `Rgba8Unorm`，而
   `ExportedFrame::into_wgpu_texture` 返回 `Bgra8Unorm`。实测可以直接注册：
   红色 `(253,0,0)`、灰色 `(127,128,127)` 都正确，不需要额外 blit。
   这是当前 eframe/egui-wgpu 0.37 的行为；升级版本时需要回归这条测试。
4. **色彩空间**：mpv 的 8-bit 输出是 SDR/BT.709（HDR 会被 tone map）。
   50% 灰的截图值与输入一致，未见明显 sRGB 偏移；HDR 素材仍未测。
5. **回退路径**：如果 device 没有 DMA-BUF feature、驱动拒绝导入、或平台不是
   Vulkan/Metal/DX12，需要回退到 `render_sw`（CPU RGBA）或现有外部 FFmpeg 预览。
   UI 的“解码后端”必须显示实际生效的路径。

## 许可证

- `mpv-engine` 代码：MIT OR Apache-2.0。
- 它链接的 libmpv：mpv 默认构建是 GPLv2+；只有 `-Dgpl=false` 构建才是
  LGPLv2.1+。Arch 包 SPDX 为 `GPL-2.0-or-later AND LGPL-2.1-or-later`，
  分发前必须确认实际 libmpv 的构建选项。
- 如果分发时带上 GPL 构建的 libmpv，整个组合作品按 GPLv2+ 分发；
  如果不带、依赖用户系统库，动态链接 GPL 库仍可能被视为衍生作品。
- 与外部 `ffmpeg` 进程不同：后者没有链接，不构成衍生作品。

## 未验证

- 窗口 resize / Wayland scale factor：`set_export_size` 目前固定为逻辑 1280x720，
  未按物理像素更新。
- 音频：多音轨选择、音频设备切换、真实扬声器延迟；10 分钟以上的 A/V 漂移。
- Scrub：拖动期间的快速 scrub 在真实 eframe 渲染 + 音频下的延迟；
  关键帧列表与 `frame-step` 行为。
- HDR / 10-bit 素材的实际 tone map 结果。
- Windows D3D11VA / macOS VideoToolbox 的导出路径（本机无法验证）。
- 长时间播放的内存稳定性。

## 对 ADR 0009 的影响

本 spike 证明 `mpv-engine` 的 Linux 路径可以做到 GPU-resident 的 DMA-BUF → wgpu 导入，
4K HEVC 约 30 fps、CPU 约 18.5%、RSS 约 269 MiB。它满足 ADR 0009 的 zero-copy 目标，
但底层是 libmpv，不是 libav*。

因此 ADR 0009 需要补充一条候选方案：

> 方案 F：libmpv 隐藏 GL context 渲染 → DMA-BUF / IOSurface / shared D3D11 导出 →
> wgpu 导入（mpv-engine）。

eframe 集成、mpv 音频输出和快速/精确 scrub 均已在 Linux 上验证。
剩余阻断项只有窗口 resize / scale factor、HDR 预览和 Windows/macOS 路径。
如果采用，ADR 需要明确：

- 预览音频由 mpv `ao` 输出，`position()` 作为 UI 时钟；ADR 0004 的 cpal 主时钟
  被取代，现有 `PreviewAudio` 需要移除或改为非主时钟用途。
- 拖动时 `hr-seek=no`，松手时 `hr-seek=yes`；拖动期间 UI 显示实际关键帧位置。
- 不能把 `position()` 当作源帧 PTS；Segment 边界和导出仍使用 FFprobe / FFmpeg。

如果不采用，`src/export/wgpu_linux.rs` 的导入与生命周期设计仍可作为
`media/platform` 的参考实现；不复制代码，只记录设计结论。

## 2026-10-26 产品决策与待办

产品负责人确认：

- **许可证不是阻碍**。接受 GPLv2+；发布时按实际链接的 libmpv 许可证分发。
- **第二套播放运行时不是阻碍**。预览与导出可以分开：libmpv 负责预览，外部 FFmpeg
  负责 stream copy 导出。

因此方案 F 的剩余评估项不是许可证或“第二套栈”，而是：

1. ~~eframe 0.37 端到端~~：已通过（红色/灰色、DMA-BUF feature、纹理生命周期）；
   剩余 resize / Wayland scale factor。
2. ~~音频~~：mpv `ao` 输出可用，`position()` 与 `audio-pts` 稳定相差约 11–16 ms；
   音量/静音/暂停/seek 均正常。整合方向是 mpv 独占音频，取代 cpal 主时钟。
   剩余：多音轨、设备切换、真实扬声器延迟、长时漂移。
3. ~~快速 scrub~~：`hr-seek=no` 约 130–190 ms、关键帧粒度约 5 s；
   `hr-seek=yes` 约 200–330 ms、落点精确；真实 eframe + 4K + 音频下
   `hr-seek=yes` 为 365 ms。剩余：拖动期间快速 scrub 的 eframe 复测。
4. HDR / 10-bit：mpv tone map 后的预览是否满足产品预期。
5. Windows D3D11VA / macOS VideoToolbox 的导出路径（本机无法验证）。

# 未解决问题

本文件记录已经识别、但当前阶段不解决的问题。

规则：

- 每条包含：问题、当前状态、影响、计划处理时机、解除条件。
- 已解决的问题从本文件删除，或在标题标注「已解决」并写明结论所在文档。
- 不把可以立刻查清的技术事实写在这里；先调查，再决定是否记录。

---

## 预览解码（来源：ADR 0009）

> 2026-10-26：方案 D 已被 `docs/decisions/0010-preview-via-mpv-engine.md` 取代。
> OQ-1（设备选择）与 OQ-2（HDR）在方案 F 下仍相关；OQ-7 已按方案 F 更新；
> OQ-3/4/5/6/8 保留为方案 D 或旧路径的历史记录。

### OQ-1 多 GPU / 多 render node 的设备选择

**问题**：Linux 上存在多个 `/dev/dri/renderD*`（核显 + 独显）时，如何选择 VAAPI 设备；Windows 上如何在 D3D11 适配器之间选择。

**当前状态**：未实现。现有外部 FFmpeg 路径固定使用 `/dev/dri/renderD128`。

**影响**：错误的设备会导致解码失败或性能显著下降；双显卡笔记本风险最高。

**计划时机**：Phase 4C 实现 `libavcodec` 设备初始化时。

**解除条件**：有明确的设备枚举与选择规则，并能在至少一台多 GPU 机器上验证。

### OQ-2 10-bit / HDR / 非 4:2:0 的 surface 格式与色彩转换

**问题**：硬件解码输出可能是 NV12、P010、YUV420P10 等格式；HDR 素材需要正确的色彩空间与传递函数处理。wgpu 侧需要相应的纹理格式与 shader 转换。

**当前状态**：未实现。Phase 4A 只测了 8-bit `yuv420p`。

**影响**：10-bit / HDR 素材可能出现颜色错误、带宽错误或无法导入。

**计划时机**：Phase 4C 完成 8-bit 4:2:0 后再处理。

**解除条件**：至少覆盖 8-bit 4:2:0 与 10-bit 4:2:0，并有与软件解码的像素级或容差内对比测试。

### OQ-3 `wgpu-hal` 外部纹理导入的驱动兼容性

**问题**：`create_texture_from_hal` 与各后端 `texture_from_raw` 在 Mesa（RADV）、NVIDIA、Intel 驱动上的实际可用性、对齐要求与同步语义。

**当前状态**：仅在 API 层面确认存在；未做端到端验证。

**影响**：某些驱动可能拒绝导入，或需要特定的 modifier / 内存布局。

**计划时机**：Phase 4C 在 Linux 上先做最小 DMA-BUF 导入实验。

**解除条件**：至少在本机 RADV 上验证成功，并记录失败时的回退行为。

### OQ-4 Windows D3D11 shared handle 与 DX12 后端

**问题**：D3D11VA 解码出的纹理需要通过 shared handle 导入 DX12 后端的 wgpu。共享标志、纹理格式、同步与生命周期需要确认。

**当前状态**：未实现，无法在本机验证。

**影响**：Windows 路径可能无法 zero-copy，只能回退 readback。

**计划时机**：Phase 4C 的 Windows 部分。

**解除条件**：在真实 Windows + D3D11VA 环境验证；若不可行，明确记录回退方案。

### OQ-5 macOS IOSurface ownership 与生命周期

**问题**：VideoToolbox 输出的 `CVPixelBuffer` / `IOSurface` 由谁持有、何时释放，以及导入 wgpu Metal 纹理后的引用关系。

**当前状态**：未实现，无法在本机验证。

**影响**：生命周期错误会导致崩溃或花屏。

**计划时机**：Phase 4C 的 macOS 部分。

**解除条件**：在真实 macOS 环境验证，并有释放顺序测试。

### OQ-6 `libav*` 最低版本与发行版差异

**问题**：项目将链接系统 `libavformat` / `libavcodec`。不同发行版的 FFmpeg 版本、hwaccel API 与头文件可能不同。

**当前状态**：本机为 FFmpeg 9.0.2；未确定最低支持版本。

**影响**：构建失败或运行时符号缺失。

**计划时机**：Phase 4C 开始前确定版本下限与检测方式。

**解除条件**：在 `Cargo.toml` / 构建脚本中声明最低版本，并在 CI 或文档中记录。

---

## Phase 4A 遗留（来源：`docs/investigations/playback-pipeline.md`）

### OQ-7 预览时间戳不是源 PTS

**问题**：当前预览帧时间戳是 `fps=30` 合成 CFR，不是源 PTS。

**当前状态**：方案 F 下预览不暴露源 PTS；UI 位置使用 mpv 的
`position()`（`time-pos`），它是播放时钟，不是当前纹理对应的源 PTS。

**影响**：若把 `position()` 当成源 PTS 用于 Segment 或导出，会产生错误。

**计划时机**：如果未来需要帧精确预览，另立 ADR。

**解除条件**：预览位置只用于 UI / scrub 反馈；Segment 与导出仍使用
FFprobe / FFmpeg。需要帧精确预览时再评估方案 D 或扩展 mpv-engine。

### OQ-8 音频设备缓冲提前量

**问题**：`cpal` 回调写入设备缓冲不等于已被听到，音频时钟可能领先一个输出缓冲周期。

**当前状态**：Phase 4A 未测量。

**影响**：稳定的固定偏移可能被误判为同步误差；极端情况下影响帧选择。

**计划时机**：Phase 4C 音频迁入进程内后。

**解除条件**：有可重复的测量方法与可接受误差记录。

### OQ-9 分钟级长时 A/V 漂移

**问题**：Phase 4A 在外部 FFmpeg 路径上测到 40 秒内 delta 增长 11.6 秒。
换到 mpv 主路径后，长时播放是否仍会不同步未知。

**当前状态**：原漂移是外部 FFmpeg 路径的结构性问题（音频时钟与独立视频帧调度）。
mpv 主路径由 libmpv 内部同步：默认 `video-sync=audio`，应用层只读取 `time-pos`，
不再自行选择视频帧或推算墙钟，因此原漂移机制不存在。尚未跑过 10 分钟连续播放；
剩余风险是 mpv 与音频设备的时钟处理、VFR 与长时内存稳定性。

**影响**：只影响长素材预览；不影响导出。

**计划时机**：Phase 4C 完成后，作为发布前回归。

**解除条件**：mpv 路径连续播放至少 10 分钟，记录 `time-pos` 与 `audio-pts`
差值、丢帧/重复帧计数与内存趋势，结果在可接受范围内。

---

## mpv-engine 方案（来源：`docs/investigations/mpv-engine-spike.md`）

### OQ-10 eframe 0.37 端到端导入

**问题**：mpv-engine 的 `ExportedFrame` 是 `Bgra8Unorm`，egui-wgpu 的
`register_native_texture` 文档要求 `Rgba8Unorm`；需要确认纹理注册、颜色/声道顺序、
设备 feature 请求与生命周期。

**当前状态**：Linux 独立 wgpu 与 eframe 0.37 窗口内显示均已验证；纯红
`(253,0,0)`、50% 灰 `(127,128,127)` 正确，`Bgra8Unorm` 可直接注册，无需额外 blit。
预览导出尺寸已改为按预览窗格大小等比适配源宽高比（横屏、竖屏均覆盖，且不放大），
按物理像素下发给 mpv，尺寸不变时不重复配置；窗口拖动过程中连续变化的尺寸
会等到连续两帧一致后才下发，避免每帧重新配置 export。
颜色、BGRA 通道顺序与纹理生命周期已由 `tests/mpv_gpu_regression.rs` 自动化覆盖
（默认 ignored，需 Vulkan + FFmpeg）。剩余人工拖动窗口边验证。

**影响**：若无法直接注册，需要在 GPU 上做一次 BGRA→RGBA blit 或自定义 paint
callback；不得回退到 CPU 回读。

**计划时机**：方案 F 评估阶段。

**解除条件**：eframe 窗口中显示正确颜色，且有暂停/seek/resize 的回归测试。
颜色、暂停/seek 与窗格等比尺寸已满足；颜色与生命周期已有自动化回归；
剩余窗口缩放的交互验证与自动 GPU 回归的 CI 接入。

### OQ-11 mpv 音频与 cpal 主时钟

**问题**：若采用 mpv-engine，音频由 mpv 的 `ao` 输出还是继续由项目 `cpal` 输出；
两种方式的播放位置、暂停、seek 与音量控制如何统一。

**当前状态**：mpv `ao` 输出已验证（`floatp`/44100/单声道）；稳定播放时
`position()` 与 `audio-pts` 相差约 11–16 ms；音量、静音、暂停、暂停中 seek
均正常。方向是 mpv 独占音频，取代 cpal 主时钟。

**影响**：影响 ADR 0004 的音频主时钟设计与现有 `PreviewAudio`。

**计划时机**：方案 F 决策后。

**解除条件**：确定单一音频输出路径；剩余多音轨、设备切换、真实扬声器延迟、
10 分钟以上 A/V 漂移。

### OQ-12 快速 scrub 与帧精确 seek

**问题**：mpv 的 `hr-seek` 行为、关键帧 seek 延迟、暂停时单帧产出，是否满足
时间轴拖动与 Segment 边界预览。

**当前状态**：`hr-seek=no` 延迟约 130–190 ms（播放中）/128–313 ms（暂停），
落在前一个关键帧，误差约 5 s；`hr-seek=yes` 延迟约 200–330 ms，落点误差 0。
真实 eframe + 4K + 音频下 `hr-seek=yes` 到 6.0 用时 365 ms，落点 6.039。
headless/`vo=null`/`audio=no` 下精确 seek 不触发 `PlaybackRestart`，需用
`position()` 或观察 `time-pos` 兜底；eframe + 音频下事件正常触发。
实现已在松手时使用指针最终位置发起精确 seek，避免节流目标滞后；
`PlaybackRestart` 未到达时，精确 seek 落点与目标相差 ≤100 ms 即视为完成。
`tests/mpv_preview.rs` 覆盖快速/精确 seek 与暂停恢复。

**影响**：拖动体验；不得把预览位置用于 Segment 或导出。

**计划时机**：方案 F 决策后。

**解除条件**：拖动期间的快速 scrub 在真实 eframe + 音频下的连续拖动手感复测；
关键帧列表与 `frame-step` 行为。

### OQ-13 HDR / 10-bit 预览

**问题**：mpv-engine 导出 8-bit BGRA；HDR/10-bit 素材会被 tone map。

**当前状态**：未测。当前产品以 8-bit 裁切为主。

**影响**：只影响预览颜色，不影响 stream copy 导出；smart cut 重编码必须从源流
保留颜色元数据。

**计划时机**：8-bit 路径稳定后。

**解除条件**：有 HDR/10-bit 素材的对比结果，并明确是否需要更高位深导出。

---

## 多段导出（来源：Phase 5 / `docs/investigations/multi-segment-concat.md`）

### OQ-14 拼接处 ≤1 帧的视频间隙

**问题**：concat demuxer 按每段容器时长偏移；音视频流长度不完全对齐时，拼接处会出现约 16–33ms 的视频间隙。音频连续。

**当前状态**：已确认。`-shortest` 可把额外间隙从约 33ms 降到约 16ms，但无法消除。

**影响**：多段输出的拼接点可能有一帧停顿；不是帧精确。

**计划时机**：无明确计划。

**解除条件**：有实验证明某种无损 remux 方式可以消除间隙，或产品接受该语义并写入用户文档。

### OQ-15 起点回退到关键帧

**问题**：Segment 起点不是关键帧时，切片从更早的关键帧开始，输出包含额外内容；输出时长不等于 `sum(end - start)`。

**当前状态**：已确认并用帧哈希验证。

**影响**：输出范围可能比用户设置的范围大。

**计划时机**：Phase 8 的 smart cut 调查。

**解除条件**：smart cut 决策记录后，明确何时使用重编码来精确起点。

### OQ-16 Stream selection、metadata 与 chapter

**问题**：当前导出使用 `-map 0 -c copy`，导出全部流；没有显式 metadata / chapter 处理。

**当前状态**：未实现。PLANS 将 Stream selection、Metadata、Chapters 归入 Phase 8。

**影响**：多余流会被一起导出；章节不会按 Segment 生成。

**计划时机**：Phase 8。

**解除条件**：按 Phase 8 的独立设计实现。

### OQ-17 4K / 60fps / VFR / 字幕 / 章节验证

**问题**：多段合并目前只在 1080p30 H.264/AAC 上验证。

**当前状态**：未测。

**影响**：高帧率或 VFR 素材的拼接间隙、时间戳行为可能不同。

**计划时机**：Phase 5 回归扩展或 Phase 8。

**解除条件**：至少覆盖 4K60、VFR、含字幕/章节的容器。

### OQ-18 多段导出的 UI 交互自动化

**问题**：导出按钮、进度、取消只做了启动冒烟，没有自动化 UI 交互测试。

**当前状态**：未实现。

**影响**：UI 回归只能靠人工。

**计划时机**：有 UI 自动化基础设施后。

**解除条件**：能自动触发导出并断言状态。

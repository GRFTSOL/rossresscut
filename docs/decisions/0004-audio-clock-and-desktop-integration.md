# 0004 音频主时钟、快速 scrub 与桌面集成

- 状态：已接受；§2 由 0005 取代；§1 音频主时钟与 §3 快速 scrub 的主路径由 0010 取代
- 日期：2026-10-24
- 阶段：Phase 2 修订

## 背景

真实 4K HEVC 文件测试暴露了 Phase 2 初始实现的问题：

- 拖动时间轴只在松开后更新位置，预览画面不跟随。
- 独立 `ffplay` 音频进程与墙钟驱动的视频之间存在明显 A/V 偏移。
- 打开文件依赖手工路径；Linux 应使用 XDG desktop portal。
- 缺少类似 mpv Tab 的播放信息，以及深色 / 浅色主题切换。

## 决策

### 1. 音频主时钟（取代 0003 的 ffplay 音频方案）

- 使用 `cpal` 的默认输出设备。
- FFmpeg 将 `0:a:0` 解码为匹配设备采样率与声道数的 interleaved `f32le` PCM。
- cpal 回调实际消费的样本帧数决定播放位置：

```text
position = start_position + played_frames / sample_rate
```

- 视频只显示 `timestamp <= position` 的最新帧；视频解码快于音频时保留有限背压。
- 无音轨时正常回退墙钟；音频设备不可用或输出格式不支持 f32 时回退墙钟并显示 warning。
- 暂停、seek、换片时停止并重建音频 / 视频会话。
- 不保留 `ffplay`。`ffplay` 只播放，不提供可查询的播放时钟。

`cpal` 只负责 PCM 输出；解码仍由 FFmpeg 完成，符合媒体后端边界。

### 2. 硬件解码：默认启用

产品决定：默认使用硬件解码。

- Linux：`-hwaccel vaapi -hwaccel_device /dev/dri/renderD128`。
- Windows：`-hwaccel d3d11va`。
- macOS：`-hwaccel videotoolbox`。

检测与回退：

- Linux 在启动时检查 `/dev/dri/renderD128`；设备不存在时使用软件解码并在日志中说明原因。
- Windows 与 macOS 直接请求对应后端。
- 解码后的帧仍下载到 CPU，缩放为 RGB24 后交给 egui。该管线不是 GPU 零拷贝，但硬件解码仍是默认路径。
- FFmpeg 对硬件后端不支持的 codec 可能内部回退到软件解码；当前不解析 stderr 以报告实际解码器。播放信息中的 Decoder 显示请求的后端。

本机是 CPU 受限的测试环境，不能以本机 wall-clock 结果否定硬件解码。详细记录见 `0005-hardware-decoding.md`。

### 3. 快速 scrub

- 拖动时间轴开始时暂停播放，并记录拖动前是否在播放。
- 拖动过程中使用 `-noaccurate_seek` 单帧预览，最多约 12Hz 启动一次。
- 同一时间只允许一个 scrub 解码任务；上一个预览未完成时不启动新的任务，避免持续 kill 进程。
- 松开时使用精确 seek；如果拖动前正在播放，则恢复播放。
- 快速 scrub 可能显示目标位置之前的关键帧。该位置只能用于视觉反馈，不能用于 Segment 边界、编辑结果或导出。

实测 4K HEVC：快速 keyframe seek 约 0.4s，精确 seek 约 3.3s。

### 4. Linux 文件打开

- 使用 `rfd` 的 `xdg-portal` 后端调用 `org.freedesktop.portal.FileChooser`。
- 文件对话框在后台线程运行，UI 通过 `mpsc` 获取结果，不阻塞 egui 帧。
- 顶部不再提供路径输入框；只显示不含扩展名的文件标题标签。
- 命令行仍支持传入初始媒体路径。
- 非 Linux 平台暂时禁用 Open 按钮；其他平台后续再实现。
- 依赖：`rfd`，仅 Linux 目标，`default-features = false, features = ["xdg-portal"]`。
  - 否决自建文件选择器：范围过大，且不符合桌面集成约定。
  - 否决 GTK 后端：引入 GTK 运行时，且不使用 portal。

### 5. 主题

- 使用 egui 的 `ThemePreference::{System, Dark, Light}`。
- 设置页提供“跟随系统主题”复选框与深色 / 浅色下拉框；默认跟随系统。
- 主题选择通过 eframe storage 持久化，见 `0006-ui-localization-and-settings.md`。

### 6. 播放信息

右侧面板顶部显示 mpv Tab 风格的播放信息，位于媒体文件信息之上：

```text
State / Position / Sync / Decoder / Preview / Frame / Video / Audio
```

- State、Position、Frame、Video、Audio 来自当前应用状态。
- Sync 显示 audio clock / wall clock / scrub preview。
- Decoder 显示请求的 FFmpeg 解码后端（例如 `ffmpeg (vaapi)` 或 `ffmpeg (software)`）。
- 媒体文件详情保持在下方，流信息默认折叠。

## 依赖选择

- `cpal` 0.18（Apache-2.0）：跨平台音频输出，Rust API 安全；Linux 使用 ALSA 后端。
- `rfd` 0.17：Linux 使用 XDG portal。被否决的替代方案见上。
- `eframe` 的 `persistence` feature：复用 eframe 自带的 storage 保存设置与 egui memory；新增传递依赖 `ron`。
- 没有引入 GStreamer、libmpv 或播放器 crate。预览后端与媒体处理是不同职责，选用播放器 crate 不算架构冲突；当前不引入是因为现有 FFmpeg 预览已满足阶段需求，且替换需要单独评估绑定与跨平台成本。

## 后果与限制

- 有音频的媒体需要音频输出设备；仅支持 f32 输出格式，其他格式回退墙钟并 warning。
- A/V 同步受音频设备缓冲延迟影响，通常为数十毫秒；没有独立播放器进程的启动偏移。
- 硬件解码受设备与 codec 支持影响；FFmpeg 可能内部回退到软件。
- 快速 scrub 是关键帧近似；播放信息中的 Frame 时间戳表示已显示帧，可能落后于 scrub 目标。
- 文件门户仅 Linux；未自动化实际选择文件，已确认 portal 服务与 `FileChooser` 接口存在。
- 主题切换通过截图与 egui Context 测试。
- 预览仍固定 30fps、最大 960x540；非方形 SAR 尚未补偿。
- 暂无音量、音频设备选择、音轨选择，也没有掉帧统计。

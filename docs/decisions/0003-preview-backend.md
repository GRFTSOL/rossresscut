# 0003 预览后端：外部 FFmpeg 帧解码

- 状态：视频帧解码部分有效；音频部分已被 0004 取代
- 日期：2026-10-24
- 阶段：Phase 2

> 2026-10-24 更新：真实文件测试发现独立 `ffplay` 进程会造成 A/V 偏移。
> 音频已改为 FFmpeg PCM + `cpal`，音频样本时钟为主时钟，见 `0004-audio-clock-and-desktop-integration.md`。
> 本文档的视频帧解码、背压、暂停时单帧预览与 seek 设计继续有效；快速 scrub 的 `-noaccurate_seek` 由 0004 引入。

## 背景

Phase 2 需要视频预览、播放 / 暂停 / seek、时间显示与音频播放。项目约束：

- 禁止 `unsafe` 与 FFI。
- 媒体处理由外部 FFmpeg / FFprobe 负责。
- egui 帧内不能执行长任务。
- 不引入未经评估的播放器 crate。

## 候选方案

1. 外部 `ffmpeg` 解码视频帧 + 外部 `ffplay` 播放音频（最初选中，音频部分已被 0004 取代）。
2. 外部 `ffmpeg` 解码音频 PCM + `cpal` / `rodio` 输出（0004 最终采用）。
3. GStreamer。
4. libmpv / mpv IPC。
5. 纯 Rust 解码器 crate。

评估：

- GStreamer / mpv：当前机器上开发库可用。粗看会引入另一套预览运行时；但这不构成对“FFmpeg 是媒体处理后端”的架构冲突。
  “媒体处理”指导出、裁切、合并等最终产出文件的操作；预览是播放与交互，不属于媒体处理。
  如果 libmpv 能显著改善预览正确性或硬件解码，可以按下面的流程替换预览后端。
  libmpv 需要窗口嵌入或 FFI；本项目禁止 FFI，因此实际选型必须先确认可用的安全绑定或 IPC 方案，并另立 ADR。
- `cpal` / `rodio`：可行，并能提供更精确的音频主时钟，但需要新增依赖、设备配置、PCM 缓冲和同步逻辑。Phase 2 先用更简单的 ffplay；如果实测 A/V 偏移不可接受，再以 ADR 替换音频后端。
- 纯 Rust 解码器：codec 覆盖不足，且违反“不自己实现 codec”。
- 外部 `ffmpeg` + `ffplay`：不增加 Rust 依赖，沿用现有进程管理模型，可以直接实现与测试。

## 决策

### 视频

```text
ffmpeg -nostdin -loglevel error -xerror
       -ss <position> -i <path>
       -map 0:v:0 -an -sn -dn
       -vf fps=30,scale=<w>:<h>:flags=fast_bilinear
       -pix_fmt rgb24 -f rawvideo pipe:1
```

- 预览输出固定 30fps CFR，最大 960x540，按像素宽高比缩放。
- 独立线程读取 stdout，帧通过容量 4 的同步通道发送；应用层最多保留一个 pending frame，形成背压。
- stderr 由读取线程在 stdout EOF 后读取。`-loglevel error` 下任何 stderr 输出都视为解码错误。
- 播放 / 暂停 / seek 由应用层启动或终止进程；暂停时不保留解码线程。
- 暂停状态下 seek 只解码一帧用于预览，然后停止进程。

### 音频（已被 0004 取代）

```text
ffplay -nodisp -vn -autoexit -loglevel error -ss <position> -i <path>
```

- 这是 Phase 2 的初始实现，已被 0004 的 cpal 音频主时钟取代。
- `ffplay` 只负责播放，不暴露可查询的播放时钟，无法与视频墙钟可靠同步。

### 时钟与 UI

- 音频可用时，播放位置由 cpal 输出设备实际消费的样本帧驱动；无音频或音频设备不可用时才使用 `Instant` 墙钟（见 0004）。
- 播放时 UI 每 16ms 请求重绘；纯音频每 100ms；暂停 seek 预览每 50ms；空闲不重绘。
- 帧由 UI 转换为 egui texture；应用层与媒体层不依赖 egui。
- 布局按 `GOALS.md` 的 LosslessCut 方向：顶部菜单栏、中央预览、底部时间轴、右侧媒体信息。
- 拖动时间轴的快速 keyframe scrub 见 0004；松开后使用精确 seek。

### 不引入 PreviewBackend trait

当前只有一套具体实现。按 AGENTS 的“抽象必须由真实复用驱动”，不提前建立 trait。未来出现第二个后端时再抽取接口。

## 后果与限制

- 运行依赖 `ffmpeg`、`ffprobe` 与音频输出设备；`ffplay` 已不再需要（见 0004）。
- 预览后端不是媒体处理后端：导出、裁切与合并仍只使用 FFmpeg。浏览器式的“第二套媒体处理栈”担忧不适用于预览。
- 当前仍使用外部 FFmpeg 帧解码；是否改用 libmpv 需要单独评估（安全绑定 / IPC、FFI 禁令、窗口嵌入）并另立 ADR。
- 参考：LosslessCut 的预览使用 Chromium HTML5 `<video>` + Media Source Extensions，seek 时重启 FFmpeg 流而不是逐次启动 CLI；调研记录见 `PLANS.md` 的“预览性能与音画同步待办”。
- A/V 同步由 0004 的音频主时钟驱动，受音频设备缓冲延迟影响。
- 预览固定 30fps、最大 960x540，不保留源时间戳与帧精确性。导出与编辑必须使用 FFmpeg 和源时间戳，不能依赖预览时间。
- `-ss` 输入 seek 是预览 seek，不保证帧精确；拖动时使用快速关键帧 seek，最终 seek 使用精确 seek（见 0004）。
- 非方形 SAR 尚未补偿，按像素宽高比缩放；需要时在媒体模型中增加 SAR 并修正。
- 暂无音量、音频设备选择、音轨选择。
- 测试覆盖启动、seek、解码失败、应用层播放 / 暂停 / seek；音频测试默认 ignored，因为 CI 环境没有音频设备。

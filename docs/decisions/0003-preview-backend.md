# 0003 预览后端：外部 FFmpeg 帧解码与 ffplay 音频

- 状态：已接受
- 日期：2026-10-24
- 阶段：Phase 2

## 背景

Phase 2 需要视频预览、播放 / 暂停 / seek、时间显示与音频播放。项目约束：

- 禁止 `unsafe` 与 FFI。
- 媒体处理由外部 FFmpeg / FFprobe 负责。
- egui 帧内不能执行长任务。
- 不引入未经评估的播放器 crate。

## 候选方案

1. 外部 `ffmpeg` 解码视频帧 + 外部 `ffplay` 播放音频（选中）。
2. 外部 `ffmpeg` 解码音频 PCM + `cpal` / `rodio` 输出。
3. GStreamer。
4. libmpv / mpv IPC。
5. 纯 Rust 解码器 crate。

评估：

- GStreamer / mpv：开发库在当前机器可用，但会引入第二套媒体栈与运行时，和“FFmpeg 是媒体处理后端”的架构冲突；libmpv 还需要窗口嵌入或 FFI。
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

### 音频

```text
ffplay -nodisp -vn -autoexit -loglevel error -ss <position> -i <path>
```

- 播放时启动；暂停、seek、换片时终止，并在新位置重启。
- 启动失败或进程错误作为非致命 warning 显示，视频预览继续。
- 运行环境需要提供 `ffplay`（属于 FFmpeg 工具集）。

### 时钟与 UI

- 应用层用 `Instant` 推进播放位置，位置使用领域 `Timestamp`。
- 播放时 UI 每 16ms 请求重绘；纯音频每 100ms；暂停 seek 预览每 50ms；空闲不重绘。
- 帧由 UI 转换为 egui texture；应用层与媒体层不依赖 egui。
- 布局按 `GOALS.md` 的 LosslessCut 方向：顶部工具栏、中央预览、底部时间轴、右侧媒体信息。

### 不引入 PreviewBackend trait

当前只有一套具体实现。按 AGENTS 的“抽象必须由真实复用驱动”，不提前建立 trait。未来出现第二个后端时再抽取接口。

## 后果与限制

- 运行依赖 `ffmpeg`、`ffprobe`、`ffplay`。
- A/V 同步是近似的：音频进程独立启动，视频由墙钟驱动，存在启动偏移，长时间播放可能漂移。若成为问题，用 `cpal` / `rodio` 或统一音频主时钟替换音频后端。
- 预览固定 30fps、最大 960x540，不保留源时间戳与帧精确性。导出与编辑必须使用 FFmpeg 和源时间戳，不能依赖预览时间。
- `-ss` 输入 seek 是预览 seek，不保证帧精确。
- 非方形 SAR 尚未补偿，按像素宽高比缩放；需要时在媒体模型中增加 SAR 并修正。
- 暂无音量、音频设备选择、音轨选择。
- 测试覆盖启动、seek、解码失败、应用层播放 / 暂停 / seek；音频测试默认 ignored，因为 CI 环境没有音频设备。

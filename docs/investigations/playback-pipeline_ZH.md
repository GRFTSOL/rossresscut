# 播放链路调查（Phase 4A）

状态：`已完成测量；Phase 4B 已由 ADR 0010 决策，并经 Phase 4C 实现（mpv-engine 预览）`

本文件只记录实际运行的命令与数据。结论未验证的部分单独列出。

## 目的

用户报告实际播放音画不同步、卡顿。本调查用测量判断瓶颈属于：

- decoder
- frame conversion / GPU download
- GPU upload
- egui / wgpu
- presentation
- clock / scheduler

调查不改动产品功能，也不预先选择 UI 框架。

## 环境

| 项 | 值 |
| --- | --- |
| OS | Arch Linux，Wayland（niri） |
| 内核 | `7.2.11-zen1-1-zen` |
| GPU | AMD Radeon Vega 8（RADV / radeonsi） |
| 渲染节点 | `/dev/dri/renderD128` |
| FFmpeg | `n9.0.2` |
| mpv | `v0.41.0` |
| 测试文件 | `/home/chasel/videos/bilibili/Immich相册部署和使用教程.mp4` |
| 素材参数 | HEVC Main，`yuv420p`，3840x2160，`60000/1001` fps，`time_base=1/16000`，duration 1313.32s，start 0 |
| 对照素材 | `/home/chasel/videos/episode/[非自然死亡] Unnatural (2018)/[非自然死亡] Unnatural S01E09.mkv`，H.264，1920x1080，`2997/100` fps |

## 当前实现事实

### 预览视频命令

`src/media/preview/video.rs` 生成：

```text
ffmpeg -nostdin -loglevel error -xerror
       [-hwaccel vaapi -hwaccel_device /dev/dri/renderD128]
       [-noaccurate_seek]
       -ss <start> -i <path>
       -map 0:v:0 -an -sn -dn
       -vf fps=30,scale=<w>x<h>:flags=fast_bilinear
       -pix_fmt rgb24 -f rawvideo pipe:1
```

关键点：

- 没有 `-hwaccel_output_format vaapi`，因此 FFmpeg 在解码后立即把帧下载到系统内存。
- `scale` 和 `fps` 都在 CPU 上执行。
- 最终通过管道传输 `rgb24`。
- 外部进程 + 管道模型只能传输系统内存，**结构上无法做 GPU 零拷贝**。

### 帧时间戳

预览帧时间戳不是源 PTS：

```text
frame_timestamp = start + frame_index * 1_000_000 / 30
```

即 30fps 合成 CFR。源是 59.94fps，`fps=30` 滤镜按每两帧取一帧。

### 音频时钟

- FFmpeg 解码 `0:a:0` 为 f32le PCM，写入 cpal 默认输出设备。
- 播放位置 = `start + 已写入设备缓冲的样本帧数 / sample_rate`。
- 已知偏差：写入设备缓冲不等于已被听到，时钟可能领先最多一个输出缓冲周期。

### application 层调度

`Playback::update` 每帧：

```text
poll_audio -> advance -> poll_video
```

视频只显示 `timestamp <= position` 的最新帧，未来帧留在 `pending_frame`。

## 测量工具

### 1. `examples/phase4a_probe.rs`（绕开 egui）

该 example 不创建窗口、不做纹理上传，只驱动 `Application`：

```bash
cargo run --release --example phase4a_probe -- <file> <software|vaapi> [seconds] [start]
```

它记录：

- `presented_frames`、`effective_fps`
- 相邻帧实际呈现间隔
- `delta = audio_clock - video_timestamp`
- delta 的 min / max / mean / stddev，以及首尾三分之一的漂移

### 2. GUI 渲染诊断（环境变量门控）

`src/ui/mod.rs` 的 `RenderDiagnostics` 仅在 `ROSS_RESCUT_DIAG` 存在时构造，输出：

```text
render diag: ui_fps=.. uploads=.. upload_avg_ms=.. upload_max_ms=..
```

辅助开关：

| 变量 | 作用 |
| --- | --- |
| `ROSS_RESCUT_DIAG` | 启用每 2 秒的渲染统计 |
| `ROSS_RESCUT_DIAG_AUTOPLAY` | 打开媒体后自动 seek 到 60s 并播放，用于无输入注入的重复测量 |
| `ROSS_RESCUT_DIAG_SCRUB` | 打开媒体后自动播放并模拟 40 步时间轴拖动，记录 scrub 与最终位置 |
| `ROSS_RESCUT_PREVIEW_HW` | `software` / `vaapi` / `d3d11va` / `videotoolbox`，覆盖本次运行的解码后端 |

三者都不设置时默认行为不变。

## 结果

### A / B：application 层（绕开 egui）

命令：

```bash
cargo run --release --example phase4a_probe -- <4K HEVC> software 40 60
cargo run --release --example phase4a_probe -- <4K HEVC> vaapi    40 60
```

| 指标 | A：software | B：VAAPI |
| --- | --- | --- |
| presented frames | 1192 | 416 |
| effective fps | 29.80 | 10.40 |
| present interval mean | 31.21 ms | 74.97 ms |
| present interval max | 53.96 ms | 142.84 ms |
| stalls > 100 ms | 0 | 19 |
| delta mean | +194.67 ms | +17328.90 ms |
| delta min / max | 0 / +2709.33 ms | +8693.33 / +25964.00 ms |
| delta drift（首三分 → 末三分） | +575.05 → +4.79 ms | +11507.28 → +23102.60 ms |

A 在启动阶段 delta 从约 +2.7s 收敛到 0~10ms，之后保持稳定。启动阶段的正 delta 来自 seek 后首帧解码延迟：音频立即开始，视频首帧要等约 1.5s。

B 的 delta 在 40 秒内持续增长 11.6 秒，视频帧产出只有 10.4 fps。

重复性核对（`--release`，20 秒）：

| 指标 | A：software | B：VAAPI |
| --- | --- | --- |
| effective fps | 29.34 | 5.70 |
| present interval mean | 29.30 ms | 76.43 ms |
| stalls > 100 ms | 0 | 11 |
| delta drift（首三分 → 末三分） | +1136.06 → +4.70 ms | +11931.54 → +15185.16 ms |

A 两次运行一致（29.80 / 29.34 fps），说明测量工具本身稳定。B 的绝对帧率在 5.7–10.4 fps 之间波动，但两次都远低于实时，且 delta 持续增长。

### A / B：GUI 路径（egui + wgpu）

命令：

```bash
ROSS_RESCUT_DIAG=1 ROSS_RESCUT_DIAG_AUTOPLAY=1 ROSS_RESCUT_PREVIEW_HW=<backend> \
  RUST_LOG=rossresscut=info ./target/release/rossresscut <4K HEVC>
```

| 指标 | A：software | B：VAAPI |
| --- | --- | --- |
| ui_fps | 25–43 | 40–51 |
| texture uploads / 2s | 52–60（约 26–30 fps） | 30–42（约 15–21 fps） |
| upload avg | 0.01–0.12 ms | 0.01 ms |
| upload max | 3.42 ms（首次） | 0.02 ms |

纹理上传耗时比一帧预算低三个数量级。egui 帧率高于预览帧率。两者都不是瓶颈。

### FFmpeg 微基准（4K HEVC，`-ss 60 -t 20`）

```bash
# 1. VAAPI 解码，默认下载到系统内存，丢弃
ffmpeg -hwaccel vaapi -hwaccel_device /dev/dri/renderD128 -ss 60 -i <file> \
  -map 0:v:0 -an -t 20 -f null -

# 2. VAAPI 解码 + fps=30,scale=960x540,rgb24
ffmpeg -hwaccel vaapi -hwaccel_device /dev/dri/renderD128 -ss 60 -i <file> \
  -map 0:v:0 -an -t 20 -vf "fps=30,scale=960:540:flags=fast_bilinear" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null

# 3. software 解码 + 相同滤镜
ffmpeg -ss 60 -i <file> -map 0:v:0 -an -t 20 \
  -vf "fps=30,scale=960:540:flags=fast_bilinear" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null

# 4. VAAPI 帧留在 GPU，scale_vaapi 缩到 960x540，再下载 + fps
ffmpeg -hwaccel vaapi -hwaccel_output_format vaapi -hwaccel_device /dev/dri/renderD128 \
  -ss 60 -i <file> -map 0:v:0 -an -t 20 \
  -vf "scale_vaapi=w=960:h=540,hwdownload,format=nv12,fps=30" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null

# 5. VAAPI 帧全程留在 GPU，丢弃
ffmpeg -hwaccel vaapi -hwaccel_output_format vaapi -hwaccel_device /dev/dri/renderD128 \
  -ss 60 -i <file> -map 0:v:0 -an -t 20 -f null -

# 6. VAAPI 帧留在 GPU，scale_vaapi 后下载，不加 fps 滤镜
ffmpeg -hwaccel vaapi -hwaccel_output_format vaapi -hwaccel_device /dev/dri/renderD128 \
  -ss 60 -i <file> -map 0:v:0 -an -t 20 \
  -vf "scale_vaapi=w=960:h=540,hwdownload,format=nv12" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null
```

| # | 路径 | 输出帧 | 耗时 | 吞吐 |
| --- | --- | --- | --- | --- |
| 1 | VAAPI → CPU 下载 → null | 1198 | 58.1 s | 20.6 fps / 0.34x |
| 2 | VAAPI → CPU 下载 → CPU scale → rgb24 | 600 | 58.4 s | 10.3 fps / 0.34x |
| 3 | software → CPU scale → rgb24 | 600 | 13.3 s | 45 fps / 1.51x |
| 4 | VAAPI GPU scale → 下载 960x540 → fps → rgb24 | 600 | 43.0 s | 14.0 fps / 0.47x |
| 5 | VAAPI 全程 GPU → null | 1198 | 13.9 s | **86 fps / 1.44x** |
| 6 | VAAPI GPU scale → 下载 960x540 → rgb24 | 1199 | 61.0 s | 20 fps / 0.33x |

### mpv 基准

```bash
# C：VAAPI，GPU 渲染
mpv --no-config --no-audio --vo=gpu-next --hwdec=vaapi --untimed --start=60 --frames=300 <file>

# D：software，GPU 渲染
mpv --no-config --no-audio --vo=gpu-next --hwdec=no --untimed --start=60 --frames=300 <file>

# 仅解码吞吐
mpv --no-config --no-audio --vo=null --hwdec=vaapi --start=60 --frames=600 <file>
mpv --no-config --no-audio --vo=null --hwdec=no    --start=60 --frames=600 <file>
```

| 测试 | 帧 | 实际耗时 | 吞吐 | user CPU |
| --- | --- | --- | --- | --- |
| C：VAAPI + gpu-next | 300 | 8.98 s | 33.4 fps | 1.67 s |
| D：software + gpu-next | 300 | 9.65 s | 31.1 fps | 21.48 s |
| VAAPI + vo=null | 600 | 11.81 s | 50.8 fps | 28.94 s |
| software + vo=null | 600 | 11.60 s | 51.7 fps | 27.62 s |

`gpu-next` 会开窗，测量包含窗口创建与 seek。C 与 D 的吞吐接近，但 C 的 CPU 消耗低一个数量级。

### 1080p H.264 对照

| 路径 | 输出帧 | 耗时 | 吞吐 |
| --- | --- | --- | --- |
| VAAPI + fps/scale/rgb24 | 600 | 8.4 s | 71 fps / 2.38x |
| software + fps/scale/rgb24 | 600 | 2.4 s | 252 fps / 8.38x |

## 判定

VAAPI 4K HEVC 的瓶颈是 **VAAPI 帧下载到系统内存（`hwdownload`）**，不是解码：

- 帧全程留在 GPU 时吞吐 86 fps（测试 5）。
- 只要发生下载，即使 GPU 已缩放到 960x540，吞吐也只有 20 fps（测试 6）。
- 下载后再做 CPU `fps` / `scale`，进一步降到 10 fps（测试 2）。
- 纹理上传 0.01 ms，egui `ui_fps` 40+，均不是瓶颈。
- software 路径在 application 层稳定 29.8 fps、delta ≈ 0，说明 clock 与 frame scheduling 本身没有持续漂移。

因此 B（VAAPI → egui/wgpu）不同步的根因在解码器之后、egui 之前。egui/wgpu 不是原因。

补充事实：

- 1080p H.264 下 VAAPI 有 2.38x，可用。
- 4K HEVC 下 VAAPI 只有 0.33–0.47x，不可用于实时预览。
- 软件解码 4K HEVC 有 1.51x，能满足 30fps 预览，但 CPU 占用高（utime 91s / 20s 素材）。

## 与 mpv 的差异

mpv 在同样 4K HEVC + VAAPI 下能到 33 fps 且 CPU 占用很低。差异来自架构：

- mpv 让解码帧留在 GPU，并在 GPU 上完成缩放与呈现。
- 本项目通过外部 FFmpeg 进程 + `pipe:1` 取帧，管道只能是系统内存，**必然经过一次同步下载**。
- 这不是参数调优能消除的开销，而是「外部进程 + CPU 内存帧」模型的固有成本。

## 尚未验证

以下项目本轮没有测量，不应据此下结论：

- VAAPI 下载为何在这台机器上每帧约 50ms：是 radeonsi 的同步策略、FFmpeg 的 `av_hwframe_transfer_data` 实现，还是 PCIe / 内存带宽。未做 profiling。
- 其他 4K 素材是否同样慢。只测了一个 4K HEVC 文件。
- Windows D3D11VA 与 macOS VideoToolbox 的下载开销。当前机器无法验证。
- 源 PTS 的正确性。预览时间戳是合成的 CFR，不是源 PTS。
- 音频时钟相对真实扬声器输出的提前量。`cpal` 写入缓冲与 audible 时刻之间没有测量。
- 长时段（分钟级）delta 是否仍有缓慢漂移。只测了 40 秒。
  （mpv 主路径已在 2026-11-04 复测，见下方「长时漂移复测」）
- Slint 或独立 video surface 是否能改善。未实现任何替代渲染路径。

## 2026-11-04 复测：mpv 主路径 10 分钟 A/V 漂移

Phase 4A 的漂移发生在外部 FFmpeg + cpal 路径。切换到 mpv 主路径后，
使用 `examples/mpv_long_playback.rs` 复核长时行为。

环境：

| 项 | 值 |
| --- | --- |
| OS / GPU | Arch Linux（Wayland），AMD Radeon Vega 8 / RADV |
| mpv | `v0.41.0`，FFmpeg `n9.0.2` |
| 解码 | `hwdec=auto-safe`，实际 `vaapi` |
| 音频 | PipeWire，48000 Hz，单声道 `floatp` |
| 同步 | `video-sync=audio` |
| 素材 | `testsrc2` 1280x720@30 + 440 Hz 正弦，H.264/AAC，660 s |

命令：

```bash
cargo build --release --features mpv-preview --example mpv_long_playback
./target/release/examples/mpv_long_playback /tmp/rossresscut-oq9-660s.mp4 600 1280 720
```

探针使用与预览相同的 exported-frame 后端，每秒采样 `time-pos`、`audio-pts`、
`frame-drop-count`、`mistimed-frame-count`、`decoder-frame-drop-count` 与 RSS，
每 5 ms 取走并丢弃一帧，模拟 UI 的纹理消费节奏。

结果（600 s 连续播放，599 个采样）：

| 指标 | 值 |
| --- | --- |
| `delta = time-pos - audio-pts` 均值 | +22.0 ms |
| delta min / max | +2.1 / +49.0 ms |
| delta stddev | 10.4 ms |
| delta 首三分 / 末三分 | +22.6 / +25.0 ms |
| 首尾三分漂移 | **+2.3 ms / 10 分钟** |
| frame drops / mistimed / decoder drops | 0 / 0 / 0 |
| RSS 起始 / 峰值 / 结束 | 151 / 151 / 151 MiB |

判定：

- delta 在 10 分钟内围绕 +22 ms 波动，没有持续增长；首尾三分差异 2.3 ms，
  远小于一帧（33 ms），不构成累积漂移。
- 固定 +22 ms 是 `time-pos` 与 `audio-pts` 的语义差（播放时钟相对正在播放的
  音频帧），不是音画不同步。
- 未出现丢帧、误时帧或解码器丢帧。
- RSS 全程持平，没有随播放时长增长。
- 短跑复核（20 s）确认 `current-ao=pipewire`、`audio-out-params` 为
  48000 Hz 单声道 `floatp`，不是 null 输出。

限制：

- 探针不做 wgpu 导入与 egui 呈现；它验证的是 mpv 内部 A/V 同步与导出帧消费，
  不覆盖 UI 纹理池，后者由 `tests/mpv_gpu_regression.rs` 覆盖。
- 只测了一台机器、一种分辨率与编码（720p30 H.264/AAC）。4K/VFR 的长时间行为
  仍未覆盖。
- 10 分钟不是小时级；内存与漂移趋势只能说明该时长内稳定。

## 对 Phase 4B 的输入

> 2026-10-24：Phase 4B 已完成决策，见 `docs/decisions/0009-preview-decode-zero-copy.md`。
> 选择进程内 `libav*` + 平台 hwaccel + `media/platform` zero-copy；本节保留为决策前的调查记录。

决策必须回答：在「外部 FFmpeg 进程」约束下，如何避免每帧同步下载 4K 帧。

候选方向（未选择）：

1. **egui + wgpu，软件解码预览**：当前 A 路径已达 29.8 fps / delta ≈ 0，代价是 CPU。
2. **egui + 独立 video renderer**：仍受外部进程管道限制，除非渲染器直接消费 GPU 帧。
3. **libmpv**：可保持 GPU 帧，但需要 FFI / `unsafe`，与 `AGENTS.md` 当前禁令冲突。
4. **代理文件 / 降低预览分辨率**：不解决下载固定开销（测试 6）。
5. **修改 `AGENTS.md` 允许 libmpv 或特定 FFI**：属于架构与安全约束变更，必须单独决策并写 ADR。

Phase 4B 不得在缺少上述取舍讨论的情况下直接开始实现。

# Playback pipeline investigation (Phase 4A)

Status: `measurement complete; Phase 4B was decided by ADR 0010 and implemented by Phase 4C (mpv-engine preview)`

This file records only the commands that were actually run and the data. Parts of the conclusions that are not verified are listed separately.

## Purpose

Users reported A/V desync and stutter during actual playback. This investigation uses measurements to determine whether the bottleneck is:

- decoder
- frame conversion / GPU download
- GPU upload
- egui / wgpu
- presentation
- clock / scheduler

The investigation does not change product features and does not pre-select a UI framework.

## Environment

| Item | Value |
| --- | --- |
| OS | Arch Linux, Wayland (niri) |
| Kernel | `7.2.11-zen1-1-zen` |
| GPU | AMD Radeon Vega 8 (RADV / radeonsi) |
| Render node | `/dev/dri/renderD128` |
| FFmpeg | `n9.0.2` |
| mpv | `v0.41.0` |
| Test file | `/home/chasel/videos/bilibili/Immich相册部署和使用教程.mp4` |
| Asset parameters | HEVC Main, `yuv420p`, 3840x2160, `60000/1001` fps, `time_base=1/16000`, duration 1313.32s, start 0 |
| Control asset | `/home/chasel/videos/episode/[非自然死亡] Unnatural (2018)/[非自然死亡] Unnatural S01E09.mkv`, H.264, 1920x1080, `2997/100` fps |

## Current implementation facts

### Preview video command

`src/media/preview/video.rs` generates:

```text
ffmpeg -nostdin -loglevel error -xerror
       [-hwaccel vaapi -hwaccel_device /dev/dri/renderD128]
       [-noaccurate_seek]
       -ss <start> -i <path>
       -map 0:v:0 -an -sn -dn
       -vf fps=30,scale=<w>x<h>:flags=fast_bilinear
       -pix_fmt rgb24 -f rawvideo pipe:1
```

Key points:

- There is no `-hwaccel_output_format vaapi`, so FFmpeg downloads frames to system memory right after decoding.
- Both `scale` and `fps` run on the CPU.
- Frames are finally transmitted as `rgb24` over the pipe.
- The external process + pipe model can only carry system memory, so **GPU zero-copy is structurally impossible**.

### Frame timestamps

Preview frame timestamps are not source PTS:

```text
frame_timestamp = start + frame_index * 1_000_000 / 30
```

That is, synthetic 30fps CFR. The source is 59.94fps, and the `fps=30` filter keeps one frame out of every two.

### Audio clock

- FFmpeg decodes `0:a:0` to f32le PCM and writes it to the cpal default output device.
- Playback position = `start + sample frames written to the device buffer / sample_rate`.
- Known deviation: being written to the device buffer is not the same as having been heard, so the clock may lead
  by up to one output buffer period.

### Application layer scheduling

`Playback::update` every frame:

```text
poll_audio -> advance -> poll_video
```

Video only shows the newest frame with `timestamp <= position`; future frames stay in `pending_frame`.

## Measurement tools

### 1. `examples/phase4a_probe.rs` (bypasses egui)

This example creates no window and does no texture upload; it only drives `Application`:

```bash
cargo run --release --example phase4a_probe -- <file> <software|vaapi> [seconds] [start]
```

It records:

- `presented_frames`, `effective_fps`
- the actual presentation interval between adjacent frames
- `delta = audio_clock - video_timestamp`
- min / max / mean / stddev of delta, plus the drift between the first and last third

### 2. GUI render diagnostics (gated by environment variables)

`RenderDiagnostics` in `src/ui/mod.rs` is only constructed when `ROSS_RESCUT_DIAG` exists, and outputs:

```text
render diag: ui_fps=.. uploads=.. upload_avg_ms=.. upload_max_ms=..
```

Helper switches:

| Variable | Effect |
| --- | --- |
| `ROSS_RESCUT_DIAG` | Enables render statistics every 2 seconds |
| `ROSS_RESCUT_DIAG_AUTOPLAY` | After opening the media, automatically seek to 60s and play, for repeated measurement without input injection |
| `ROSS_RESCUT_DIAG_SCRUB` | After opening the media, automatically play and simulate a 40-step timeline drag, recording the scrub and the final position |
| `ROSS_RESCUT_PREVIEW_HW` | `software` / `vaapi` / `d3d11va` / `videotoolbox`, overrides the decode backend for this run |

When none of them is set, the default behavior is unchanged.

## Results

### A / B: application layer (bypassing egui)

Commands:

```bash
cargo run --release --example phase4a_probe -- <4K HEVC> software 40 60
cargo run --release --example phase4a_probe -- <4K HEVC> vaapi    40 60
```

| Metric | A: software | B: VAAPI |
| --- | --- | --- |
| presented frames | 1192 | 416 |
| effective fps | 29.80 | 10.40 |
| present interval mean | 31.21 ms | 74.97 ms |
| present interval max | 53.96 ms | 142.84 ms |
| stalls > 100 ms | 0 | 19 |
| delta mean | +194.67 ms | +17328.90 ms |
| delta min / max | 0 / +2709.33 ms | +8693.33 / +25964.00 ms |
| delta drift (first third → last third) | +575.05 → +4.79 ms | +11507.28 → +23102.60 ms |

In A, delta converges from about +2.7s to 0~10ms during startup and then stays stable. The positive delta during
startup comes from the first-frame decode latency after the seek: audio starts immediately, while the first video
frame takes about 1.5s.

In B, delta keeps growing by 11.6 seconds over 40 seconds, and video frame output is only 10.4 fps.

Repeatability check (`--release`, 20 seconds):

| Metric | A: software | B: VAAPI |
| --- | --- | --- |
| effective fps | 29.34 | 5.70 |
| present interval mean | 29.30 ms | 76.43 ms |
| stalls > 100 ms | 0 | 11 |
| delta drift (first third → last third) | +1136.06 → +4.70 ms | +11931.54 → +15185.16 ms |

The two A runs agree (29.80 / 29.34 fps), which shows that the measurement tool itself is stable. B's absolute frame
rate fluctuates between 5.7–10.4 fps, but both runs are far below real time, and delta keeps growing.

### A / B: GUI path (egui + wgpu)

Commands:

```bash
ROSS_RESCUT_DIAG=1 ROSS_RESCUT_DIAG_AUTOPLAY=1 ROSS_RESCUT_PREVIEW_HW=<backend> \
  RUST_LOG=rossresscut=info ./target/release/rossresscut <4K HEVC>
```

| Metric | A: software | B: VAAPI |
| --- | --- | --- |
| ui_fps | 25–43 | 40–51 |
| texture uploads / 2s | 52–60 (about 26–30 fps) | 30–42 (about 15–21 fps) |
| upload avg | 0.01–0.12 ms | 0.01 ms |
| upload max | 3.42 ms (first time) | 0.02 ms |

Texture upload time is three orders of magnitude below one frame budget. The egui frame rate is higher than the
preview frame rate. Neither is a bottleneck.

### FFmpeg micro-benchmark (4K HEVC, `-ss 60 -t 20`)

```bash
# 1. VAAPI decode, downloads to system memory by default, output discarded
ffmpeg -hwaccel vaapi -hwaccel_device /dev/dri/renderD128 -ss 60 -i <file> \
  -map 0:v:0 -an -t 20 -f null -

# 2. VAAPI decode + fps=30,scale=960x540,rgb24
ffmpeg -hwaccel vaapi -hwaccel_device /dev/dri/renderD128 -ss 60 -i <file> \
  -map 0:v:0 -an -t 20 -vf "fps=30,scale=960:540:flags=fast_bilinear" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null

# 3. software decode + the same filters
ffmpeg -ss 60 -i <file> -map 0:v:0 -an -t 20 \
  -vf "fps=30,scale=960:540:flags=fast_bilinear" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null

# 4. VAAPI frames stay on the GPU, scale_vaapi scales to 960x540, then download + fps
ffmpeg -hwaccel vaapi -hwaccel_output_format vaapi -hwaccel_device /dev/dri/renderD128 \
  -ss 60 -i <file> -map 0:v:0 -an -t 20 \
  -vf "scale_vaapi=w=960:h=540,hwdownload,format=nv12,fps=30" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null

# 5. VAAPI frames stay on the GPU the whole time, output discarded
ffmpeg -hwaccel vaapi -hwaccel_output_format vaapi -hwaccel_device /dev/dri/renderD128 \
  -ss 60 -i <file> -map 0:v:0 -an -t 20 -f null -

# 6. VAAPI frames stay on the GPU, download after scale_vaapi, no fps filter
ffmpeg -hwaccel vaapi -hwaccel_output_format vaapi -hwaccel_device /dev/dri/renderD128 \
  -ss 60 -i <file> -map 0:v:0 -an -t 20 \
  -vf "scale_vaapi=w=960:h=540,hwdownload,format=nv12" \
  -pix_fmt rgb24 -f rawvideo -y /dev/null
```

| # | Path | Output frames | Elapsed | Throughput |
| --- | --- | --- | --- | --- |
| 1 | VAAPI → CPU download → null | 1198 | 58.1 s | 20.6 fps / 0.34x |
| 2 | VAAPI → CPU download → CPU scale → rgb24 | 600 | 58.4 s | 10.3 fps / 0.34x |
| 3 | software → CPU scale → rgb24 | 600 | 13.3 s | 45 fps / 1.51x |
| 4 | VAAPI GPU scale → download 960x540 → fps → rgb24 | 600 | 43.0 s | 14.0 fps / 0.47x |
| 5 | VAAPI all on GPU → null | 1198 | 13.9 s | **86 fps / 1.44x** |
| 6 | VAAPI GPU scale → download 960x540 → rgb24 | 1199 | 61.0 s | 20 fps / 0.33x |

### mpv benchmark

```bash
# C: VAAPI, GPU rendering
mpv --no-config --no-audio --vo=gpu-next --hwdec=vaapi --untimed --start=60 --frames=300 <file>

# D: software, GPU rendering
mpv --no-config --no-audio --vo=gpu-next --hwdec=no --untimed --start=60 --frames=300 <file>

# decode throughput only
mpv --no-config --no-audio --vo=null --hwdec=vaapi --start=60 --frames=600 <file>
mpv --no-config --no-audio --vo=null --hwdec=no    --start=60 --frames=600 <file>
```

| Test | Frames | Real elapsed | Throughput | user CPU |
| --- | --- | --- | --- | --- |
| C: VAAPI + gpu-next | 300 | 8.98 s | 33.4 fps | 1.67 s |
| D: software + gpu-next | 300 | 9.65 s | 31.1 fps | 21.48 s |
| VAAPI + vo=null | 600 | 11.81 s | 50.8 fps | 28.94 s |
| software + vo=null | 600 | 11.60 s | 51.7 fps | 27.62 s |

`gpu-next` opens a window, so the measurement includes window creation and seek. C and D have similar throughput,
but C's CPU consumption is an order of magnitude lower.

### 1080p H.264 control

| Path | Output frames | Elapsed | Throughput |
| --- | --- | --- | --- |
| VAAPI + fps/scale/rgb24 | 600 | 8.4 s | 71 fps / 2.38x |
| software + fps/scale/rgb24 | 600 | 2.4 s | 252 fps / 8.38x |

## Verdict

The bottleneck of VAAPI 4K HEVC is **downloading VAAPI frames to system memory (`hwdownload`)**, not decoding:

- When frames stay on the GPU the whole time, throughput is 86 fps (test 5).
- As soon as a download happens, even with the GPU already scaled to 960x540, throughput is only 20 fps (test 6).
- Doing CPU `fps` / `scale` after the download lowers it further to 10 fps (test 2).
- Texture upload is 0.01 ms and egui `ui_fps` is 40+, neither of which is a bottleneck.
- The software path is stable at 29.8 fps with delta ≈ 0 at the application layer, which shows that clock and frame
  scheduling themselves have no steady drift.

Therefore the root cause of B (VAAPI → egui/wgpu) desync lies after the decoder and before egui. egui/wgpu is not the cause.

Additional facts:

- At 1080p H.264, VAAPI reaches 2.38x and is usable.
- At 4K HEVC, VAAPI only reaches 0.33–0.47x and is not usable for real-time preview.
- Software decoding of 4K HEVC reaches 1.51x, which is enough for 30fps preview, but with high CPU usage
  (utime 91s for 20s of material).

## Differences from mpv

With the same 4K HEVC + VAAPI, mpv reaches 33 fps with very low CPU usage. The difference comes from the architecture:

- mpv keeps decoded frames on the GPU and performs scaling and presentation on the GPU.
- This project fetches frames through an external FFmpeg process + `pipe:1`; a pipe can only carry system memory,
  so it **necessarily goes through one synchronous download**.
- This is not an overhead that parameter tuning can eliminate; it is the inherent cost of the
  "external process + CPU memory frames" model.

## Not yet verified

The following items were not measured in this round and should not be used to draw conclusions:

- Why a VAAPI download costs about 50ms per frame on this machine: whether it is radeonsi's synchronization strategy,
  FFmpeg's `av_hwframe_transfer_data` implementation, or PCIe / memory bandwidth. No profiling was done.
- Whether other 4K assets are equally slow. Only one 4K HEVC file was measured.
- The download overhead of Windows D3D11VA and macOS VideoToolbox. The current machine cannot verify it.
- The correctness of the source PTS. Preview timestamps are synthetic CFR, not source PTS.
- How far the audio clock leads the real speaker output. No measurement was made between the `cpal` buffer write
  and the audible moment.
- Whether delta still has a slow drift over long periods (minutes). Only 40 seconds were measured.
  (The mpv main path was re-tested on 2026-11-04, see the long-run drift re-test below)
- Whether Slint or a separate video surface would improve matters. No alternative rendering path was implemented.

## 2026-11-04 re-test: 10-minute A/V drift on the mpv main path

The drift in Phase 4A occurred on the external FFmpeg + cpal path. After switching to the mpv main path,
`examples/mpv_long_playback.rs` was used to check long-run behavior.

Environment:

| Item | Value |
| --- | --- |
| OS / GPU | Arch Linux (Wayland), AMD Radeon Vega 8 / RADV |
| mpv | `v0.41.0`, FFmpeg `n9.0.2` |
| Decode | `hwdec=auto-safe`, effectively `vaapi` |
| Audio | PipeWire, 48000 Hz, mono `floatp` |
| Sync | `video-sync=audio` |
| Asset | `testsrc2` 1280x720@30 + 440 Hz sine, H.264/AAC, 660 s |

Commands:

```bash
cargo build --release --features mpv-preview --example mpv_long_playback
./target/release/examples/mpv_long_playback /tmp/rossresscut-oq9-660s.mp4 600 1280 720
```

The probe uses the same exported-frame backend as the preview and samples `time-pos`, `audio-pts`,
`frame-drop-count`, `mistimed-frame-count`, `decoder-frame-drop-count` and RSS every second,
taking and discarding one frame every 5 ms to simulate the UI's texture consumption cadence.

Results (600 s of continuous playback, 599 samples):

| Metric | Value |
| --- | --- |
| `delta = time-pos - audio-pts` mean | +22.0 ms |
| delta min / max | +2.1 / +49.0 ms |
| delta stddev | 10.4 ms |
| delta first third / last third | +22.6 / +25.0 ms |
| first-to-last third drift | **+2.3 ms / 10 minutes** |
| frame drops / mistimed / decoder drops | 0 / 0 / 0 |
| RSS start / peak / end | 151 / 151 / 151 MiB |

Verdict:

- Delta fluctuates around +22 ms within 10 minutes and does not keep growing; the difference between the first and
  last third is 2.3 ms, far less than one frame (33 ms), so it does not constitute cumulative drift.
- The constant +22 ms is the semantic difference between `time-pos` and `audio-pts` (the playback clock relative to
  the audio frame currently being played), not A/V desync.
- No frame drops, mistimed frames or decoder frame drops occurred.
- RSS stays flat for the whole run and does not grow with playback duration.
- A short-run check (20 s) confirms `current-ao=pipewire` and `audio-out-params` as
  48000 Hz mono `floatp`, not a null output.

Limitations:

- The probe does no wgpu import and no egui presentation; it verifies mpv's internal A/V sync and exported frame
  consumption, and does not cover the UI texture pool, which is covered by `tests/mpv_gpu_regression.rs`.
- Only one machine, one resolution and one encoding (720p30 H.264/AAC) were measured. Long-run behavior at 4K/VFR
  is still not covered.
- 10 minutes is not hour-scale; the memory and drift trends only show stability within that duration.

## Input for Phase 4B

> 2026-10-24: Phase 4B has been decided, see `docs/decisions/0009-preview-decode-zero-copy.md`.
> It chose in-process `libav*` + platform hwaccel + `media/platform` zero-copy; this section is kept as the
> investigation record from before the decision.

The decision must answer: under the "external FFmpeg process" constraint, how can the synchronous download of 4K
frames be avoided.

Candidate directions (not chosen):

1. **egui + wgpu, software-decoded preview**: the current A path already reaches 29.8 fps / delta ≈ 0, at the cost of CPU.
2. **egui + a separate video renderer**: still limited by the external process pipe, unless the renderer consumes GPU
   frames directly.
3. **libmpv**: can keep GPU frames, but requires FFI / `unsafe`, which conflicts with the current prohibition in `AGENTS.md`.
4. **Proxy files / lower preview resolution**: does not solve the fixed download overhead (test 6).
5. **Amend `AGENTS.md` to allow libmpv or specific FFI**: this is an architecture and safety constraint change, and must
   be decided separately with an ADR.

Phase 4B must not start implementation directly without the tradeoff discussion above.

# mpv-engine export + wgpu spike

Status: `Linux standalone wgpu, eframe, audio, scrub, resize and HDR preview have been measured; Windows / macOS not verified`

This file records only commands that were actually run and the resulting data. Parts of the conclusions marked as "inferred" were not measured.

## Purpose

Evaluate whether the `export` + `wgpu` feature of `mpv-engine 0.2.1` can replace or supplement the ADR 0009
"in-process libav* + platform hwaccel + surface import" preview approach.

The key questions are:

- Whether Linux DMA-BUF → wgpu import is truly zero-copy, and whether object lifetimes are safe.
- The actual throughput and resource usage of 4K HEVC on this machine's AMD Vega 8 / RADV.
- Whether new frames can be produced on demand after a paused seek, and whether the position is usable.
- The concrete risks of integrating with eframe 0.37.

## Environment

| Item | Value |
| --- | --- |
| OS | Arch Linux, Wayland (niri) |
| GPU | AMD Radeon Vega 8 (RADV / radeonsi) |
| Render node | `/dev/dri/renderD128` |
| FFmpeg | `n9.0.2` |
| mpv | `1:0.41.0-6` (package SPDX: `GPL-2.0-or-later AND LGPL-2.1-or-later`) |
| libmpv | `pkg-config --modversion mpv` = `2.5.0` |
| EGL / GBM | `1.5` / `26.2.5-arch1.1` |
| Rust / Cargo | `1.99.1` |
| Test asset | `/home/chasel/videos/bilibili/Immich相册部署和使用教程.mp4` (HEVC 3840×2160, 59.94 fps) |
| spike directory | `target/spike/mpv-engine` (already ignored by `.gitignore`) |

## Commands executed

```bash
git clone --depth 1 https://github.com/ophymx/mpv-engine target/spike/mpv-engine

cd target/spike/mpv-engine
cargo test --release --features wgpu --test export_wgpu -- --nocapture
cargo build --release --features wgpu --example winit_player
cargo build --release --features wgpu --example spike_stats

./target/release/examples/spike_stats '/home/chasel/videos/bilibili/Immich相册部署和使用教程.mp4'
```

`spike_stats` is a local copy of `winit_player` that only adds frame counting, DMA-BUF parameter output,
`position()` output, and one pause + `seek_absolute(60.0)`. It does not enter the project repository.

## Results

### 1. wgpu import tests

```text
running 3 tests
test imported_texture_matches_frame_pixels ... ok
test texture_outlives_frame_and_detach ... ok
test continuous_import_recycles_buffers_and_stays_correct ... ok
test result: ok. 3 passed; 0 failed; finished in 1.76s
```

What they cover:

- After the exported buffer goes through `into_wgpu_texture`, the pixels read back through wgpu match the bytes from `copy_pixels()`.
- The texture is still readable after `detach_render()` and `drop(engine)`, which shows that wgpu owns the underlying memory.
- When more frames than the pool capacity are imported continuously, buffers are recycled and reused and no frame content is mixed up.

This machine did not print the "adapter lacks `VULKAN_EXTERNAL_MEMORY_DMA_BUF`" skip message, which means RADV
exposes that feature and wgpu enabled it successfully.

### 2. 4K HEVC playback

```text
spike: frame 2017x1508 fourcc=0x34325241 stride=8192 modifier=0x0
spike: t=2.0s frames=52  fps=25.8 paused=false time-pos=1.735
spike: t=4.0s frames=118 fps=29.2 paused=false time-pos=3.754
spike: t=6.1s frames=183 fps=30.2 paused=false time-pos=5.756
```

- `fourcc=0x34325241` is `AR24`; `modifier=0x0` is the linear modifier.
- The export size `2017x1508` is the window size; mpv performs scaling and letterboxing inside the GPU.
- Playback stays stable at about 29–30 fps.
- `ps` sample at 12 seconds: `%CPU 18.5`, `RSS 275452 KB` (about 269 MiB).
- `Cannot load libcuda.so.1` comes from wgpu enumerating the NVIDIA backend and does not affect the Vulkan path.

### 3. Pause + seek

```text
spike: paused + seek 60.0 at frames=183
spike: paused-after-seek frames_delta=4 time-pos=60.010
spike: t=17.0s frames=200 fps=11.7 paused=false time-pos=62.012
```

- Even in the paused state, `seek_absolute(60.0)` still produced new frames (`frames_delta=4`).
- `position()` (`time-pos`) is `60.010`, 10 ms away from the target.
- After resuming playback it keeps advancing normally.
- The `fps` field is "total frames / total wall clock", so it is pulled down while paused; it is not an instantaneous frame rate.

### 4. eframe 0.37 end-to-end

```text
cargo run --release --features wgpu --example eframe_spike -- red.mp4
Cannot load libcuda.so.1
screenshot 1707x960 center=(253, 0, 0, 255)
spike: center pixel is red -> BGRA channel order OK
```

- Window logical size 1280x720; under the Wayland scale the screenshot is 1707x960 (physical pixels).
- Center pixel of the pure red video `(253, 0, 0, 255)`: a `Bgra8Unorm` texture can be handed directly to
  `register_native_texture`, and the color channel order is correct.
- Center pixel of the 50% gray video `(127, 128, 127, 255)`: no obvious sRGB brightness shift was observed.
- eframe's device requests `mpv_engine::REQUIRED_WGPU_FEATURES` through `WgpuSetupCreateNew.device_descriptor`,
  and pins `instance_descriptor.backends` to `mpv_engine::WGPU_BACKEND` (Vulkan on Linux).
- eframe 0.37's `App` trait uses `ui(&mut self, &mut Ui, &mut Frame)`, not `update`; this matches the
  project's current eframe version.

### 5. Audio

Command:

```bash
cargo run --release --example audio_spike -- av.mp4
```

Asset: H.264 + AAC, 44.1 kHz, mono, 10 seconds. The engine is `Engine::headless()`,
using mpv's default audio output with volume 10.

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

- Audio output initialized successfully: `audio-params/format=floatp`, `samplerate=44100`,
  `channel-count=1`. `audio-device` reads back `auto`; the actual AO was not queried separately.
- During steady playback `position()` and `audio-pts` differ by about 11–16 ms; at 0.8 seconds the difference
  is about 79 ms, a transient of startup / output buffering.
- Pause, seek while paused, resume, volume and mute all work; after a paused seek `position()` equals the
  target value.
- The agent cannot listen to the audio by ear; the evidence above is that the audio parameters are non-empty,
  `audio-pts` keeps advancing, and `position()` stays in sync with the audio PTS.

### 6. Scrub

Command:

```bash
cargo run --release --example scrub_spike -- <4K HEVC>
```

The engine is `Engine::headless()` with `audio=no` and `hwdec=auto-safe`, targeting the same 4K HEVC
asset. The measurement is the time from issuing `seek_absolute` until `PlaybackRestart` (`hr-seek=no`) or
until `position()` reaches the target ±0.05s (`hr-seek=yes`).

| Mode | State | Latency | Landing error |
| --- | --- | --- | --- |
| `hr-seek=no` | playing | 132–193 ms | 4.87–4.98 s (previous keyframe) |
| `hr-seek=no` | paused | 128–313 ms | 4.88–5.00 s (previous keyframe) |
| `hr-seek=yes` | playing | 46–331 ms | 0.00 s |
| `hr-seek=yes` | paused | 0.4–305 ms | 0.00 s |

- The keyframe interval of this asset is about 5 seconds, so `hr-seek=no` falls back to the keyframe about
  5 seconds before the target. This is expected behavior; while dragging, the UI must display the actual
  landing point rather than the requested target time.
- Even on 4K HEVC, `hr-seek=yes` only takes about 200–330 ms (46 ms the first time), so a precise seek can
  serve as the final seek after the user releases the handle.
- `PlaybackRestart` fires with `hr-seek=no`; with the headless / `vo=null` / `audio=no` combination and
  `hr-seek=yes` it did not fire within 20 seconds, but `position()` had already reached the target.
  The implementation cannot rely on that event alone and needs `position()` or observation of `time-pos`
  as a fallback.
- Re-measured with real eframe rendering + audio: for 4K HEVC + AAC 48 kHz 2ch, `hr-seek=yes` to 6.0 took
  365 ms, `position=6.039`, `audio-pts=6.015`, `PlaybackRestart=true`; the same path took 50 ms on a
  320x240 red clip.
- The center pixel of the 4K eframe screenshot is `(244,244,244)`, because that asset is not pure red at
  6 seconds; color correctness is judged by the `(253,0,0)` of `red.mp4` and the `(127,128,127)` of `gray.mp4`.

### 7. GPU engine attribution (2026-10-26)

The user observed GPU encode engine usage during preview. An `nvtop -s` snapshot shows:

```text
during the benchmark (4K HEVC autoplay):
  /usr/bin/sunshine          encode=35%  gpu_usage=null
  ./target/release/rossresscut encode=null  decode=72%  gpu_usage=77%

idle baseline (rossresscut not running):
  /usr/bin/sunshine          encode=40%
```

- The encode engine usage comes from **Sunshine streaming encoding**, not from the rossresscut preview.
- The rossresscut preview path uses the **decode engine (VAAPI HEVC decoding)** and the
  **GPU 3D/shader**; `encode` is `null`.
- The `960x540` export limit only affects the FBO size and GPU scaling/bandwidth; it does not trigger
  video encoding.
- Therefore "playing the source asset without scaling" would not save an encoding step; it would only move
  scaling from mpv's export FBO to the egui drawing stage and significantly increase the VRAM/bandwidth of
  the exported texture (4K BGRA is about 33 MB per frame). The current 8-bit cut preview keeps the
  960x540 output.

## Key facts

- The public API of `ExportedFrame` **has no PTS**. The available fields are `width`, `height`,
  `fourcc`, `stride`, `modifier`, `dma_buf_fd` (Linux) and `copy_pixels`.
  Playback position can only be read as `time-pos` through `Engine::position()`.
- `Engine` provides `seek_absolute(f64)`, `seek_relative(f64)`, `set_paused`,
  `load_paused`, `render_update`, `acquire_frame`, `position`.
- mpv-engine's `wgpu` feature and the project's eframe 0.37 both resolve to `wgpu 30.0.1` /
  `wgpu-hal 30.0.1`; the versions match.
- The Linux import uses `texture_from_raw` + `DropCallback` + `TextureMemory::External`,
  avoiding the per-frame reallocation that `texture_from_dmabuf_fd` causes with `drop_callback: None`.
- The export format is 8-bit BGRA (`AR24`/`XR24`), not 10-bit/HDR.
- When mpv outputs audio itself, `position()` and `audio-pts` consistently differ by about 11–16 ms;
  volume, mute, pause and seek while paused all work.
- `hr-seek=no` is a fast keyframe seek (about 5 second granularity); `hr-seek=yes` is a precise seek,
  about 200–330 ms on 4K HEVC.
- `nvtop` encode usage may come from Sunshine streaming; to judge the preview load look at the rossresscut
  process's `decode` and `gpu_usage`, and at whether `encode` is `null`.

## eframe integration risks

Items 1–4 below have already been verified end-to-end on Linux; the rest are still to be verified.

1. **device feature**: mpv-engine's import requires
   `wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`. eframe's default
   `WgpuSetupCreateNew.device_descriptor` does not request it, so
   `device_descriptor` must be replaced in `NativeOptions.wgpu_options`.
   The spike already proved that RADV supports this feature.
2. **version consistency**: eframe 0.37 and mpv-engine both use wgpu 30.0.1; if the project later upgrades
   eframe, mpv-engine must be upgraded in step, otherwise the `Device::as_hal` types will not match.
3. **egui texture format**: the `register_native_texture` documentation requires `Rgba8Unorm`, while
   `ExportedFrame::into_wgpu_texture` returns `Bgra8Unorm`. In practice it can be registered directly:
   red `(253,0,0)` and gray `(127,128,127)` are both correct, and no extra blit is needed.
   This is the behavior of the current eframe/egui-wgpu 0.37; the test must be repeated as a regression
   when upgrading versions.
4. **color space**: mpv's 8-bit output is SDR/BT.709 (HDR is tone mapped).
   The 50% gray screenshot values match the input, with no obvious sRGB shift; for HDR assets see the
   HDR verification of 2026-11-04.
5. **fallback path**: if the device lacks the DMA-BUF feature, the driver refuses the import, or the platform
   is not Vulkan/Metal/DX12, it must fall back to `render_sw` (CPU RGBA) or the existing external FFmpeg
   preview. The UI's "decode backend" must display the path that is actually in effect.

## Licensing

- `mpv-engine` code: MIT OR Apache-2.0.
- The libmpv it links: mpv's default build is GPLv2+; only a `-Dgpl=false` build is
  LGPLv2.1+. The Arch package SPDX is `GPL-2.0-or-later AND LGPL-2.1-or-later`, so
  the actual libmpv build options must be confirmed before distribution.
- If a GPL-built libmpv is shipped with distribution, the whole combined work is distributed under GPLv2+;
  if it is not shipped and the user's system library is relied on instead, dynamically linking a GPL library
  may still be considered a derivative work.
- This differs from an external `ffmpeg` process: the latter is not linked and does not form a derivative work.

## Not verified

- The export paths for Windows D3D11VA / macOS VideoToolbox (cannot be verified on this machine).
- Memory stability over longer (hour-scale) playback. The 10-minute re-test is stable, see
  `docs/investigations/playback-pipeline.md`.

## 2026-11-04 window resize verification

`set_export_size` has been changed to fit the preview pane's physical pixels proportionally, and a
continuously changing size is only applied after two identical values. Verification results:

- Automated: `tests/mpv_gpu_regression.rs::resizing_the_export_updates_the_texture`
  changes the export from 64x64 to 96x54 and then to 32x32 in the paused state, and asserts that a
  texture of the new size is reachable and the center pixel is still red; all 5 GPU regression tests
  pass on RADV.
- Pure logic: `ui::tests::preview_resize_waits_for_a_repeated_size` and
  `ui::tests::preview_bounds_use_physical_pixels` cover the two-frame debounce and the conversion from
  logical points to physical pixels.
- Real machine: a 1280x720 asset exports 640x360 in a 900x700 window and 1280x720 in a 1400x1000 window;
  continuous rapid resizing only reconfigures with the stabilized size, the picture keeps its 16:9 ratio,
  and the source resolution is never exceeded.
- Release gate: the GPU regression and the manual window-resize check have been added to the pre-release
  checks in `RELEASING.md`; the CI runner has no DRM render node, so they do not run in GitHub Actions.

## 2026-11-04 audio switching and output latency

Measured with `tests/fixtures/sample.mkv` (eng 440 Hz + jpn 880 Hz, two audio tracks) and the mpv IPC:

- Track: switching `aid` from 1 to 2 moves `selected` in `track-list` from eng to jpn, and
  `audio-pts` keeps advancing; the file does not need to be reloaded.
- Device: switching `audio-device` from `auto` to `pulse/...` changes `current-ao` from `pipewire`
  to `pulse`; switching back to `pipewire/...` restores it. When switching to the unavailable
  `alsa/sysdefault:CARD=Generic_1`, `current-ao` and `audio-params` become unavailable and subsequent
  seeks fail until a usable device is selected again. Device selection must handle open failures and
  must not assume the switch succeeded.
- Latency: under PipeWire `pactl` reports Buffer/Sink Latency as 0 usec, and the mpv node's
  `Latency` in `pw-dump` is 0, so the real speaker latency is not obtainable at the software level.
  What can be measured is `time-pos - audio-pts`: the 10-minute re-test holds steady at about +22 ms
  (see the measurement for OQ-9), which is a fixed output offset; it does not accumulate over time and
  does not affect Segments or export.

Conclusion: the main audio path is definitively owned by mpv; the project currently uses the default
audio track and the default device and provides no switching UI. Multi-track and device switching are
product feature gaps, not unresolved technical questions; when needed, use `aid` and `audio-device` and
handle device open failures.

## 2026-11-04 rapid scrub and frame-step

- `frame-step` / `frame-back-step` have been changed to native mpv commands. Measured in the paused state
  on `sample-keyframes.mp4`: 0.5 → 0.6 → 0.7 → 0.8, then back to 0.5, with landing points matching the
  source frame boundaries. The external FFmpeg fallback still jumps by nominal frame duration, which is an
  approximation under VFR.
- `tests/mpv_preview.rs::frame_steps_use_source_frame_boundaries` covers stepping forward and backward;
  `rapid_scrub_with_audio_reaches_the_final_target` performs 20 consecutive scrubs while playing an asset
  with audio, then a precise seek to 1.5 s, with a landing error ≤100 ms, and the calls are not blocked.
- Real eframe + audio: running `ROSS_RESCUT_DIAG=1 ROSS_RESCUT_DIAG_SCRUB=1` in a headless cage (Vulkan
  renderer), a 40-step scrub takes about 0.75 s, and on completion the position matches the target
  `09:54.000`, `09:54.700` after resuming playback, with the log confirming `audio_clock=true`. This path
  calls the same `scrub_to` / `finish_scrub` as pointer dragging.
- The preview position is still not the source PTS and cannot be used for Segment boundaries or export.

## 2026-11-04 HDR / 10-bit verification

Asset: `/home/chasel/videos/movie/[盗梦空间] Inception (2010).mkv`,
HEVC Main 10, 1920x800, `yuv420p10le`, BT.2020 / SMPTE ST 2084,
Mastering display `R(34000,16000) G(13250,34500) B(7500,3000)
WP(15635,16450) L(10000000,1)`, MaxCLL 1538 / MaxFALL 308.

- Preview: mpv-engine exports 8-bit BGRA (`860x358`) and tone maps to SDR.
  Compared with the same frame from FFmpeg `zscale` + hable tone map, the average RGB is close
  (preview 35.9/23.4/17.8, reference 35.6/21.8/17.0); PSNR is 21.4 dB, and the difference comes from
  the two tone curves rather than channel misalignment or an overall color cast.
- Verdict: the SDR tone mapped preview meets the needs of editing and navigation; no higher bit depth
  preview will be implemented. Preview color is not used as an HDR master or final color reference.
- Export: stream copy preserves 10-bit/HDR as-is. Smart cut re-encoding now passes the source
  `-pix_fmt`, `-color_range`, `-colorspace`, `-color_trc`, `-color_primaries` explicitly;
  FFmpeg still passes the Mastering display / Content light level side data to libx265.
- Test: `tests/smart_cut_export.rs::smart_cut_preserves_10_bit_hdr_metadata`
  generates a 10-bit HDR10 asset; after the smart cut it is still `yuv420p10le`, BT.2020/PQ,
  and the Mastering display and Content light level are still present.
- Short-clip verification on a real asset: the smart cut output is HEVC Main 10, `yuv420p10le`,
  `bt2020nc`/`smpte2084`/`bt2020`, with the side data preserved.

Conclusion: no higher bit depth preview or higher bit depth export is needed; both stream copy and
smart cut preserve the source 10-bit/HDR data. HDR color grading review still requires an external player.

## 2026-11-04 precise seek stale-frame gating

With an asset that is 1 second each of red/green/blue the stale-frame problem can be reproduced reliably:
after pausing in the blue segment (2.5s) and precise seeking to the red segment (0.5s), mpv may still
publish frames from before the seek around `PlaybackRestart`;
`time-pos` may also reach the target before the target texture has been published. If the UI keeps drawing
the previous texture during that window, the user sees "the frame from before the seek".

Current handling:

- On a precise seek, `Application` establishes an `MpvSeekGate` and discards stale frames already
  published by mpv.
- While `is_preview_stale()` is true, `Application` publishes no new frame; the UI keeps drawing the last
  published texture.
- The gate opens on `PlaybackRestart` when the position is close to the target; without that event the
  position must stay near the target for 100 consecutive ms. After opening it waits another 50 ms before
  accepting textures, avoiding the brief stale frame between `PlaybackRestart` and the target frame.
- A 5-second timeout abandons the gate and shows the newest available frame, so the picture does not stay
  frozen forever when a seek fails.
- Rapid scrub does not go through the gate and keeps showing keyframes.

Test: `tests/mpv_gpu_regression.rs::precise_seek_hides_the_previous_texture_until_the_target_frame`
asserts that the gate takes effect immediately after the seek and that both the first blue frame obtained
and the first red frame obtained afterwards are correct;
the full GPU regression passed 10 times in a row.

Limitation: exported frames have no PTS, so the gate can only rely on position/event heuristics; each
precise seek adds up to about 150 ms of loading state. The preview position still cannot be used for
Segments or export.

## 2026-11-06 precise seek does not clear the preview (measured on Windows)

On Windows, after dragging / clicking the progress bar the preview area flashes: during the gating window
the UI treats the texture as absent and draws a centered loading state instead, and even 100–150 ms of
gating is long enough to be seen.

Current handling:

- The gate stays in `Application`: no new texture is published before the target frame arrives, so a stale
  frame is never mistaken for the target frame.
- The drawing path no longer clears the preview because of `is_preview_stale()`; if a texture exists it keeps
  drawing the previous one, and only shows the loading state when no frame is available at all.
- The timeout fallback is unchanged (5 s), so the picture does not stay forever on a stale frame when a seek
  fails.

Cost: for up to about 150 ms after a precise seek the picture lags behind the timeline position. This matches
the behavior of mainstream editing software and eliminates the flashing.

## Impact on ADR 0009

This spike proves that the Linux path of `mpv-engine` can achieve GPU-resident DMA-BUF → wgpu import,
with 4K HEVC at about 30 fps, CPU about 18.5% and RSS about 269 MiB. It satisfies ADR 0009's zero-copy goal,
but the underlying layer is libmpv, not libav*.

ADR 0009 therefore needs one more candidate approach:

> Option F: libmpv renders into a hidden GL context → DMA-BUF / IOSurface / shared D3D11 export →
> wgpu import (mpv-engine).

eframe integration, mpv audio output, window resizing, HDR tone mapped preview and fast/precise scrub
have all been verified on Linux. The only remaining blocker is the Windows/macOS path.
If adopted, the ADR must state:

- Preview audio is output by mpv `ao`, with `position()` as the UI clock; ADR 0004's cpal master clock
  is superseded, and the existing `PreviewAudio` must be removed or repurposed as a non-master clock.
- Use `hr-seek=no` while dragging and `hr-seek=yes` on release; during dragging the UI shows the actual
  keyframe position.
- `position()` must not be treated as the source frame PTS; Segment boundaries and export still use
  FFprobe / FFmpeg.

If not adopted, the import and lifetime design of `src/export/wgpu_linux.rs` can still serve as a
reference implementation for `media/platform`; do not copy the code, only record the design conclusions.

## 2026-10-26 product decision and TODOs

The product owner confirmed:

- **Licensing is not a blocker**. GPLv2+ is accepted; at release time distribute under the license of the
  libmpv that is actually linked.
- **A second playback runtime is not a blocker**. Preview and export can be separated: libmpv handles
  preview, external FFmpeg handles stream copy export.

The remaining evaluation items for Option F are therefore not licensing or "a second stack", but:

1. ~~eframe 0.37 end-to-end~~: passed (red/gray, DMA-BUF feature, texture lifetime,
   window resizing and physical pixels); the GPU regression tests have been added to the pre-release
   checks in `RELEASING.md`.
2. ~~Audio~~: mpv `ao` output works, `position()` and `audio-pts` consistently differ by about 11–16 ms;
   volume/mute/pause/seek all work. The integration direction is mpv owning audio exclusively, replacing
   the cpal master clock. Multi-track/device switching semantics and output latency have been examined
   (see the 2026-11-04 section); the UI is not wired up, which is a product feature gap.
3. ~~Rapid scrub~~: `hr-seek=no` about 130–190 ms with about 5 s keyframe granularity;
   `hr-seek=yes` about 200–330 ms with a precise landing point; under real eframe + 4K + audio
   `hr-seek=yes` takes 365 ms. Consecutive scrub, eframe diagnostics and `frame-step` are covered,
   see the 2026-11-04 section.
4. ~~HDR / 10-bit~~: the SDR preview after mpv tone mapping meets editing and navigation needs; stream copy
   and smart cut preserve the source 10-bit/HDR, see the HDR verification of 2026-11-04.
5. The export paths for Windows D3D11VA / macOS VideoToolbox (cannot be verified on this machine).

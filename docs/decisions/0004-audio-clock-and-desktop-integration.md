# 0004 Audio Master Clock, Fast Scrub, and Desktop Integration

- Status: Accepted; §2 superseded by 0005; the audio master clock in §1 and the main path of fast scrub in §3 are superseded by 0010
- Date: 2026-10-24
- Phase: Phase 2 revision

## Context

Testing with real 4K HEVC files exposed problems in the initial Phase 2 implementation:

- Dragging the timeline only updated the position after release, and the preview image did not follow.
- There was a noticeable A/V drift between the separate `ffplay` audio process and the wall-clock-driven video.
- Opening files relied on manual paths; Linux should use the XDG desktop portal.
- There was no mpv Tab-style playback information, and no dark / light theme switching.

## Decision

### 1. Audio master clock (replaces the ffplay audio approach in 0003)

- Use the default output device of `cpal`.
- FFmpeg decodes `0:a:0` into interleaved `f32le` PCM matching the device sample rate and channel count.
- The number of sample frames actually consumed by the cpal callback determines the playback position:

```text
position = start_position + played_frames / sample_rate
```

- Video only displays the newest frame with `timestamp <= position`; when video decoding is faster than audio, limited backpressure is retained.
- Falls back to the wall clock normally when there is no audio track; falls back to the wall clock with a warning when the audio device is unavailable or the output format does not support f32.
- On pause, seek, or media change, the audio / video sessions are stopped and rebuilt.
- `ffplay` is not retained. `ffplay` only plays back and provides no queryable playback clock.

`cpal` only handles PCM output; decoding is still done by FFmpeg, consistent with the media backend boundary.

### 2. Hardware decoding: enabled by default

Product decision: hardware decoding is used by default.

- Linux: `-hwaccel vaapi -hwaccel_device /dev/dri/renderD128`.
- Windows: `-hwaccel d3d11va`.
- macOS: `-hwaccel videotoolbox`.

Detection and fallback:

- Linux checks `/dev/dri/renderD128` at startup; when the device does not exist, software decoding is used and the reason is stated in the logs.
- Windows and macOS request the corresponding backend directly.
- Decoded frames are still downloaded to the CPU, scaled to RGB24, and handed to egui. This pipeline is not GPU zero-copy, but hardware decoding remains the default path.
- FFmpeg may internally fall back to software decoding for codecs unsupported by the hardware backend; stderr is currently not parsed to report the actual decoder. The Decoder in the playback information shows the requested backend.

This machine is a CPU-constrained test environment, so wall-clock results from it cannot be used to reject hardware decoding. Details are recorded in `0005-hardware-decoding.md`.

### 3. Fast scrub

- Pausing playback when dragging the timeline begins, and recording whether playback was active before the drag.
- During the drag, a single-frame preview with `-noaccurate_seek` is used, started at most about 12Hz.
- Only one scrub decoding task is allowed at a time; no new task is started while the previous preview is unfinished, avoiding repeatedly killing processes.
- A precise seek is used on release; playback resumes if it was playing before the drag.
- Fast scrub may display a keyframe before the target position. That position may only be used for visual feedback and must not be used for Segment boundaries, editing results, or export.

Measured on 4K HEVC: fast keyframe seek about 0.4s, precise seek about 3.3s.

### 4. File opening on Linux

- Use the `rfd` `xdg-portal` backend to call `org.freedesktop.portal.FileChooser`.
- The file dialog runs on a background thread, and the UI obtains the result via `mpsc` without blocking the egui frame.
- The path input field is no longer provided at the top; only a file title label without the extension is shown.
- The command line still supports passing an initial media path.
- The Open button is temporarily disabled on non-Linux platforms; other platforms will be implemented later.
- Dependency: `rfd`, Linux target only, `default-features = false, features = ["xdg-portal"]`.
  - A self-built file picker is rejected: the scope is too large and it does not follow desktop integration conventions.
  - The GTK backend is rejected: it introduces the GTK runtime and does not use the portal.

### 5. Theme

- Use egui's `ThemePreference::{System, Dark, Light}`.
- The settings page provides a "Follow system theme" checkbox and a dark / light dropdown; it follows the system by default.
- The theme choice is persisted through eframe storage, see `0006-ui-localization-and-settings.md`.

### 6. Playback information

The top of the right panel shows mpv Tab-style playback information above the media file information:

```text
State / Position / Sync / Decoder / Preview / Frame / Video / Audio
```

- State, Position, Frame, Video, and Audio come from the current application state.
- Sync shows audio clock / wall clock / scrub preview.
- Decoder shows the requested FFmpeg decoding backend (for example `ffmpeg (vaapi)` or `ffmpeg (software)`).
- Media file details remain below, with stream information collapsed by default.

## Dependency choices

- `cpal` 0.18 (Apache-2.0): cross-platform audio output with a safe Rust API; Linux uses the ALSA backend.
- `rfd` 0.17: Linux uses the XDG portal. Rejected alternatives are above.
- The `persistence` feature of `eframe`: reuses eframe's built-in storage to save settings and egui memory; adds a transitive dependency on `ron`.
- No GStreamer, libmpv, or player crate is introduced. The preview backend and media processing are different responsibilities, so choosing a player crate is not an architectural conflict; it is not introduced now because the existing FFmpeg preview already meets the phase's needs and replacing it would require a separate evaluation of binding and cross-platform costs.

## Consequences and limitations

- Media with audio requires an audio output device; only the f32 output format is supported, and other formats fall back to the wall clock with a warning.
- A/V sync is affected by audio device buffering latency, typically tens of milliseconds; there is no startup offset from a separate player process.
- Hardware decoding depends on device and codec support; FFmpeg may internally fall back to software.
- Fast scrub is a keyframe approximation; the Frame timestamp in the playback information indicates the displayed frame, which may lag behind the scrub target.
- The file portal is Linux only; actual file selection has not been automated, and the portal service and the `FileChooser` interface have been confirmed to exist.
- Theme switching is tested via screenshots and the egui Context.
- Preview is still fixed at 30fps and at most 960x540; non-square SAR is not yet compensated.
- No volume, audio device selection, or audio track selection, and no frame-drop statistics yet.

# 0003 Preview Backend: External FFmpeg Frame Decoding

- Status: The video frame decoding part has been superseded by `0009-preview-decode-zero-copy.md`; the audio part has been superseded by `0004`
- Date: 2026-10-24
- Phase: Phase 2

> 2026-10-24 update: Phase 4A proved that external FFmpeg + `pipe:1` cannot make use of GPU-resident frames.
> Preview decoding has been decided to change to in-process `libav*` + platform hwaccel + zero-copy, see `0009-preview-decode-zero-copy.md`.
> This document is kept as a historical record of Phase 2 and no longer describes the current preview implementation.

> 2026-10-24 update: Testing with real files found that a separate `ffplay` process causes A/V drift.
> Audio has been changed to FFmpeg PCM + `cpal`, with the audio sample clock as the master clock, see `0004-audio-clock-and-desktop-integration.md`.
> The video frame decoding, backpressure, single-frame preview while paused, and seek design in this document remain valid; the `-noaccurate_seek` fast scrub was introduced by 0004.

## Context

Phase 2 needs video preview, play / pause / seek, time display, and audio playback. Project constraints:

- `unsafe` and FFI are forbidden.
- Media processing is handled by external FFmpeg / FFprobe.
- Long tasks must not run inside an egui frame.
- No unevaluated player crate is introduced.

## Candidate approaches

1. External `ffmpeg` decodes video frames + external `ffplay` plays audio (initially selected; the audio part has been superseded by 0004).
2. External `ffmpeg` decodes audio PCM + `cpal` / `rodio` output (finally adopted by 0004).
3. GStreamer.
4. libmpv / mpv IPC.
5. A pure-Rust decoder crate.

Evaluation:

- GStreamer / mpv: the development libraries are available on the current machine. At a glance this would introduce another preview runtime; but this does not constitute an architectural conflict with "FFmpeg is the media-processing backend".
  "Media processing" refers to operations that ultimately produce files, such as export, trimming, and merging; preview is playback and interaction and does not count as media processing.
  If libmpv can significantly improve preview correctness or hardware decoding, the preview backend can be replaced according to the process below.
  libmpv requires window embedding or FFI; this project forbids FFI, so the actual selection must first confirm an available safe binding or IPC approach, and a separate ADR must be filed.
- `cpal` / `rodio`: feasible, and can provide a more precise audio master clock, but it requires new dependencies, device configuration, PCM buffering, and synchronization logic. Phase 2 first uses the simpler ffplay; if the measured A/V drift is unacceptable, the audio backend will be replaced via an ADR.
- Pure-Rust decoder: insufficient codec coverage, and it violates "do not implement codecs yourself".
- External `ffmpeg` + `ffplay`: adds no Rust dependency, reuses the existing process management model, and can be implemented and tested directly.

## Decision

### Video

```text
ffmpeg -nostdin -loglevel error -xerror
       -ss <position> -i <path>
       -map 0:v:0 -an -sn -dn
       -vf fps=30,scale=<w>:<h>:flags=fast_bilinear
       -pix_fmt rgb24 -f rawvideo pipe:1
```

- Preview output is fixed at 30fps CFR, at most 960x540, scaled by pixel aspect ratio.
- A dedicated thread reads stdout, and frames are sent over a synchronous channel with capacity 4; the application layer keeps at most one pending frame, forming backpressure.
- stderr is read by the reading thread after stdout EOF. Under `-loglevel error`, any stderr output is treated as a decoding error.
- Play / pause / seek start or terminate the process from the application layer; no decoding thread is kept while paused.
- Seeking while paused decodes only one frame for preview, then stops the process.

### Audio (superseded by 0004)

```text
ffplay -nodisp -vn -autoexit -loglevel error -ss <position> -i <path>
```

- This was the initial Phase 2 implementation and has been superseded by the cpal audio master clock in 0004.
- `ffplay` only handles playback and exposes no queryable playback clock, so it cannot be reliably synchronized with the video wall clock.

### Clock and UI

- When audio is available, the playback position is driven by the sample frames actually consumed by the cpal output device; only when there is no audio or the audio device is unavailable is the `Instant` wall clock used (see 0004).
- During playback the UI requests a repaint every 16ms; every 100ms for audio-only; every 50ms for paused seek previews; no repaint when idle.
- Frames are converted into egui textures by the UI; the application and media layers do not depend on egui.
- The layout follows the LosslessCut direction in `GOALS.md`: top menu bar, central preview, bottom timeline, right-side media information.
- Fast keyframe scrubbing while dragging the timeline is described in 0004; a precise seek is used after release.

### No PreviewBackend trait

There is currently only one concrete implementation. Following AGENTS' "abstractions must be driven by real reuse", no trait is established in advance. An interface will be extracted when a second backend appears in the future.

## Consequences and limitations

- Running depends on `ffmpeg`, `ffprobe`, and an audio output device; `ffplay` is no longer needed (see 0004).
- The preview backend is not the media-processing backend: export, trimming, and merging still use only FFmpeg. The browser-style concern about a "second media-processing stack" does not apply to preview.
- External FFmpeg frame decoding is still used currently; whether to switch to libmpv needs a separate evaluation (safe binding / IPC, FFI ban, window embedding) and a separate ADR.
- Reference: LosslessCut's preview uses Chromium HTML5 `<video>` + Media Source Extensions, restarting the FFmpeg stream on seek rather than launching the CLI each time; the investigation record is under "Preview performance and A/V sync TODOs" in `docs/history/completed-phases.md`.
- A/V sync is driven by the audio master clock in 0004 and is affected by audio device buffering latency.
- Preview is fixed at 30fps and at most 960x540, and does not preserve source timestamps or frame accuracy. Export and editing must use FFmpeg and source timestamps and must not rely on preview time.
- `-ss` input seek is a preview seek and does not guarantee frame accuracy; fast keyframe seek is used while dragging, and precise seek for the final seek (see 0004).
- Non-square SAR is not yet compensated and is scaled by pixel aspect ratio; when needed, SAR will be added to the media model and corrected.
- No volume, audio device selection, or audio track selection yet.
- Tests cover startup, seek, decoding failure, and application-layer play / pause / seek; audio tests are ignored by default because the CI environment has no audio device.

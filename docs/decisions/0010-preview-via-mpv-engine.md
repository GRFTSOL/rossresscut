# 0010 Preview Backend: mpv-engine export + wgpu (Option F)

- Status: Accepted; implementation complete (Phase 4C steps 1–8, old preview path retained as fallback)
- Date: 2026-10-26
- Phase: Phase 4C revision
- Supersedes: the preview decoding decision in `0009-preview-decode-zero-copy.md`;
  the main path of the audio master clock in `0004-audio-clock-and-desktop-integration.md` §1
  and of fast scrub in §3

## Context

ADR 0009 chose Option D: in-process `libav*` + platform hwaccel + `media/platform` surface
import. Phase 4C had not started, and the project still used an external FFmpeg process +
RGB24 pipeline preview.

Phase 4A already proved that an external process can only deliver system memory frames, and that
the 4K HEVC VAAPI download is the bottleneck. The Phase 4B candidate set did not cover a path
that was later validated:

> libmpv renders to an exportable framebuffer using a hidden GL context, then exports
> a DMA-BUF / IOSurface / shared D3D11 texture, which wgpu imports.

`mpv-engine 0.2.1` already implements this path and offers a safe Rust API. The measurements are
recorded in `docs/investigations/mpv-engine-spike.md`; key data:

| Item | Result |
| --- | --- |
| Linux DMA-BUF → wgpu | 3/3 import tests pass; byte-identical, lifetime-safe, frame pool reclaims |
| 4K HEVC preview | 29–30 fps; CPU about 18.5%; RSS about 269 MiB |
| DMA-BUF parameters | `AR24`, linear modifier, stride 8192 |
| eframe 0.37 integration | red `(253,0,0)` and gray `(127,128,127)` correct; no CPU readback |
| Audio | mpv `ao` works; `position()` and `audio-pts` differ steadily by about 11–16 ms |
| Fast seek | `hr-seek=no` about 130–190 ms, landing on the previous keyframe |
| Precise seek | 4K headless about 50–330 ms (mostly 200–330 ms); eframe + 4K + audio 365 ms |

## Candidate options

### Option A: Keep external FFmpeg software preview

Pros: no new runtime introduced, and the existing implementation works.
Cons: high CPU usage on 4K HEVC; an external process + `pipe:1` structurally cannot deliver GPU frames.

### Option B: libmpv software render

Pros: measured throughput is higher than external FFmpeg.
Cons: still has GPU → CPU readback; cannot satisfy zero-copy.

### Option C: libmpv OpenGL render + eframe glow

Pros: GPU-resident.
Cons: only OpenGL across platforms; sharing GL state with egui is complex; inconsistent with the wgpu rendering stack already chosen for the project.

### Option D: In-process libav* + platform hwaccel + media/platform

Pros: does not depend on a player runtime; zero-copy.
Cons: requires hand-built libav* bindings, three-platform surface import, and audio / seek synchronization; the highest engineering effort and platform risk.

### Option E: Fully hand-written demux + platform media API

Pros: does not depend on libav*.
Cons: amounts to a separate container parsing project; scope is unacceptable.

### Option F: libmpv + hidden GL context + exportable surface + wgpu

Pros: zero-copy; cross-platform; audio, seek, container, and codec coverage provided by mpv; project code needs no unsafe.
Cons: introduces the libmpv runtime and GPL constraints; wgpu version lock; export is fixed to 8-bit BGRA.

## Decision

Adopt Option F:

- Video decoding, scaling, color conversion, frame scheduling, audio output, and seek for
  preview are delegated to **libmpv**, used through the **`export` + `wgpu` features of
  `mpv-engine 0.2.1`**.
- The project only consumes `ExportedFrame` → `wgpu::Texture` and registers it with egui;
  the preview path does no CPU readback.
- Export, trimming, merging, and probing continue to use external FFmpeg / FFprobe;
  stream copy semantics are unchanged.
- The existing external FFmpeg preview is retained as a fallback path during the migration,
  until Option F is stable.

This ADR supersedes the "hand-built `libav*` + `media/platform`" preview approach in ADR 0009.
`media/platform` is no longer needed for the preview path; if Option D is chosen again in the
future, that plan can be revived.

## Rationale

1. Phase 4A already proved that an external process can only deliver system memory frames, so Option A cannot satisfy 4K HEVC preview.
2. Option B still has GPU → CPU readback, and Option C is tied to OpenGL and conflicts with eframe's wgpu rendering stack.
3. Every positive goal of Option D (zero-copy, cross-platform, unified codec) is met by Option F,
   but Option F does not need hand-built libav* bindings, three-platform hwaccel, or surface import.
4. The spike validated zero-copy, color, audio, seek, and eframe integration on Linux RADV;
   data is in `docs/investigations/mpv-engine-spike.md`.
5. The product owner accepts GPLv2+ distribution and confirmed that "preview and export can be separated".

## Architecture and boundaries

```text
UI (egui)
  ↑ wgpu::Texture / TextureId
Application (commands / state / clock)
  ↑ mpv-engine safe API
media::preview::mpv
  ↑
mpv-engine (export + wgpu)
  ↑
libmpv → hwaccel (VAAPI / D3D11VA / VideoToolbox)
        → hidden GL context → DMA-BUF / IOSurface / shared D3D11
        → wgpu::Texture
```

Rules:

1. Add `src/media/preview/mpv.rs` (or a submodule at the same layer); only this module depends on
   `mpv-engine`. Domain and Application do not depend on `mpv-engine` types directly.
2. `Application` owns commands, state, errors, and position; the UI only sends commands and displays results.
   The UI does not build mpv commands and does not parse mpv properties.
3. `ExportedFrame` / `wgpu::Texture` are GPU handles and **must not enter the command state of Domain or
   Application**, and must not be written into `Segment`, `Timeline`, or persisted settings.
   They may only be passed per frame between `media::preview` and the UI rendering adapter layer.
4. Project code continues to hold `unsafe_code = "forbid"`. The FFI and `unsafe` inside `mpv-engine`
   are borne by the third-party dependency; this project adds no unsafe code.
5. `media/platform` is reintroduced only if GPU handles must later be manipulated directly inside the project;
   Option F does not require it.
6. The eframe wgpu device must request
   `wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF` (Linux), and must use
   `WgpuSetupCreateNew.device_descriptor` in `NativeOptions.wgpu_options`.
   `instance_descriptor.backends` is fixed to `mpv_engine::WGPU_BACKEND`.
7. `wgpu` / `wgpu-hal` must resolve to the same version as eframe (currently 30.0.1).
   When upgrading eframe or mpv-engine, both must be upgraded together, otherwise the types of
   `Device::as_hal` do not match.
8. `register_native_texture` in egui-wgpu 0.37 accepts `Bgra8Unorm` directly; the documentation
   requires `Rgba8Unorm`, which contradicts the measurement, so the color regression test must be kept.

## Audio

Use mpv's exclusive audio output:

- `Engine::video()` uses the default `ao`; the audio device is selected through mpv's `audio-device`
  property.
- The UI play position uses `Engine::position()` (`time-pos`) instead of the cpal
  clock that consumes sample frames.
- Pause and seek are wired to `set_paused` and `seek_absolute`; volume and mute are carried by mpv's
  `volume` / `mute` properties, and the UI has no controls for them yet.
- The main path removes `PreviewAudio` and the cpal audio output; cpal remains only in the external FFmpeg
  fallback path and is removed together with that path.
- mpv and cpal never output audio at the same time, avoiding dual audio sources and two clocks.

This section supersedes the cpal master clock in ADR 0004 §1 and the FFmpeg scrub design in §3.
Audio measurements:

- During steady playback, `position()` and `audio-pts` differ by about 11–16 ms;
  there is a transient of about 79 ms during startup.
- Pause, seek while paused, resume, volume, and mute have all been validated in the spike.
- Multi-track, device switching semantics, and output latency have been checked (2026-11-04), see
  `docs/investigations/mpv-engine-spike.md`; drift beyond 10 minutes was re-tested and passed on
  2026-11-04, see `docs/investigations/playback-pipeline.md`.

## Hardware decoding

Preview startup explicitly sets `hwdec=auto-safe`:

- VAAPI by default on Linux, VideoToolbox by default on macOS;
- mpv falls back to software decoding when the device is unavailable;
- an unavailable decoder must not fail silently, and playback info must show the actual backend.

> Correction from Windows measurements on 2026-11-06: preview goes through `vo=libmpv` + OpenGL
> (`WGL_NV_DX_interop2`), and that path does not support `d3d11va` (which requires
> `gpu-context=d3d11`), so `auto-safe` resolves to `nvdec` on NVIDIA machines.
> This is a normal value of `auto-safe`, neither `hwdec=auto` nor an explicit setting.

The UI shows the actual backend via `hwdec-current` (for example `vaapi`, `vaapi-copy`,
`videotoolbox`, `nvdec`, `no`). `Application` reads and caches this property every 500 ms,
off the synchronous path of the egui frame.

## Fast scrub and precise seek

Interaction rules:

1. Drag start: set `hr-seek=no` and issue a seek.
2. During dragging: the UI shows the actual landing point of `Engine::position()`; that landing point
   is the previous keyframe, not the target time the user requested.
3. Release: set `hr-seek=yes` and issue the final precise seek.
4. Precise seek completion is determined by `PlaybackRestart`; if the event does not arrive, fall back
   to `position()` or by observing `time-pos`.
5. The preview position is only for visual feedback and must not be used for Segment boundaries,
   keyframe indices, or export.

Measurements:

- `hr-seek=no`: 132–193 ms during playback, 128–313 ms while paused;
  the test footage has a GOP of about 5 seconds, and the landing error is about 4.9–5.0 seconds.
- `hr-seek=yes`: 4K headless about 50–330 ms (mostly 200–330 ms), error 0;
  eframe + 4K + audio 365 ms, `position=6.039`, `audio-pts=6.015`.
- Under headless / `vo=null` / `audio=no`, a precise seek may not emit
  `PlaybackRestart`; with real eframe + audio it does. The implementation must support both paths.

## License

The product owner has confirmed acceptance of GPLv2+, and releases are distributed under the
license of the libmpv actually linked.

- `mpv-engine`, `rsmpv`, `rsmpv-sys`: MIT / Apache-2.0 / ISC, permissive.
- libmpv: the default mpv build is GPLv2+; only `-Dgpl=false` builds are LGPLv2.1+.
- If a GPL build of libmpv is shipped at distribution time, the combined work must be distributed
  in a GPL-compatible way, with the corresponding source and license provided. Because the project's
  dependencies include Apache-2.0 code, the conservative choice is **GPLv3+**; GPLv2-only is
  incompatible with Apache-2.0.
- If only the user's system-provided libmpv is depended on, dynamically linking a GPL library may
  still be considered a derivative work; "not bundling" cannot be treated as a license workaround.
- The external `ffmpeg` process is unaffected: it is not linked and does not form a derivative work.
- The distribution manifest must include the libmpv and FFmpeg notices in `LICENSES` / the About page.
  If permissive-license distribution is needed in the future, switch to an LGPL libmpv built with
  `-Dgpl=false`, or abandon Option F.

License details are subject to review at actual release time; this section records a product
decision, not legal advice.

## Dependencies and platforms

New dependency:

```toml
mpv-engine = { version = "0.2.1", features = ["wgpu"] }
```

The `wgpu` feature implies `export`. Transitive dependencies: `rsmpv 0.2.1`,
`wgpu 30.0.1`, `wgpu-hal 30.0.1`.

System dependencies:

- Linux: libmpv (system package), libEGL, libgbm; running requires a readable
  `/dev/dri/renderD*`.
- Windows: libmpv, OpenGL, D3D11, DXGI; the export path uses
  `WGL_NV_DX_interop2`.
- macOS: libmpv, CGL, IOSurface, Metal.

Rejected alternatives:

- Using `rsmpv` / `libmpv2` directly: they only provide libmpv bindings and GL/software render,
  so the GL context, FBO, DMA-BUF export, and wgpu import would still have to be implemented
  by hand.
- `egui-sharkplayer`: its wgpu path is actually GL rendering + `glReadPixels` + egui
  texture; it also pins eframe 0.35, incompatible with the project's 0.37.
- `alhazen-core`: on Linux it only supports NVIDIA NVDEC, so this machine's AMD has no hardware
  decoding; it outputs CPU BGRA; its API is that of a player, not an editor.
- Continuing with Option D: it requires hand-built libav* bindings, three-platform surface import,
  and synchronization; the spike already proved Option F can reach the same goal with less code.

## Fallback and errors

- Fall back to the existing external FFmpeg preview when libmpv is unavailable, `Engine`
  initialization fails, the export backend is unavailable, or the device lacks the DMA-BUF feature.
- mpv-engine's loading API accepts only UTF-8 paths; non-UTF-8 paths must fall back to external
  FFmpeg, and the error message must explain why.
- Fallback must be visible: playback info shows the actual backend (mpv / ffmpeg software / ffmpeg vaapi).
- Silently degrading zero-copy to CPU readback is not allowed; if `render_sw` is used,
  it must be labeled in the UI.
- Import failure, driver rejection, and unsupported size must return errors with context, never panic.

## Frame semantics and editing constraints

- `ExportedFrame` does not provide a PTS. `Engine::position()` is the mpv playback clock,
  **not the source PTS corresponding to the current texture**.
- The preview position is only for UI display and scrub feedback; Segment boundaries, keyframe indices,
  and export parameters still come from FFprobe / FFmpeg.
- Internal media time continues to use `Timestamp` / `Duration`; `f64` seconds appear only at the
  mpv API boundary and must not become the project's time representation.
- Preview is fixed to display size; export and editing must not depend on preview frames or preview time.
- OQ-7 from ADR 0009 (preview must use source PTS) no longer applies under Option F;
  if frame-accurate preview is needed later, a separate ADR should be written.

## HDR / 10-bit

mpv-engine exports 8-bit BGRA (Linux `AR24`/`XR24`), and HDR footage is tone mapped
to SDR by mpv.

- The current product focuses on 8-bit stream copy trimming, so 8-bit preview meets the need.
- stream copy export is unaffected by preview bit depth.
- smart cut re-encoding reads the source `pix_fmt`, primaries, transfer, and matrix, and passes them
  explicitly to the encoder to preserve 10-bit/HDR metadata; preview's 8-bit frames must not be used
  as re-encode input.
- Validated on 2026-11-04: preview keeps the SDR tone map; both stream copy and smart cut preserve
  10-bit/HDR, with no need for higher-bit-depth export. See `docs/investigations/mpv-engine-spike.md`.

## Window size

- `set_export_size` must use **physical pixels**, not egui logical points.
- Under Wayland scale in eframe 0.37, logical 1280x720 corresponds to physical 1707x960;
  the implementation must obtain the pixel size from `egui::Context` / `ViewportInfo`.
- Resize must be throttled so export is not reconfigured on every event; update only after the raw
  frame size changes.
- Resize was not validated in the spike at the time and was a remaining item of OQ-10; it was implemented
  and validated on 2026-11-04, see `docs/investigations/mpv-engine-spike.md`.

## Tests and acceptance

### Unit tests

- Conversion between `Timestamp` and mpv seconds arguments.
- The `hr-seek=no/yes` state machine; idempotency of drag start/end.
- Fallback selection: engine unavailable, feature unavailable, import failure.
- Error context includes path, target time, and backend.

### Integration tests (required on Linux)

- 4K HEVC: exported frames → wgpu readback matches `copy_pixels()` byte-for-byte.
- Texture lifetime: still readable after detach / drop; frame pool reclamation does not mix frames.
- eframe red/gray color regression: BGRA channel order and sRGB are not shifted.
- Audio: play, pause, seek while paused, volume, mute;
  `position()` and `audio-pts` within tolerance.
- scrub: `hr-seek=no` lands on a keyframe; `hr-seek=yes` lands within 1 frame;
  latency is recorded in the report.
- resize: aspect ratio and sharpness are correct after the physical pixel size changes.
- fallback: returns to external FFmpeg when libmpv / the DMA-BUF feature is missing, and shows it in the UI.

### Platform acceptance

- Linux: measured on this machine's RADV.
- Windows: D3D11VA + DX12 import, verified on real hardware.
- macOS: VideoToolbox + Metal/IOSurface, verified on real hardware.
- Until completed, documentation or the UI must not claim support for the corresponding platform.

### Performance targets (this machine, 4K HEVC)

- Preview ≥ 30 fps.
- CPU ≤ 25% (single-core basis; about 18.5% per the spike).
- Fast seek ≤ 300 ms; precise seek ≤ 500 ms.
- No CPU pixel readback on the preview path.

## Consequences

### Positive

- Preview stays GPU-resident, avoiding the 4K VAAPI download bottleneck.
- Hand-built libav* bindings, three-platform hwaccel, and surface import are no longer needed.
- Audio, seek, VFR, container, and codec coverage are provided by mpv.
- eframe 0.37 / wgpu 30 and mpv-engine 0.2.1 versions are already aligned.
- Project code still has no unsafe; the license risk is accepted by a product decision.

### Negative

- Runtime dependency on libmpv; distribution is subject to GPL.
- The wgpu major version is tightly coupled to mpv-engine, so upgrades must be synchronized.
- The hidden GL context adds platform dependencies and debugging complexity.
- Preview does not expose source PTS; it cannot be used for frame-accurate editing.
- Preview is fixed at 8-bit BGRA; HDR preview is an SDR tone map. Export still preserves source bit depth
  and HDR metadata, so an external player is needed for HDR color review.
- The Windows / macOS paths have not been measured yet.
- mpv-engine 0.2.1 is very new, so maintenance and upgrade risk falls on the project.
- The existing `PreviewAudio`, `PreviewVideo`, and related tests need to be replaced or retained as fallback.

## Phase 4C implementation steps

All of the following steps happen inside Phase 4C and are **not project Phase 1**. Phases 1/2/3 are
complete: media analysis, FFprobe, the timeline, and existing editing features are not redone.

1. Add the `mpv-engine` dependency and platform detection; retain the external FFmpeg fallback.
2. Implement `media::preview::mpv`: lifetime, attach export, acquire frame,
   commands, errors, and state.
3. UI integration: request wgpu features, register textures, color / lifetime regression.
4. Audio switch: mpv `ao` becomes the main path, and the cpal master clock is removed.
5. scrub state machine: `hr-seek=no` → `hr-seek=yes`, with event and position fallback.
6. Physical pixel handling for resize / Wayland scale.
7. Fallback and playback info: show the actual backend; make error paths visible.
8. Tests, documentation, and AGENTS clause revisions; delete the old preview path once stable.

## Documents that need synchronized revision

- `AGENTS.md`: preview decoding / audio boundaries, GPU handle passing scope, dependency and license clauses.
- `docs/architecture.md`: preview data flow.
- `docs/media-model.md`: the boundary between preview time and editing time (if needed).
- `PLANS.md`: change the Phase 4C task to Option F.
- `docs/open-questions.md`: mark OQ-3/4/5/6/7 as Option D historical items;
  OQ-10..13 are the todos for Option F (OQ-10 was closed on 2026-11-04).

## Exit conditions for the old preview path

`PreviewVideo` and `PreviewAudio` (external FFmpeg + cpal) currently serve three fallbacks at once:

1. Builds with the `mpv-preview` feature disabled (no libmpv).
2. eframe has no wgpu render state.
3. mpv startup or frame import fails.

Therefore they cannot be deleted in Phase 4C. The preconditions for deletion are:

1. The project ships only builds with libmpv, or the fallback is changed to an implementation that
   does not depend on these two process paths.
2. The mpv import paths on Windows and macOS are verified on real hardware (currently only Linux).
3. `tests/mpv_gpu_regression.rs` and `tests/mpv_preview.rs` become part of the release checks,
   covering color, lifetime, seek, and the audio clock.
4. OQ-9 (long-run drift) has an acceptable conclusion, see
   `docs/investigations/playback-pipeline.md`.

When deleting, also remove the `cpal` dependency, `PreviewAudio`, `PreviewVideo`, and the
corresponding tests, and update `AGENTS.md`, `PLANS.md`, and `docs/architecture.md`.

## Open questions

> 2026-11-04: OQ-9, OQ-10, OQ-11, OQ-12, OQ-13, and OQ-21 are all closed. OQ-9 was the 10-minute
> drift re-test, OQ-10 was window scaling and GPU regression, OQ-11 was track / device switching semantics
> and output latency, OQ-12 was continuous scrub and frame-step, OQ-13 was HDR/10-bit preview and export,
> and OQ-21 was the old-frame gating for precise seek; see
> `docs/investigations/playback-pipeline.md` and `docs/investigations/mpv-engine-spike.md`.

- ~~Real-hardware verification of Windows D3D11VA / macOS VideoToolbox.~~ Windows has been measured:
  preview uses `nvdec`, and `d3d11va` is unavailable on the libmpv/OpenGL export path (2026-11-06);
  macOS remains unverified.
- ~~mpv's device selection strategy with multiple GPUs / multiple render nodes.~~ Delegated to mpv:
  Linux uses `--vaapi-device` (default `/dev/dri/renderD128`), and the project does not enumerate
  devices; Windows D3D11 adapter selection is unverified. See `docs/architecture.md`.
- Final review of the libmpv distribution license and the `LICENSES` manifest.

## References

- `docs/investigations/mpv-engine-spike.md`
- `docs/decisions/0009-preview-decode-zero-copy.md`
- `docs/decisions/0004-audio-clock-and-desktop-integration.md`
- mpv-engine README and `src/export/wgpu_linux.rs` (design reference, no code copied)

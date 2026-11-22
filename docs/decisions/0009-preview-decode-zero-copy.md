# 0009 Preview Decoding and Rendering: In-Process libav* + Platform hwaccel + zero-copy

- Status: Superseded by `0010-preview-via-mpv-engine.md` (2026-10-26); retained as Phase 4B history
- Date: 2026-10-24
- Phase: Phase 4B
- Supersedes: the video frame decoding part of `0003-preview-backend.md`; the preview decoding implementation part of `0005-hardware-decoding.md`

> Update 2026-10-26: the spike proved that libmpv + a hidden GL context + DMA-BUF / IOSurface /
> shared D3D11 export + wgpu import satisfies zero-copy, with less engineering effort than
> a hand-built libav* + three-platform surface import. The preview backend switches to Option F;
> see `0010-preview-via-mpv-engine.md`. Option D in this document is no longer implemented, and
> `media/platform` is no longer needed for the preview path.

## Context

Phase 4A measured with real 4K HEVC footage:

| Path | Throughput |
| --- | --- |
| VAAPI stays on the GPU throughout (`-hwaccel_output_format vaapi`) | 86 fps / 1.44x |
| VAAPI + `hwdownload` (even after downscaling to 960x540) | 20 fps / 0.33x |
| VAAPI default download + CPU scale + rgb24 | 10.3 fps / 0.34x |
| Software decode + CPU scale + rgb24 | 45 fps / 1.51x |

Application-layer measurements: software decode 29.8 fps, `delta ≈ 0`; VAAPI 10.4 fps, `delta` growing by 11.6 seconds within 40 seconds. Texture upload 0.01 ms, egui `ui_fps` 40+.

Conclusion: the bottleneck is the GPU → CPU download, not egui / wgpu / frame scheduling. An external FFmpeg process can only transfer system memory through `pipe:1` and structurally cannot deliver GPU-resident frames.

Product owner decision: preview must support 4K hardware decoding and stay GPU-resident through to presentation; export continues to use external FFmpeg stream copy.

## Known constraints

- Export, trimming, merging, and probing remain the responsibility of external FFmpeg / FFprobe processes.
- Platform media APIs (VAAPI / D3D11VA / VideoToolbox) only decode; they do not parse containers.
- `wgpu 30.0.1` exposes import of external textures only as `unsafe fn`: `create_texture_from_hal` and each backend's `texture_from_raw`.
- No mature crate currently provides a safe wrapper around "DMA-BUF / IOSurface / D3D11 shared handle → wgpu".
- The project's existing `unsafe_code = "forbid"` cannot hold simultaneously with zero-copy.

## Candidate options

### Option A: External FFmpeg software decode preview

Pros: no FFI introduced, and it already reaches 29.8 fps.
Cons: high CPU usage on 4K HEVC; does not meet the hardware decoding and zero-copy requirements.

### Option B: libmpv software render

Pros: measured 67.9 fps (4K HEVC), keeps wgpu; the measurement program is in `docs/investigations/mpv-sw-render-probe.c`.
Cons: requires introducing libmpv and FFI; still has one GPU → CPU readback; inconsistent with the direction of a self-built media engine.

### Option C: libmpv OpenGL render + eframe glow

Pros: can keep GPU frames.
Cons: requires switching the rendering stack; sharing GL state between egui and libmpv is complex; only OpenGL across platforms.

### Option D: In-process libav* + platform hwaccel + zero-copy

Pros: unified container and codec; zero-copy; does not depend on a player runtime.
Cons: introduces FFI; surface import implemented separately per platform; incompatible with the existing external-process preview implementation.

### Option E: Fully hand-written demux + platform media API

Pros: does not depend on libav*.
Cons: requires implementing container parsing for MP4 / MKV / TS and more, which is close to an independent project in effort.

## Decision

Choice:

> Option D: in-process `libavformat` + `libavcodec` hwaccel + platform surface import into wgpu, zero-copy.

Concrete boundaries:

```text
libavformat              demux (in-process, FFI dependency)
        ↓
libavcodec + hwaccel     VAAPI / D3D11VA / VideoToolbox (in-process, FFI dependency)
        ↓
hardware surface         DMA-BUF / D3D11 shared handle / IOSurface
        ↓
media/platform           surface → wgpu texture (the only crate in the project allowed to use unsafe)
        ↓
wgpu external texture
        ↓
egui
```

Rules:

1. Add a `media/platform` crate; it is the **only** crate in the entire workspace allowed to use `unsafe_code`.
2. All other crates keep `unsafe_code = "forbid"`, including `domain`, `application`, `ui`, and `media::ffprobe`.
3. `media/platform` exposes only safe APIs externally; types must not leak raw pointers, `wgpu-hal` handles, or FFI types.
4. Every `unsafe` block must carry a `// SAFETY:` comment stating the invariants that hold.
5. Export, trimming, merging, and probing continue to use external FFmpeg / FFprobe processes; they must not switch to linking libav*.
6. Hardware decoding failure must fall back to in-process software decoding, and the fallback must be visible in the UI.
7. Preview audio is also decoded in-process with `libavcodec`, PCM is still output through `cpal`, and the audio sample clock remains the master clock.
8. The preview decoding path must not affect Segment, timeline, or export semantics.

## Rationale

1. Phase 4A already proved that an external process pipeline cannot exploit GPU-resident frames, and no parameter tuning can eliminate the synchronous download.
2. zero-copy is an explicit goal set by the product owner.
3. `libavformat` avoids hand-written container parsing; the hwaccel in `libavcodec` unifies the three platforms with less effort than talking to each platform media API directly.
4. unsafe is isolated in a single crate while the rest of the code stays `forbid`, consistent with FilmCraft's `crates/platform` pattern.
5. Export still goes through external FFmpeg, preserving lossless stream copy semantics and the existing process management.

## Consequences

### Positive

- 4K HEVC preview can stay GPU-resident, avoiding a synchronous download per frame.
- Container and codec coverage follows FFmpeg.
- The unsafe boundary is explicit and can be constrained by crate-level lints and code review.

### Negative

- Runtime dependency on the system's `libavformat` / `libavcodec` / `libavutil` and other dynamic libraries.
- Three sets of platform surface import code: Linux DMA-BUF / Vulkan, Windows D3D11 shared handle / DX12, macOS IOSurface / Metal.
- The wgpu backend must match the platform handle, and the `wgpu-hal` version is tightly coupled to `wgpu`.
- The existing external FFmpeg paths in `media/preview/video.rs` and `audio.rs` will be replaced, and the related tests need to be rewritten.
- Audio decoding and video decoding both move in-process, leaving `cpal` with only PCM output.
- Only Linux / VAAPI can be verified on this machine; the Windows and macOS paths cannot be tested at this stage.

### Follow-up effects

- The unsafe/FFI and "FFmpeg must be executed directly" clauses in `AGENTS.md` need to be narrowed in line with this ADR.
- New tests are needed:
  - Output consistency between the hardware path and the software path (within tolerance).
  - Fallback when hwaccel initialization fails.
  - Lifetime of zero-copy textures and frame release.
  - The A/V clock still uses audio as the master clock.
- The Decoder in playback info must report the backend actually in effect, not the requested value.

## Open questions

See `docs/open-questions.md`. That document records the implementation questions this ADR leaves open, when they should be handled, and the conditions for closing them.

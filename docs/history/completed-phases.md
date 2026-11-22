# Completed phases

This file archives the task lists, implementation records, verification records, and known limitations for Phase 0–7.
The current plan and unfinished tasks are in `PLANS.md`; architecture decisions are in `docs/decisions/`.

---

## Overview: Phase 5 and the UI batches

- Single-Segment export is done and committed (`7a81434`).
- Phase 5 investigation: `docs/investigations/multi-segment-concat.md`.
- Multi-Segment implementation: temporary parts + concat demuxer; the temporary directory is cleaned up automatically after success, failure, or cancellation.
- Order strictly follows `Timeline.segments()`; non-keyframe start points fall back to the preceding keyframe.
- Concat offsets are computed from each part's real timestamps; no 1-frame gap is inserted anymore; see the investigation document for details.
  B-frame holes at the end of purely stream-copied parts are resolved by the encoded tail of smart cut (ADR-0013).
- Export chapters accumulate by the actual container duration of each temporary part; the test is in
  `tests/multi_export.rs::chapters_follow_actual_part_lengths`.
- Slicing no longer uses `-shortest`: when the source carries attached pictures or subtitle streams it ends early, and the output may be nearly empty.
- Not done: container pre-checks. End-to-end verification of 4K/60fps/VFR/subtitles/chapters is complete; see
  `docs/investigations/multi-segment-concat.md`.

### Completed UI batches

- Timeline and control row are layered: the timeline occupies its own row, and the control buttons sit below it and are responsively centered.
- Symbol buttons + hover tooltips; segments are numbered in Timeline order. The numbers on the timeline are small
  white-on-black badges and are simply not shown when a segment is too narrow; the list on the right still shows the full numbers.
- The settings page is collapsible (General / Export / Shortcuts) and supports UI font selection, font scaling, and reset.
- Fonts are grouped by family; families can be searched and a weight selected; when the system theme does not report, fall back to GTK / Qt config.
- Auto theme is detected only at startup, when opening settings, or when toggling auto theme; there is no periodic polling.
- Shortcuts are recorded on key release, ignoring modifier keys; they can be empty, take effect immediately, and playback control actions can also be bound.
- The "View" menu: fullscreen toggle, reset / increase / decrease font size.
- The "Edit" menu: undo / redo / cut / copy / paste / select all; timeline history is capped at 200 steps.
- The "Segments" menu: random segments, sort by start time / shuffle, merge all overlapping segments / invert segments (complement), clear all segments.
- The "Tools" menu: view and copy the last executed FFmpeg command (including all commands of a multi-part export).
- The export button is now "Export…" and opens the export panel; the source file directory or a specified directory can be selected, and a button inside the panel runs the export.
- The font slider takes effect after the mouse is released, steps by 0.05, and has a reset button.
- The timeline and the segment list on the right use the same color scheme; unselected segments use half intensity; Ctrl multi-select is supported.
- The playback info in the sidebar keeps only the decoder (the actual hwdec such as `mpv (vaapi)`), frame, video, and audio.
- Volume control: the volume button at the far right of the control row (above the export button) opens a narrow vertical volume bar;
  the wheel and the slider adjust in 5% steps and show a percentage; clicking outside the bar or `Esc` closes it.
  The wheel in the preview area and on the timeline is still used for keyframe seek (`Ctrl`+wheel jumps 10 seconds).
  Volume is written to `[general] volume` in `config.toml` and only applies to the mpv preview backend;
  the external FFmpeg fallback preview has no gain stage.
- When no media is open, clicking anywhere in the preview area opens a file; the hint text is not selectable.
- After dragging to the end of the timeline, mpv stays loaded, so dragging back to the middle no longer falls back to the slow external FFmpeg preview.
- Contextual cut shortcuts: with a pending cut point, `Alt+I` continues the cut point flow; otherwise it moves the segment head within the selected segment,
  and in all other cases it adds a cut point; with a pending cut point, `Alt+O` sets the playhead to the cut point end and creates a segment,
  otherwise it moves the segment tail when a segment is selected; `Delete` deletes the selected segment; `Alt+B` splits a segment at the playhead.
  `ArrowUp` / `ArrowDown` select the previous/next segment, `Alt+Left` / `Alt+Right` jump to the previous/next keyframe.
  The old defaults `I` / `O` / `Enter` are migrated once if none of the three has been changed.
- Right segment area: the header shows the count and the total duration of all segments; below the list is a horizontally centered row of icon buttons
  (plus / minus / move up / move down / blade), and the plus button becomes delete cut point when a pending cut point exists.
- The random segments dialog accepts a count (1–100); the timeline is divided into slots, each segment is 3%–10% of the total duration,
  placed randomly within the same slot, and segments never overlap.
- The right segment list disables egui's hover/active expansion and hover stroke,
  so the selection box size and stroke width stay unchanged while hovering over a selected segment.
- Playback controls and menu buttons use Nerd Font icons (embedded JetBrains Mono Nerd Font Propo),
  with fixed-width icon glyphs; previous/next segment use up/down arrows, setting segment head/tail use an upward-facing left/right hand,
  the play button is 1.5× the other buttons, and the control row is vertically centered as a whole; the batch processing button uses a list icon.
  Font licenses are in `ATTRIBUTION.md`.
- The batch processing panel offers an "Add files…" multi-select button and explains inside the panel "apply the currently
  enabled segments to every file"; a hint is shown when the queue is empty or no segment is enabled.
- The file menu "Open" and the command line both support multiple files: a single file opens directly, multiple files pop up a
  merge / batch processing question dialog.
- `Esc` closes the topmost floating window (unsaved prompt, about, commands, export, random segments, drag and drop, scenes, mute, settings).
- Opening media with container chapters automatically creates segments by chapter; chapter titles are editable and saved with the project.
- Projects opened from `.rrc` or saved manually prompt to save when closing the media / window / application.
- The menu bar, settings button, and export button are enlarged; the export button is pinned to the bottom right corner.
- Precise seek publishes no new frame before the target texture arrives; gating and timeouts are in
  `docs/investigations/mpv-engine-spike.md`. (Since 2026-11-06 the UI keeps drawing the last
  published texture instead of showing a loading state, see the 2026-11-06 section of the same document.)

Unless the user explicitly requests it, features of later phases are not implemented in advance.

---


---

# Phase 0: Project foundation

Status: `done`

## Goals

Set up a minimal runnable Rust + egui project.

## Tasks

- [x] Create the Cargo project
- [x] Set up the module structure
- [x] Add egui / eframe
- [x] Create a minimal window
- [x] Set up error handling
- [x] Set up logging
- [x] Set up the test structure
- [x] Set up basic CI checks (`scripts/check.sh`)
- [x] Create `AGENTS.md`
- [x] Create `GOALS.md`
- [x] Create `PLANS.md`
- [x] Create the architecture documents

Hosted CI: `.github/workflows/ci.yml`. The Git repository is initialized; both the initial commit and the Phase 2 revision commit have been created.

## Completion criteria

```text
cargo check
cargo test
cargo clippy
cargo fmt --check
```

All pass.

Verification record (2026-10-24):

- `./scripts/check.sh` passes, actually running `cargo fmt --all --check`, `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `cargo build`.
- The minimal window was actually launched in the current Wayland session and terminated by `timeout` after 8 seconds.

---

# Phase 1: Media analysis

Status: `done`

## Goals

Be able to open media and obtain reliable media information.

## Tasks

- [x] FFprobe process wrapper
- [x] FFprobe JSON parsing
- [x] MediaFile
- [x] MediaFormat
- [x] VideoStream
- [x] AudioStream
- [x] SubtitleStream
- [x] Chapter
- [x] Duration
- [x] Timestamp
- [x] Error handling
- [x] Test media

## Completion criteria

Correctly read typical:

- MP4
- MKV
- WMV
- AVI
- MPEG-TS

and display basic media information.

Verification record (2026-10-24):

- `./scripts/check.sh` passes, actually running `cargo fmt --all --check`, `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `cargo build`.
- 17 unit tests and 7 integration tests, all passing.
- Integration tests use the real FFprobe to read `sample.mp4`, `sample.mkv`, `sample.wmv`, `sample.avi`, `sample.ts`, asserting container, codec, dimensions, stream count, chapters, non-zero start time, and error paths.
- The window was actually launched via `./target/debug/rossresscut tests/fixtures/sample.mkv`; background probing reported 4 streams and 2 chapters.
- `.github/workflows/ci.yml` has been added but has not actually run in a GitHub Actions environment.

---

# Phase 2: Media preview

Status: `done`

## Goals

Implement smooth video preview.

## Tasks

- [x] Decide the preview backend
- [x] Design PreviewBackend
- [x] Play
- [x] Pause
- [x] Seek
- [x] Time display
- [x] Error handling
- [x] Audio playback

## Notes

The preview backend must first undergo an architecture evaluation.

Do not add an unevaluated player crate just because "video playback is needed".

The main preview path is `mpv-engine` (libmpv), and external FFmpeg + cpal is kept as a fallback; both paths are unified by `Playback`, and no `PreviewBackend` trait was introduced.

## Phase 2 revision (2026-10-24)

Completed based on real file testing:

- [x] Refresh the preview while dragging the timeline (fast keyframe scrub, precise seek after release)
- [x] Audio master clock (FFmpeg PCM + cpal), fixing the A/V offset caused by the separate ffplay process
- [x] Hardware decoding enabled by default: Linux VAAPI, Windows D3D11VA, macOS VideoToolbox; fall back to software decoding when the device is unavailable
- [x] Linux XDG desktop portal file open
- [x] Top bar changed to a menu bar (File / View / Help)
- [x] Path display changed to a title label showing only the file name without extension
- [x] Clicking the preview area toggles play / pause
- [x] The wheel in the preview area and on the timeline steps by keyframe, mod + wheel steps 10 seconds
- [x] Background keyframe index (FFprobe packet flags, no video decoding)
- [x] Custom timeline and precise playhead
- [x] Sidebar playback info (mpv Tab style) shown above the media info
- [x] Stream info collapsed by default, with its state recorded in egui memory
- [x] Settings page: theme dropdown + follow system checkbox, language selection
- [x] UI localization: `locales/*.json` embedded at compile time, language selected at runtime
- [x] Persist theme, language, and egui memory using eframe storage

## Known limitations

- A/V sync is driven by mpv's `time-pos`, which follows the audio output; it is still affected by device buffer latency (usually tens of milliseconds).
- The runtime environment needs `ffmpeg`, `ffprobe`, libmpv, and an audio output device; `ffplay` is no longer needed.
- Audio output is handled by libmpv `ao`; `cpal` is only used on the external FFmpeg fallback path.
- Hardware decoding is selected automatically by libmpv (`hwdec=auto-safe`); falls back to software when the device is unavailable.
- The playback info distinguishes `mpv (libmpv)` from external `ffmpeg (...)`; the actual decoder inside mpv is not reported separately.
- The keyframe index is generated in the background after opening media; before the index completes, the wheel falls back to 1-second steps.
- The timeline is already colored by Segment (Phase 3 done). The playhead and dragging are implemented.
- Precise seek on 4K HEVC may still take hundreds of milliseconds; the test machine's CPU is limited, so it is not representative of general hardware. Fast seek investigation is in the next section.
- Fast scrub may show a keyframe before the target time; it is only usable for visual feedback, never for Segments or export.
- The preview size is scaled to the preview pane and the source aspect ratio without upscaling; the preview is not guaranteed frame-accurate and is not a basis for editing or export.
- Volume is implemented in the control row; audio device selection and audio track selection are still not wired into the UI (mpv already supports runtime `aid` / `audio-device`).
- UI strings are localized; application-layer warnings and media-layer errors are still in English.
- The file portal is implemented on Linux only; other platforms temporarily disable Open.
- File dialogs have no automated selection tests; the XDG portal service and the `FileChooser` interface were confirmed to exist.
- The light theme is verified by screenshots and egui Context tests; the Chinese UI is verified by screenshots.

Verification record (2026-10-24):

- `./scripts/check.sh` passes, actually running `cargo fmt --all --check`, `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `cargo build`.
- 46 unit tests and 14 integration tests, all passing; 2 audio tests are ignored by default.
- Integration tests cover: first frame decoding, decoding from a 0.5s seek, decode failure paths, fast scrub, application-layer play / pause / seek, keyframe index, and keyframe navigation.
- `cargo test --test preview_audio -- --ignored --nocapture --test-threads=1` passes in a session with an audio device (2 tests, 1.94s).
- The GUI actually loaded a real 4K HEVC asset (3840x2160, 59.94fps) and took screenshots: the menu bar, title label, read-only media info, stream info collapsed by default, timeline playhead, and the `ffmpeg (vaapi)` decoder status all displayed correctly.
- In actual operation, 265 keyframes of the file were indexed in the background.
- The Chinese UI was verified by actually launching and screenshotting after changing the persisted language setting; the theme dropdown and language selection in the settings window are visible.
- Hardware decoding command construction is covered by unit tests (VAAPI arguments and render node); the `-loglevel verbose` output of a real file confirms the `hevc` decoder is selected and radeonsi VAAPI is initialized.
- Wheel direction is covered by the `wheel_direction` unit test; real wheel events cannot be injected in a headless environment, so the direction was corrected based on user feedback.
- Clicking the preview area in the empty state / failure state to open a file is implemented; no automated click verification was done.
- `.github/workflows/ci.yml` has not actually run in a GitHub Actions environment.

## Preview performance and A/V sync TODO (Phase 2 follow-up)

Status: `done via ADR 0010 and Phase 4C; the following is kept as investigation history`

### Fast seek investigation (LosslessCut 3.70.1)

Facts:

- LosslessCut's preview is not external ffmpeg CLI seeking each time; `package.json` has no mpv dependency.
- `src/renderer/src/MediaSourcePlayer.tsx` uses Chromium HTML5 `<video>` + Media Source Extensions:
  - the master video handles playback, `currentTime`, and audio;
  - the main process's `compatPlayer.createMediaSourceStream` continuously writes FFmpeg output as fragmented MP4 and appends it to a `SourceBuffer`;
  - on seek it aborts the current stream and restarts ffmpeg with a new `seekTo`, debounce 500ms;
  - the preview can be limited to 800/420px and 30/15fps, reducing decode and upload cost;
  - synchronization via `sourceBuffer.timestampOffset = seekTo - frameDuration` and `currentTime` at 30ms intervals.
- Sources:
  - https://github.com/mifi/lossless-cut/blob/master/src/renderer/src/MediaSourcePlayer.tsx
  - https://github.com/mifi/lossless-cut/blob/master/package.json

Candidate optimizations (undecided, ordered by cost):

1. Keep external FFmpeg and keep reducing preview cost:
   - VAAPI decode + `scale_vaapi`, only downloading downscaled frames;
   - lower preview resolution / fps, or generate a low-resolution proxy file;
   - during dragging only approximate by keyframe, then seek precisely after release (already implemented).
2. Switch to libmpv:
   - may improve seek, hardware decoding, and A/V sync at the same time;
   - requires rendering mpv into egui, involving GL/Vulkan interop, window embedding, and cross-platform work.
3. Other persistent decoders: GStreamer / libav bindings, not recommended for now.

### A/V sync

- The current clock is the sample frames actually consumed by cpal; writing to the device buffer in the cpal callback does not mean it has been heard, so the clock may lead by one output buffer period.
- To verify / optimize:
  - reduce the cpal output buffer;
  - subtract the device buffer latency from the playback position;
  - start audio only after the first video frame is ready;
  - do quantifiable A/V offset tests with real files.
- If still unacceptable after optimization, then evaluate libmpv.

### libmpv and unsafe (to be evaluated)

- libmpv's render API needs to render mpv frames into a texture usable by egui, usually requiring OpenGL/Vulkan interop; if this project's code calls GL/FFI, it introduces `unsafe`, violating `AGENTS.md`.
- Pure JSON IPC (external mpv process) introduces no `unsafe`, but makes it hard to embed video into egui; especially limited under Wayland.
- Conclusion: a minimal experiment and an ADR must be done before switching to libmpv; do not switch now.

## Notes before entering Phase 3 (Phase 3 is done; kept as historical constraints)

- Segment boundaries must use precise time and FFprobe data, never the keyframe approximation positions of fast scrub.
- The timeline playback position must read the application-layer audio clock; do not derive playback time from the UI wall clock.
- A keyframe index already exists (`KeyframeIndex`) and can be a candidate for Segment boundaries and navigation; but a keyframe is not the precise cut point the user expects.
- The timeline is already colored by Segment and shows the precise playhead; dragging Segment boundaries is not implemented yet.
- Hardware decoding is enabled by default. Do not disable it by default in later phases for preview timing; the preview decode path and the export path must stay separate.
- Precise seek on 4K HEVC may take hundreds of milliseconds; timeline interaction needs to accept that latency, or introduce a proxy file / GPU zero-copy preview solution first.
- Keep `Timestamp` as integer microseconds; the UI only does coordinate conversion and does not introduce floating point as the media time representation.
- The preview is not guaranteed frame-accurate and cannot be the basis for edit or export precision.

---

# Phase 3: Timeline

Status: `done`

## Goals

Implement the core timeline.

## Tasks

- [x] Timestamp
- [x] Segment
- [x] Timeline
- [x] Current playback position
- [x] Timeline colored by Segment (different Segments, different colors)
- [x] Precise playhead linked with Segment boundaries
- [x] In point
- [x] Out point
- [x] Segment creation
- [x] Segment deletion
- [x] Segment modification (set start / end from the playhead)
- [x] Segment sorting
- [x] Segment enable/disable
- [x] Keyboard operations

## Implementation record

- `src/domain/segment.rs`: `Segment`, `SegmentId`, and `Timeline`; integer microseconds, `start < end` invariant; `Vec` order is the output order, new Segments are appended, and sorting by start is an explicit operation.
- Application layer: in/out markers, select / edit / delete / move up / move down / sort / enable / disable by `SegmentId`; command errors return `SegmentCommandError`.
- UI: Segment list on the right (output order, can move up / down), colored Segments on the bottom timeline, in / out markers, precise playhead.
- Shortcuts: `Space` play / pause, `I` in point, `O` out point, `Enter` add Segment, `Delete` delete the selected Segment, `Esc` clear markers.
- Clicking the timeline seeks and selects the Segment at the click position by `SegmentId`; clicking a Segment row seeks to its start.

## Known limitations

- Segments may overlap; the timeline draws them in list order, and later-drawn Segments may cover earlier ones. Overlaps are not rejected, trimmed, or merged automatically.
- List order is the output order; modifying start/end does not automatically re-sort by start, `Sort` is an explicit operation.
- Dragging Segment boundaries or drag-reordering on the timeline is not implemented yet; output order is currently changed with the "move up / move down" buttons.
- A Timeline is bound to a single source media; opening a new file clears the Segments. Project file persistence was implemented in Phase 7A.
- Segments take part in export (since Phase 5); export interprets enabled Segments in Timeline order.

Verification record (2026-10-24):

- `./scripts/check.sh` passes, actually running `cargo fmt --all --check`, `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `cargo build`.
- 58 unit tests and 22 integration tests, all passing; 2 audio tests are ignored by default.
- Integration test `tests/timeline.rs` covers append order, explicit sorting, select / edit / delete by ID, move up / down, enable / disable, invalid markers, the EDL order guard (B → C → A is not auto-sorted), and clearing when closing media.
- The GUI actually loaded a real 4K HEVC file and took screenshots with temporary seeded Segments: different colors, in / out markers, the Segment list, the Move up / Move down buttons, and selection by `SegmentId` all displayed correctly; the seed code was removed after the screenshot.
- Timeline dragging and real keyboard input were not tested automatically; shortcut logic is guaranteed by the code path, and no real key presses were injected.

---

# Phase 4: Playback pipeline investigation and rendering architecture

Status: `done (Phase 4A–4D); multi-Segment moved to Phase 5`

Phase 4 no longer proceeds directly to export. Current actual playback performance and A/V sync are unacceptable, so the playback pipeline investigation and the rendering architecture decision are completed first, then lossless export resumes.

## Phase 4A: Player performance investigation

Goal: determine by measurement rather than guesswork whether the bottleneck is decoder, frame conversion, GPU upload, egui, wgpu, presentation, or clock/scheduler.

Tasks:

- [x] Confirm the current FFmpeg / VAAPI decode path and ffmpeg arguments.
- [x] Confirm the relationship between frame PTS / DTS / time base and the preview timestamps.
- [x] Confirm the audio clock implementation; device buffer lead was not measured (see "not yet verified" in the investigation document).
- [x] Measure A/V drift (`audio_clock - presented_video_pts`).
- [x] Measure decode → texture upload latency.
- [x] Measure egui frame / render cadence.
- [x] Measure actual presentation cadence.
- [x] Compare with mpv.
- [x] Run experiments A/B/C/D:
  - A: FFmpeg software → egui/wgpu
  - B: VAAPI → egui/wgpu
  - C: VAAPI → mpv
  - D: software → mpv
- [x] Produce the investigation document: `docs/investigations/playback-pipeline.md`.

Measured conclusions (details in the investigation document):

- With 4K HEVC the VAAPI path only reaches 10.4 fps, and `delta` grows by 11.6 seconds within 40 seconds.
- Software decoding of the same file reaches 29.8 fps, with `delta` ≈ 0–10 ms after stabilizing.
- The FFmpeg microbenchmark shows the bottleneck is VAAPI frame download into system memory: 86 fps when frames stay on the GPU the whole time, but only 20 fps once `hwdownload` is used, even after scaling down to 960x540.
- Texture upload is 0.01 ms and egui `ui_fps` is 40+, neither is a bottleneck.
- mpv reaches 33 fps with the same file and VAAPI because it keeps frames on the GPU; the external FFmpeg process + pipe model structurally requires a synchronous download.
- With 1080p H.264 VAAPI has a 2.38x advantage and is usable; the problem is concentrated in 4K HEVC.

Measurement tools:

- `examples/phase4a_probe.rs`: drives `Application` bypassing egui and outputs frame rate, presentation interval, and A/V delta.
- `ROSS_RESCUT_DIAG`: GUI rendering statistics (every 2 seconds).
- `ROSS_RESCUT_DIAG_AUTOPLAY`: automatically seek to 60s and play during diagnostics.
- `ROSS_RESCUT_PREVIEW_HW`: overrides the decode backend for this run.

Not done: download cost of Windows D3D11VA / macOS VideoToolbox; the specific reason VAAPI downloads are slow (no profiling). Minute-level long-term drift has been re-verified and passes on the mpv main path (10 minutes, see `docs/investigations/playback-pipeline.md`).

Constraints:

- Do not add more egui video features.
- Measurement tools may be temporary and rough, but must be repeatable.
- Diagnostic logging must not affect default behavior.

## Phase 4B: Decide the video rendering architecture

Status: `implemented (ADR 0010; Phase 4C steps 1–8)`

The original decision ADR 0009 option D (in-process `libavformat` + `libavcodec` + `media/platform`)
has been superseded by ADR 0010. The current decision is option F: `mpv-engine` `export` + `wgpu`;
libmpv handles decode, audio, and seek; export, trimming, merging, and probing keep using external FFmpeg / FFprobe.

Project code stays `unsafe_code = "forbid"`; GPU handles are passed frame by frame only between `media::preview` and
the UI rendering adapter layer. See `docs/decisions/0010-preview-via-mpv-engine.md`.

Rejected directions (relative to option F):

- External FFmpeg + `pipe:1`: cannot deliver GPU-resident frames.
- libmpv SW render: GPU → CPU readback.
- libmpv OpenGL + eframe glow: requires switching the rendering stack; GL state sharing is complex.
- Self-built libav* + `media/platform` (original option D): higher engineering effort and platform risk.
- Self-written demux + platform media APIs: container parsing is too much engineering effort.
- Slint: the UI framework is not the level of this problem.

Implementation tasks are in Phase 4C.

## Phase 4C: Preview decode implementation (option F)

Status: `steps 1–8 done; the old path is kept as a fallback`

Tasks:

- [x] Step 1: add the `mpv-engine` dependency, CI system packages, headless link/platform tests.
- [x] Step 2: add `media::preview::mpv`, implementing engine lifecycle, export attach,
  frame retrieval, commands, events, and errors; the UI is not wired up yet, and the external FFmpeg fallback is still in place.
- [x] Step 3: wire the UI to the wgpu feature, mpv texture registration, the Application mpv path, and
  the external FFmpeg fallback. Runtime verification shows 4K mpv textures scaled to the preview pane, `uploads=0`;
  with the feature disabled it falls back to external FFmpeg, `uploads>0`. Color/lifecycle automated regressions are in step 8.
- [x] Step 4: switch audio to mpv `ao`; `Application::uses_audio_clock` reports the mpv
  audio path, and `PreviewAudio` / `cpal` start only on the external FFmpeg fallback path.
- [x] Step 5: `hr-seek=no` while dragging / `hr-seek=yes` on release; release uses the pointer's final position,
  and precise seek falls back to a landing point ≤100 ms when `PlaybackRestart` is missing.
  Frame stepping uses mpv's native `frame-step` / `frame-back-step`; the external FFmpeg fallback
  jumps by the nominal frame duration. `tests/mpv_preview.rs` covers landing point, pause-resume, continuous scrub,
  and forward/backward frame stepping.
- [x] Step 6: scale the source dimensions to the preview pane's physical pixels (cover in both landscape and portrait, no upscaling);
  when the size changes continuously, only dispatch after two consecutive frames agree, avoiding reconfiguring export every frame while dragging the window.
  Debouncing and physical pixel conversion have unit tests; mpv export size changes have a resize regression in `tests/mpv_gpu_regression.rs`;
  real-machine window scaling verification is in `docs/investigations/mpv-engine-spike.md`.
- [x] Step 7: the playback info distinguishes `mpv (libmpv)` from the external `ffmpeg (...)` backend;
  a fallback log entry is recorded when there is no wgpu render state, and mpv failures are shown through warnings.
- [x] Step 8: test and documentation revision. Added `tests/mpv_gpu_regression.rs` (BGRA channels,
  no color shift, texture usable after drop, consecutive instances do not leak frames, size/content after resize, precise seek
  old-frame gating) and `tests/mpv_preview.rs` (seek / pause-resume / audio clock / continuous
  scrub / frame-step), all `ignored` by default and listed in the pre-release checks of `RELEASING.md`.
  HDR/10-bit preview comparison and smart cut color metadata regression are in `tests/smart_cut_export.rs`.
  AGENTS, architecture, and ADR 0010 have been synced. The old preview path is **kept**: it serves at the same time as the fallback for no
  feature / no wgpu / mpv failure; the removal conditions are in ADR 0010 "Exit conditions for the old preview path".

Completion criteria:

- 4K HEVC stays GPU-resident under VAAPI, with no per-frame synchronous download.
- Automatically fall back to software when hardware is unavailable, and it must be visible in the UI.
- Preview time is only for visual feedback and must not be used for Segment boundaries or export; actual edit times come from the source stream.
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.

Open issues are in `docs/open-questions.md`.

## Phase 4D: Lossless export (original Phase 4C / original Phase 4)

Status: `single Segment implemented; multi-Segment moved to Phase 5`

## Goals

Implement lossless Segment export with FFmpeg.

## Tasks

- [x] FFmpeg command builder (`media/export/command.rs`)
- [x] ExportJob (`application/export.rs`)
- [x] FFmpeg process runner (`ExportProcess`)
- [x] progress parser (`-progress pipe:1`)
- [x] cancellation
- [x] Single Segment export
- [x] Multi-Segment export (done in Phase 5)
- [x] Output file verification (codec / resolution / sample rate / channel count / duration)
- [x] Regression tests (`tests/export.rs`, `tests/application_export.rs`)

Implementation semantics:

- `-ss` before `-i`, fast seek, actual start falls back to the preceding keyframe.
- `-t` after `-i`, using the Segment duration.
- `-map 0 -c copy`, no re-encoding.
- Add `-avoid_negative_ts make_zero` when cutting the start.
- Progress uses `-progress pipe:1`, not relying on stderr text.
- The output path must not equal the source path; the UI uses a save dialog, and the default file name has the `_export` suffix.

Verification record (2026-10-26):

- `tests/export.rs`: a generated 1080p30 H.264/AAC asset, after exporting `[2s, 6s)` ffprobe shows matching codec, resolution, sample rate, and channel count; duration about 4 seconds.
- `tests/application_export.rs`: goes through the full `Application` flow (open → in/out → add_segment → start_export → poll), status is `Completed`.
- `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check` all pass.
- No automated UI interaction tests; menus and progress bar verified via compilation and code paths.
- Not repeated with 4K60 assets; per product requirements 1080p30 is the daily regression asset.

## Research tasks (no code copying)

- [ ] Study LosslessCut single Segment lossless export: FFmpeg arguments, `-ss` / `-to` / `-t`, stream copy, timestamps, and audio/video stream handling.
- [ ] Verify the keyframe fallback semantics of ordinary lossless cutting, and add regression tests for the actual fallback behavior.
- [ ] Distinguish product behavior, media processing algorithms, FFmpeg arguments, and Electron / Chromium-specific implementation.

## Core requirement

Must not silently switch from:

```text
stream copy
```

to:

```text
re-encode
```

---

# Phase 5: Multi-part merging

Status: `implementation done (single part + multiple parts)`

Investigation document: `docs/investigations/multi-segment-concat.md`.

Confirmed semantics:

- Use "temporary parts + concat demuxer"; the concat filter would re-encode, so it is not used.
- The concat demuxer's `inpoint/outpoint` breaks timestamps with non-keyframe start points, so it is not used.
- Each Segment's start falls back to the preceding keyframe; the output duration does not equal `sum(end - start)`.
- Concat offsets are computed from real timestamps; no frame gap is inserted anymore; the end-of-part hole of a pure copy can be
  eliminated by the encoded tail of smart cut (ADR-0013).
  Implementation is `concat_duration` in `src/media/export/concat.rs`.
- The output container should match the source; parts from the same source have a consistent stream layout.
- Timeline order is the only output order and must not be re-sorted by `start`.

## Tasks

- [x] Segment compatibility (enabled segments of the same source)
- [x] Temporary part slicing (`-ss` before `-i`, `-t`, `-map 0`, `-c copy`, `-avoid_negative_ts make_zero`)
  (`-shortest` was used before and was removed because attached pictures/subtitle streams caused early termination)
- [x] concat demuxer merging
- [x] Temporary directory lifecycle (cleaned up on success / failure / cancellation)
- [x] Multi-part progress (weighted by the requested duration of each part)
- [x] stream selection (`StreamSelection` is passed per part to each slice; `None` keeps `-map 0`)
- [x] timestamp handling (slicing uses `-avoid_negative_ts make_zero`; after slicing, probe the first/last timestamps and correct the list `duration`)
- [x] metadata / chapter explicit handling (Phase 7A / 7C)
- [x] Incompatible case handling (FFmpeg errors are propagated upward and displayed)
- [x] Order guard test (export must read the existing Timeline order and must not re-sort by start)
- [x] Regression tests (including non-keyframe start points, overlapping Segments, cancellation)
- [x] Export UI automation: `scripts/ui_export_smoke.py` drives run, complete, and cancel via AT-SPI

Verification record (2026-10-26):

- `tests/multi_export.rs`: a multi-color source (red/green/blue, 5 seconds each) exported as blue → red → green, output frames extracted to verify the order; the temporary directory does not exist after either success or cancellation.
- `tests/application_export.rs`: full `Application` flow (open → 3 Segments → start_export → poll), output order is Timeline order.
- `tests/export.rs`: single-part stream copy unchanged, codec / resolution / sample rate / channel count match.
- `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check` all pass.
- On 2026-11-05, multi-part export verification was completed for 4K60 (HEVC / AV1-WebM), 1080p60, real VFR, and assets with text subtitles and
  chapters; there is no extra frame gap at the joins. See the "2026-11-05" section of
  `docs/investigations/multi-segment-concat.md`.
- `scripts/ui_export_smoke.py` completed two exports automatically in a headless cage: one asserting
  running and completed states, output duration, and chapters; one cancelling while running and asserting there is no leftover output.

## Research tasks (no code copying)

- [x] Study LosslessCut's concat / timestamp handling and container compatibility boundaries.
- [ ] Study historical boundary fixes such as audio-only trimming and leading non-keyframe audio packets.
- [x] Add this project's own tests for the adopted media semantics.
- [x] Verify concat offsets and end-of-part holes on 60fps / 4K / VFR assets.
- [x] Verify subtitles, chapters, attached_pic, and different output containers.
  - Subtitles and chapters verified; attached_pic still uses stream copy and was not re-tested separately.

## Order semantics

- Multi-Segment export interprets enabled Segments in `Timeline.segments` order.
- Export code must not re-sort Segments by `start`; the order is determined by the Timeline.
- Overlapping Segments appear repeatedly in the output by default; they are not merged or trimmed automatically.
- SegmentId and `next_id` need to be saved during project persistence.

## UI interaction revision (2026-10-26)

- The export button moved to the bottom right corner of the main window; the File menu no longer has a single-part export entry.
- The timeline occupies its own row, playback controls moved below it, 15 symbol buttons + two time input fields in total.
- Control buttons use ASCII symbols (egui's default font lacks most media control emoji), all with tooltips.
- Cut point interaction: the first "add cut point" sets the start point; adding again after it automatically creates a Segment;
  "delete cut point" clears the pending cut point, and deletes the selected Segment when there is no pending one;
  "split segment" splits at the playhead.
- A single click on the timeline selects a segment; dragging the playhead only moves the position and does not change the selection.
- Export chapters: a setting toggle, enabled by default. Chapters are accumulated from the requested duration of each Segment.
- The about window shows the version, git commit, and current Phase.
- The media info block on the right was removed; only Playback and Segments are kept.
- Verification: `tests/application_export.rs` covers automatic cut point creation, splitting at the playhead, and deletion;
  the `ffprobe` chapter count assertion of multi-part export passes; GUI screenshots confirm the symbols and layout.

---

# Phase 6: Waveform

Status: `implementation done`

ADR: `docs/decisions/0011-waveform-in-timeline.md`.

## Tasks

- [x] Audio data acquisition (external FFmpeg decodes to mono `s16le`, one peak covers 5 ms)
- [x] waveform data model (`domain::waveform::{Peak, Waveform}`, aggregated by column)
- [x] Asynchronous generation (background thread + `Application::poll_waveform`)
- [x] Cache (aggregated columns cached by "pixel width + peak count", memory only)
- [x] Timeline drawing (waveform above the background and below the segments; segments drawn semi-transparently on top)

## Design points

- Timeline height: 42px without audio (1.5× the original 28px), 56px with audio.
- Height is based on "whether there is an audio stream", not "whether the waveform is loaded", avoiding a height jump when loading completes.
- Segment opacity: 64 unselected, 130 selected, 34 disabled; the waveform is still visible through the selected state.
- When the waveform is missing or extraction fails, the timeline works as usual, just without drawing the envelope.
- The waveform is positioned by the audio stream's own start: `start = stream.start_time - container.start_time`,
  consistent with the coordinate normalization of the keyframe index; when audio starts later than the container, leave the left side blank.
- The waveform is not persisted; audio is decoded again each time media is opened (expected behavior, not cached to disk).

## Verification record (2026-10-29)

- `tests/waveform.rs`: real FFmpeg scans `tests/fixtures/sample.mp4` (1 second 440 Hz),
  the peak count and duration match expectations; both an audio-less file and a nonexistent file produce errors with context.
- Unit tests: `Peak` merging and amplitude, `Waveform::columns` behavior on widen/narrow/zero width,
  `read_peaks` sample folding and trailing incomplete samples.
- Real asset: a 40-second 576×768/30fps Bilibili video, extracting 8109 peaks;
  silent and speech sections are visually distinguishable on the timeline; after creating 3 random segments,
  unselected segments show the waveform through, and selected segments still show it.
- Offset verification: using `-itsoffset 3` to generate an MKV with audio starting at 3 seconds, container from 0, total length 6 seconds,
  the waveform occupies only the right half of the timeline and the left side stays blank.
- Very long audio measurement (*Oppenheimer* 10822 seconds / 1920×1080 HEVC / AAC 48 kHz 6ch):
  - 2,164,429 peaks, peak memory 8.66 MB (`peaks × 4 bytes`).
  - `media::waveform::extract` in a release build takes **33.9 s** alone, of which
    ffmpeg decoding alone accounts for **31.0 s**, and the Rust side processes in about 3 s.
  - `columns(1600)` aggregation takes **1.28 ms** in a release build, and with `WaveformCache`
    it is recomputed only when the width or the peak count changes.
  - Measured in-app it takes about 88 s (mpv is decoding the 4K preview at the same time, CPU contention),
    the UI stays usable throughout, and the waveform appears automatically when it arrives.

## Not done

- When a media file is replaced outside the application, the waveform is not refreshed automatically.

---

# Phase 7: Advanced features

## Phase 7A: Project persistence (done)

ADR: `docs/decisions/0012-project-and-settings-format.md`.

- [x] `.rrc` project file (TOML; `version` / `[source]` / `[timeline]` / `[[timeline.segments]]`)
- [x] `SegmentId` and `next_id` saved as-is, identity unchanged after loading
- [x] Auto save: toggle, optional path (source file directory / specified directory), throttled writes 2 seconds after editing
- [x] Menu "File": export project… / import project… / rebind source file…
- [x] Missing source file: load the project read-only, the preview area shows the missing hint and offers rebinding
- [x] `config.toml` split into `[general]` / `[export]`, compatible with the old flat format
- [x] Metadata: the "title" in export settings is written to the output `-metadata title=`

## Phase 7B: Automation (segments menu)

- [x] Automatic splitting based on audio volume
  - Dialog parameters: silence threshold (default `-60 dB`, range -80…-20) and minimum silence duration (default 1 second, 0.1…10)
  - Split at the **midpoint** of silence intervals; keep only intervals containing audible peaks,
    so long silences at the head and tail are dropped
  - Reuse the peak data from `domain::waveform`, no more FFmpeg calls
  - **Replaces** existing segments after applying; `checkpoint` has been recorded and can be restored with undo
  - Implementation: `domain::silence` (detection and interval computation), `application::silence` (writing to the Timeline)

  Verification record (2026-11-02):

  - Unit tests: threshold conversion, silence interval detection, midpoint splitting, dropping head/tail silence, all-silence returns empty,
    error paths for missing waveform and missing duration.
  - Synthetic asset (2 seconds of sound + 2 seconds of silence + 2 seconds of sound): the preview shows "2 segments will be created",
    and after applying, `[0, 3s)` and `[3s, 6s)` are produced, with cut points exactly at the silence midpoints.
  - Real asset (40-second dialogue video): there is no silence ≥ 1 second at `-60 dB`, 1 segment is created,
    as expected (the background noise floor is above the threshold).
- [x] Scene detection
  - Dialog parameters: scene threshold (default 0.40, range 0.1…0.9)
  - External FFmpeg `select='gt(scene,N)',showinfo`, parsing only `pts_time`, not decoding into this process
  - Timestamps are normalized by the container `start_time`, consistent with the keyframe index
  - **Replaces** existing segments after applying; can be restored with undo
  - Implementation: `media::scene` (detection and parsing), `application::scene` (scan state and writing to the Timeline)
  - Verification: a 40-second asset detects 6 cut points at the 0.40 threshold and creates 7 segments,
    consistent with running FFmpeg directly
- [x] Batch processing
  - A dialog pops up when multiple files are dropped; it offers "batch processing" and "merge into one video"
  - The queue toggle button is to the left of the settings button in the menu bar; the list is above "Playback" in the right sidebar
  - Each file applies the **enabled segments** of the current timeline; parts beyond the file duration are truncated,
    and segments that fall entirely outside the file are skipped
  - Output is written as `<stem>-export.<ext>` next to each source file, or into the export directory
  - A background thread probes + exports one by one, the UI does not block; can be cancelled, completed items can be cleared
  - Implementation: `application::batch`; integration test `tests/batch_export.rs`
- [x] Merging multiple files
  - The multi-file dialog offers "merge into one video": files are sorted by name, added to the queue, and started immediately
  - Each file is first exported by the current segments as `<stem>-export.<ext>`,
    and after all succeed the concat demuxer combines them into `<first file>-merged.<ext>`
  - Remux only, no re-encoding; chapter generation is disabled during merging (chapters of different files cannot be reliably concatenated)
  - The merge result and failure reasons are shown in the batch processing panel
  - Implementation: `media::export::merge`; integration test `tests/batch_export.rs::merges_the_batch_outputs_into_one_file`

### Phase 7B known limitations

- Merging requires the export results of all files to have matching codec / resolution / time base (an inherent limitation of stream copy);
  FFmpeg reports an error when they do not match, and the error is shown in the batch processing panel.
- Merging does not generate chapters: chapters of different files cannot be reliably stacked into the merged result.
- The drag-and-drop interaction has no automated test (requires a DnD protocol); the queue logic is covered by integration tests.
- Batch processing order is fixed to the drop order (sorted by file name when merging), no manual reordering is offered.
- Batch processing output is written as `<stem>-export.<ext>`; files with the same name are overwritten.
- Scene detection needs to decode the whole video, so long assets take noticeably long; no progress percentage is shown during the scan.
- Silence detection depends entirely on waveform precision (5 ms / peak); very short silence gaps get rounded.

## Phase 7C: Media streams and smart cut

- [x] Subtitles (text subtitles)
  - Only text subtitles are handled (`srt` / `subrip` / `ass` / `ssa` / `mov_text` / `webvtt` / `text`), taking the first text subtitle track
  - External FFmpeg exports the whole track as SRT, and Rust trims it per segment and muxes it back into the output as a new subtitle track
  - General setting "keep subtitles truncated by segment boundaries": when off, the whole cue is dropped; when on, trim to the segment range (off by default)
  - Wired into both single-part and multi-part export; in multi-part each part is trimmed separately and takes part in concat
  - Implementation: `media::subtitle` (extract / parse / trim / generate), `media::export::SubtitlePlan`
  - Verified: `tests/subtitle_export.rs` covers both keep and discard strategies;
    `tests/multi_export.rs::multipart_export_replaces_source_subtitles`
    covers replacing the source subtitle track in multi-part export

  ### Known limitations

  - The `-ss` input seek does not rewrite subtitle packet timestamps, so the source subtitle track must be replaced;
    after replacement, data streams and attachments (fonts etc.) are no longer copied.
  - Bitmap subtitles (`hdmv_pgs_subtitle` / `dvd_subtitle` / `dvb_subtitle`) are not trimmed and
    keep stream copy; DVB subtitles have a smart cut regression, PGS/DVD were not directly tested.
  - In multi-part export, parts without cues get a placeholder empty subtitle track so that all parts have a consistent stream layout;
    regression test: `tests/multi_export.rs::multipart_export_replaces_source_subtitles`.
  - Subtitle extraction runs synchronously when export starts, so very long assets have a short pause.
  - Only the first text subtitle track is used; multiple subtitle track scenarios are not covered.
- [x] Stream selection (choose which streams to keep on export)
  - Decision: **stream selection takes priority**. Only subtitle tracks checked by the user take part in export; the content is still the trimmed subtitles
  - `StreamSelection { video, audio, subtitle }` stores **absolute stream indices**; `None` = keep all streams via `-map 0`
  - Subtitles checked but the trim result is empty → drop the subtitle track; no subtitles checked → no `-map 1:s` or `-c:s`
  - Wired into both single-part (`ExportRequest::with_streams`) and multi-part (`MultiExportRequest::with_streams`)
  - UI: one checkbox per stream in the export panel (`#index type codec`); reset to `None` when opening new media/a project
  - Implementation: `media::export::StreamSelection`, `application::ExportOptions`, `ui::windows::stream_selection_ui`

  ### Known limitations

  - Stream indices are based on the **probe result at open time**; replacing the source file after opening causes index mismatch.
  - Only trimming by stream is allowed; metadata operations such as changing the default track or language tag are not offered.
  - In multi-part export all parts must get the same stream layout, otherwise concat fails (same limitation as subtitle trimming).
  - An empty selection set (user unchecks everything) is equivalent to "map no streams" and FFmpeg reports an error;
    this is currently not prevented, the error is only shown to the user.
- [x] Smart cut (both start and end can be frame-accurate; when a boundary is not on a keyframe, re-encode at the part's average bitrate)
  - Decision in `docs/decisions/0013-smart-cut.md`; investigation and experiments in `docs/investigations/smart-cut.md`
  - Start on a keyframe → pure copy; start not on a keyframe → encoded head + lossless middle;
    end not on a keyframe → lossless middle + encoded tail; segment entirely between two keyframes → encode the whole part
  - Default bitrate = source average bitrate of the parts to be encoded × 1.5 (the planning thread probes packet sizes per part);
    on probe failure, fall back to the whole video stream's average bitrate × 1.5; can be changed to a fixed kbps in settings (0 = auto)
  - Re-encoding explicitly passes the source `pix_fmt` and color range / space / transfer / primaries,
    preserving 10-bit/HDR; `tests/smart_cut_export.rs` has an HDR10 regression
  - Video encoder is chosen by source codec: `h264`→`libx264`, `hevc`→`libx265`, `av1`→`libsvtav1`,
    `vp9`→`libvpx-vp9`, `prores`→`prores_ks`, etc.; hardware encoding is not used
  - Cost-effective presets: `libx264` → `faster`, `libx265` → `veryfast`,
    `libsvtav1` → `9`; the decision and VMAF measurements are in
    `docs/decisions/0014-smart-cut-encoding-defaults.md`
  - Implementation: `media::export::smart_cut` (part planning and encoder mapping),
    `ExportRequest::with_encode` / `with_rebase_negative`, `SmartCutContext` of `MultiExport`
  - With smart cut enabled, even a single segment goes through temporary parts + concat, to reuse the same path
  - VFR is supported: encoded parts add `-fps_mode passthrough`; the planning thread computes local frame durations
    from the real PTS near the cut points instead of using the average frame rate
  - Verified: `tests/smart_cut_export.rs` covers exact start, ordinary copy fallback comparison,
    multi-part Timeline order, error when no encoder is available, preset and manual bitrate, VFR frame intervals,
    DVB bitmap subtitles
  - Real asset verification: 1080p30 HEVC request 5s → output about 5.04s, first frame PSNR 36.9 dB against the source cut point
  - 2026-11-05 re-test: 1080p60 and 4K60 HEVC, two parts each, max video interval 17ms,
    max audio interval 21.3ms; the pure copy comparison shows end-of-part holes of 67ms / 110ms respectively

  ### Known limitations

  - The output head and tail are not lossless; this must be communicated to the user via the setting tooltip.
  - The end of the lossless part is two frames early and is filled in by the encoded tail from that end point, so the part end has no B-frame hole.
  - The video junction between the lossless part and the encoded tail no longer skips content; audio is affected by AAC packet boundaries,
    with a measured max gap of about 72ms for B-frame VFR and about 27ms for VFR without B frames.
  - About one frame is dropped between the start and the next keyframe, to avoid duplicate frames at the join.
  - Containers with sparse cue points probe the actual seek landing point during planning; extend the encoded head or encode the whole part if necessary.
  - Supports exactly one video stream, and it must be the stream used by preview; otherwise it fails directly.
  - Fails when the keyframe index has not finished loading; it does not wait automatically.
  - Batch processing does not support smart cut for now (each file needs its own keyframe index).
- [x] Smart cut technical investigation and implementation (ADR 0013 / 0014)
  - `docs/investigations/smart-cut.md`, `docs/decisions/0013-smart-cut.md`
- [x] Chapters editing (export chapters implemented, editing UI implemented)
  - Export chapters of all enabled segments are listed below "Segments" on the right, viewable in output time order
  - Each chapter title is editable and saved by `SegmentId`, and reordering/deleting segments does not misalign them
  - When the title is empty, export falls back to `Segment N`; wired into both single-part and multi-part export
  - Chapter titles are saved with the project: written to `chapter_title` of `[[timeline.segments]]` in `.rrc`,
    and restored as-is when loading the project (ADR-0012 has been amended)
  - Implementation: `application::chapters` (title read/write), `ui::panels::chapters_panel`,
    `MultiExportRequest::with_chapter_titles`
  - Verified: `tests/application_export.rs::exports_custom_chapter_titles`,
    `tests/application_export.rs::single_segment_chapter_uses_the_actual_output_duration`,
    `tests/project.rs::saves_and_restores_chapter_titles`

  ### Known limitations

  - For media with a configured project, only export chapters are shown; the source container's own chapters cannot be edited or imported.
  - All chapters accumulate by the **actual container duration** of each temporary part; to get the actual durations,
    single-part export with chapters enabled also goes through temporary parts + concat (one extra stream copy remux).
  - The reference implementation (LosslessCut) likewise re-probes each part after export and then generates chapters with the actual durations.
- [x] Project close / exit prompt
  - For projects opened from `.rrc` or saved manually, prompt to save when closing the media, closing the window, or exiting the application
    if there are unsaved changes
  - Importing media directly does not prompt; continuing to edit after a manual save prompts again
  - Title and other edits also advance `timeline_revision`, so they also trigger the prompt and auto save
  - Implementation: `ui::dialogs::{has_unsaved_project, request_close_media, finish_close}`,
    `ui::windows::close_prompt_window`
  - Verified: dirty detection, close after save, and close without saving in `ui::tests`
- [x] Container chapters automatically converted to segments
  - When opening media with no project to restore, if the container has chapters, create Segments by chapter
  - Chapter times subtract the container `start_time` and are trimmed to the media duration; empty or reversed chapters are skipped
  - Chapter titles are also written to the chapter titles of the corresponding Segments
  - Verified: `tests/project.rs::creates_segments_from_container_chapters`
- [x] Waveform generation progress
  - "Generating audio waveform, progress: " + progress bar displayed next to the time at the bottom left of the main window
  - Uses an indeterminate animation when the total sample count is unknown; requests a repaint every 100ms during generation
  - Implementation: `media::waveform::WaveformProgress`, `Application::waveform_progress`
- [x] Unified control sizes in menus and the settings/export windows
  - Global `button_padding = (10, 6)`, `interact_size.y = 28`
  - Menu bar settings/menu buttons 36x30; settings page shortcut button height 26

Every feature needs its own design before implementation.

---


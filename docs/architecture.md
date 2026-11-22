# Project Architecture

## 1. Overall Architecture

```text
                    ┌──────────────┐
                    │    egui UI   │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │ Application  │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │    Domain    │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │    Media     │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │FFmpeg/FFprobe│
                    └──────────────┘
```

---

## Dependency Direction

Allowed:

```text
UI
 ↓
Application
 ↓
Domain
 ↓
Media
 ↓
FFmpeg / FFprobe (external process: export / probe)
libmpv (in-process: preview decoding and audio)
```

Forbidden:

- Domain → UI / egui
- Media → UI / egui
- FFmpeg output → UI
- `media::preview` GPU texture / FFI handle types → Domain / Application / UI

Core media logic must be testable without starting a GUI.

---

# 2. UI Layer

Responsible for:

- drawing the interface
- receiving user input
- displaying application state
- displaying job progress
- displaying errors
- triggering Linux file selection through the XDG desktop portal
- displaying the read-only media title, playback info, and settings window
- selecting the UI language and reading localized strings (`ui/i18n.rs`)

Not responsible for:

- FFmpeg commands
- FFprobe JSON
- media parsing
- Segment business logic
- file export
- a self-built file picker
- computing the playback position
- constructing FFprobe keyframe queries

---

# 3. Application Layer

The application layer expresses user operations.

For example:

```text
OpenMedia
CloseMedia
Play
Pause
Seek
ScrubPreview
StepKeyframe
Skip
SetInPoint
SetOutPoint
CreateSegment
UpdateSegment
DeleteSegment
MoveSegment
SortSegments
ToggleSegment
SelectStream
ExportSegments
CancelExport
```

The UI sends commands.

The application layer handles commands.

---

# 4. Domain Layer

The domain layer expresses media editing itself.

Main models:

```text
MediaFile
MediaFormat
MediaStream
VideoStream
AudioStream
SubtitleStream
Chapter
KeyframeIndex
Timestamp
Duration
Segment
SegmentId
Timeline
ExportRequest
```

The domain layer must not depend on egui.

---

# 5. Media Layer

Responsible for:

- FFprobe
- FFmpeg
- process management
- unified construction of external processes (`media/process.rs`, sets
  `CREATE_NO_WINDOW` on Windows); the same entry point records the FFmpeg
  commands and purposes executed in this session (export / scene / waveform /
  subtitle / merge / probe), preview decoding is not recorded, and history is
  capped at 500 entries.
- argument construction
- progress parsing
- cancellation
- media operations
- preview video frame decoding (`mpv-engine` / libmpv, in-process, GPU-resident)
- preview audio output (libmpv `ao`)
- keyframe indexing (external ffprobe reads packet flags, without decoding)
- text subtitle extraction, clipping, and re-muxing (`media/subtitle.rs`)
- scene detection (`media/scene.rs`, external FFmpeg streams progress and
  cut-point timestamps)
- merging export results (`media/export/merge.rs`, remux only)

Preview boundary after the Phase 4B revision (ADR 0010):

```text
libmpv (mpv-engine, in-process)
        ↓ decoded frame (export)
      wgpu external texture
        ↓
media::preview imported as wgpu::Texture
```

Export, cutting, merging, and probing still use external FFmpeg / FFprobe
processes. See `docs/decisions/0010-preview-via-mpv-engine.md` for details.

Export flow:

```text
Single Segment
  → ffmpeg -ss before -i -t -map 0 -c copy -avoid_negative_ts make_zero

Multiple Segments
  → cut each Segment into a temporary file
  → ffprobe reads the actual duration of each temporary part
  → generate chapters from the actual durations (the start falls back to a keyframe, the requested duration differs from the actual duration)
  → ffmpeg -f concat -safe 0 -i list -map 0 -c copy
  → delete the temporary directory on success / failure / cancellation
```

Cutting does not use `-shortest`: when the source has attached pictures or
subtitle streams, it makes FFmpeg stop early at the shortest stream, and the
output may be nearly empty. The length of each part is controlled by `-t`.

The multi-segment order comes only from `Timeline.segments()`; it must not be
re-sorted by `start`. Concatenation semantics and known gaps are described in
`docs/investigations/multi-segment-concat.md`.

The media layer must not depend on egui.

`cpal` is responsible for audio PCM output only on the external FFmpeg fallback
path; on the main path audio is output by libmpv `ao`.

Preview decode device selection: the default is `hwdec=auto-safe`; the settings
page can switch it to `auto`, `d3d11va`, `nvdec`, `cuda`, `vaapi`, or `no` (only
the entries available on the current platform are listed), and can also switch
to the external FFmpeg backend. On Linux mpv exposes `--vaapi-device` (default
`/dev/dri/renderD128`); the project does not enumerate GPUs. On Windows the
preview uses `vo=libmpv` + OpenGL, so in practice the default `auto-safe`
resolves to `nvdec` (`d3d11va` requires `gpu-context=d3d11`, which is
unavailable on that path); when switching to `d3d11va`, the result reported by
mpv is authoritative.

---

# 6. Job System

Long-running work:

```text
FFprobe
FFmpeg export / probe (external process)
Preview decode (libmpv via mpv-engine, in-process)
Keyframe indexing
Audio output (libmpv ao; cpal only in the FFmpeg fallback)
Waveform
Scene Detection
Silence Detection
Batch / Merge export
```

Must not run synchronously inside the egui update.

Basic model:

```text
UI
 ↓
Command
 ↓
Application
 ↓
Job
 ↓
Worker
 ↓
FFmpeg
 ↓
Event
 ↓
Application State
 ↓
UI
```

---

# 7. State

## Domain State

Expresses the actual media editing result.

For example:

```text
Timeline
Segment
StreamSelection
```

## Application State

Expresses the program's running state.

For example:

```text
PlaybackState
PlaybackPosition
KeyframeIndex
PendingInPoint
PendingOutPoint
SelectedSegmentId
ExportProgress
ExportStatus
Error
```

Playback position belongs to application state. With audio, it advances by the
sample frames consumed by the audio output device; the wall clock is used only
when there is no audio or the audio device is unavailable.

## UI State

Expresses the interface interaction state.

For example:

```text
HoveredSegment
DraggingHandle
OpenPanel
```

The three kinds of state must keep clear boundaries.

---

# 8. Core Principles

## 1. Domain Does Not Know the UI

Domain is not allowed to:

```rust
use egui::*;
```

---

## 2. Media Does Not Know the UI

Media must not depend on:

```text
egui
eframe
UI widget
```

---

## 3. The UI Does Not Execute FFmpeg Directly

All media operations must go through the application layer.

---

# 9. Feature Implementation Order

New features should preferably follow: requirement → domain model → application command → media
operation → test → UI. Do not start by designing an entire feature from an egui widget.

---

# 10. Architecture Decisions

Major architecture decisions are recorded in `docs/decisions/`, named
`number-title.md` (for example `0010-preview-via-mpv-engine.md`).

---

# 11. Projects, Batch Processing, and Derived Data

## Project Files (ADR 0012)

- `.rrc` stores the source path, ordered Segments, `SegmentId` / `next_id`, and
  chapter titles.
- Media metadata, keyframes, waveforms, and scene detection results are not
  saved; the file is probed again when the project is opened.
- When the source file is missing, the project loads read-only; export is
  allowed only after rebinding.
- Autosave is throttled to 2 seconds after an edit; when closing the media /
  window / application, it asks if there are unsaved changes.

## Batch Processing and Merging

- Batch processing applies the enabled Segments of the current Timeline to
  each file in the queue.
- Each file is probed and exported in the background, written next to the
  source file or into the export directory; the UI does not block.
- Merging first exports each file, then remuxes them with the concat demuxer;
  no re-encoding, no chapter generation.
- Neither batch processing nor merging replaces the currently open media.

## Derived Data

- Waveforms, keyframe indices, and scene and silence detection results are all
  derived data and do not go into the project file.
- Waveform: external FFmpeg decodes to `s16le`, folded into min/max peaks
  (ADR 0011).
- Scene detection: external FFmpeg
  `showinfo@progress,select=gt(scene,N),showinfo@cuts`, parsing decode progress
  and cut-point timestamps as a stream; progress is reported only when the
  percentage increases.
- Silence cutting: reuses waveform peaks and splits at the midpoint of each
  silent interval.
- All derived data is generated on background threads; the UI only polls the
  results.

---

# 12. Current Code Structure

```text
src/
├── main.rs                    binary entry point: logging initialization, window startup, error exit
├── lib.rs                     library root: exports the layer modules
├── error.rs                   application error type (context + source chain)
├── logging.rs                 process-level logging initialization
├── application/
│   ├── mod.rs                 Application state, OpenState, query and export entry points
│   ├── media.rs               opening media, FFprobe polling, background keyframe indexing
│   ├── transport.rs           play / pause / seek / scrub / step / skip
│   ├── editing.rs             markers, selection, clipboard, undo/redo, Segment commands
│   ├── playback.rs            Playback engine: clock, decoding, mpv backend and fallback
│   ├── waveform.rs            background waveform extraction, `has_audio` and `poll_waveform`
│   ├── export.rs              ExportStatus, ExportJob (single / multi segment), export command errors
│   ├── batch.rs               batch queue: probe + export multiple files one by one
│   ├── chapters.rs            export chapter titles (saved by `SegmentId`)
│   ├── project.rs             `.rrc` project file save / restore and source rebinding
│   ├── scene.rs               background scene scan, state, and writing results into Timeline
│   └── silence.rs             automatic segment creation from silence detection (reuses waveform peaks)
├── domain/
│   ├── media.rs               MediaFile / MediaFormat / streams / Chapter / KeyframeIndex
│   ├── segment.rs             Segment / SegmentId / Timeline (ordered EDL, stable identity)
│   ├── time.rs                TimeBase / Timestamp / Duration / FrameRate
│   ├── waveform.rs            Peak / Waveform (audio envelope and per-column aggregation)
│   └── silence.rs             silent interval detection and midpoint split rules
├── media/
│   ├── export/
│   │   ├── mod.rs             ExportProcess, progress/finish events, cancellation and reclamation
│   │   ├── command.rs         ExportRequest and FFmpeg stream-copy arguments
│   │   ├── concat.rs          multi-segment cutting, concat demuxer, temporary directory lifecycle
│   │   ├── progress.rs        `-progress pipe:1` parsing
│   │   ├── chapters.rs        `ffmetadata` chapter file generation
│   │   ├── merge.rs           concat merge of already-exported files (remux only)
│   │   ├── planning.rs        multi-segment smart cut planning and seek landing point probing
│   │   └── smart_cut.rs       smart cut segmentation: encoded head/tail + lossless middle
│   ├── ffprobe/
│   │   ├── mod.rs             FFprobe process wrapper and ProbeError
│   │   ├── raw.rs             private serde structs (JSON boundary)
│   │   ├── parse.rs           JSON → domain model
│   │   ├── keyframes.rs       packet flags → keyframe index
│   │   ├── packets.rs         packet first/last timestamp boundaries (concat alignment)
│   │   └── seek.rs            probing the actual landing point of an input seek
│   ├── preview/
│   │   ├── mod.rs             PreviewError, shared process cleanup, and argument formatting
│   │   ├── mpv.rs             mpv-engine wrapper: lifecycle, export, frame retrieval, commands and events
│   │   ├── video.rs           fallback path: ffmpeg RGB frame decoding, hardware decode selection, backpressure, seek
│   │   └── audio.rs           fallback path: ffmpeg PCM decoding + cpal output
│   ├── scene.rs               external FFmpeg scene detection (parses timestamps only)
│   ├── process.rs             external process construction (`CREATE_NO_WINDOW` on Windows)
│   ├── subtitle.rs            text subtitle extraction, clipping, and re-muxing
│   └── waveform.rs            ffmpeg → s16le, folded into min/max peaks
└── ui/
    ├── mod.rs                 RossresscutApp state, window entry point and `eframe::App`
    ├── actions.rs             shortcut dispatch, segment input field synchronization
    ├── timeline.rs            timeline and waveform drawing, playback controls, wheel and coordinate conversion
    ├── preview.rs             preview drawing, texture upload, render diagnostics
    ├── panels.rs              right-side playback info, segment and chapter panels, export status row
    ├── menu.rs                menu bar
    ├── titlebar.rs            custom-drawn title bar for the undecorated window (Windows)
    ├── windows.rs             settings, about, export panel, batch, and dialog windows
    ├── settings.rs            UiSettings, config.toml read/write, theme detection and `apply_theme`
    ├── shortcuts.rs           ShortcutAction, Shortcut parsing and default bindings
    ├── fonts.rs               font discovery and font picker
    ├── dialogs.rs             XDG portal file/directory selection and close confirmation
    ├── diagnostics.rs         test diagnostics driver (preview size, autoplay, etc.)
    ├── icons.rs               Nerd Font icon glyph constants
    └── i18n.rs                language enum, strongly typed strings, and embedded resources
```

```text
locales/
├── en.json
└── zh-CN.json
```

Dependency direction:

- `main` → `ui`, `logging`, `error`
- `ui` → `eframe`, `application`, `domain` (read-only domain state); on Linux → `rfd`
- `ui::i18n` → embedded locale JSON, `serde`
- `application` → `domain`, `media::ffprobe`, `media::preview`, `error`
- `media` → `domain`, external FFmpeg / FFprobe processes, `mpv-engine` (main
  preview path), `cpal` (external FFmpeg fallback path only)
- `domain` → standard library, `serde`; must not depend on `eframe`, `egui`, or
  FFprobe JSON
- `ffprobe::raw` is only visible inside the `ffprobe` module

Opening media flow:

```text
UI Open (Linux: rfd xdg-portal, background thread) / command-line argument
        ↓
Application::open_media
        ↓
background thread → ffprobe::probe
        ↓
raw JSON → parse → MediaFile
        ↓
Application::update (non-blocking)
        ↓
background thread → ffprobe probe_keyframes (packet flags)
        ↓
KeyframeIndex
        ↓
playback state machine / UI rendering
```

Preview and clock flow:

```text
UI Play / Pause / Seek / ScrubPreview / StepKeyframe / Skip
        ↓
Application
        ├── MpvPreview (libmpv: decoding + ao + seek)
        │   └── export → wgpu texture (GPU-resident, no CPU read-back)
        └── fallback: PreviewVideo (ffmpeg → RGB24)
                      + PreviewAudio (ffmpeg → PCM → cpal)
        ↓
mpv time-pos (follows audio output) = playback position
        ↓
Application::update (non-blocking polling of events and position)
        ↓
UI texture / timeline / playback info / warnings
```

Fast scrub flow:

```text
timeline drag
        ↓
pause playback (record whether it was playing before the drag)
        ↓
Application::scrub → mpv hr-seek=no + seek
        - lands on the previous keyframe, throttled to about 80ms
        ↓
release the timeline (use the pointer's final position)
        ↓
Application::seek → mpv hr-seek=yes + precise seek
        ↓
if it was playing before the drag, then Application::play
```

Fast scrub positions are used only for visual feedback, not for edit boundaries
or export.

No FFprobe, FFmpeg, mpv command, or audio decoding runs inside an egui frame,
and channels are never blocked on.

UI layout:

```text
Top: self-drawn title bar merged with the menu bar on Windows (32 px high):
     menus on the left (File / Edit / Segments / Tools / View / Help), only minimize / maximize / close on the right
Center: video preview (click to toggle play / pause, wheel steps by a fixed duration)
Right: optional playback info and batch queue; below them two tabs, Segments / Chapters (Segments by default)
Bottom: the timeline on its own row; below it are the playback controls (11 control buttons + volume button + segment start/end time fields);
     the bottom row shows the current file name on the left, the timecode in the middle, and the Export button on the right
```

Settings window:

```text
A separate OS window (a second egui viewport), switching tabs internally:
General: theme, UI language, font and zoom, project saving, etc.
Preview: decode backend (the libmpv hwdec options / FFmpeg), wheel seek step
Export: export directory, chapters toggle, smart cut and bit rate
Shortcuts: two columns in pairs, click to record, Backspace clears while recording
```

Theme, language, and egui memory (stream collapse state, window size) are
persisted through eframe storage.

Wheel stepping in the timeline and preview area:

```text
Wheel up → go back one step
Wheel down → go forward one step
mod + wheel → ±10 seconds per notch
```

The step is configured under “Settings → Preview → Seek step”, in seconds, and
saved to `[preview] seek_step_micros` in `config.toml` (integer microseconds).
The default is `6_666_667` microseconds, i.e. 200 frames at 30 fps. For
reference, LosslessCut's wheel behavior is in
`src/renderer/src/timelineWheel.ts` (master, consulted 2026-11-07): with no
modifier, `amount = delta * wheelSensitivity * 0.2` seconds, with
`wheelSensitivity` defaulting to `0.2` (`src/main/configStore.ts`); one wheel
notch on Windows is about 100 px, hence about 4 seconds. Shift+wheel steps by
keyframes, Alt+wheel steps by single frames. This project's default step is
larger than LosslessCut's default. The previous second / next second control
buttons correspond to `previous_second` / `next_second` (default Alt+Left /
Alt+Right); single-frame stepping only has the `previous_frame` /
`next_frame` shortcuts (default `,` / `.`), with no buttons.

Wheel interaction consumes the command / ctrl modifier, so egui's ctrl+wheel UI
zoom is explicitly disabled (`InputOptions::zoom_modifier = Modifiers::NONE`).

Segment editing flow:

```text
UI: Alt+I / Alt+O / Alt+B / Delete / Buttons
        ↓
Application: set_in_point / set_out_point / add_segment
        ↓
Domain: Timeline::push(start, end) → SegmentId
        ↓
UI: Segment list (output order), timeline colors, in point / out point markers
        ↓
Delete / change boundaries / toggle / sort / move up / move down → Application commands (by SegmentId)
```

- The in point / out point and the selected `SegmentId` belong to application
  state, not to `Timeline`.
- The order of `Timeline.segments` is the output order; new Segments are
  appended to the end, and sorting by start time is an explicit command.
- Segments may overlap; they are not automatically rejected, clipped, or merged.
- Timeline clicks / drags still use `Seek` / `ScrubPreview`; a click also
  selects, by SegmentId, the Segment at the clicked position.
- See `docs/decisions/0008-segment-timeline-semantics.md` for details.

The layout direction is recorded in `GOALS.md`.

Validation entry point:

```bash
./scripts/check.sh
```

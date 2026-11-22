# Media processing rules

This document is the source of constraints for media handling, FFprobe, media time, segments,
lossless editing and export. See `docs/media-model.md` for the data model and
`docs/architecture.md` for layering.

## 1. FFmpeg is the media processing backend

Do not reimplement:

- codecs
- container parsing
- audio/video encoding
- mux
- demux
- a complete alternative implementation of timestamp handling

These jobs belong to FFmpeg.

Two invocation modes coexist:

- Export, cutting, merging and probing: external FFmpeg / FFprobe processes.
- Preview decoding and audio output: in-process `mpv-engine` (libmpv).

Both must use the capabilities of FFmpeg / libmpv; writing your own codec or container parser is
not allowed. The in-process preview boundary and zero-copy rules are in
`docs/decisions/0010-preview-via-mpv-engine.md`.

Rust is responsible for:

- command construction
- argument validation
- process spawning
- process management
- progress parsing
- cancellation
- error handling
- the media model
- application-layer logic

---

## 2. How FFmpeg is invoked

Export, cutting, merging and probing must execute the external program directly:

```text
Command
 ↓
FFmpeg executable
```

Avoid:

```text
Shell
 ↓
FFmpeg
```

Do not concatenate user input through a shell.

File paths, arguments and so on must be passed as separate process arguments.

All external processes (`ffmpeg` / `ffprobe`) must be created through
`media::process::command`; `std::process::Command::new` must not be called directly: it sets
`CREATE_NO_WINDOW` on Windows, which keeps a GUI program from popping up a console window for
every probe, waveform or decode.

Preview decoding and audio use the in-process `mpv-engine` (libmpv), which likewise must not go
through a shell, and must not be called synchronously inside an egui frame.

---

## 3. Preview and audio

- The main preview path uses `mpv-engine` (libmpv) for decoding, rendering and audio output;
  external FFmpeg preview remains as a fallback path.
- Playback position uses libmpv's `time-pos`, which follows the audio output; the same clock is
  still valid when there is no audio track.
- Audio is output by libmpv's `ao`; `cpal` is only used on the external FFmpeg fallback path and
  must not output audio at the same time as mpv.
- Dragging the timeline uses `hr-seek=no` (landing on the previous keyframe) for visual feedback;
  on release it uses the pointer's final position and switches to `hr-seek=yes` for an exact seek.
- When the `PlaybackRestart` event is missing, the exact seek falls back to the landing point of
  `position()`, with a tolerance of 100 ms.
- The position during fast scrubbing is approximate and must not be used for segment boundaries,
  edit results or export.
- Hardware decoding is chosen by libmpv by default (`hwdec=auto-safe`); it automatically falls
  back to software when the device is unavailable.
  The “Preview backend” on the settings page can select `auto` / `d3d11va` / `nvdec` / `cuda` /
  `vaapi` / `no` (only the items available on the current platform are listed), or switch to
  external FFmpeg.
  Preview uses `vo=libmpv` + the OpenGL render API, so `d3d11va` is usually unavailable on
  Windows (that hwdec requires `gpu-context=d3d11`). The default `auto-safe` measured
  `hwdec-current` as `nvdec` on NVIDIA machines, which is the normal value for this render path.
- The preview export size is scaled proportionally to the preview pane size and the source aspect
  ratio, is submitted in physical pixels, and must not exceed the source resolution (no upscaling).
- Decode failures or mpv start failures must be visible; playback info shows
  `mpv (<hwdec-current>)`, for example `mpv (vaapi)`, `mpv (vaapi-copy)`, `mpv (no)`; the external
  fallback path shows `ffmpeg (...)`.
- Clicking in the preview area toggles play / pause; the mouse wheel steps by a fixed step
  (scrolling up goes back, scrolling down goes forward, 6.67 seconds by default), and while a mod
  key is held each notch steps 10 seconds. The step can be adjusted in
  “Settings → Preview → Seek step”. The previous second / next second buttons correspond to
  `Alt+Left` / `Alt+Right`; single-frame stepping only has the `,` / `.` shortcuts and no buttons.
- Preview is not guaranteed to be frame-accurate; export and editing must not depend on preview
  time.
- Until the target texture arrives, the exact seek is gated by `Application` and does not publish
  the old position texture; the UI therefore does not clear the preview but keeps drawing the last
  published texture, avoiding a loading state flash on every seek.
  The gate has a timeout fallback so the picture does not freeze. Fast scrubbing is not affected
  by this gate.

---

## FFprobe

FFprobe output must be converted into explicit Rust data structures as soon as possible.

Recommended:

```text
FFprobe JSON
    ↓
Parser
    ↓
MediaFile
    ├── Format
    ├── VideoStream
    ├── AudioStream
    ├── SubtitleStream
    └── Chapter
```

Do not let `serde_json::Value` propagate everywhere across the project.

FFprobe JSON should only exist at the media parsing boundary.

---

## Time system

Media time is the core data of this project.

### Prohibited

Do not use:

```rust
f32
f64
```

as the sole media time representation in core logic.

---

### Rationale

Video editing involves:

- frame boundaries
- timestamps
- PTS
- DTS
- time base
- VFR
- audio/video sync
- exact seek
- container timestamps

Floating-point seconds easily cause precision and boundary problems.

---

### Requirements

The project must define explicit:

- `Timestamp`
- `Duration`
- `TimeBase`

and explicit conversion rules between them.

The concrete design is recorded in:

```text
docs/media-model.md
```

---

## Segment

Segments are the core data of user editing behavior.

Conceptual model:

```text
Segment
├── id (SegmentId)
├── start
├── end
└── enabled
```

Segments must be able to exist independently of the UI.

They must not rely on:

```text
some egui widget
some drag state
some temporary UI value
```

to express the final edit result.

The current implementation binds `source` to `Timeline` rather than storing it repeatedly in each
`Segment`. Only one source file is edited at a time; opening a new file clears the Timeline.

`Timeline.segments` is an **EDL with output order**: list order is export order, and sorting by
start time is an explicit operation, not an invariant.
Segments may overlap; they are not automatically rejected, trimmed or merged.
Selection, editing, deletion and reordering must use the stable `SegmentId` and must not rely on
the Vec index.
See `docs/decisions/0008-segment-timeline-semantics.md`.

---

## Lossless editing

The project's core feature is **lossless media editing**.

By default, prefer:

```text
stream copy
```

over re-encoding.

If an operation requires re-encoding:

- You must know the reason explicitly.
- You must explicitly tell the user.
- Silent switching to re-encoding is not allowed.
- A re-encoding operation must not be labeled as a lossless operation.
- The source's `pix_fmt` and color range / space / transfer / primaries must be passed explicitly,
  preserving 10-bit/HDR metadata; the preview's 8-bit frames must not be used as re-encoding input.
- HDR10 static metadata (Mastering display / Content light level) is only visible in the side data
  of decoded frames; the stream layer does not have it. Smart cut additionally probes the first
  frame and passes it explicitly to `libx265` with
  `-x265-params master-display=...:max-cll=...`, without relying on whether the FFmpeg version
  forwards the input frame's side data to the encoder (ffmpeg 9.0 forwards it, 6.x does not). This
  probe only runs when the transfer is PQ (`smpte2084`) or HLG (`arib-std-b67`), and a failure
  does not block opening. Other encoders do not get this set of parameters: `-x265-params` is
  x265-private syntax.
- Smart cut re-encoding always uses cost-effective presets (x264 `faster`, x265 `veryfast`,
  SVT-AV1 preset 9); the bitrate defaults to the source average bitrate of the segments that need
  encoding × 1.5, and can be changed to a fixed kbps in settings. Decision in
  `docs/decisions/0014-smart-cut-encoding-defaults.md`.
- Encoded segments of VFR material must use `-fps_mode passthrough` and compute local frame
  durations from the real PTS around the cut point; graphical subtitles keep stream copy and are
  rebased by concat.

### Degradation reasons must be specific

The degradation reason must be specific text that can be shown to the user, for example "the cut
point is not on a keyframe, so smart cut needs to re-encode"; a single boolean flag meaning
"re-encoding will happen" is not enough.

The layer that decides whether to degrade is responsible for producing the reason text; the UI only
displays it and does not assemble or rewrite the reason itself.

Multi-segment concat does not rewrite packet timestamps. Export first probes, in the background,
the first packet PTS and the last packet end time of each segment, and then writes `duration` into
the concat list as “this segment's end - the next segment's first PTS”, so the next segment's first
frame follows immediately after the previous segment's last frame without inserting a 1-frame gap.
Audio packet timestamps stay as they are.
Multi-segment output must not be described as frame-accurate: non-keyframe start points still
include the preceding content, and non-keyframe end points may leave a brief frozen picture on
material with B-frames. With smart cut enabled, the start and end are filled in by encoded
segments and the segment tail no longer has that pause; but the re-encoded head/tail must be
clearly communicated to the user. The user-facing notes are in the README and the export panel.

---

## Export

An export job should contain at least:

```text
ExportJob
├── source
├── segments
├── stream_selection
├── output
├── mode
├── progress
├── cancellation
└── result
```

Export must support:

- progress
- errors
- cancellation
- output file checking

An export must not be marked successful before it has finished successfully.

### Stream selection

- Stream selection is optional pruning: when unspecified, all streams of the source file are kept
  (`-map 0`).
- Once specified, only the listed **absolute stream indices** are mapped, without relying on
  relative order.
- Stream selection takes precedence over subtitle replacement: if the user did not check a
  subtitle track, no subtitle track is output even when subtitles were already cut.
- Subtitle content is always the **post-cut** content; stream selection only decides whether that
  track appears.
- Concatenating segments requires identical stream layouts; when they differ, streams must not be
  silently dropped to "force" success.

---

## Subtitles

- Only text subtitles are handled (`srt` / `subrip` / `ass` / `ssa` / `mov_text` / `webvtt` /
  `text`), taking the first text subtitle track.
- FFmpeg's `-ss` input seek does not rewrite subtitle packet timestamps, so the subtitle track
  cannot be stream copied directly; the whole track must be exported as SRT, trimmed on the media
  time axis, and muxed back into the output as a new subtitle track.
- Graphical subtitles (`hdmv_pgs_subtitle` / `dvd_subtitle` / `dvb_subtitle`) keep stream copy and
  are not trimmed.
- In multi-segment export, segments without cues get a placeholder empty subtitle track so that
  every segment has the same stream layout.
- General settings “Keep subtitles cut by a segment boundary”: when off, whole cues are dropped;
  when on, they are clipped to the segment range (off by default).

## Chapters

- Chapters are accumulated from the actual container duration of each temporary segment, not from
  the requested duration; for this reason, single-segment export with chapters enabled also goes
  through temporary segments + concat.
- Chapter titles are stored by `SegmentId`; a blank title falls back to `Segment N`.
- Merging does not generate chapters: chapters from different files cannot be reliably stacked.

## Batch processing and merging

- Batch processing applies the currently enabled segments of the Timeline to every file; parts
  beyond the file duration are truncated, and segments entirely outside the file are skipped.
- Batch output is written next to the source file as `<stem>-export.<ext>`, or into the export
  directory; files with the same name are overwritten.
- Merging sorts by file name, exports each segment first, and then remuxes with the concat
  demuxer; no re-encoding.
- Merging requires the codec / resolution / time base of each export result to match; when they
  differ FFmpeg reports an error, and streams must not be silently dropped to "force" success.

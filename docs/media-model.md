# Media data model

## 1. Purpose

This document defines the core media-related data structures in the project.

The media data model must be independent of the UI.

---

# 2. MediaFile

```text
MediaFile
├── path
├── format
├── duration
├── start_time
├── streams
└── chapters
```

Represents an actual media file.

---

# 3. MediaFormat

Describes container-level information.

May include:

- format name
- format long name
- duration
- size
- bitrate
- start time

---

# 4. MediaStream

All media streams share base information.

For example:

```text
index
codec
codec_name
codec_long_name
language
title
disposition
bitrate
time_base
start_time
duration
```

The concrete fields are determined by the actual FFprobe output.

Do not create information up front that FFprobe does not provide.

---

# 5. VideoStream

Video streams additionally contain:

- width
- height
- pixel format
- frame rate
- average frame rate
- codec profile
- color information

---

# 6. AudioStream

Audio streams additionally contain:

- sample rate
- channels
- channel layout
- sample format
- bitrate

---

# 7. SubtitleStream

Subtitle streams contain:

- codec
- language
- title
- disposition

Subtitle handling capabilities are extended step by step as actual features are added.

---

# 8. Timestamp

`Timestamp` is the core type of the media timeline.

It is forbidden to use the following in the domain layer:

```text
f32
f64
```

as the sole representation of media time.

Timestamp must be able to:

- compare
- add and subtract
- convert
- format
- convert to a specified time base

---

# 9. Duration

Duration represents a length of time.

It must be clearly distinguished from Timestamp.

---

# 10. TimeBase

FFmpeg uses:

```text
time_base
```

to express the unit of media timestamps.

For example:

```text
timestamp × time_base = seconds
```

The project must explicitly record:

- internal time unit
- FFmpeg time unit
- UI display unit
- conversion rules

---

# 11. Segment

```text
Segment
├── source
├── start
├── end
└── enabled
```

Requirements:

```text
start < end
```

Segment must not depend on the UI.

---

# 12. Timeline

```text
Timeline
├── source
└── segments[]
```

Timeline expresses the user's editing result.

---

# 13. Media time and UI time

The UI may display:

```text
HH:MM:SS.mmm
```

But the UI display format is not the data format of the domain layer.

Recommended:

```text
Domain Timestamp
        ↓
Formatter
        ↓
UI String
```

---

# 14. VFR

For variable frame rate media, one must not simply assume that:

```text
frame_number × constant_frame_duration
```

yields an accurate time.

For VFR:

- Timestamps must come from the media's actual data.
- Seek behavior must take FFmpeg's actual semantics into account.
- Frame navigation must not simply rely on a fixed FPS.

---

# 15. PTS / DTS

The project needs to distinguish:

- PTS
- DTS
- presentation time
- decoding time

Do not rename all timestamps to `timestamp` and lose the semantics in the process.

---

# 16. Principles

The media model must be:

- testable
- serializable
- unrelated to egui
- unrelated to concrete widgets
- decoupled from FFprobe JSON as much as possible

FFprobe is the input source.

`MediaFile` is the official data model used by the application layer.

---

# 17. Phase 1 implementation record

## 1. Internal time unit

| Type | Representation | Constraint |
| --- | --- | --- |
| `Timestamp` | `i64` microseconds | May be negative, used for the container start time |
| `Duration` | `u64` microseconds | Non-negative |
| `TimeBase` | `u32/u32` | Neither the numerator nor the denominator may be 0 |
| `FrameRate` | `u32/u32` | Neither the numerator nor the denominator may be 0 |

`TimeBase::MICROSECONDS = 1/1_000_000` is the internal canonical base. The UI display
unit is `HH:MM:SS.mmm`, formatted by the domain layer; the UI does not compute time by
itself.

Conversion rules:

```text
seconds = ticks × numerator / denominator
micros  = ticks × numerator × 1_000_000 / denominator
```

- Use `i128` intermediate values to avoid overflow.
- Round to the nearest value, with exact halves rounded away from zero.
- Overflow returns `None` instead of panicking.
- FFprobe's decimal-seconds strings are parsed as integers: at most 6 fractional digits,
  with the 7th digit rounded, without going through `f64`.
- Chapters use integer `start` / `end` + `time_base` rather than decimal strings.
- VFR: Phase 1 only stores `r_frame_rate` / `avg_frame_rate` and does not derive frame
  times or seek positions from them.

## 2. Actual model fields

`MediaFile`:

```text
path: PathBuf
format: MediaFormat
duration: Option<Duration>
start_time: Option<Timestamp>
streams: Vec<MediaStream>
chapters: Vec<Chapter>
```

`MediaFormat`:

```text
name: String
long_name: Option<String>
duration: Option<Duration>
size: Option<u64>
bit_rate: Option<u64>
start_time: Option<Timestamp>
```

`StreamInfo` (shared by all streams):

```text
index: usize
codec_name: Option<String>
codec_long_name: Option<String>
profile: Option<String>
language: Option<String>
title: Option<String>
disposition: Disposition
bit_rate: Option<u64>
time_base: Option<TimeBase>
start_time: Option<Timestamp>
duration: Option<Duration>
```

Stream types:

- `MediaStream::Video(VideoStream)`: additionally contains width and height, pixel
  format, `r_frame_rate`, `avg_frame_rate`, color range/space/transfer/primaries.
- `MediaStream::Audio(AudioStream)`: additionally contains sample rate, channels,
  channel layout, sample format.
- `MediaStream::Subtitle(SubtitleStream)`: contains only the shared fields.
- `MediaStream::Other(StreamInfo)`: other stream types reported by FFprobe; the base
  information is kept rather than discarded.

`Disposition` currently models only `default`, `forced`, `hearing_impaired`,
`visual_impaired`, `attached_pic`.

`Chapter`:

```text
id: i64
start: Timestamp
end: Timestamp
title: Option<String>
```

Missing fields or FFprobe's `N/A` map to `None`; malformed times, numbers, time bases or
chapter ordering return `ProbeError::InvalidData` carrying the path and the field name.

## 3. Serialization

The domain model derives `Serialize`. Project files (`.rrc`, ADR 0012) use a separate
persistent structure, and deserialization validates the `TimeBase` non-zero invariant,
segment ordering, and the consistency of `SegmentId` / `next_id`.

---

# 18. Playback position (Phase 2)

The playback position is application state and does not belong to `MediaFile` or
`Timeline`.

- The main path uses libmpv: the position comes from `time-pos`, which follows the audio
  output; when there is no audio track the same clock is still valid.
- The external FFmpeg fallback path is advanced by the sample frames actually consumed
  by `cpal`:

```text
position = start_position + played_frames / sample_rate
```

- The wall clock is used only when mpv is unavailable, there is no audio output, or the
  fallback path is in use.
- Video frames only display the newest frame with `timestamp <= position`, so audio is
  the master clock and video may drop frames but cannot accumulate drift.
- Fast scrub uses keyframe seek, and its position is approximate; it must not be used for
  segment boundaries, editing results, or export.
- An exact seek must be used when releasing the timeline or performing a playback seek.
- The preview frame timestamp is the preview clock, not the source file's PTS; export
  must use FFmpeg and the source timestamps.

Playback position still uses `Timestamp` (integer microseconds); no floating point is
introduced as a domain time representation.

---

# 19. Keyframe index (Phase 2 revision)

`KeyframeIndex` represents the keyframe time points of a video stream, lives in the
domain layer, and does not depend on FFprobe JSON or the UI.

```text
KeyframeIndex
└── times: Vec<Timestamp>
```

Rules:

- Timestamps are ascending and deduplicated.
- Timestamps are normalized to the media timeline: FFprobe packet PTS minus the
  container `start_time`, with negative values clamped to 0. This matches the relative
  positioning semantics of the preview input `-ss`.
- `previous(position)` returns the nearest keyframe strictly less than `position`.
- `next(position)` returns the nearest keyframe strictly greater than `position`.
- The index is generated from FFprobe's `packet=pts_time,flags` and reads only packet
  headers, without decoding video.
- The index is generated on a background thread and does not block UI frames.
- Keyframes are important boundaries for seek and for subsequent lossless cutting; the
  approximate position of fast scrub cannot replace them.

---

# 20. Segment and Timeline (Phase 3)

## 1. Segment

```text
Segment
├── id: SegmentId
├── start: Timestamp
├── end: Timestamp
└── enabled: bool
```

- The interval semantics are the half-open interval `[start, end)`, representing source
  time on a single source.
- Invariant: `start < end`; `Segment::new` returns `None` when it does not hold.
- `duration()` uses `Timestamp::duration_since` and does not go through floating point.
- The enabled state belongs to the Segment; changing the boundaries preserves the ID and
  the enabled state.
- `SegmentId` is a stable identity inside the project, allocated by `Timeline`, and does
  not depend on the Vec index.

## 2. Timeline

```text
Timeline
├── source: Option<PathBuf>
├── segments: Vec<Segment>
└── next_id: u64
```

- `segments` is an **EDL with an output order**; the Vec order is exactly the output
  order the user can observe and modify.
- The concept model's `source` is held by the Timeline rather than repeated in each
  Segment. Currently only one media file is open at a time.
- `Timeline::reset(source)` when opening media and `Timeline::clear()` when closing
  media; both reset the ID counter.
- `push` appends to the end of the output order and allocates a new `SegmentId`; it does
  not sort automatically by start point.
- `sort_by_start` is an explicit user action, not an invariant; SegmentId does not change
  after sorting.
- `move_to` / `move_up` / `move_down` change the output order and do not change the
  source time interval.
- `update` / `remove` / `set_enabled` operate by SegmentId.
- Segment overlap is allowed; segments are not automatically rejected, trimmed, or
  merged. Overlap means the same source time interval can appear multiple times in the
  output.
- IDs are not reused within the same Timeline; a Segment created after a deletion gets a
  larger ID.

## 3. Source time and output time

- `Segment.start` / `end` are source times on the 0-based media timeline, not absolute
  container PTS.
- Output time is a derived value obtained during export/merge by accumulating the
  enabled Segments in Timeline order; it is not stored in the Segment.
- Overlapping Segments increase the total output duration; disabled Segments do not take
  part in the output.

## 4. Application state

The following do not belong to the domain model:

- The pending in point / out point (temporary marks on `Application`).
- The currently selected `Option<SegmentId>`.
- The error text of segment commands.

Selection state is tracked by SegmentId, so sorting, reordering, and boundary changes
never change "which Segment is selected".

# Project Goals

## 1. Project Vision

Build a native desktop media tool based on **Rust + egui + FFmpeg**.

The goal is to deliver the core experience of **LosslessCut** while staying:

- native
- lightweight
- fast
- low on resources
- predictable in behavior
- simple in architecture

The project focuses on **fast processing of existing media files**, not on becoming a full
non-linear video editor.

---

# 2. Core Goals

## 1. Open Media

The user can open common media files.

The program can display:

- file info
- container format
- duration
- video streams
- audio streams
- subtitle streams
- encoder
- resolution
- frame rate
- bitrate
- chapters

---

## 2. Media Preview

The user can:

- play
- pause
- seek
- step forward
- step backward
- jump to a specified time
- see the current time
- see the total duration
- have the preview frame follow along while the timeline is dragged
- keep audio and video in sync
- click the preview area to toggle play / pause
- scroll the wheel to step forward / backward by a fixed step (6.67 seconds by default,
  adjustable in settings)
- hold the mod key and scroll to step forward / backward 10 seconds

The preview must stay smooth and use hardware decoding by default.

---

## 3. Timeline

The user can:

- view the media timeline
- move the playback position
- set the start position
- set the end position
- create a Segment
- modify a Segment
- delete a Segment
- sort Segments
- enable / disable a Segment
- different Segments use different colors on the timeline
- in / out point markers and a precise playhead

Segment order is a user-observable, user-modifiable output order; sorting by start point is an
explicit action. Segments may overlap; they are not automatically rejected, trimmed, or merged.

---

## 4. Lossless Cutting

By default:

```text
No re-encoding
```

Prefer FFmpeg stream copy.

If the work cannot be done losslessly because of the media format or the editing requirements:

the user must be told explicitly.

---

## 5. Multi-Segment Export

The user can:

```text
Video
 ↓
Segment A
Segment B
Segment C
 ↓
Export
```

Supported:

- a single Segment
- multiple Segments
- Segment ordering
- Segment enable / disable

Multi-segment export interprets enabled Segments in the current Timeline order.

---

# 3. Phase Two Goals

Once the core features were stable, the following enhancements were completed (Phase 6–7):
waveform, scene detection, chapter editing, subtitles, batch processing, metadata title, project
saving, shortcut improvements, volume-based auto splitting, chapters in exports, smart cut
(ADR 0013 / 0014), and the libmpv preview and A/V sync optimisation (ADR 0010). See
`docs/history/completed-phases.md` for the per-item record.

---

# 4. Explicit Non-Goals

## 1. A Complete NLE

Not implemented for now:

- video effects
- transitions
- color grading
- multi-layer tracks
- keyframe animation
- compositing
- motion graphics

---

## 2. Implementing Codecs Ourselves

Not implemented:

- H.264 decoder
- H.265 decoder
- AV1 decoder
- AAC encoder
- MP3 encoder
- custom muxer
- custom demuxer

That work is left to FFmpeg.

---

## 3. Web UI Runtime

Trading off resource usage and startup speed, this project does not bring in a browser engine
or a Web UI runtime.

Not used:

- browser engine / WebView
- Electron / Tauri
- Node.js runtime
- frontend frameworks such as React / Vue
- a UI layer implemented in TypeScript / JavaScript

## 4. Thumbnails

No timeline thumbnail strip / frame preview thumbnails are implemented. The timeline expresses
content with the waveform and Segment color blocks; the preview is carried by the video picture.

---

# 5. Product Principles

Prefer:

1. Fast
2. Simple
3. Predictable
4. Low resource usage
5. Keyboard-friendly
6. Clear error messages
7. Explicit lossless operations

Avoid:

1. overly complex settings (shortcut settings / UI settings / feature settings and other
   necessary settings)
2. processing media secretly in the background
3. deleting files automatically
4. overwriting the source file automatically
5. re-encoding the whole exported video (because smart cut does involve re-encoding)
6. building overly complex architecture in advance for future features

## Behavior Reference

LosslessCut is a reference implementation for product behavior, media semantics, and FFmpeg
workflows, not an architecture or code template for this project.

An ordinary lossless cut is limited by keyframes: the actual cut point may fall back to an
earlier keyframe. The UI must state this truthfully and must not describe an approximate cut
point as an exact one.

Smart cut is implemented (ADR 0013 / 0014): off by default; when enabled it re-encodes only the
head and tail between keyframes, and the user must be told explicitly which parts are not
lossless.

## UI Layout Direction

The desktop UI borrows **LosslessCut**'s information zoning and operating efficiency, not its
code, assets, or visual styling; every implementation uses Rust + egui. The high-level zones are:
the menu bar and media title at the top, the video preview in the center (click toggles
play / pause), the timeline, precise playhead, and timecode at the bottom, and playback info,
stream info, the Segment list, and export chapters in the side panel. Concrete widget and desktop
integration constraints are in `docs/ui-guidelines.md`.

---

# 6. First-Version Success Criteria

The user can complete:

```text
Open video
 ↓
Read media info
 ↓
Play
 ↓
Seek
 ↓
Set start position
 ↓
Set end position
 ↓
Create Segment
 ↓
Repeat creating Segments
 ↓
Export
 ↓
Get a correct output file
```

The whole flow should make the user feel that this is a **tool dedicated to fast media
processing**.

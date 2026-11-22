# ADR-0011: Waveform Preview Embedded in the Timeline

## Status

Accepted

## Context

Phase 6 needs to display the audio envelope on the editing timeline, so that users
can see at a glance where the silent sections are and thus place cut points more
accurately. The waveform is purely derived data: it does not participate in editing,
does not participate in export, and is only a visual aid for the timeline.

## Known constraints

- The timeline is entirely custom-drawn by `ui::timeline` (`egui::Painter`), with no child widgets.
- `AGENTS.md` requires that no FFmpeg, decoding, or blocking waits run inside a UI frame.
- The project adds no new decoding dependency; the only backend for audio decoding is an external
  FFmpeg process (`media::preview::audio` already uses the same pattern).
- Waveform drawing must follow the theme; there must not be a separate set of hardcoded colors
  maintained for the light and dark themes.
- The timeline represents **source time**, and segments are intervals overlaid on it.

## Candidate options

### Option A: A separate waveform bar below the timeline

The timeline keeps its original height, and a waveform area of about 56px is added below it.

Pros:

- The waveform is never obscured by segments.
- The change is localized, and the color semantics of segments are unchanged.

Cons:

- It consumes an extra 56px of vertical space, squeezing the preview area.
- The waveform and segments are visually separated, and the two areas must each be aligned to the
  timeline.

### Option B: Waveform embedded in the timeline, with segments overlaid semi-transparently

The timeline height increases from 28px to 56px, and the waveform is drawn above the background
and below the segments; segment fills become semi-transparent, with the selected state more opaque
but still showing the waveform through.

Pros:

- It consumes no extra vertical space.
- Segments and waveform share the same coordinate system, so the semantics are direct: the waveform
  is the base layer and segments are overlays.
- egui natively supports semi-transparent drawing, and the project already uses
  `Color32::from_rgba_unmultiplied` in the in/out highlight areas.

Cons:

- The segment color scheme expands from two states (enabled / disabled) to three states
  (selected / unselected / disabled).
- The contrast after stacking the waveform, the segment colors, and the index text needs to be tuned
  by measurement.

## Decision

Choose:

> Option B: waveform embedded in the timeline, with segments overlaid semi-transparently.

Additional decisions (confirmed in the same round):

- The waveform **is shown only when the media has an audio stream**. When there is no audio, the
  timeline stays at 42px (1.5× the original 28px).
- Waveform data is decoded by external FFmpeg into mono `s16le`, and then min/max peaks are computed
  on a background thread, never on the UI thread.
- Segment opacity: unselected `alpha = 64`, selected `alpha = 130`, disabled `alpha = 34`.
- The waveform **is not persisted**: the audio is decoded again every time the media is opened.
  This is intended behavior, not a gap.
- The waveform is positioned according to the audio stream's own start point:
  `start = stream.start_time - container.start_time`, with negative values clamped to 0. This is
  consistent with the coordinate normalization of the keyframe index; when the audio starts later
  than the container start, the left side is left blank.

## Rationale

1. The user explicitly chose this option, and it introduces no new technical obstacles.
2. A timeline height of 56px is still within an acceptable range, and there is no need to open a
   separate band for the waveform.
3. It preserves the existing semantics of "timeline = source time, segments = overlays" without
   introducing a second coordinate system.
4. It reuses the existing external FFmpeg + background thread pattern, adds no dependency, and
   violates no constraint about not decoding inside a UI frame.

## Consequences

### Positive

- The timeline expresses both the audio envelope and the segment layout, so locating silent sections
  requires no view switching.
- The new `domain::waveform` and `media::waveform` can be tested independently of egui.
- When the waveform is missing (no audio, decoding failure), the timeline falls back automatically
  without affecting editing.

### Negative

- Segment colors need to be measured separately for readability under the light and dark themes.
- The waveform is aggregated per pixel column and recomputed once when the window width changes; the
  result is cached only in memory.

### Follow-up impact

- The signature of `ui::timeline::segment_color` includes the selected state, and the segment list
  on the right reuses the same function.
- `ui::timeline::WaveformCache` uses "width + number of peaks" as its invalidation condition; if
  timeline zoom is added later, that key needs to be extended.

## Open questions

- The waveform is generated only once when the media is opened; it is not automatically refreshed
  when the media file is replaced externally.
- The extraction time for long audio is entirely determined by ffmpeg decoding (about 34 s standalone
  and about 88 s when running concurrently with a 4K preview, for 3 hours of material), and no
  further optimization has been done yet.

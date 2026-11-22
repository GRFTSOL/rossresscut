# ADR-0013: Smart Cut Boundaries and Failure Semantics

## Status

Accepted

## Context

A normal stream copy's leading `-ss` seek rewinds the Segment start to a preceding keyframe. When
the user places the start point between two keyframes, the output contains content outside the
Segment.

Phase 7C needs to provide an optional smart cut: re-encode two small pieces, "requested start → next
keyframe" and "last keyframe before the end → requested end", while the rest remains stream copy.

Investigation and experiment records are in `docs/investigations/smart-cut.md`.

## Known constraints

- Export must use external FFmpeg, with arguments passed one by one.
- Re-encoding must not be labeled lossless, and there must be no silent degradation.
- Time uses only integer `Timestamp` / `Duration`.
- External probe results are untrusted, and errors must carry context.
- The project already has a full-file keyframe index (`domain::media::KeyframeIndex`), loaded
  asynchronously.

## Known facts

These were confirmed by source reading and experiments, not decisions:

1. The encoded segment can start exactly at the requested start using
   `-ss <start> -i <source> -ss 0 -t <len>`.
2. The encoded segment and the lossless segment can be concatenated directly, with no gap at the
   seam.
3. The encoded tail starts from the lossless segment's end point (not from a keyframe) and covers the
   boundary around the keyframe; this way the video has no holes, and the audio no longer spans 2
   frames.
4. The lossless segment cannot be cut directly at a keyframe: `-t` truncation by DTS keeps the
   keyframe but drops the B-frames in between, leaving a 2–4 frame picture freeze; let the lossless
   segment end two frames early, and then have the encoded tail take over from that end point.
5. Neither the intermediate segments of smart cut nor the final concat may add
   `-avoid_negative_ts make_zero`, otherwise a hole of about 2 frames appears at the beginning.
6. It cannot be done when there is no usable software encoder for the source codec.
7. When a segment lies entirely between two keyframes, the whole segment must be re-encoded.
8. Matroska cue points may be sparse; the `-ss` of a copy segment may land on an earlier cue. During
   planning, a "slightly later seek target" can be used to make the demuxer land exactly on the
   planned keyframe, and then the encoded segment is extended to that landing point. When no usable
   landing point is found, the whole segment is encoded.
9. The average frame rate of VFR material cannot represent the local frame duration at the cut point.
   During planning, packet PTS is read within ±0.5s around the cut point, and the maximum adjacent
   gap is taken as the head/tail margin; the encoded segment must add `-fps_mode passthrough`,
   otherwise FFmpeg will resample the VFR into a guessed constant frame rate.
10. When graphic subtitles (at least DVB subtitle / Matroska) are kept as stream copy, `-ss` and
    concat will rebase the subtitles along with the video; measurements show that DVB subtitles land
    in the correct time ranges in all three sub-segments: the encoded head, the lossless middle, and
    the encoded tail.

## Decision

### D1: Strict failure when unavailable

Choose:

> Execute smart cut when exact cutting is possible; re-encode the whole segment when the segment
> lies entirely between two keyframes; in all other cases that cannot be completed, fail and explain
> the reason, without falling back to normal copy.

Specific conditions that trigger failure:

- No keyframe can be found after the start point;
- The frame rate is unknown, so a frame duration cannot be computed;
- The selected video stream is not exactly one;
- The source video codec has no corresponding software encoder;
- The keyframe index has not finished loading.

A failure aborts the entire export, and the error message must include the specific reason.

### D2: Encoding quality follows the source video bitrate

> Update: this decision has been adjusted by
> `docs/decisions/0014-smart-cut-encoding-defaults.md`: the default bitrate is changed to the source
> average bitrate of the segment that needs encoding × 1.5, with manual override supported in
> settings; presets are fixed to x264 `faster`, x265 `veryfast`, and SVT-AV1 preset 9.

Choose:

> Target bitrate of the encoded segment = source video bitrate × 1.2, computed automatically, with
> no additional UI.

When the source video's `bit_rate` is missing, estimate it as container size × 8 / container
duration, then multiply by 1.2. If it still cannot be determined, fail (this is the "cannot be
completed" case of D1).

## Rationale

1. Falling back to normal copy would give the user output that has a wrong start point but claims to
   be "exact", violating "no silent degradation".
2. Re-encoding the whole segment is the natural form of smart cut on short segments; if this were
   also considered a failure, the feature would be almost unusable when segments are shorter than a
   GOP.
3. Following the source bitrate with a +20% margin has been validated in the reference
   implementation; additional quality control would expand the UI and test surface, so it is left
   until a real need arises.

## Implementation boundaries

- Only one video stream is re-encoded; audio, subtitles, and attached pictures remain copy.
- For multi-segment export, each segment is split into at most three sub-segments: "encoded head +
  lossless middle + encoded tail", and all sub-segments enter the same concat list in Timeline order.
- During planning, the actual seek landing point of each lossless segment is probed; when the
  landing point is later than the requested start, the encoded head is extended to the landing point,
  and `-ss` uses a target that makes the demuxer land exactly on that point. Probing is asynchronous
  and does not block the UI.
- Subtitles are clipped separately by each sub-segment's interval and then muxed, and finally merged
  by concat.
- Chapters are still accumulated by the actual container duration of each Segment.
- The stream layout of the encoded segment and the lossless segment must be consistent; when they are
  inconsistent, FFmpeg reports an error and does not silently drop streams.

## Consequences

### Positive

- Both the Segment start and end can be frame-accurate, with no forced rewind to keyframes.
- The failure path is explicit, so there is no output that "looks successful but has a wrong start
  point".
- It reuses the existing keyframe index, temporary segments, and concat mechanisms.

### Negative

- The output is no longer a full-frame stream copy; the re-encoded head and tail lose quality.
- When a segment lies entirely between two keyframes, re-encoding the whole segment may be
  noticeably slower.
- About one frame is discarded between the start point and the next keyframe, to avoid duplicate
  frames at the splice.
- The lossless segment ends two frames early, and the encoded tail fills in from that end point;
  these two frames also have re-encoding loss.
- The video boundary between the lossless segment and the encoded tail no longer skips content; the
  audio is affected by AAC packet boundaries, with a measured maximum gap of about 72ms for B-frame
  VFR and about 27ms for non-B-frame VFR.
- Only one video stream is supported; material with multiple video streams cannot use smart cut.

### Follow-up impact

- UI text must explain that smart cut re-encodes the head and must not imply it is fully lossless.
- Smart cut failure reasons need to be displayable to the user rather than merely logged.

## Open questions

- Hardware encoders (NVENC / VAAPI / VideoToolbox) are out of the current scope; see
  `docs/decisions/0014-smart-cut-encoding-defaults.md`.
- PGS / DVD subtitles were not directly measured; DVB subtitle already covers the same stream copy
  path.
- The behavior of attached fonts under graphic subtitle stream copy is not verified.

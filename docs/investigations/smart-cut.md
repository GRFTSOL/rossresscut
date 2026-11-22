# Smart cut investigation (Phase 7C prerequisite)

Status: `Investigation complete; implemented in Phase 7C, decisions in ADR 0013 / 0014`

This file records only external behavior that was actually read, the current state of this project, and real experiment results.
See the ADRs for the implementation plan and final tradeoffs (links to be added after the decision).

## Problem

A plain stream copy's `-ss` start rolls back to the preceding keyframe.
If the user places a Segment start between two keyframes, the output is longer than requested, and the extra content lies outside the Segment.

The goal of smart cut is:

```text
Request: [5.5s, 9.0s)      Keyframe: 6.0s

plain copy:  4.0s(keyframe) ──────────────── 9.0s     ← start rolls back
smart cut:   5.5s(encoded head) │ 6.0s(keyframe) ─────── 9.0s
```

That is: re-encode only the small stretch from "requested start → next keyframe", and stream copy the rest.

## Reference implementation: LosslessCut's actual behavior

Subject read: LosslessCut main branch (GPL-2.0). The following describes behavior only; no code is copied.

### 1. Keyframe detection

- Read packets within ±10 seconds of the start with `ffprobe -read_intervals <from>%<to> -show_packets -select_streams <v>
  -show_entries packet=pts_time,flags -of json`.
- Keep only packets whose `flags` begin with `K`.
- If the start time is within `1e-6` seconds of a keyframe time → treat it as already on a keyframe, and smart cut is unnecessary.
- Otherwise take "the next keyframe after the start" as the lossless-segment start.
- If none is found, widen the window to ±60 seconds; if it is still not found, raise an error
  (`Cannot find any keyframe after the desired start cut point`).

### 2. Decision and segmentation

```text
desiredCutFrom = the start the user requested
desiredCutTo = the end the user requested
losslessCutFrom = the next keyframe after the start
lastKeyframeBeforeEnd = the keyframe before the end (or exactly at the end)

if the start is already a keyframe and the end is also a keyframe:
    plain copy the whole segment
else if losslessCutFrom >= the end:
    re-encode the whole segment (the segment lies entirely between two keyframes)
else:
    encode part [desiredCutFrom, losslessCutFrom - one frame)      # exact start
    lossless part [losslessCutFrom, lastKeyframeBeforeEnd - two frames)
    encode part [lastKeyframeBeforeEnd, desiredCutTo)              # exact end
```

The encoded part's end point is `max(desiredCutFrom + one frame, losslessCutFrom - one frame)`,
to avoid a duplicate frame at the join while also avoiding a zero-length segment.

The two-frame margin at the end avoids the problem of `-t` truncating on DTS: if the lossless part ends
exactly on a keyframe, FFmpeg keeps the keyframe and grabs one or two extra packets from the next GOP while
dropping the B frames in between, producing a still frame. Ending the lossless part two frames early and
re-encoding from the keyframe to the end in the encoded part leaves no hole at the end.

### 3. FFmpeg arguments for the encoded part (behavior level)

```text
-ss <desiredCutFrom>          # input seek, placed before -i
-i <source>
-ss 0                         # output-side reset; removing it makes the concat result start with a hole
-t <encoded part duration>
-map ...                      # one video stream + all non-video streams
-c:<video output> <encoder corresponding to the source codec>
-b:<video output> <source video bitrate × 1.2>
-bf <has_b_frames>            # only when the source reports B frames
-video_track_timescale <source video time base>
```

- The video encoder is mapped from the source `codec_name`; AV1 maps to `libsvtav1`, others keep the same name.
- When the source video `bit_rate` is missing, estimate it as "file size × 8 / duration", then multiply by 1.2.
- Re-encode only **one** video stream; audio, subtitles, and attached pictures are all copied.
- The lossless part uses a keyframe start, `keyframeCut` mode, and does **not** add `avoid_negative_ts`.
- After concat, `avoid_negative_ts` is not applied to the whole output (see Experiment 3).

### 4. Product-level limitations (acknowledged by LosslessCut itself)

- The UI states `Smart cut is experimental and will not work on all files`.
- It refuses outright when FPS is unknown (a frame duration is needed).
- It can only handle exactly one video stream.
- It supports only the main file, not external attached files.
- When bitrate detection fails, it falls back to estimating by file size.

## Current state of this project

Parts that already exist and can be reused directly:

- `domain::media::KeyframeIndex`
  - Already indexed for the whole file from `ffprobe`'s `-show_packets`, loaded on a background thread, with timestamps normalised to the media timeline.
  - Provides `previous` / `next`; `next` is **strictly greater**, so a check for "already on a keyframe" needs to be added.
- `media::export::command::build_args`
  - Argument construction for single-segment export; `-ss` is already placed before the input, `-t` after it, with `-c copy`.
- `media::export::concat`
  - Cuts each segment to a temporary file + concat demuxer; the temporary directory is cleaned up on `Drop`.
- `Application::start_export`
  - Single-segment/multi-segment branching; `ExportOptions` already carries export settings.
- The `smart_cut` setting exists and is persisted, but no code reads it.

Missing facts:

- `VideoStream` does not model `has_b_frames`; a field needs to be added from the FFprobe JSON.
- There is no "source codec → encoder" mapping.
- There is no encoded-part construction or two-part concat for smart cut.

## Experiments

Environment: Arch Linux, FFmpeg from the system packages. Material: 320x240, 30fps, 10 seconds, H.264 + AAC,
`-g 60 -keyint_min 60 -sc_threshold 0`, keyframes at 0/2/4/6/8 seconds.

### Experiment 1: Is an encoded head plus lossless tail viable?

```text
encoded part: [-ss 5.5, -ss 0, -t 0.466667]  → libx264, b:v = source bitrate × 1.2
lossless part: [-ss 6.0, -t 3.0]             → -c copy, no avoid_negative_ts
concat: -f concat -safe 0 -map 0 -c copy (no avoid_negative_ts)
```

Results:

| Item | Result |
| --- | --- |
| Encoded part | 14 frames, 0.466667s |
| Lossless part | 92 frames, 3.066667s (3.0s requested, 2 extra frames) |
| Concat output | 106 frames, 3.539974s |
| Video join | No gap; frame interval steady at 0.0333s |
| End | About 0.0667s more than requested, from the lossless part's own end overflow |

### Experiment 2: Is the start content really accurate?

Extract the first frame of the concat output and compare PSNR against the source frames at 5.5s / 6.0s:

| Comparison | PSNR |
| --- | --- |
| Output first frame vs source 5.5s frame | 36.58 dB |
| Output first frame vs source 6.0s frame | 11.07 dB |

Conclusion: the output really starts at the requested 5.5s instead of rolling back to the 6.0s keyframe.
The 36.58 dB difference comes from re-encoding loss in the encoded part.

### Experiment 3: Consequences of adding `avoid_negative_ts` at concat time

The encoded part's first frame has a negative DTS (B frames). With `-avoid_negative_ts make_zero` at concat time:

```text
video start_time = 0.066667
first frame PTS = 0.066667        ← about a 2-frame hole appears at the start
```

Without it:

```text
video start_time = 0.000000
first frame PTS = 0.000000, DTS = -0.066667
```

Conclusion: `-avoid_negative_ts make_zero` must not be applied wholesale to smart cut output.
Using it for the current plain multi-segment export is safe (each segment's input has already been zeroed by copy),
but the smart cut path must be handled differently.

### Experiment 4: Dropping `-ss 0` from the encoded part

```text
first frame PTS = 1.019987        ← the input seek's internal offset is retained
output duration 4.559961s         ← about 1 second more than requested
```

Conclusion: `-ss 0` in the encoded part cannot be omitted.

### Experiment 5: `-video_track_timescale` for Matroska output

Matroska output accepts this argument without error. Whether it actually takes effect does not affect the feasibility judgement.

## Facts / inferences / decisions

### Facts

1. The encoded part can start exactly at the requested start, provided input seek plus decode discard (`-ss` before the input + `-ss 0`).
2. The encoded part and the lossless part can be concatenated directly with no gap at the join.
3. The end can be exact as well: cover the range from the last keyframe to the requested end with an encoded part.
4. The lossless part must not end exactly on a keyframe; `-t` truncating on DTS keeps the keyframe but drops the B frames
   in between, leaving 2–4 frames of still image. Ending two frames early and handing over to the encoded part avoids this.
5. `avoid_negative_ts make_zero` must not be added when concatenating smart cut segments.
6. Matroska cue points may be sparser than keyframes. In copy mode, `-ss <keyframe>` may
   land on an earlier cue and copy content from before the keyframe. Measured with an Inception HEVC MKV:
   `-ss 60.56` landed on `53.22`, and only `-ss 61.06` landed on `60.56`.
   At planning time, use `ffmpeg -ss <target> -copyts ... -frames:v 1` + ffprobe
   to probe the actual landing point; once a target with `landing point >= requested start` is found, extend the encoded head to the landing point
   and use that target for the copy part. If none is found, encode the whole segment.
7. The current project already has a whole-file keyframe index, so it does not need LosslessCut's per-window on-demand lookup.
8. `has_b_frames` is not modelled, but can be filled in from the FFprobe JSON.

### Inferences

1. During multi-segment export, each segment may produce three sub-segments: "encoded head + lossless middle + encoded tail";
   all sub-segments can be placed into the same concat list in Timeline order.
2. The encoded part and the lossless part must have consistent stream layout, codec, and time base, otherwise concat fails.
3. When the source codec has no corresponding software encoder, smart cut cannot be completed.
4. The feature is essentially "partial re-encoding", and the output is no longer strictly
   "every frame copied as-is"; this must be made clear in product copy and export results.

### Decisions required from the project owner

See "Pending decisions" below.

## LosslessCut comparison (2026-11-04)

Compared against `smartcut.ts`, `useFfmpegOperations.ts`, and
`util/streams.ts` in `mifi/lossless-cut` master (read-only reference, no code copied):

- Its smart cut handles the **start** only: `needsSmartCut()` finds the keyframe after the start,
  `cutEncodeSmartPart()` encodes `[requested start, that keyframe)`, and the rest of the range continues as copy.
  The segment end is still truncated by copy with `-t`; there is no tail encoding.
- The early seek for Matroska + B frames (issue #1216) uses fixed compensation:
  `getKeyframeCutSeekCompensation()` adds `3/23 + 0.001` seconds to `-ss`,
  then uses `-itsoffset` to move the output zero point back to the original cut point.
- The export arguments match our encoded head: `-ss` before `-i`, an additional `-ss 0` in the encoded part,
  `-t` after the input, merging with the concat demuxer, and bitrate ×1.2.

Differences from this project:

- OQ-25: this project additionally encodes the **last GOP at the tail**; LosslessCut does not, and its segment end still
  relies on `-t` copy truncation. Both follow the same idea (partial re-encode + copy the middle +
  concat), and this project extends the scope to the end point.
- OQ-26: this project replaces fixed compensation with probing the actual seek landing point. Fixed compensation covers only about 0.13s
  of Matroska offset; measured sparse cues can be more than 7s early, so planning must be based on the landing point.
  The `-itsoffset` approach also shifts the audio timestamps, whereas probing + extending the encoded head does not change the A/V timeline.

## Pending decisions

### D1: What to do when smart cut is unavailable

Trigger conditions (any one):

- No keyframe is found after the start;
- FPS is unknown (a frame duration cannot be computed);
- More than one video stream is selected;
- The source video codec has no available software encoder;
- The segment lies entirely between two keyframes (the whole segment would need re-encoding).

Candidates:

- **A. Fail strictly**: abort the export and give a specific reason. Consistent with LosslessCut,
  and in line with "never degrade silently".
- **B. Fall back to plain copy**: the start rolls back to the preceding keyframe, and the result clearly notes
  "smart cut was not used for this segment, the start has rolled back". The export still succeeds.

### D2: Encoding quality target

- **A. Follow LosslessCut**: source video bitrate × 1.2, automatic, no UI exposure.
- **B. Provide custom bitrate/quality**: add an export setting.

A is recommended. smart cut's re-encoding range is usually only a few hundred milliseconds to a few seconds,
so automatically following the source bitrate is already sufficient; additional quality control is a later optimisation.

## 2026-11-05 VFR and graphical subtitle verification

### VFR

Synthesised VFR material: three sections at 120/60/40fps, with B frames, keyframe every 30 frames.
Smart cut ranges `[4.1,7.2)` and `[8.1,10.2)`.

Implementation:

- Add `-fps_mode passthrough` to the encoded part, to stop FFmpeg from resampling VFR into a guessed CFR.
- Detect VFR when `r_frame_rate` and `avg_frame_rate` differ by more than 0.5%.
- The planning thread reads packet PTS within ±0.5s of the cut point and takes the largest adjacent interval as the local frame duration at that boundary;
  the encoded head and tail use the local duration at the start and end respectively.
- The lossless part ends 2 local frames early, and the encoded tail starts from that end point (not from the keyframe),
  so there is no video hole and the audio does not span an entire keyframe interval.

Measured (B-frame VFR): maximum video interval 25ms (the normal frame interval of the 40fps section),
maximum audio join 72ms; the audio join for VFR without B frames is about 27ms.
AAC packets are 21.3ms, and the join gap is bounded by packet boundaries, recorded in ADR-0013 as a known limitation.
Regression: `tests/smart_cut_export.rs::smart_cut_preserves_vfr_timestamps_and_audio_cadence`.

### Graphical subtitles

FFmpeg has no PGS/DVD/DVB encoder, so GStreamer `dvbsubenc` was used to generate
`tests/fixtures/dvb_subtitles.mkv` (80x60, 10fps, 3s, DVB subtitle).

The smart cut range `[0.2,2.8)` splits into encoded head + lossless middle + encoded tail;
the output DVB subtitles have 40 packets spanning `0..2.532s`, and the video spans `0..2.466s`,
with no obvious holes beyond the output or across the join. Graphical subtitles stay as stream copy,
`-ss` and concat rebase them together with the video.

Regression: `tests/smart_cut_export.rs::smart_cut_keeps_graphical_subtitles_in_range`.
PGS / DVD were not tested directly, but they go through the same non-text subtitle copy path.

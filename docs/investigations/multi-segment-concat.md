# Multi-segment lossless concat investigation (Phase 5 prerequisite)

Status: `Investigation complete; implemented in Phase 5 (multi-segment cutting + concat demuxer); 4K / 60fps / VFR / subtitles / chapters verified end to end`

This file records only experiments that were actually run, plus their conclusions. The implementation was completed in Phase 5.

## Problem

The Timeline is an ordered EDL:

```text
Timeline
 ├─ Segment A [10s, 20s)
 ├─ Segment B [35s, 42s)
 └─ Segment C [70s, 80s)
        ↓
    A → B → C
```

We need to write multiple source-time ranges into a single file in Timeline order without re-encoding.

## Reference implementation

How LosslessCut does it:

1. Export each Segment separately to a temporary file (`-ss` before `-i`, `-t`, `-map`, `-c copy`, `-avoid_negative_ts make_zero`).
2. Merge with the concat demuxer:

```text
ffmpeg -f concat -safe 0 -protocol_whitelist file,pipe,fd -i -
       -map ... -c copy -ignore_unknown -f <format> -y <output>
```

3. Pass the concat list over stdin.
4. Input metadata separately from the first segment; chapters go through an ffmetadata input.

The concat filter is not used, because it requires decoding and re-encoding.

## Experiment environment

- FFmpeg 9.0.2
- Source material: a generated 20-second 1080p30 H.264 + AAC clip, `-g 60 -keyint_min 60 -sc_threshold 0`, keyframe every 2 seconds
- Requested ranges:
  - p1: `[3.3, 7.3)`, the start is not a keyframe
  - p2: `[10.7, 13.7)`, the start is not a keyframe

## Experiment 1: MP4 temporary segments + concat demuxer

```text
ffmpeg -ss 3.3 -i src.mp4 -t 4 -map 0 -c copy -avoid_negative_ts make_zero p1.mp4
ffmpeg -ss 10.7 -i src.mp4 -t 3 -map 0 -c copy -avoid_negative_ts make_zero p2.mp4
ffmpeg -f concat -safe 0 -i list.txt -map 0 -c copy -avoid_negative_ts make_zero out.mp4
```

Results:

| Item | Video | Audio | Container duration |
| --- | --- | --- | --- |
| p1 (requested 4s) | 5.300s / 159 frames | 5.333s / 250 packets | 5.333s |
| p2 (requested 3s) | 3.700s / 111 frames | 3.733s / 175 packets | 3.733s |
| out | 9.033s | 9.067s | 9.067s |

Key observations:

- The request `[3.3, 7.3)` actually outputs **5.333 seconds**, with the start rolled back to the keyframe at 2.0.
- The request `[10.7, 13.7)` actually outputs **3.733 seconds**, with the start rolled back to the keyframe at 10.0.
- The output duration is the sum of the actual segment durations, not `sum(end - start)`.
- Video PTS at the join: `5.282682 → 5.349349`, a gap of 66.7ms (normal frame interval 33.3ms), i.e. about a 33ms gap.
- Audio is continuous at the join: `5.312000 → 5.333333 → 5.354667`.
- The first video frame PTS is `0.016016`, not 0.

## Experiment 2: the concat demuxer's `inpoint/outpoint`

### 2a: Keyframe alignment

```text
file 'src.mp4'
inpoint 2
outpoint 7.3
file 'src.mp4'
inpoint 10
outpoint 13.7
```

Result: output 9.033s, video 9.000s / 270 frames, audio 9.033s. At the join the video goes `5.282682 → 5.316016`, a normal interval.

But FFmpeg reports `Non-monotonic DTS` and rewrites the timestamps automatically.

### 2b: Non-keyframe alignment

```text
inpoint 3.3
outpoint 7.3
inpoint 10.7
outpoint 13.7
```

Result: output 8.333s, with many `Non-monotonic DTS` warnings at the join; the second segment's video PTS advances in steps of `1/15360` (`5.282747, 5.282813, 5.282878...`), and the time base is completely scrambled.

**Conclusion: `inpoint/outpoint` is not usable for this project.**

## Experiment 3: TS temporary segments

```text
ffmpeg -ss 3.3 -i src.mp4 -t 4 -map 0 -c copy -f mpegts p1.ts
ffmpeg -ss 10.7 -i src.mp4 -t 3 -map 0 -c copy -f mpegts p2.ts
ffmpeg -f concat -safe 0 -i list.txt -map 0 -c copy -avoid_negative_ts make_zero out_ts.mp4
```

Results:

- p1: video 5.300s, audio 5.312s
- p2: video 3.700s, audio 3.605s (audio is 95ms shorter than video)
- out: 9.049s, video 9.016s, audio 9.049s
- Video at the join goes `5.282667 → 5.332000`, a 49.3ms interval

TS did not eliminate the gap, and it introduced a mismatch between audio and video length.

## Experiment 4: MKV temporary segments

```text
ffmpeg -i src.mp4 -c copy src.mkv
ffmpeg -ss 3.3 -i src.mkv -t 4 -map 0 -c copy -avoid_negative_ts make_zero m1.mkv
ffmpeg -ss 10.7 -i src.mkv -t 3 -map 0 -c copy -avoid_negative_ts make_zero m2.mkv
ffmpeg -f concat -safe 0 -i list.txt -map 0 -c copy -avoid_negative_ts make_zero out_mkv.mkv
```

Results:

- Segment start_time: video 0.000, audio 0.026
- out: 9.076s
- Video at the join goes `5.267000 → 5.338000`, a 71ms interval

MKV is no better than MP4.

## Experiment 5: `-shortest`

Adding `-shortest` when cutting:

```text
ffmpeg -ss 3.3 -i src.mp4 -t 4 -map 0 -c copy -avoid_negative_ts make_zero -shortest p1_short.mp4
```

Results:

- p1: video 5.300s, audio 5.312s
- p2: video 3.700s, audio 3.712s
- out: 9.032s, video 9.016s, audio 9.028s
- Video at the join goes `5.282682 → 5.332031`, a 49.3ms interval

Compared with the baseline, the extra gap drops from about 33ms to about 16ms.

## Content verification: start rollback

Extract the source frames at 2.0s and 3.3s, plus the first frame of p1:

```text
ffmpeg -i src.mp4 -ss 2.0 -frames:v 1 src_2.png
ffmpeg -i src.mp4 -ss 3.3 -frames:v 1 src_3.png
ffmpeg -i p1.mp4 -frames:v 1 p1_first.png
md5sum
```

Results:

```text
9819a768...  src_2.png
5be3bfaf...  src_3.png
9819a768...  p1_first.png
```

The first frame of p1 equals the source keyframe at 2.0s, not the requested 3.3s.

## Conclusion

### Approach selection

| Approach | Lossless | Conclusion |
| --- | --- | --- |
| Temporary segments + concat demuxer | Yes | **Adopted** |
| concat demuxer `inpoint/outpoint` | Yes | Non-keyframe cuts break timestamps; not used |
| concat protocol | Yes | Only suitable for MPEG-TS and similar; not general |
| concat filter | No (re-encode) | Not used |

### Known semantics

1. **The start rolls back to a keyframe**: when a Segment start is not a keyframe, the output includes the content from that keyframe up to the requested start.
2. **The output duration is not `sum(end - start)`**: it equals the sum of the actual segment durations.
3. **The concat offset is computed from real timestamps**: no extra gap is inserted any more; the measured frame interval at 30fps is 33.3ms.
   A non-keyframe end point can still leave a B-frame reorder hole under pure stream copy;
   with smart cut enabled this is filled in by the encoded tail (ADR-0013).
4. **The first video frame PTS may be non-zero**: about 16ms for MP4; this is a side effect of zeroing the B-frame DTS.
5. **MP4 and MKV behave differently**: MKV offsets the audio start by 26ms and has a larger join gap; the output container should match the source.
6. **Streams must be consistent**: segments from the same source satisfy this; different containers or stream layouts are not supported.
7. **Non-monotonic DTS warnings**: the concat demuxer may rewrite timestamps; this must be recorded rather than ignored.

### 2026-11-04 Join offset fix (implemented)

Retested with 640x360@30 H.264 + AAC material with a keyframe every 2 seconds, requesting two ranges
`[3.3,7.3)` and `[10.7,13.7)`; the baseline video join is
`5.282682 → 5.349349`, a 66.7ms interval; the normal frame interval is 33.3ms, i.e. about one extra frame.

Approaches that failed:

- Adding `-shortest` when cutting: the interval drops to 49.3ms; but it ends early when the source carries an
  attached picture or subtitle stream, so it cannot be the default behavior.
- Adding `duration` to the concat list only: the video interval is squeezed to 24–33ms, but audio packets overlap
  and `Non-monotonic DTS` appears.
- Per-stream `setts` rebasing: B frames make the first video DTS negative, and `make_zero` shifts the whole segment
  back; and when streams have different first PTS, per-stream rebasing breaks A/V sync.

Final approach: **do not rewrite any packet timestamps, only fix the `duration` in the concat list**.

1. Keep the cutting flags `-ss` before `-i`, `-t`, `-map 0`, `-c copy`, `-avoid_negative_ts make_zero`.
2. After all cutting is done, probe, on the worker thread, the first video packet PTS and the maximum `PTS + duration` for each segment.
3. Each segment's `duration` in the concat list =
   `this segment's maximum video end time - the next segment's first video packet PTS`; the last segment uses its own span.
4. Concat still uses `-avoid_negative_ts make_zero`; it performs a single global shift,
   moving all segments together without changing their relative join.

Measured (320x240@30, H.264 + AAC, keyframe every 1 second, requesting `[3.3,7.3)`,
`[10.7,13.7)`):

- Video: `4.267 → 4.300 → 4.333`, a 33.3ms interval, no stall.
- Audio: `4.296 → 4.300 → 4.323`, monotonically increasing.
- Output duration 8.038s (the audio tail is about 19ms longer than the video, due to AAC packet boundary differences).

Measured with real HDR material (HEVC 23.976fps + AAC, with B frames):

- The second segment's first frame follows directly after the first segment's last video packet (`4.796 → 4.837`, 41ms),
  with no extra gap inserted at the join.
- But the first segment itself has a still frame at its end, `4.671 → 4.796` (125ms). This is caused by the cut
  `-t` truncating on DTS and dropping B frames in the middle of the reference chain, unrelated to the concat offset,
  and is recorded in ADR-0013; the smart cut encoded tail can eliminate the segment-end hole.

The span end takes the maximum `PTS + duration` across all packets, not the last line in mux order:
B-frame reordering means the last muxed packet's PTS is not the maximum PTS.
Matroska's stream duration is `N/A`, so `probe().duration` cannot be used.

The concat offset cannot simply use each segment's own span: the first PTS of a segment cut by `-ss` is not necessarily 0,
and can differ between adjacent segments. Only `end time - next segment's first PTS` makes the next frame follow immediately after the previous one.

Conclusion: **implement "probe after cutting + concat offset fix"**, so that no frame gap is inserted at the join.
See `start_probing_timings` / `concat_duration` in `src/media/export/concat.rs`;
the regression test is `tests/multi_export.rs::joins_parts_without_a_frame_gap`.

### 2026-11-05 4K / 60fps / VFR / subtitle and chapter verification

Environment: FFmpeg 9.0.2, material from `/home/chasel/videos`.

Verification goes through the project's real code paths: multi-segment export uses `MultiExport`; smart cut uses
`Application::open_media` + `start_export`. The join gap is computed from "the previous segment's last packet end time"
and "the next segment's first packet PTS", and the segment-end hole is not counted as part of the join gap.

| Material | Range | Video interval at join | Extra stall under pure copy (video segment end / audio join) | Output duration |
| --- | --- | --- | --- | --- |
| 4K60 HEVC (3840x1920, 59.94) | `[10.010,15.015)` + `[30.030,35.035)` | 16.0ms | 67ms video / 110ms audio | 10.278s |
| 1080p60 HEVC (1920x886, 59.94) | Same as above | 16.0ms | 67ms video / 110ms audio | 10.278s |
| 4K60 AV1 / WebM (3840x1934) | `[4.033,8.333)` + `[13.300,14.967)` | 16.0ms | None | 5.988s |
| VFR (synthesised 120/60/40fps three-region clip) | `[1.0,4.0)` + `[7.0,10.0)` | 8.0ms | None | 6.026s |
| Oppenheimer 1080p HEVC 23.976 + 2 text subtitle tracks + 20 chapters | `[57.391,67.818)` + `[72.531,82.958)` | 41.0ms (one frame) | 250ms video / 231ms audio | 27.567s |

Observations:

1. The **join** for every format is separated by exactly one normal frame (about 16ms at 60fps, about 41ms at 23.976fps),
   with no extra one-frame gap inserted.
2. 60fps / HEVC material still has a segment-end B-frame hole under pure copy, showing up as 67ms of video and
   110ms at the audio join; this is the segment-end semantics already recorded in ADR-0013, not part of the join gap.
   It disappears with smart cut enabled: measured maximum video interval 17ms and
   maximum audio interval 21.33ms for 1080p60 and 4K60, with an output of 10.113s.
3. The pure-copy path for AV1/WebM and VFR material has no segment-end hole, and the join is normal as well.
4. VFR material was verified with a synthesised file: source frame intervals are 8.33 / 16.67 / 25.00ms,
   and after export the average interval is 11.97ms, the maximum is 25ms, with no timestamp regression.
   No genuine VFR files were found in the `bilibili` directory (all are CFR).
5. Material with subtitles and chapters: the output has 2 chapters, is replaced with 1 trimmed subtitle track
   and 6 subtitle packets; the source subtitle stream does not leak through.

An issue found and fixed: `MultiExport` passed the trimmed SRT to FFmpeg as an input,
but did not set `with_subtitle` on the `ExportRequest`, so `build_args` still used
`-map 0` and copied the source subtitle stream, and the SRT input was never mapped. After the fix each slice maps
the replaced subtitle track; segments with no cue get a 1ms empty placeholder cue, avoiding
inconsistent stream layouts at concat time. Regression test:
`tests/multi_export.rs::multipart_export_replaces_source_subtitles`.

Conclusion: end-to-end concat is now covered for 4K60, 60fps, VFR, and text-subtitle plus chapter containers;
OQ-17 is resolved.

### Phase 5 implementation recommendations

1. Extend `ExportRequest` from a single segment into a list of Segments in Timeline order.
2. Write each Segment first to a temporary file in the output directory or the system temp directory, with the same extension as the output.
3. Cut with `-ss` before `-i`, `-t`, `-map 0`, `-c copy`, `-avoid_negative_ts make_zero`.
4. After cutting, probe each segment's first packet PTS and last packet end time, then merge with the concat demuxer;
   compute each segment's `duration` in the list as "this segment's end - next segment's first PTS".
5. Guard the temporary directory with RAII, deleting it on success, failure, and cancellation.
6. Weight progress by each segment's actual output duration; the total duration must be read from the cut results, not just `sum(end-start)`.
7. Use `Timeline.segments()` for ordering strictly; do not re-sort by `start`.
8. Output overlapping Segments repeatedly in order; do not merge or trim them automatically.
9. Check stream consistency before starting; fail explicitly on mismatch, never re-encode silently.
10. The UI must surface the start-rollback semantics and must not describe the result as frame-accurate.

### Not verified

- Concat across different containers (for example MP4 segments → MKV output); this round only tested input and output in the same container.
- Cumulative drift of audio priming after repeated concat.
- A separate retest of attached_pic (currently kept as stream copy).

VFR and graphical subtitles for smart cut were verified separately: see
"2026-11-05 VFR and graphical subtitle verification" in `docs/investigations/smart-cut.md`.

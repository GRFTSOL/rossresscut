# ADR-0014: Smart Cut Encoding Backends and Default Parameters

## Status

Accepted

## Context

`OQ-22` asks whether to integrate hardware encoders (NVENC / VAAPI / VideoToolbox). Smart cut
re-encodes only from the requested start to the next keyframe, and from the last keyframe before the
end to the end; when a segment lies entirely between two keyframes, the whole segment is re-encoded.
The amount of encoding depends on the GOP length, not a fixed number of frames.

Measured (FFmpeg 9.0.2, on this machine):

| Scenario | Re-encode amount | Time |
| --- | --- | --- |
| 4K60 HEVC, segment entirely within one 5s GOP | 2s | 18.6s (default `medium`) |
| 4K60 HEVC, about 4s each at head and tail | 8s | 64.6s (default `medium`) |
| 1080p60 HEVC, segment entirely within one 5s GOP | 2s | 8.4s (default `medium`) |

So CPU encoding is functionally usable, but 4K long-GOP scenarios still take a noticeable amount of
CPU.

## Decision

### D1: Keep software encoding for now, no hardware encoders

Do not implement NVENC / VAAPI / VideoToolbox. Reasons:

- The availability, quality, and output differences of hardware encoders depend on the local GPU and
  driver;
- The project requires reproducible export results and does not introduce device probing and
  fallback branches for hypothetical needs;
- The re-encoded pieces of the current smart cut are usually only a few seconds, so software encoding
  is acceptable.

The conditions for resolving `OQ-22` are recorded by this ADR: the usage boundaries and degradation
semantics of hardware encoding are not introduced for now. If a real performance need for 4K long
segments arises later, "optional hardware encoding + explicit degradation" will be evaluated
separately.

### D2: Fixed cost-effective presets

Chosen using real material and VMAF. The test segments are all 2s, with target bitrate = source video
average bitrate × 1.5; VMAF uses the default `vmaf_v0.6.1` model, aligned by frame index first.

Material:

- 1080p60 HEVC `戴上耳机别眨眼，带你领略中华武术的魅力.mp4` (3.10 Mbps)
- 1080p60 HEVC `【硬核】干掉光猫！…mp4` (0.45 Mbps, low-bitrate recheck)
- 4K60 HEVC `“监控”四位同事30天…mp4` (2.07 Mbps)

x264 (1080p60, 4.65 Mbps):

| preset | Time | VMAF |
| --- | --- | --- |
| superfast | 1.54s | 91.11 |
| veryfast | 1.90s | 93.29 |
| **faster** | **2.77s** | **94.50** |
| fast | 3.36s | 94.49 |
| medium | 3.97s | 94.78 |

Low-bitrate recheck: veryfast 75.81, faster 79.43.
`faster` is the last tier with a clear gain; beyond that, VMAF basically stops rising.

x265:

| Material | preset | Time | VMAF |
| --- | --- | --- | --- |
| 4K60 | superfast | 10.30s | 91.43 |
| 4K60 | **veryfast** | **13.86s** | **92.33** |
| 4K60 | faster | 14.07s | 92.31 |
| 4K60 | fast | 19.53s | 92.28 |
| 1080p60 | superfast | 5.24s | 94.61 |
| 1080p60 | **veryfast** | **7.29s** | **95.23** |
| 1080p60 | faster | 7.52s | 95.26 |

After `veryfast`, VMAF no longer improves, and `faster` / `fast` only add time.

SVT-AV1 (1080p60, 4.65 Mbps):

| preset | Time | VMAF |
| --- | --- | --- |
| 10 | 2.65s | 93.51 |
| **9** | **3.13s** | **95.27** |
| 8 | 4.58s | 95.52 |
| 7 | 6.30s | 95.73 |
| 6 | 10.39s | 96.66 |

10 → 9 is the biggest quality jump; after 9, each gain of less than 0.5 VMAF costs more than 50%
extra time.

Final choice:

| Encoder | preset |
| --- | --- |
| `libx264` | `faster` |
| `libx265` | `veryfast` |
| `libsvtav1` | `9` |

Other encoders (`libvpx-vp9`, `prores_ks`, etc.) keep FFmpeg's default parameters. Presets are used
only when smart cut re-encodes, and are passed explicitly via `-preset`.

VMAF alignment note: using `setpts=PTS-STARTPTS` directly is affected by first-frame timestamp
differences, compressing even lossless encoding down to around 85. Tests must align by frame index
(for example, `settb=1/1000,setpts=N`); after alignment, the lossless encoding VMAF for the same
material is 98.99.

### D3: Default bitrate is the source average bitrate of the segment that needs encoding × 1.5

- During planning, the video packet sizes of segments that need re-encoding are probed one by one on
  a worker thread, and the source average bitrate of that segment is computed.
- Target bitrate = the segment's source average bitrate × 1.5; when probing fails, fall back to the
  average bitrate of the entire video stream × 1.5.
- A fixed bitrate (kbps) can be entered in settings; `0` means automatic. A manual value is used
  directly and is not multiplied by 1.5.
- The bitrate affects only re-encoded segments, not the lossless middle.

## Consequences

- No GPU dependency is added; export still works on machines without hardware encoders.
- Short-segment encoding speed for `libx264` / `libx265` / `libsvtav1` is noticeably improved.
- 4K long GOP may still take tens of seconds; the UI must continue to show progress and cancellation.
- Each segment that needs encoding incurs one more FFprobe packet-size probe, which happens on the
  planning thread and does not block the UI.
- Hardware encoding remains a possible future direction and requires a new ADR.

## Related

- `docs/decisions/0013-smart-cut.md`
- `docs/investigations/multi-segment-concat.md`
- `src/media/export/planning.rs`
- `src/media/export/command.rs`

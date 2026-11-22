# 0005 Hardware Decoding Enabled by Default

- Status: The preview implementation part has been superseded by `0009-preview-decode-zero-copy.md`; the principle of "hardware decoding enabled by default, fallback must be visible" remains in effect
- Date: 2026-10-24
- Phase: Phase 2 revision
- Supersedes: `0004-audio-clock-and-desktop-integration.md` §2

## Context

`0004` once decided to disable hardware decoding by default based on wall-clock measurements on this machine and recorded the conclusion that "VAAPI is slower". The project owner pointed out that the test machine was running an editor at the same time and its CPU was heavily loaded, so the measurement results cannot represent general hardware; and explicitly required hardware decoding to be enabled by default:

- Linux: VAAPI
- Windows: D3D11VA
- macOS: chosen by the implementation

Therefore the decision needs to be recorded again, and local timing must no longer be used as a reason to disable hardware decoding.

## Known constraints

- `unsafe` and FFI are forbidden; hardware decoding can only be used through an external FFmpeg process.
- Preview frames ultimately need to be scaled to RGB24 and uploaded to egui; this is currently not a GPU zero-copy pipeline.
- Hardware device and codec support vary by machine, so a software fallback path is required.
- Media processing (export, trimming, merging) and preview are different responsibilities; this decision only affects preview.

## Candidate approaches

### Approach A: Fixed software decoding

Advantages:

- The most consistent behavior, not dependent on a GPU device.
- No additional device detection and fallback logic.

Disadvantages:

- Conflicts with the project owner's product requirement.
- Higher CPU usage on modern GPUs.

### Approach B: Platform-default hardware decoding with software fallback when the device is unavailable

Advantages:

- Meets the product requirement.
- High-resolution material can significantly reduce CPU usage.
- A single command construction point can cover all three desktop platforms.

Disadvantages:

- FFmpeg may internally fall back for unsupported codecs, and the actual decoder currently cannot be reliably distinguished from the process output.
- Frames still need to be downloaded to the CPU, so the benefit is limited by the preview pipeline.

### Approach C: GPU zero-copy preview pipeline

Beyond the current phase's scope; it requires reworking frame transfer and texture upload.

## Decision

Choice:

> Approach B.

Specific parameters:

```text
Linux:   -hwaccel vaapi -hwaccel_device /dev/dri/renderD128
Windows: -hwaccel d3d11va
macOS:   -hwaccel videotoolbox
```

- Linux checks the render node at startup; when it does not exist, the hardware parameters are not added and a warning is recorded.
- Playback information shows the requested backend; the software path shows `ffmpeg (software)`.
- Hardware decoding is not disabled by default just to work around hardware decoding problems; fallback must be visible.

## Rationale

1. The project owner explicitly requires it to be enabled, and the local measurement environment is unreliable.
2. Hardware decoding reduces CPU usage, consistent with the low-resource product goal.
3. Falling back to software when the device is missing will not prevent machines without a GPU from playing.

## Consequences

### Positive

- Material such as high-resolution HEVC uses GPU decoding by default.
- The decoding backend is centralized in `PreviewVideoConfig`, and command construction is unit-testable.

### Negative

- The Decoder in the playback information is the requested value and does not guarantee that FFmpeg actually used that backend.
- Hardware-decoded frames still need to be downloaded to the CPU; this is not zero-copy.

### Follow-up implications

- Reporting the actual decoder accurately would require parsing FFmpeg stderr or using `-loglevel verbose`, which is a separate follow-up change.
- Segment, export, and lossless semantics must not depend on the preview decoding path.

## Open questions

- How to choose a device on multi-GPU machines other than `/dev/dri/renderD128`.
- Whether to introduce GPU zero-copy preview.

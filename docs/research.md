# Reference implementation research

This file records the boundaries of use and the research order for external reference
projects. LosslessCut is a behavioral reference, not a code template.

LosslessCut is this project's **behavioral reference implementation** and
**engineering reference library**, not this project's architecture or code template.

```text
LosslessCut
    │
    ├── product behavior        ← may reference
    ├── media semantics         ← strong reference
    ├── FFmpeg parameters       ← may study
    ├── edge cases              ← strong reference
    ├── user interaction        ← may reference
    │
    ├── TypeScript architecture ← do not copy
    ├── Electron                ← do not adopt
    ├── Chromium video          ← do not adopt
    └── Web UI                  ← do not adopt
```

Cloning the LosslessCut main branch source for research is allowed. Focus on:

- FFmpeg argument construction for single-Segment lossless export, especially `-ss`,
  `-to` / `-t`, stream copy, timestamps, and audio/video stream handling.
- keyframe boundaries, non-keyframe cut points, boundary determination and re-encode
  range for smart cut.
- PTS, DTS, time base, start time, negative timestamps, VFR, timestamps after concat.
- different audio / video start positions, audio-only trimming, leading non-keyframe
  audio packets.
- container compatibility, metadata, subtitles, chapters.
- concrete edge-case bugs and fix strategies in historical issues / commits.

LosslessCut's official documentation states that ordinary lossless cutting is limited
by keyframes, and the actual cut point may fall back to the preceding keyframe. This is
behavioral semantics that Phase 4 must understand and explain truthfully to users.

### Research method

The correct path:

```text
LosslessCut source
        ↓
understand why it does this
        ↓
verify FFmpeg behavior
        ↓
convert into Rust / domain / application / media design
```

The wrong path:

```text
LosslessCut TypeScript
        ↓
translate into Rust
```

The latter easily produces "TypeScript wearing a Rust coat", and may drag
Electron / Chromium-specific behavior into this project.

### Research order

1. This project's existing code / ADRs.
2. Official FFmpeg documentation.
3. LosslessCut implementation and issues / commits.
4. Experimental verification.
5. Design this project's implementation.

### License constraints

LosslessCut uses GPL-2.0. Allowed:

- Read the source and understand product behavior and media semantics.
- Check FFmpeg behavior and run minimal experiments.
- Design this project's own Rust implementation and tests.

Forbidden:

- Copy LosslessCut source code.
- Translate TypeScript functions line by line into Rust.
- Ports that "only change the syntax".

### Investigate LosslessCut behavior for <problem>

1. Check the current LosslessCut source.
2. Find the relevant implementation, commit, issue, or documentation.
3. Explain the specific problem it solves.
4. Distinguish: product behavior, media processing algorithms, FFmpeg parameters,
   Electron / Chromium-specific implementation.
5. Check which parts apply to this project.
6. Do not copy LosslessCut source code.
7. Do not change this project's Rust + egui + FFmpeg architecture constraints.
8. If the conclusion affects the Domain / Application / Media architecture, enter the
   `architecture-decision` process.
9. Add this project's own tests for key conclusions.

---

## FilmCraft / EffectCraft

FilmCraft and EffectCraft can serve as references for Rust + egui engineering practice,
especially:

- engine-first, with the UI toolkit at the topmost layer;
- media / render / gpu separated from the UI;
- module boundaries and testing approach for a large media project.

The boundaries of use are the same as for LosslessCut:

- Only reference architecture tradeoffs and behavior; do not copy code.
- Do not bring their dependencies, platform constraints, or product scope directly into
  this project.
- Any conclusion that affects the Domain / Application / Media architecture goes through
  `architecture-decision`.

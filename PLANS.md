# Development Plan

## Current Phase

**Phases 0–8 are all complete.**

- The wrap-up records for the release phase (Phase 8) are below; since v0.4.2 the Linux pacman
  package is built by GitHub Actions on `v*` tags and attached to the Release of the same name,
  while the Windows portable package is still built on this machine.
- Detailed implementation and verification records are archived in
  `docs/history/completed-phases.md`.
- Later work is feature polishing and bug fixing, no longer numbered by Phase; an ADR is still
  written when there is a real trade-off.
- Open issues are recorded in `docs/open-questions.md`.

## Completed Phase Summary

| Phase | Result | Key constraint / decision |
| --- | --- | --- |
| Phase 0 | Project foundation | Rust + egui; `unsafe_code = "forbid"` |
| Phase 1 | Media analysis | FFprobe JSON boundary; integer time types |
| Phase 2 | Media preview | libmpv main path (ADR 0010); external FFmpeg fallback |
| Phase 3 | Timeline | ordered EDL, stable `SegmentId`, overlap allowed (ADR 0008) |
| Phase 4 | Playback pipeline and lossless export | 4A measurement; 4B/4C mpv preview; 4D single-segment stream copy |
| Phase 5 | Multi-segment merging | temporary parts + concat demuxer; order comes only from the Timeline |
| Phase 6 | Waveform | external FFmpeg decodes the peak; drawn as the timeline background (ADR 0011) |
| Phase 7 | Advanced features | project files (ADR 0012), batch / merge, subtitle / stream selection, smart cut (ADR 0013/0014) |

Details such as the Phase 5 export semantics, the Phase 6 waveform design, and the Phase 7 known
limitations are in `docs/history/completed-phases.md`.

---

# Reference Implementation Research

LosslessCut is a reference implementation for product behavior, media semantics, and FFmpeg
workflows, not an architecture or code template for this project.

- Research order: this project's code / ADRs → the official FFmpeg documentation → the
  LosslessCut implementation and its issues / commits → experimental verification → designing
  this project's implementation.
- Focus: FFmpeg export parameters, `-ss` / `-to` / `-t`, stream copy, PTS/DTS/time base,
  keyframe, smart cut, concat, audio/video sync, container compatibility, metadata, subtitles,
  chapters.
- LosslessCut is GPL-2.0: read, understand, and verify behavior only; do not copy source code
  and do not do a line-by-line TypeScript → Rust translation.
- Phase constraint: an ordinary lossless export only does stream copy; smart cut was implemented
  in Phase 7C and is off by default, and its re-encode scope and notification requirements are
  in ADR 0013 / 0014.

---

# Phase 8: Release (Complete)

- **Linux**: pacman package (`packaging/PKGBUILD`, `./scripts/package-linux.sh`) with the
  desktop entry and icons, verified on Arch / Wayland; since v0.4.2 GitHub Actions builds it on
  `v*` tags and attaches it to the same-named Release.
- **Windows**: native MSVC build; `./scripts/package-windows.sh` produces the portable zip (exe
  + `libmpv-2.dll` + FFmpeg + license + README) and a `.sha256`; still packaged locally.
- **License**: GPL-3.0-or-later (ADR 0015); `libmpv` (GPL-2.0-or-later) and `cpal`
  (Apache-2.0) are compatible with GPLv3.
- **Not yet done**: a Windows installer; on-device acceptance of the file dialogs, drag and
  drop, and high DPI; the Windows package entering CI.
- The per-item verification record is in `docs/history/completed-phases.md`.

---

# Development Principles

Only handle:

> The smallest next step in the current phase.

Do not implement an entire Phase at once.

On completing each stage:

1. Update this file.
2. Update the architecture document.
3. Update the tests.
4. Update the related ADRs.
5. Confirm the exit criteria.

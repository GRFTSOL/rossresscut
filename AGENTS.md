# AGENTS.md

This project is a native desktop media tool built with **Rust + egui/eframe**, aiming to be a
lightweight, fast, low-resource alternative to LosslessCut. Media processing is handled by an
external FFmpeg / FFprobe and in-process libmpv.

This file keeps only **entry points, hard constraints, and the documentation index**. Concrete
rules live in the topic documents; where a topic document conflicts with the hard constraints
here, this file wins.

## Documentation index

| Content | Document |
| --- | --- |
| Product goals and non-goals | `GOALS.md` |
| Phase breakdown and current tasks | `PLANS.md` |
| Contribution process | `CONTRIBUTING.md` |
| Release process | `RELEASING.md` |
| Layering, data flow, module boundaries | `docs/architecture.md` |
| Media data model and time system | `docs/media-model.md` |
| UI and desktop integration | `docs/ui-guidelines.md` |
| FFmpeg / FFprobe / lossless export rules | `docs/media-rules.md` |
| Error handling, Rust style, dependencies, testing, performance | `docs/coding-standards.md` |
| Commands, environment, privilege boundaries, Git | `docs/development.md` |
| This machine's development environment (not a support claim) | `docs/local-environment.md` |
| Boundaries for using external reference projects | `docs/research.md` |
| Licenses of third-party components and reference projects | `ATTRIBUTION.md` |
| Entry point for Claude-family agents | `CLAUDE.md` |
| Open questions | `docs/open-questions.md` |
| Architecture decisions / investigation records | `docs/decisions/`, `docs/investigations/` |
| Completed phase records | `docs/history/` |

## Hard constraints

- Keep the native Rust architecture: the UI and business logic run in the same native process.
  Memory footprint and startup time take priority, so no browser engine or Web UI runtime is
  brought in.
- Project code stays at `unsafe_code = "forbid"`. FFI and `unsafe` may only exist inside
  dependencies; GPU handles must not cross `media::preview` or the UI rendering adapter layer.
- Media time must use integer `Timestamp` / `Duration` / `TimeBase`; `f32` / `f64` seconds are
  forbidden as the sole representation.
- No FFmpeg, FFprobe, decoding, waveform generation, or blocking waits may run inside a UI frame.
  Domain and Media must not depend on egui.
- All domain state changes must go through Application commands; the UI only builds parameters,
  sends commands, and displays results.
- Export and probing use external FFmpeg / FFprobe with arguments passed one by one; shell command
  concatenation is forbidden.
- Default to `stream copy`. When re-encoding is necessary, give a concrete reason and tell the
  user; never label re-encoding as lossless, and never downgrade silently.
- Treat all external input as untrusted: media files, FFprobe output, FFmpeg output, paths,
  indices, timestamps, codecs, containers. Errors must carry context; `unwrap` / `expect` /
  `panic!` are forbidden for handling user input or external output.
- Do not expand the task scope; do not introduce traits, generics, event buses, or new
  dependencies for hypothetical future needs. New modules must state their layering and
  dependencies.
- LosslessCut, FilmCraft, and EffectCraft serve only as **behavioral and engineering-practice
  references**; do not copy their code or transplant their architecture. See `docs/research.md`
  for the boundaries.

## Build

- Icons are build inputs: `build.rs` generates the window icon from `assets/img/icon.png`, and on
  Windows `assets/img/icon.ico` is also used as the executable resource. Neither may be deleted or
  renamed; `icon.ico` must keep multi-size frames.
- A native Windows MSVC build needs `MPV_LIBRARY_DIR` pointing to the directory containing
  `mpv.lib`; a full build and test also need `libmpv-2.dll` in the same directory as the exe /
  test exe.
- External `ffmpeg` / `ffprobe` must be on `PATH` or in the same directory as the exe; see
  `docs/development.md` for details.

## How agents work

1. Before changing anything, read `AGENTS.md`, `GOALS.md`, `PLANS.md`, the relevant `docs/`, and
   the ADRs, and confirm whether the existing modules already provide the capability.
2. Do not guess APIs, dependencies, or FFmpeg behavior. Check the code, official docs, actual
   command output, or a minimal experiment first.
3. For real tradeoffs (architecture, media semantics, rendering/export strategy, dependency
   choice), use the `architecture-decision` skill.
4. When studying external implementations, follow the order and license constraints in
   `docs/research.md`; do not translate TypeScript / Electron implementations directly into Rust.
5. **Run `fuck-u-code-analysis` after every piece of code you finish writing; read its output and
   judge whether it is reasonable. If it is, correct according to its output, then commit.**
6. Run `./scripts/check.sh` before committing and make sure everything passes. Never claim a check
   passed when you did not actually run it.
7. Check the UI: on Linux, prefer `scripts/ui_a11y.py` to read and click widgets; on Windows, use
   screenshots plus input injection. Screenshots are only for color, layout, and the video image.
   See `docs/development.md` for usage.
8. Do not commit, push, reset, rebase, or rewrite history automatically unless the user explicitly
   asks.
9. Use Simplified Chinese for thinking (including internal reasoning), plans, and answers; code,
   identifiers, commands, logs, and proper nouns are not force-translated.

## Definition of done

See "Definition of done" in `docs/development.md`. Features, refactors, formatting, and dependency
upgrades must not be mixed into the same change.

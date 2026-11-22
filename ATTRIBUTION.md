# Third-Party Components and Reference Projects

This project is released under GPL-3.0-or-later; see `LICENSE`. The licenses of the
components and reference projects below must be reviewed before distribution.

## The Project Itself

| Component | License |
| --- | --- |
| `rossresscut` project code | GPL-3.0-or-later (`LICENSE`) |

## Direct Dependencies

| Component | Purpose | License |
| --- | --- | --- |
| `eframe` / `egui` | Desktop window and UI | MIT OR Apache-2.0 |
| `wgpu` | GPU rendering / texture import | MIT OR Apache-2.0 |
| `mpv-engine` | libmpv safe wrapper and exported frame | MIT OR Apache-2.0 |
| `libmpv` / `mpv` | Preview decoding, audio output, hardware decoding | GPL-2.0-or-later |
| `fontdb` | System font enumeration | MIT |
| `rfd` | Linux XDG portal / Windows Shell COM file dialogs | MIT |
| `cpal` | Audio output for the external FFmpeg fallback path | Apache-2.0 |
| `serde` / `serde_json` | Data serialization | MIT OR Apache-2.0 |
| `toml` | Configuration files | MIT OR Apache-2.0 |
| `log` / `env_logger` | Logging | MIT OR Apache-2.0 |

`libmpv` is a system library linked dynamically at runtime and is not distributed
with the source repository. GPL-3.0-or-later is compatible with `libmpv`'s
GPL-2.0-or-later; see `docs/decisions/0015-project-license.md` for the related
decision.

## Reference Projects

| Project | Purpose | License | Usage |
| --- | --- | --- | --- |
| LosslessCut | Reference for product behavior, media semantics, and FFmpeg workflows | GPL-2.0 | Read and verify behavior only; no code copied |
| FilmCraft | Reference for Rust + egui project layering | See the project repository | Reference architecture trade-offs only; no code copied |
| EffectCraft | Reference for engine-first layering | See the project repository | Reference architecture trade-offs only; no code copied |

This repository does not contain the source code of the projects listed above.

## Assets and Fonts

- `assets/fonts/JetBrainsMonoNerdFontPropo-Regular.ttf`: JetBrains Mono Nerd Font
  Propo Regular, from Nerd Fonts v3.5.1 (patched JetBrains Mono), licensed under
  the SIL Open Font License 1.1, see `assets/fonts/OFL.txt`.
  It is used only as a fallback font for UI icon glyphs, not for body typesetting.
- The media files in `tests/fixtures/` are synthetic assets generated with FFmpeg
  `lavfi` and contain no third-party content; see `tests/fixtures/README.md` for how
  to regenerate them.
- Body UI fonts come from the user's system and are enumerated through `fontdb`;
  the project does not distribute body typesetting fonts.
- If other assets are added later, their source, author, and license must be
  recorded, and this file must be updated.

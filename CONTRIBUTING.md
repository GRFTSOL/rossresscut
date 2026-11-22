# Contributing

This project is a personal project for self-use / learning: Issues, Discussions, the Wiki, and PRs
are all closed. This file only records the maintainer's own development workflow and environment
notes.

Read `AGENTS.md` first. It lists the non-negotiable hard constraints; the concrete rules are in
`docs/`.

## Environment

```bash
rustup component add rustfmt clippy
sudo pacman -S --needed ffmpeg mpv
cargo build
```

The toolchain is pinned by `rust-toolchain.toml`. `libmpv` and `ffmpeg` / `ffprobe` are runtime
dependencies; the project does not bundle binaries.

## Before committing

```bash
./scripts/check.sh
```

The script covers formatting, the build, clippy under two feature combinations, and
unit/integration tests.

After writing code, run `fuck-u-code-analysis`, read the output, and judge whether it is
reasonable; when it is, fix according to its suggestions, then commit.

## Change workflow

1. Keep the change scope single; do not mix refactoring, formatting, or dependency upgrades into a
   feature commit.
2. Check the existing code, ADRs, and docs first, then decide whether a new module or dependency
   is needed.
3. When real tradeoffs are involved (architecture, media semantics, rendering/export strategy,
   dependency choice), write an ADR or use the `architecture-decision` process first.
4. External implementations are behavioral references only; do not copy the code of LosslessCut /
   FilmCraft / EffectCraft. See `docs/research.md`.
5. Consider error paths, external input, and cancellation/cleanup; tests must cover the new
   behavior.
6. Update the affected docs: `PLANS.md`, `docs/architecture.md`, the relevant ADRs,
   `docs/open-questions.md`.
7. Documentation has an English primary file and a Chinese `<name>_ZH.md`; changing one requires
   updating the other.

## Code style

See `docs/coding-standards.md`. Key points:

- `unsafe` is forbidden in project code.
- External input is untrusted; errors must carry context.
- Media time uses integer types.
- The UI is non-blocking and does not run business logic.

## License

The project is released under **GPL-3.0-or-later** (ADR 0015). Do not introduce code or assets
that are incompatible with the existing dependencies (especially the GPL-2.0-or-later `libmpv`),
and do not commit third-party material before confirming its terms.

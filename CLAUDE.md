# CLAUDE.md

The rules entry point for this project is `AGENTS.md`. Read it first, then the `docs/` and ADRs
relevant to the task.

## Work loop

1. Read `AGENTS.md`, `GOALS.md`, `PLANS.md`, and the relevant topic documents.
2. Check the existing code and ADRs; do not guess APIs or FFmpeg behavior.
3. Use the `architecture-decision` skill when real tradeoffs are involved.
4. Run `./scripts/check.sh` after implementing.
5. For UI feature/state checks, prefer `scripts/ui_a11y.py` (see `docs/development.md`);
   screenshots are only for verifying color, layout, and the video image.
6. Then run `fuck-u-code-analysis`, read the output, and judge whether it is reasonable; when it
   is, fix according to the output.
7. Do not commit, push, reset, or rebase automatically unless the user explicitly asks.

## Key constraints

- Native Rust + egui/eframe; do not introduce a browser engine or Web UI runtime (low-overhead
  goal).
- Project code is `unsafe_code = "forbid"`.
- Media time uses integer types.
- The UI is non-blocking and does not run business logic.
- External FFmpeg / FFprobe arguments are passed one by one; do not concatenate shell commands.
- External input is untrusted; errors must carry context.
- Default to `stream copy`; re-encoding must be justified and must not be labeled lossless.
- LosslessCut / FilmCraft / EffectCraft are behavioral and engineering references only; do not
  copy code.

## Detailed rules

- `docs/architecture.md`, `docs/media-model.md`
- `docs/ui-guidelines.md`, `docs/media-rules.md`
- `docs/coding-standards.md`, `docs/development.md`
- `docs/research.md`, `docs/open-questions.md`

Use Simplified Chinese for reasoning and answers; code, commands, identifiers, and logs are not
force-translated.

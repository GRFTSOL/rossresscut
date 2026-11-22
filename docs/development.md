# Development and Toolchain

This document records the local development environment, commands, privilege boundaries, and
commit rules. See `CONTRIBUTING.md` for the contribution process.

## Entry gate

Once the Cargo project is set up, the following commands are the shared entry gate for local
runs and CI alike:

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --all --check
```

## Build inputs

- `assets/img/icon.png` is the source image for the window icon, and `assets/img/icon.ico` is
  the Windows multi-size executable icon; `build.rs` reads both, so deleting or renaming them
  breaks the build.
- The `build.rs` dependencies are build-only: `image` (scales the PNG to the window icon size)
  and `winresource` on Windows (embeds the ico and the version string). Neither of them enters
  the runtime.
- The window icon size is determined by `WINDOW_ICON_SIZE` in `build.rs`, and `src/ui/mod.rs`
  reads it from `OUT_DIR/window-icon.rgba`; when changing the size, both places must be kept in
  sync.

## Rules

- All of the above commands must pass before committing.
- The `--no-default-features` clippy run verifies that the external FFmpeg fallback path still
  compiles without `mpv-preview`; it does not run the tests that need libmpv.
- Zero tolerance for compiler warnings: `unused import`, `dead_code`, and `deprecated API` must
  all be zero.
- Formatting issues are not left to manual review; `cargo fmt` handles them. A failing
  `cargo fmt --all --check` counts as unfinished work.
- The toolchain version is pinned by `rust-toolchain.toml` to avoid behavior differences between
  machines; upgrading the toolchain is a separate change.
- The arguments in the command block and `clippy`'s `-D warnings` must not be trimmed in order
  to get around an error.
- For `fuck-u-code analyze`, `Overall Score` (and `score` in the JSON) is **higher is better**;
  `Issue Score` in the text report is the inverse value (`100 - score`), where higher is worse;
  the percentages under `Metrics Details` are the share of issues, where lower is better.
- The function-level metrics of `fuck-u-code` rely on the WASM grammar files from
  `tree-sitter-wasms`. A global pnpm install does not link it into `fuck-u-code/node_modules`,
  and when it is missing the tool **silently falls back to the regex parser**, treating the
  whole `impl` block as a single function so that function length and complexity are all
  distorted.

  How to tell: if `Tree-sitter init failed for rust, falling back to regex parser` appears in
  the output, the data is not trustworthy; in that case rely on
  `cargo clippy -W clippy::too_many_lines`.

  Fix (may recur after reinstalling):

  ```bash
  PNPM_GLOBAL=$(pnpm root -g)
  for dir in "$PNPM_GLOBAL"/.pnpm/fuck-u-code@*/node_modules/fuck-u-code/node_modules; do
    ln -s "$PNPM_GLOBAL"/.pnpm/tree-sitter-wasms@*/node_modules/tree-sitter-wasms "$dir/tree-sitter-wasms"
  done
  ```

  On Windows `ln -s` is unreliable (it needs Developer Mode or administrator rights), so use a
  directory junction instead; replace the paths on both sides with the actual pnpm store:

  ```powershell
  powershell -NoProfile -Command 'New-Item -ItemType Junction -Path "<fuck-u-code>\node_modules\tree-sitter-wasms" -Target "<tree-sitter-wasms>"'
  ```

  Verification: run it again and `Tree-sitter init failed for rust` no longer appears in the
  output (falling back to the regex parser for `.sh` scripts does not affect the Rust results).

## Development environment

The toolchain, paths, and known pitfalls of the current development machine are recorded
separately in [`docs/local-environment.md`](local-environment.md): that part describes only this
machine (Arch + Wayland, Windows MSVC via `msvc-kit`, `C:\bin`, and so on) and does not
represent the product's supported scope; see `GOALS.md` and `PLANS.md` for supported platforms.

For the general build inputs and entry gate, see above. The packaging scripts for the two
platforms:

- Linux: `./scripts/package-linux.sh`
- Windows: `./scripts/package-windows.sh` (Git Bash; requires the MSVC toolchain and `mpv.lib`)

At runtime on Windows, `libmpv-2.dll` must be in the same directory as the exe, and the external
`ffmpeg` / `ffprobe` must be on `PATH` or in the same directory as the exe.

## Privilege boundaries

The agent cannot escalate privileges: the current runtime sets `NoNewPrivs: 1`, and `sudo`,
`pkexec`, and `su` all fail to obtain root. This is an established limitation of the runtime,
not a configuration oversight, and attempts to bypass it are not allowed either.
**All system-level installation is performed manually by the user.**

Agent responsibilities:

1. Read-only detection: `command -v ffmpeg ffprobe`, `pacman -Q ffmpeg`.
2. If something is missing, report it and give a command that can be copied directly (for
example `sudo pacman -S --needed ffmpeg`).
3. Once the user confirms it is done, re-check the version and path, then continue.

Prohibited:

- Attempting `sudo`, `pkexec`, `su -c`, `sudo -S` to read a password, or `sudo -A` with a
  self-built askpass.
- Modifying `/etc/sudoers`, `/etc/sudoers.d/*`, or `/etc/sudo.conf`, or suggesting that the user
  configure `NOPASSWD` on their machine so that the agent can escalate privileges.
- Running `pacman -Syu`, `pacman -R*`, or `systemctl` to change system services when the user
  has not explicitly asked for it.
- Writing any password into scripts, environment variables, or logs, or committing it to the
  repository.

If some crate needs a system development library, report the package name and let the user
install it; the agent only detects. The project's own Rust dependencies and in-repo scripts must
not require root.

Before reporting a missing dependency, confirm its owner with `pacman -Fq usr/bin/ffprobe`
(file → package) or `pacman -Ql <pkg>` (package → files). This machine has a faster alternative,
documented in `docs/local-environment.md`. Queries only decide whether a package must be
installed; installation still follows the manual flow above.

---

## UI inspection (AT-SPI, Linux only)

`scripts/ui_a11y.py` reads or clicks the running `rossresscut` window through AT-SPI. Windows
has no AT-SPI, so only screenshots combined with `keybd_event` / injected mouse events are
available there, for example the one-off PowerShell scripts under `target/`.
It is used for development and agent verification and does not enter the product runtime.

Prerequisites:

- The application is built with the default `accesskit` feature.
- This machine has the PyGObject Atspi bindings (Arch: `python-gobject`, `at-spi2-core`).
- niri does not enable AT-SPI by default; the script's `--enable` sets the runtime
  `org.a11y.Status.IsEnabled`.

```bash
# start the application first
./target/release/rossresscut [media-file] &

./scripts/ui_a11y.py --enable
./scripts/ui_a11y.py --status
./scripts/ui_a11y.py --dump
./scripts/ui_a11y.py --dump --role button
./scripts/ui_a11y.py --dump --role button --rects
./scripts/ui_a11y.py --click "⚙" --role button
./scripts/ui_a11y.py --click "文件" --role button
./scripts/ui_a11y.py --disable
```

Notes:

- `--role` only controls what is printed or clicked; the tree is still traversed in full.
- `--contains` switches from exact name matching to substring matching; `--index` selects which
  of several same-named widgets to use.
- `--rects` prints screen coordinates and sizes, which is useful for a rough layout check.
- The custom-drawn timeline and video preview usually show up in AT-SPI as just `unknown`, with
  no button role or action; dragging, the mouse wheel, and the video picture still need
  screenshots or input injection.
- Colors, font rendering, and layout details still have to be verified with screenshots.
- The script filters the known harmless AT-SPI warnings exchanged between AccessKit and pyatspi.

### Export UI smoke test

`scripts/ui_export_smoke.py` drives the full export flow through AT-SPI without manual clicking:

```bash
./scripts/ui_export_smoke.py
./scripts/ui_export_smoke.py --keep
./scripts/ui_export_smoke.py --binary target/debug/rossresscut
```

- It starts the application in a headless `cage` and generates a 1080p30 source with 2 chapters;
  the chapters automatically produce Segments.
- It sets `smart_cut = true` so that both exports have an observable running time.
- The first export asserts the running state, the completed state, the output duration, and the
  2 chapters; the second one clicks cancel while running, asserting "Export cancelled" and that
  no output file is left behind.
- It requires that no other `rossresscut` instance is running on the machine; when the script
  ends it shuts down the cage process group it started.
- On failure it prints the current label/button list and the tail of the cage log.

## Release packaging (Linux)

`scripts/package-linux.sh` builds the release binary and produces a pacman package:

```bash
./scripts/package-linux.sh
```

- It requires a clean working tree; the script checks that the versions in `Cargo.toml` and
  `packaging/PKGBUILD` match.
- It runs `./scripts/check.sh`, `cargo build --release --locked`, and `makepkg` in order.
- The artifacts land in `dist/`: `rossresscut-<version>-<pkgrel>-x86_64.pkg.tar.zst`; see
  `RELEASING.md` for the package contents, runtime dependencies, and license notes.
- `packaging/PKGBUILD` reuses `target/release` from the current working tree instead of calling
  cargo inside `build()`: makepkg sets `SOURCE_DATE_EPOCH` to the current time on every run, and
  that value is part of the Cargo fingerprint, so it would recompile the entire dependency graph
  and flush the local release cache.

## Git and commits

Do not automatically commit, push, reset, rebase, or rewrite history unless the user explicitly
asks for it (same as `AGENTS.md`). Keep the scope of changes clear.

Do not mix the following into a single change:

- Features
- Large refactors
- Formatting
- Dependency upgrades
- Unrelated bug fixes

---

## Red lines

The hard constraints in `AGENTS.md` must be followed (native Rust, no `unsafe`, integer time, no
blocking inside a UI frame, state changed only through Application commands, no shell
concatenation, no silent re-encoding, no `unwrap` on external input). In the development
process, the following additionally count as unfinished work:

- ❌ Committing code that carries warnings or fails `cargo fmt --check`.
- ❌ Claiming "tested and passing" without actually running it.
- ❌ Substituting a `fuck-u-code` score for actual testing.

---

## Definition of done

A feature only counts as done when it meets the following conditions:

- [ ] It compiles
- [ ] The relevant tests pass
- [ ] New features have reasonable tests
- [ ] Error paths have been considered
- [ ] The UI is not blocked by long-running tasks
- [ ] There are no unnecessary new dependencies
- [ ] The rationale for new dependencies is recorded
- [ ] Public APIs have `///` doc comments
- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] The project code has no `unsafe`; external GPU handles are only passed between
      `media::preview` and the UI rendering adaptation layer
- [ ] The relevant documentation has been updated
- [ ] No unrelated changes were produced
- [ ] Features, refactors, formatting, and dependency upgrades are not mixed into the same change
- [ ] The relevant functionality was actually verified

If there was no actual testing:

**Claiming "tested and passing" is forbidden.**

If FFmpeg only supports something in theory:

**Claiming "the project supports this format" is forbidden.**

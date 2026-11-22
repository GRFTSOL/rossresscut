# Release Process

Releases are made per commit; do not build from an uncommitted working tree.

## 1. Pre-release Checks

```bash
git status --short
./scripts/check.sh
```

- The working tree may only contain explicitly explained untracked artifacts (for
  example `dist/`).
- Confirm the version in `Cargo.toml` matches `pkgver` in `packaging/PKGBUILD`;
  `scripts/package-linux.sh` checks this and exits when they differ.
- Update the completed items for the current phase in `PLANS.md`, and record known
  limitations.
- Run the GPU regression on a real Linux machine with Vulkan + DMA-BUF (the CI
  runner has no DRM render node, so it cannot cover this):

  ```bash
  cargo test --release --features mpv-preview --test mpv_gpu_regression -- --ignored
  cargo test --release --features mpv-preview --test mpv_preview -- --ignored
  ```

  Within it, `resizing_the_export_updates_the_texture` covers the texture size and
  color after the mpv export size changes.
- Open a media file larger than the window and drag a window edge to resize it:
  confirm the preview ratio stays the same and is not upscaled, a new
  `mpv preview size WxH` line appears in the log, and continuous dragging does not
  reconfigure the export on every frame.

## 2. Build

```bash
cargo build --release --locked
./target/release/rossresscut
```

At minimum, verify that the window starts, font scanning works, and a local media
file can be opened.

## 3. Packaging (Linux / pacman)

```bash
./scripts/package-linux.sh
```

The script runs, in order: clean working tree check → version consistency check →
`./scripts/check.sh` → `cargo build --release --locked` → `makepkg`.

Artifact:

```text
dist/rossresscut-<version>-<pkgrel>-x86_64.pkg.tar.zst
```

Install and uninstall:

```bash
sudo pacman -U dist/rossresscut-0.4.1-1-x86_64.pkg.tar.zst
sudo pacman -R rossresscut
```

Package contents:

```text
/usr/bin/rossresscut
/usr/share/applications/rossresscut.desktop
/usr/share/pixmaps/rossresscut.png
/usr/share/licenses/rossresscut/LICENSE
/usr/share/licenses/rossresscut/ATTRIBUTION.md
```

`packaging/PKGBUILD` builds directly on `target/release/` of the current working
tree, so it reuses the local build cache and the version, commit, and artifact stay
consistent. This route is only suitable for local/private releases; to publish to
the AUR or a public repository, switch to a tagged tarball flow instead of
`source=()` and add a real `url` plus checksums.

`dist/` and `packaging/{src,pkg}` are not committed to Git.

### GitHub Actions

If you do not package on the local machine, just push a `v*` tag (the `Release`
workflow can also be triggered manually):

```bash
git tag v0.4.2
git push origin v0.4.2
```

- Runs `cargo build --release --locked` and `makepkg` in an
  `archlinux/archlinux:base-devel` container; the artifact is the same
  `.pkg.tar.zst`, plus a `SHA256SUMS` file.
- It first verifies that the `Cargo.toml` version, `pkgver` in `PKGBUILD`, and the
  tag all agree, and fails outright when they do not.
- `makepkg` refuses to run as root, so compiling and packaging both run under a
  temporary non-root user.
- The artifact is also uploaded as a workflow artifact. When a Release with the
  same name already exists it is attached to it; when no Release exists, only the
  artifact is kept and no empty Release page is created (GitHub generates the
  Source code archives with the Release, which satisfies the GPL source provision
  requirement).
- This workflow does not run `./scripts/check.sh`: the Rust in the container is the
  Arch rolling release, so fmt / clippy / tests are handled by the `CI` workflow on
  push.
- It is still only suitable for source/private channel releases; the AUR needs a
  separate PKGBUILD based on a tagged tarball.

## 4. Runtime Dependencies

The package declares the following `depends`:

- `mpv`: `libmpv.so.2`, preview and playback
- `ffmpeg`: `ffmpeg` / `ffprobe`, probing, export, waveforms, scene detection
- `xdg-desktop-portal`: file open / save dialogs (the xdg-portal backend of `rfd`)
- `mesa` / `libglvnd` / `wayland` / `libxkbcommon`: wgpu / winit / EGL
- `alsa-lib`: audio output
- `glibc` / `gcc-libs`

`optdepends`:

- `xdg-desktop-portal-gtk`: the file chooser backend when the compositor has no
  native portal implementation
- `libx11`: X11 sessions

Release notes must state these dependencies, and that hardware decoding is handled
by mpv's `hwdec`.

## 5. License and Redistribution

- The project is released under **GPL-3.0-or-later**, with `LICENSE` holding the
  full GPLv3 text; the license fields in `Cargo.toml` and `packaging/PKGBUILD` must
  match it.
- The binary dynamically links `libmpv.so.2` (the `mpv` package is
  GPL-2.0-or-later). GPL-3.0-or-later is compatible with it; `cpal`'s Apache-2.0 is
  also compatible with GPLv3.
- When distributing binaries you must also provide the corresponding source (or a
  written offer), and keep `LICENSE` and `ATTRIBUTION.md`.
- `license=('custom')` is no longer used; see
  `docs/decisions/0015-project-license.md` for the decision.

## 6. Windows

Windows can now be built natively and produces a portable bundle. See `PLANS.md`
for the remaining items.

### Windows

Build (native MSVC, the current main path):

- Toolchain: `msvc-kit` (portable MSVC + Windows SDK). After running
  `msvc-kit setup --persistent` once, a plain `cargo build` is enough; see
  `docs/development.md` for PATH caveats and the `mpv.lib` generation steps.

File dialogs:

- The Windows backend of `rfd` 0.17 uses Shell COM's `IFileOpenDialog` /
  `IFileSaveDialog` (`IFileDialog`-derived interfaces), so no custom dialog is
  needed.
- `Cargo.toml` enables `rfd = { version = "0.17", default-features = false }` under
  `[target.'cfg(target_os = "windows")'.dependencies]`, and
  `FILE_DIALOG_SUPPORTED` covers Linux and Windows.
- Dialog behavior has not yet been manually accepted (the application itself
  already runs on real hardware).

Linking and runtime files:

- `rsmpv-sys` links with `-lmpv` at compile time, and MSVC needs the import library
  `mpv.lib`. The official Windows builds only provide `libmpv-2.dll`, so generate a
  `.def` with `dumpbin /exports` and then generate the library with
  `lib /def:mpv.def /machine:x64`; pass its directory via `MPV_LIBRARY_DIR`.
- At runtime `libmpv-2.dll` is required. Windows' default DLL search order looks in
  the executable's directory first, so placing it next to the exe is enough
  (`PATH` works too).
- Place `ffmpeg.exe` / `ffprobe.exe` next to the exe or on `PATH`; both are
  launched by `media::process::command`, with `CREATE_NO_WINDOW` on Windows.

Portable bundle:

- Produced with one command: `./scripts/package-windows.sh` (prerequisites: a clean
  git working tree, `MPV_LIBRARY_DIR`, `MPV_DLL` or
  `target/release/libmpv-2.dll`, and `ffmpeg` / `ffprobe` on `PATH`).
- Artifacts: `dist/rossresscut-<version>-windows-x64.zip` and a `.sha256` file of
  the same name; unzip and run.
- It contains `rossresscut.exe`, `libmpv-2.dll`, `ffmpeg.exe`, `ffprobe.exe`,
  `LICENSE`, `ATTRIBUTION.md`, and `README-WINDOWS.txt`; the bundled notes are
  generated from the `packaging/README-WINDOWS.txt` template.

Verification status (0.4.1):

- [x] Native MSVC build, `cargo test`, and two rounds of `cargo clippy` pass.
- [x] Preview (wgpu DX12 + libmpv exported texture), waveforms, keyframe index,
  NVIDIA `nvdec`.
- [x] Media with Chinese names and paths containing spaces opens, previews, and
  generates waveforms normally.
- [x] Startup failures show a native error dialog (the GUI has no console).
- [ ] Manual acceptance of file open / save dialogs, drag and drop, and high DPI.
- [ ] Manual acceptance of export path selection, chapter writing, and multi-segment
  concat (the automated tests already pass).

## 7. External Publishing

Without explicit user consent, you must not:

- create a GitHub Release or upload artifacts
- submit to software stores
- publish announcements or update the website

Before publishing, provide the accurate version number, commit, change summary,
and known limitations.

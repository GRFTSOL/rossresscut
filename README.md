# rossresscut

**English** · [简体中文](README_ZH.md)

A native, lightweight lossless media cutter for Windows and Linux, written in Rust with
[egui](https://github.com/emilk/egui). The workflow mirrors LosslessCut: open a file, preview it
in-process, mark segments on the timeline, and export them without re-encoding.

> **This is a vibe-coding project.** Nearly all of the code was written by AI coding agents from
> short human instructions; the human side is requirements, design choices, and acceptance.
> It is a personal project for learning and everyday use, so **issues, discussions, the wiki and
> pull requests are disabled**. There is no support commitment and no roadmap.
> You are welcome to fork it under the GPL.

<img width="1679" height="979" alt="Screenshot" src="https://github.com/user-attachments/assets/707ed9fc-7080-4c6e-b33c-b089e414e409" />

## Features

### Preview and navigation

- **In-process libmpv** playback with GPU decoding. The decoder is selectable in
  *Settings → Preview*: `auto-safe`, `auto`, `d3d11va`, `nvdec`, `cuda`, `vaapi`, `no`, or an
  external FFmpeg pipeline as a fallback.
- Wheel seeking with a configurable step (default **6.67 s** ≈ 200 frames at 30 fps);
  `Ctrl` + wheel is a fixed ±10 s.
- One-second stepping (`Alt+←` / `Alt+→`), frame stepping (`,` / `.`), segment-start/end jumps.
- Waveform drawn behind the timeline, plus a keyframe index for fast scrubbing.
- Playback info panel: the decoder actually in use, frame count, video and audio stream details.

### Cutting

- Set cut start/end, split, delete, move, sort, shuffle, merge overlapping segments, and invert
  the selection.
- Multi-select in the segment list, copy/paste segments, undo/redo for timeline edits.
- Segments can be disabled without deleting them; export follows the timeline order.

### Export

- **Stream copy by default**: a normal cut decodes and re-encodes nothing.
- Multi-segment export writes temporary parts and joins them with the concat demuxer.
- **Smart cut** (opt-in) re-encodes only the head/tail of a segment so a cut can start and end on
  an exact frame. It is reported as re-encoding, never as lossless.
- Chapters generated from segments, subtitle cue trimming (keep or drop cues crossing a cut), and
  per-stream selection for video / audio / subtitles.
- Batch queue: run the same export across many files, optionally merging the results.
- Tools → *Executed FFmpeg commands* lists every command of the session with its purpose, so the
  exact `argv` is always reproducible.

### Analysis

- **Scene detection** with a live progress bar; cancel or close the window and the FFmpeg child is
  killed immediately.
- **Silence-based auto cut** (`Alt+S`) driven by the waveform peaks, with a configurable threshold.
- Container chapters can be imported as segments when a file is opened.

### Projects and settings

- Project files (`*.rrc`) store the timeline; autosave can write next to the source file.
- Settings live in their own window, split into *General / Preview / Export / Shortcuts* tabs:
  theme, UI language (English / 简体中文), font family and zoom, preview backend, seek step, and
  export defaults.
- **43 keyboard actions, all rebindable**. Start recording by clicking a binding and press
  `Backspace` to clear it.

### Desktop integration

- **Windows**: undecorated window with a self-drawn title bar merged into the menu bar (32 px,
  borderless caption buttons), drag & drop, native file dialogs, portable zip.
- **Linux**: Wayland / X11, file dialogs through the XDG desktop portal, pacman package with a
  desktop entry and icon.
- Open a file straight from the command line: `rossresscut video.mp4`.

## Default shortcuts

| Action | Default | Action | Default |
| --- | --- | --- | --- |
| Play / pause | `Space` | Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Set cut start | `Alt+I` | Cut / copy / paste | `Ctrl+X` / `Ctrl+C` / `Ctrl+V` |
| Set cut end / finish | `Alt+O` | Select all | `Ctrl+A` |
| Split segment | `Alt+B` | Previous / next segment | `↑` / `↓` |
| Delete segment | `Delete` | Previous / next second | `Alt+←` / `Alt+→` |
| Clear markers | `Escape` | Previous / next frame | `,` / `.` |
| Close media | `Ctrl+W` | Open settings | `Ctrl+,` |
| Auto cut silence | `Alt+S` | | |

Everything in the menus can be bound too; unassigned actions simply have no default.

## How "lossless" works

A normal export copies the compressed packets with `-c copy`. Audio and video are never touched,
so quality is identical to the source and export speed is limited by disk I/O.

The trade-off is that a stream copy can only start on a keyframe. If a segment starts between
keyframes, the output begins at the previous keyframe (and the UI says so). Smart cut avoids this
by re-encoding just the head/tail of the segment, and it is explicitly labelled as re-encoding.

## Install

Prebuilt artifacts are attached to each
[release](https://github.com/GRFTSOL/rossresscut/releases):

| Platform | Artifact | Notes |
| --- | --- | --- |
| Windows 10/11 x64 | `rossresscut-<version>-windows-x64.zip` | Unzip and run `rossresscut.exe`. `libmpv-2.dll`, `ffmpeg.exe` and `ffprobe.exe` are bundled. |
| Arch Linux x86_64 | `rossresscut-<version>-1-x86_64.pkg.tar.zst` | `sudo pacman -U <file>`. Runtime deps: `mpv`, `ffmpeg`. |

## Build from source

Linux:

```bash
sudo pacman -S --needed rust ffmpeg mpv
cargo build --release
./target/release/rossresscut
```

Windows (native MSVC toolchain):

```powershell
# mpv.lib is not shipped with the official Windows build; generate it from the DLL
# with scripts/mpv-import-lib.sh, then point the build at it.
$env:MPV_LIBRARY_DIR = "C:/data/mpv-msvc"
cargo build --release
```

`libmpv-2.dll` must sit next to the executable at runtime. Full toolchain notes, packaging
scripts and the test setup are in [`docs/development.md`](docs/development.md).

Run the full local gate (fmt, clippy twice, all tests, build):

```bash
./scripts/check.sh
```

## Configuration

`config.toml` is written to `%APPDATA%\rossresscut\` on Windows and
`$XDG_CONFIG_HOME/rossresscut/` (usually `~/.config/rossresscut/`) elsewhere.

```toml
[general]
follow_system_theme = true   # when false, `theme` is used
theme = "dark"               # or "light"
language = "en"              # or "zh_hans"
font_zoom = 1.0

[preview]
seek_step_micros = 6666667   # wheel step; 6.67 s by default
backend = "auto-safe"        # libmpv hwdec value

[export]
chapters = false
smart_cut = false

[general.shortcuts]
play_pause = "Space"
add_cut = "Alt+I"
```

## Limitations

- A stream-copy segment starts at the previous keyframe unless smart cut is enabled.
- With B-frames, an end point between keyframes can leave a short freeze on the last frame
  (a container / stream-copy limitation). Smart cut removes it by re-encoding the tail.
- Frame stepping in the external FFmpeg fallback uses the nominal frame duration, which is
  approximate for variable-frame-rate sources. The libmpv path uses mpv's own frame stepping.
- There is no installer yet: Windows ships a portable zip, Linux a pacman package.

## License

**GPL-3.0-or-later** — see [`LICENSE`](LICENSE).

`libmpv` is GPL-2.0-or-later and the Rust dependencies are MIT / Apache-2.0; both are compatible
with GPLv3. The full list, including the bundled font, is in [`ATTRIBUTION.md`](ATTRIBUTION.md).

## Documentation

| Topic | Document |
| --- | --- |
| Hard constraints and entry point | [`AGENTS.md`](AGENTS.md) |
| Product goals | [`GOALS.md`](GOALS.md) |
| Phases and current work | [`PLANS.md`](PLANS.md) |
| Architecture and data flow | [`docs/architecture.md`](docs/architecture.md) |
| Media model and integer time | [`docs/media-model.md`](docs/media-model.md) |
| UI conventions | [`docs/ui-guidelines.md`](docs/ui-guidelines.md) |
| FFmpeg / export rules | [`docs/media-rules.md`](docs/media-rules.md) |
| Coding standards and tests | [`docs/coding-standards.md`](docs/coding-standards.md) |
| Build, commands, permissions | [`docs/development.md`](docs/development.md) |
| This machine's development environment | [`docs/local-environment.md`](docs/local-environment.md) |
| Reference projects and their limits | [`docs/research.md`](docs/research.md) |
| Completed phase records | [`docs/history/completed-phases.md`](docs/history/completed-phases.md) |
| Architecture decisions | [`docs/decisions/`](docs/decisions/) |
| Release process | [`RELEASING.md`](RELEASING.md) |

## Status

All planned phases are complete. Development continues as small feature work and bug fixes.

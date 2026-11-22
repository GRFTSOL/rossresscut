# Local Development Environment

This document records only the toolchain, paths, and known pitfalls of the **current development
machine**; its purpose is to let the next session reproduce the same build and verification
behavior. It does not represent the product's supported scope and must not be used as grounds
for "only Arch Linux is supported"; see `GOALS.md` and `PLANS.md` for supported platforms.

## Linux (Arch + Wayland)

- Operating system: Arch Linux (rolling release). System packages are installed with `pacman`,
  e.g. `sudo pacman -S ffmpeg`; system-level binaries are never downloaded directly by a script.
- Rust toolchain: managed by `rustup`, with the version pinned by `rust-toolchain.toml`
  (currently `1.99.1`, including `rustfmt` and `clippy`). When a component is missing, use
  `rustup component add rustfmt clippy`.
- Toolchain upgrades: bumping `rust-toolchain.toml` is a separate change and is not mixed with
  feature work.
- FFmpeg / FFprobe: the system packages are used (`/usr/sbin/ffmpeg`, `/usr/sbin/ffprobe`;
  `ffprobe` is provided by the `ffmpeg` package rather than being separate), and the project
  does not bundle its own binaries. A rolling release changes the actual version, so tests must
  not assume version-specific behavior.
- Desktop session: Wayland (`XDG_SESSION_TYPE=wayland`, compositor niri), with no X11 session.
  egui / winit behavior must be verified under Wayland and must not depend on X11-only paths.
- Interactive shell: fish. Scripts do not depend on interactive shell configuration: they write
  an explicit shebang (`#!/usr/bin/env bash` or `#!/usr/bin/env fish`) and are guaranteed to run
  in a non-interactive environment.
- Helper scripts serve development and verification only and do not enter the product runtime;
  the project introduces no Python / Node runtime dependencies.

## Windows (native MSVC build on this machine)

The same tree is also built directly with MSVC on this Windows machine, not via cross
compilation. The Linux side therefore no longer needs the `x86_64-pc-windows-gnu` target or
`mingw-w64-gcc`; those toolchains can be uninstalled, and neither the repository scripts nor CI
reference them anymore.

- **MSVC and the Windows SDK**: installed by `msvc-kit` in `C:\data\msvc-kit-data`
  (MSVC `14.40.33807`, Windows SDK `10.0.19041.0`). After one persistent setup they are written
  into the user environment variables (`PATH`, `INCLUDE`, `LIB`, `CC`, `AR`,
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER`), so plain `cargo build` and rust-analyzer both
  work directly:

  ```powershell
  msvc-kit setup --persistent   # once only; restart the editor and terminals afterwards
  cargo build
  ```

  Without the persistent setup, use `msvc-kit run` to inject the environment temporarily:

  ```bash
  msvc-kit run -- cargo build
  ```

  Note: Git for Windows' `usr\bin\link.exe` may still come before MSVC, but the persistent setup
  passes the linker path to rustc explicitly through
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER`, so the build is unaffected.

- **libmpv import library**: `mpv-engine` needs the MSVC import library `mpv.lib`, while the
  official Windows build only provides `libmpv-2.dll`. Generate it with the script (on Windows
  via `dumpbin` + `lib`, on Linux via LLVM's `llvm-readobj` + `llvm-dlltool`):

  ```bash
  ./scripts/mpv-import-lib.sh /path/to/libmpv-2.dll
  ```

  The output goes to `dist/mpv-lib/`; point the build at it:

  ```powershell
  $env:MPV_LIBRARY_DIR = "C:/project/rust/rossresscut/dist/mpv-lib"
  cargo build
  ```

- **Packaging**: run `./scripts/package-windows.sh` from a clean working tree, producing
  `dist/rossresscut-<version>-windows-x64.zip` and `.sha256`.
- **Crash logs**: uncaught panics are appended to `%LOCALAPPDATA%\rossresscut\crash.log` (UTC
  time, version, and stack trace); on Linux it is `$XDG_STATE_HOME/rossresscut/crash.log`
  (falling back to `~/.local/state`).
- **Runtime DLL**: `libmpv-2.dll` must be in the same directory as the exe or on `PATH`. The
  integration test executables live under `target\debug\deps`, so the DLL must be copied there
  as well.
- **FFmpeg / FFprobe**: `ffmpeg.exe` and `ffprobe.exe` live in `C:\bin` and are on `PATH`.
- **Entry gate**: run all checks with a single command in Git Bash:

  ```bash
  MPV_LIBRARY_DIR=C:/data/mpv-msvc bash scripts/check.sh
  ```

  Without a persistent environment setup, prefix it with `msvc-kit run --`.

- The program is built with `windows_subsystem = "windows"` and has no console of its own. All
  external `ffmpeg` / `ffprobe` processes must therefore be created through
  `media::process::command`, which sets `CREATE_NO_WINDOW` on Windows. Using
  `std::process::Command` directly would open a separate console window for every probe,
  waveform, or decode. Startup failures pop a native error dialog; see `docs/ui-guidelines.md`
  for the UI rules.
- **Never put the Windows SDK's `bin\<version>\x64` on `PATH`.** That directory ships old
  `dxcompiler.dll` / `dxil.dll`, which wgpu's D3D12 backend loads from `PATH`, after which
  `DxcCreateInstance` returns `E_NOINTERFACE` and the whole initialization fails:

  ```text
  DxcCreateInstance failed: 不支持此接口 (0x80004002)
  Failed to create surface for any enabled backend: {}
  ```

  Because the program is a GUI subsystem app with no console, double-clicking it results in
  **silently exiting with no message at all**. `msvc-kit setup --persistent` writes that
  directory into the user `PATH`, so it needs to be removed manually; `rc.exe` / `mt.exe` are
  only needed when compiling Windows resource scripts, and rustc does not use them. The build
  only needs MSVC's `Tools\MSVC\<version>\bin\Hostx64\x64` on `PATH`.
- The interactive shell is nushell, with PowerShell 5.1 kept available; scripts still write an
  explicit shebang and do not depend on interactive shell configuration.

## Privileges and package queries

- The agent runtime on this machine sets `NoNewPrivs: 1` and **CapEff / CapPrm / CapBnd are all
  0**: `sudo`, `pkexec`, and `su` all fail to obtain root. Every `sudo` attempt is rejected with
  `no new privileges`; this is not a configuration oversight.
- This machine has **`pacfiles` 0.3.0-1** installed (from the `archlinuxcn` repository, depending
  on `plocate`), a fast replacement for `pacman -F` whose search scope covers the repositories
  rather than only installed packages:

  ```bash
  pacfiles '*/ffprobe'            # file → package, supports globs
  pacfiles -q usr/bin/ffprobe     # print package names only
  pacfiles -l ffmpeg              # package → files
  pacfiles -x '.*/libav(codec|format)\.so'   # POSIX extended regex
  ```

  It is read-only and does not modify the system; when the database is stale the user runs
  `sudo pacman -Fy` in their own terminal (it writes to `/var/lib/pacman`). For example,
  `usr/bin/ffprobe` belongs to `extra/ffmpeg` and needs no separate install.
- On this machine `pacman -F` reports a database inconsistency for a third-party repository
  (`arch4edu`); prefer `pacfiles` for file queries and report that warning to the user rather
  than silently ignoring it.
- On another machine, or when `pacfiles` is not installed, fall back to
  `pacman -Fq usr/bin/ffprobe` and `pacman -Ql <pkg>`.
- These queries only decide whether a package must be installed; installation is still performed
  manually by the user, as required by `docs/development.md`.

# 0001 Foundation Stack and Project Structure

- Status: Accepted
- Date: 2026-10-24
- Phase: Phase 0

## Context

Phase 0 needs a project skeleton that can be built, tested, and can open a minimal window, while reserving clear boundaries for the UI / Application / Domain / Media layers without implementing later-phase capabilities in advance.

## Decision

### 1. A single Cargo package providing both a library and a binary

`src/lib.rs` exports the layered modules, and `src/main.rs` only handles log initialization, starting the window, and exiting on error. Integration tests access the code through the library and do not need to start a graphical interface.

The multi-crate workspace approach is rejected: there is currently no need for independent publishing or independent compilation, and multiple crates would only increase the cost of path, version, and dependency management.

### 2. GUI uses `eframe`, with egui accessed through `eframe::egui`

`egui` is not declared as a separate dependency, avoiding type conflicts caused by mismatched versions of egui and eframe.

The default features of eframe are used (Wayland / X11, wgpu, default fonts, accessibility, etc.). The renderer and preview backend are part of the Phase 2 evaluation; Phase 0 does not lock them in prematurely.

### 3. Logging uses `log` + `env_logger`

Library code only invokes the `log` macros; only the binary entry point installs a logger. The default filter is `warn,rossresscut=info`, avoiding forwarding the runtime logs of GUI dependencies; `RUST_LOG` can override it.

A self-built logger is rejected: it would require reimplementing the filter syntax, formatting, and thread safety for insufficient benefit.

### 4. Hand-written `AppError` without `thiserror` / `anyhow`

Phase 0 has only two kinds of errors: I/O failure and startup failure. `AppError` stores the operation context and preserves the underlying error chain through `source()`.

`thiserror` is rejected: there is currently only one error type, so the benefit of the derive macro is insufficient. `anyhow` is rejected: it loses matchable error types and is unsuitable as a long-term error type for the domain and media layers; when the number of error types grows significantly later, `thiserror` can be evaluated separately.

### 5. Pinned toolchain

`rust-toolchain.toml` pins `1.99.1` and declares `clippy` and `rustfmt`. Upgrading the toolchain is a separate change and is not mixed with feature changes.

### 6. `unsafe` is forbidden

`Cargo.toml` sets `[lints.rust] unsafe_code = "forbid"`, which applies to all compilation targets in the package.

## Consequences

- Domain / Media / Application can be compiled and tested without creating a window.
- Phase 0 does not pre-create placeholder types or traits for later phases; layering is currently expressed only as module boundaries and documentation.
- `eframe` brings a large dependency tree, which is an established cost of native GUI. If startup time or binary size becomes a measured problem, adjust features or the renderer based on measurements.
- The current entry gate is provided by `scripts/check.sh`; hosted CI has added `.github/workflows/ci.yml`, but it has not actually run on GitHub Actions yet.

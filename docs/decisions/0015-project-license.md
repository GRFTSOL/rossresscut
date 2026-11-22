# ADR-0015: Choosing GPL-3.0-or-later as the Project License

## Status

Accepted

## Context

The project is preparing to release Linux binary packages, but has never settled on its own license.
The main preview path dynamically links the system `libmpv`, so it cannot unilaterally choose a
permissive license without addressing GPL compliance issues.

## Known constraints

- Arch's `mpv` package is marked `GPL-2.0-or-later AND LGPL-2.1-or-later`; when distributing
  binaries, treating it as `GPL-2.0-or-later` is the safest.
- `cpal 0.18.2` is single-licensed `Apache-2.0`, used for audio output on the external FFmpeg
  fallback path.
- The remaining direct dependencies are `MIT` or `MIT OR Apache-2.0`; the embedded fonts are
  `OFL-1.1`.
- The project code does not copy any source code from LosslessCut / FilmCraft / EffectCraft.
- The project targets local desktop use only and is currently not published to AUR or a public
  repository.

## Candidate options

### Option A: GPL-3.0-or-later

Pros:

- Compatible with `libmpv`'s `GPL-2.0-or-later` (upgrading to GPLv3 is allowed).
- Compatible with `cpal`'s `Apache-2.0` (Apache-2.0 has a compatibility dispute with GPLv2, but is
  compatible with GPLv3).
- Compatible with all MIT / dual-licensed dependencies.
- Includes patent grant clauses.

Cons:

- Distributing binaries requires providing the corresponding source code.
- Closed-source distribution is not allowed.
- Incompatible with `GPL-2.0-only` code (there are currently no such dependencies).

### Option B: GPL-2.0-or-later

Pros:

- The license text is shorter.
- It can also link `libmpv`.

Cons:

- The compatibility of `cpal`'s `Apache-2.0` with GPLv2 is disputed; practical distribution requires
  using the GPLv3 terms, making the declaration ambiguous.

### Option C: MIT / Apache-2.0

Pros:

- The most permissive, allowing closed-source redistribution.

Cons:

- There is compliance risk when dynamically linking GPL `libmpv` and distributing binaries.
- It is only relatively safe when switching to an LGPL build of `libmpv` or not distributing
  binaries.

## Decision

Choose:

> Option A: GPL-3.0-or-later.

## Rationale

1. A single license covers `libmpv`, `cpal`, and all MIT dependencies at once, requiring no
   additional exceptions.
2. The `or-later` of `libmpv` allows upgrading to GPLv3, which is the simplest compliance path.
3. The project is positioned as an open-source desktop tool, and GPL's open-source obligations align
   with that goal.
4. It is gentler than AGPL and introduces no network clauses unrelated to local desktop use.

## Consequences

### Positive

- The dependency license combination is clear, and no per-item compatibility explanation is needed at
  release time.
- The project remains fully open source.

### Negative

- Distributing binaries requires providing the corresponding source code or a written offer.
- Closed-source distribution is not allowed.

### Follow-up impact

- `LICENSE` uses the full GPLv3 text.
- The license statements in `Cargo.toml`, `packaging/PKGBUILD`, `README.md`, `ATTRIBUTION.md`,
  `RELEASING.md`, and `PLANS.md` have been synchronized.
- The pacman package installs both `LICENSE` and `ATTRIBUTION.md`.

## Open questions

- None.

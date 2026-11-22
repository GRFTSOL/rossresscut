# 0006 UI Localization and Settings Persistence

- Status: Accepted
- Date: 2026-10-24
- Phase: Phase 2 revision

## Context

The Phase 2 revision calls for a settings page, theme selection, and interface language selection. The project owner requires:

- The theme uses a dropdown and provides a "follow system theme" checkbox.
- Provide language selection.
- Strings are stored separately, and the matching resource is read per language.

The project owner is not familiar with the concrete implementation of i18n; the technical approach is left to the Agent. This ADR records that technical judgment and its tradeoffs.

## Known constraints

- Do not introduce Web technology or an additional runtime.
- Do not add unnecessary dependencies.
- No significant file reads inside an egui frame.
- Adding a language must not require changes to business code.

## Candidate options

### Option A: External resource files at runtime

Read `locales/<language>.json` from the user configuration directory.

Pros:

- Users can edit or extend translations directly.
- New languages do not require recompilation.

Cons:

- Every startup must handle missing files, permissions, malformed data, and version mismatches.
- Resource search paths and priority order must be defined.
- It can be broken at runtime, so a full error fallback must be provided.

### Option B: Embed resources at compile time, select language at runtime

`include_str!` embeds `locales/en.json` and `locales/zh-CN.json`, which are deserialized into a strongly typed `Strings`.

Pros:

- Zero new dependencies (reuses the existing `serde` / `serde_json`).
- Missing fields or typos fail directly in unit tests.
- No runtime file I/O is involved, and there is no resource path problem.
- Language switching takes effect immediately.

Cons:

- Adding a language requires recompilation.
- Users cannot modify the translation files directly.

### Option C: A third-party i18n crate

Pros:

- May provide interpolation, plurals, language negotiation, and similar capabilities.

Cons:

- With only two languages and simple interpolation today, the feature set is excessive.
- Adds macro, runtime, and version maintenance costs.

## Decision

Choice:

> Option B.

Concrete implementation:

- `src/ui/i18n.rs` defines `Language`, `Strings`, and `fill`.
- Resources live in `locales/en.json` and `locales/zh-CN.json`.
- Only UI strings are localized; application-layer warnings and media-layer errors stay in English until they are modeled as structured errors.
- Chinese relies on the system font fallback enabled by default in eframe (`egui_system_font_provider`); no fonts are bundled.

Settings persistence:

- Enable the `persistence` feature of `eframe` and use eframe storage.
- `UiSettings` stores "follow system theme", dark / light, and language.
- Language serde names use RON-parseable identifiers (`en`, `zh_hans`), with a round-trip test over in-memory storage.
- egui memory is persisted as well, so a stream's collapsed state survives between sessions.

## Rationale

1. It satisfies "strings stored separately, read per language" while keeping zero new direct dependencies.
2. Failure modes surface at compile / test time, so no runtime resource fault tolerance is needed.
3. Settings persistence reuses existing eframe capabilities, avoiding a hand-rolled config file and atomic write logic.

## Consequences

### Positive

- Language switching does not require a restart.
- Missing resources are caught by the required fields of `Strings` and by unit tests.
- Theme, language, and collapsed state are retained across sessions.

### Negative

- Adding a language requires recompiling and updating all locale files at once.
- The `persistence` feature of `eframe` brings in the transitive dependency `ron`.
- The system font fallback depends on CJK fonts installed in the desktop environment; without them, Chinese may render as boxes.
- Because command / ctrl + wheel is used for seeking, egui's ctrl+wheel interface scaling is disabled.

### Follow-up effects

- If user-editable translations are needed, the loader can be replaced with "external files first, embedded resources as fallback" without affecting callers.
- Localizing application-layer errors first requires converting the strings into error codes or structured errors.

## Open questions

- Whether the interface language should follow the system locale by default.
- Whether collapsed state should be tracked per file path.

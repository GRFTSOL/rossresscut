# ADR-0012: Project File and Settings File Format

## Status

Accepted

## Context

Phase 7 needs to save editing results and reopen them: the source media path, the ordered Segments,
and the `Timeline`'s ID counter. At the same time, `config.toml` mixes application settings and
export settings together, which already hurts readability.

## Known constraints

- `Timeline` is an **ordered EDL**; the order is the user-observable output order and cannot be
  derived from `source time` (see ADR-0008).
- `SegmentId` is a stable identity, and both selection and editing depend on it; it must be restored
  exactly when a project is loaded, otherwise undo, selection, and subsequent persistence will all be
  misaligned.
- The invariant of `Segment` (`start < end`) can only be established by `Segment::new`.
- The project file is external input: parse failures, version mismatches, and duplicate IDs must all
  report errors rather than crash.
- No new dependencies: `toml = "0.8"` is already in the dependency list.

## Candidate options

### Format

A. JSON (`serde_json` already available)
B. TOML (`toml` already available), with the extension `.rrc`

### Content

A. Save a snapshot of media metadata (streams, duration, keyframes)
B. Save only the source path and editing results, and re-probe on open

## Decision

Choose:

> TOML format with the extension `.rrc`; save only the source path and editing results.

File layout:

```toml
version = 1

[source]
path = "/media/clip.mp4"

[timeline]
next_id = 92

[[timeline.segments]]
id = 17
start = 10400000
end = 20500000
enabled = true
chapter_title = "Opening"
```

`chapter_title` is an optional export chapter title. When an old project lacks this field, it is
treated as "not set", so the format version stays at `version = 1`.

The settings file is sectioned by domain: `[general]` (application settings), `[preview]` (preview
step size, etc.), and `[export]` (export settings).

## Rationale

1. TOML can directly represent `Vec<Segment>` (`[[timeline.segments]]`) and nested tables;
   `Timestamp` naturally maps to integer microseconds without custom serialization.
2. It uses the same format as `config.toml`, so it is human-readable, diffable, and hand-editable.
3. Only editing results are stored, avoiding inconsistency between media metadata and the actual
   file; re-probing on project open is the only trustworthy source.
4. The `version` field lets incompatible changes be explicitly rejected rather than guessed at.
5. `next_id` must be saved: otherwise, after reopening, new Segments could reuse old IDs and break
   the stability of `SegmentId`.

## Consequences

### Positive

- Project files are small and readable, and can be processed in version control or scripts.
- The load path shares the probe flow with normal opening, so waveform, keyframes, and preview
  behavior are consistent.
- After sectioning the settings file, export parameters and application parameters can evolve
  independently.

### Negative

- The project records no media fingerprint; when the source file is replaced with one of the same
  name but different content, this cannot be detected.
- The old flat `config.toml` needs compatible reading (already implemented; migration to the new
  format happens on write).

### Follow-up impact

- New settings entries must also update the default values of the corresponding settings section
  (`GeneralSettings` / `PreviewSettings` / `ExportSettings`).
- Project format changes must bump `version` and handle reading old versions or explicitly reject
  them; purely additive optional fields (such as `chapter_title`) may keep the version number as a
  compatible change.

## Open questions

- There is no media fingerprint verification (size / mtime / hash).
- Batch processing and multi-source projects are not yet designed; the current `Timeline` is still
  bound to a single source media.

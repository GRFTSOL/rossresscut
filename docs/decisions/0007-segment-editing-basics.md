# 0007 Basic Segment Editing Semantics

- Status: Partially superseded by `0008-segment-timeline-semantics.md` (output order / overlap / SegmentId)
- Date: 2026-10-24
- Phase: Phase 3

> Update 2026-10-24: the Timeline is now defined as an EDL with an output order, Segments use a stable `SegmentId`, and overlap is allowed and not handled automatically.
> The conclusions in this document about in / out points living in the application layer, `start < end`, and a single-source Timeline remain valid;
> order, overlap, and identity semantics are governed by `0008-segment-timeline-semantics.md`.

## Context

Phase 3 needs to land the Segment / Timeline model that downstream lossless trimming depends on. Current constraints:

- Media time uses integer microsecond `Timestamp`.
- The domain layer does not depend on egui.
- Export is not implemented yet, so Segments do not yet participate in media processing.
- Only one media file is open at a time.

Before implementation, three things must be settled: whether Segments may overlap, which layer holds the in / out points, and whether sorting is automatic.

## Known constraints

- `Segment` must satisfy `start < end`.
- Export must use exact source time, not approximate preview positions.
- The domain model should not store temporary UI state.

## Candidate options

### Overlap strategy A: Allow overlap, sort by start

Pros:

- Simple to implement, and it does not change user input on its own.
- No user data is lost; overlapping ranges can later be detected or split during export.
- Consistent with the "implement the feature first, optimize later" cadence.

Cons:

- When drawing the timeline, a later-drawn Segment may cover an earlier one.
- Export semantics must be clarified in Phase 4.

### Overlap strategy B: Enforce non-overlap

Pros:

- Timeline and export semantics become simpler.

Cons:

- Creating or modifying a Segment must trim or reject, which is a hidden change to user data.
- Requires additional interaction rules and hints.

### Overlap strategy C: Automatic split / merge

Too costly to implement at the current stage, and it would introduce undefined product behavior.

## Decision

Choice:

> Overlap strategy A; in / out points and selection state live in the application layer.
>
> **Update 2026-10-24: order, overlap, and identity semantics have been superseded by `0008`. The concrete rules below are retained as history; `0008` governs.**

Concrete rules:

1. `Segment` consists of `start`, `end`, and `enabled`, as the half-open interval `[start, end)`.
   - Extended by 0008: a stable `SegmentId` is added.
2. `Timeline` holds a single `source`; each Segment does not need to repeat the source.
3. ~~`Timeline::insert` inserts by start point and returns an index.~~ Superseded by 0008: new Segments are appended and assigned a `SegmentId`.
4. In / out points are temporary markers on `Application` and are not part of the domain model.
5. Adding a Segment requires both an in point and an out point, with `start < end`; on success the markers are cleared and the new Segment is selected.
6. ~~Modifying start or end does not reorder automatically; the user can sort explicitly. Sorting clears the selection.~~ Superseded by 0008: sorting preserves the selection, and selection is by `SegmentId`.
7. Clicking the timeline seeks and selects the Segment containing the clicked position by `SegmentId`; clicking a list row seeks to the Segment's start.

## Rationale

1. Keep the domain model minimal; do not stuff selection state and temporary markers into `Segment`.
2. Do not silently modify the boundaries the user set, in line with the predictability principle of lossless editing.
3. A single-source Timeline matches the current "one file at a time" product scope.

## Consequences

### Positive

- Create / delete / modify / sort / enable-disable in Phase 3 all have explicit commands and tests.
- Segments can be used directly as Phase 4 export input.

### Negative

- Visual coverage and export order for overlapping Segments are not yet defined.
- There is no timeline boundary dragging; boundaries can only be set from the playhead.
- There is no project persistence, so opening a new file clears the Segments.

### Follow-up effects

- Phase 4 must define how overlapping Segments are handled before export (reject, trim, or output in order).
- The project file format needs to store the source, segments, and enabled state.

## Open questions

- ~~Whether overlap is allowed, or whether it is automatically trimmed / rejected on creation.~~ Decided by `0008`: overlap is allowed and not handled automatically.
- ~~Whether a stable identity is needed.~~ Decided by `0008`: use `SegmentId`.
- Whether the timeline needs Segment boundary dragging.
- Whether Segments should be persisted per file.

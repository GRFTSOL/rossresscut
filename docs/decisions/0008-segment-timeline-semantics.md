# 0008 Segment / Timeline Core Semantics: Ordered EDL and Stable Identity

- Status: Accepted
- Date: 2026-10-24
- Phase: Phase 3
- Amends / supersedes: the parts of `0007-segment-editing-basics.md` about order, overlap, and identity

## Context

`0007` already established that a Segment is a half-open time interval and that in / out points belong to the application layer, but it left "whether overlap is allowed" as an isolated question. The actual implementation therefore ended up in a mixed state:

- `Timeline::insert` inserts by start point;
- `replace` does not guarantee order;
- overlap is allowed;
- the selection is an `Option<usize>`;
- no field defines the output order;
- export is not implemented yet.

Phase 4's single-Segment export does not depend on order, but Phase 5's multi-Segment merge must know "which Segment is output first". If overlap keeps being treated as an isolated boolean, the model will be forced to be redefined at the export stage.

## Known facts

- `Segment` is currently `{start, end, enabled}`, a half-open interval in integer microseconds.
- `Timeline` is currently `{source, segments}`, and `insert` sorts by start point.
- Selection state is `Option<usize>` in `Application`; sorting clears the selection.
- Existing tests cover create, modify, sort, enable-disable, and delete, but not order, overlap, or export.
- `ExportJob.segments` in `AGENTS.md` is a list with no separate output order field.
- `GOALS.md` requires single-Segment / multi-Segment export, Segment sorting, and enable-disable.
- `PLANS.md`: Phase 4 is single-segment export; Phase 5 is multi-segment merge.
- In the LosslessCut reference implementation:
  - Segments have a stable `segId`;
  - the list can be reordered by dragging, and list order is output order;
  - "sort by start" is a tool action, not an invariant;
  - overlap is a possible state, and the tool provides detection / merge operations rather than handling it silently.

## Decision

Choice:

> `Timeline.segments` is an EDL with a stable output order.

Concrete rules:

1. **Order is output order**
   - The order of `Vec<Segment>` is the user-observable, user-modifiable output order.
   - New Segments are appended at the end, not inserted automatically by start point.

2. **A Segment represents source time**
   - `Segment.start` / `end` always represent a source time interval on the single source.
   - It uses the 0-based media timeline (integer microseconds), not absolute container PTS.
   - Output time is not stored in the Segment; it is a derived value accumulated at export time in enabled Segment order.

3. **enabled**
   - Enabled Segments participate in export in `segments` order.
   - Disabled Segments remain in the list but do not participate in output.

4. **Overlap**
   - Overlap between Segments is allowed.
   - No automatic rejection, trimming, or merging.
   - Overlap means the same stretch of source time may appear multiple times in the output sequence.

5. **Sorting**
   - Sorting by start is an explicit user action, not a Domain invariant.
   - Sorting is stable; SegmentIds and selection state do not change.

6. **Stable identity**
   - `Segment` must have a `SegmentId`.
   - `SegmentId` is allocated by `Timeline` and is not reused within the same Timeline.
   - Selection, editing, deletion, and reordering all go by `SegmentId`, never by Vec index.
   - `SegmentId` is a lightweight project-internal identity; no UUID / database.
   - The ID counter resets on `reset` / `clear`; project persistence must save both the IDs and the counter.

7. **Domain supports reordering**
   - Domain provides `move_to`, `move_up`, and `move_down`.
   - The current UI may lack drag-and-drop reordering, but Domain must not assume the list is always sorted by source time.

8. **Export**
   - Phase 5 multi-Segment export interprets enabled Segments in Timeline order by default.
   - Single-Segment export (Phase 4) takes only that Segment's source time interval.
   - This decision does not dictate future smart cut, re-encode, or overlap optimization strategies.

## Why overlap is not an independent decision

Overlap strategy is a dependent variable of "does `segments` have an output order", not a boolean that can be chosen on its own:

- If `segments` is an ordered EDL, overlap means the same source range appears multiple times in the output sequence, which is valid input;
- If `segments` is a set of source time ranges, overlap is a set conflict and must be rejected or merged;
- If a two-layer model is used, overlap belongs to the source range set and output order belongs to another layer.

Decide "is order the output order" first, and the overlap strategy follows naturally.

## Rejected options

### A set of source time ranges

Always sorted by start point, with overlap either rejected or merged.

Reasons for rejection:

- "Segment sorting" cannot express a user-defined output order.
- Inconsistent with LosslessCut's EDL behavior.
- It would force silent trimming or rejection of user boundaries on create / modify.

### Two-layer model

`Timeline` stores a set of source ranges, and a separate `ExportSequence` stores the output order.

Reasons for rejection:

- There is currently only a single source media;
- It would introduce two synchronization models prematurely;
- An ordered `Vec<Segment>` already expresses output order sufficiently.

## Consequences

### Positive

- Phase 5 export order is clear: enabled Segments in Timeline order.
- Overlap becomes valid semantics, with no silent modification of user input at creation time.
- Stable IDs let selection, deletion, sorting, future dragging, and undo all stop relying on indices.
- Color, selection state, and edit targets can stay stable with Segment identity.

### Negative

- The automatic sorting behavior of `Timeline::insert` must be removed in favor of appending.
- UI list order no longer equals source time order; the timeline is still drawn by source time, and the two must be understood separately.
- Overlapping Segments cover each other on the timeline; list order carries the output semantics.
- Phase 5 must define / test the output result of overlapping Segments (the same source range appearing repeatedly).
- Project persistence must save `SegmentId` and `next_id`.

### Follow-up effects

- Phase 4: `ExportJob` uses a single Segment and is unaffected by order.
- Phase 5: export iterates enabled Segments; overlapping segments are output repeatedly by default.
- The UI can add drag-and-drop reordering later without changing Domain semantics.
- Overlap warnings / merging can be added later as explicit commands, never handled silently.

## Open questions

- Whether to show an overlap warning, and whether to provide an explicit "merge overlapping Segments" command.
- Interaction details of drag-and-drop reordering in the UI.
- The ID format for project persistence and how `next_id` is saved.
- Whether source identity (path / size / mtime / hash) goes into the project file.
- smart cut / non-keyframe cut points / re-encode strategy are all separate follow-up decisions.

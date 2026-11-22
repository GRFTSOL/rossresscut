# Coding Standards

This document is the source of constraints for error handling, Rust style, dependencies,
testing, performance and architecture changes. Entry points and hard constraints are in
`AGENTS.md`.

## Error Handling

Do not use the following in production code without justification:

```rust
unwrap()
expect()
panic!()
todo!()
unimplemented!()
```

Prefer:

```rust
Result<T, E>
Option<T>
```

External data must be treated as untrusted:

- media files
- FFprobe output
- FFmpeg output
- file paths
- stream index
- timestamp
- codec
- container

Corrupted media should produce comprehensible errors instead of causing the program to panic.

### 1. Exceptions to unwrap

`.unwrap()` / `.expect()` are allowed only when an invariant guarantees `Some` / `Ok`, and
the reason must be stated on its own line directly above:

```rust
// INVARIANT: The caller has verified that segments is non-empty.
let first = segments.first().unwrap();
```

Using `expect()` as a substitute for a meaningful error message is not allowed.

### 2. Errors must carry context

An error message contains at least:

- the operation that failed
- the relevant file path
- the relevant parameters (stream index, timestamp, etc.)

Swallowing FFmpeg's stderr and returning only "ffmpeg failed" is not allowed.

### 3. Swallowing errors is forbidden

Errors must not be silently lost in the following ways:

- `let _ = result;`
- an empty `match` arm
- an `Err` arm that only logs but returns `Ok`

Unless the reason why it may be ignored is stated at the same location.

---

## Rust Coding Standards

### 1. Style

- 4-space indentation.
- Modules, functions, variables: `snake_case`.
- Types, traits, enums: `CamelCase`.
- Constants, statics: `SCREAMING_SNAKE_CASE`.
- Formatting follows `cargo fmt`; do not align by hand.

### 2. Expression

- Prefer iterator combinators: `map` / `filter` / `collect`, rather than a hand-written `for`
  with `push`.
- Use a loop when it is clearer (multiple states, early exit, order-sensitive side effects);
  do not sacrifice readability for style.
- Avoid unnecessary clones, allocations and intermediate collections.
- Abstraction must be driven by real reuse; do not introduce traits or generics in order to
  "look more general".

### 3. Memory safety: unsafe is forbidden in project code

Crates inside the project must keep `unsafe_code = "forbid"`; `unsafe`, FFI, raw pointers,
`transmute`, `union`, `MaybeUninit`, `ManuallyDrop` and `static mut` are forbidden.

Preview uses the external crates `mpv-engine` (wrapping libmpv) and `wgpu`; their `unsafe` is
borne inside the dependency, and that is not a reason to enable `unsafe` in this project.
Boundary requirements:

- Frames exported by `mpv-engine` enter `media::preview` only as `wgpu::Texture`;
  raw pointers, `wgpu-hal` handles and FFI types must not appear in project code.
- GPU texture handles are passed frame by frame only between `media::preview` and the UI
  rendering adapter layer; they must not enter the Domain, the edit state or persistence.
- When upgrading `mpv-engine` / `wgpu`, whether their safe API boundaries still hold must be
  reviewed again.
- There must be tests that do not depend on specific hardware, plus consistency tests between
  the hardware path and the software path (see the "Testing" section).

#### Rationale

Option F (ADR 0010) replaced the former `media/platform` self-built libav* bindings with
`mpv-engine`. The external texture import that previously required in-project `unsafe` is now
handled by the dependency, so project code can stay at zero `unsafe`. If self-built FFI or
platform surface import is reintroduced in the future, the "single crate allowed to use
unsafe" rule must be restored, and an ADR must be recorded first.

#### Memory safety requirements within safe Rust

Not writing `unsafe` does not make code automatically safe; the following must still be
observed:

- Global mutable state is forbidden; share across threads through ownership transfer, `Arc` or
  message channels, not through `Send` / `Sync` guaranteed by convention.
- Indices computed from external data must not be used to index a slice unconditionally; use
  `get` / `get_mut` and handle out-of-bounds.
- Buffer lengths coming from FFprobe, file headers or user input must be validated; the sizes
  declared externally are not trusted.
- Do not make zero-copy layout assumptions, for example reinterpreting a byte buffer as a
  struct.
- Do not use `mem::forget` to bypass destructors, and do not trade leaks for "simplicity".

### 4. Naming conversions

- `as_`: cheap borrow conversion.
- `to_`: conversion that may allocate or compute.
- `into_`: conversion that consumes ownership.
- Getters do not take a `get_` prefix: use `duration()`, not `get_duration()`.
- Test functions do not take a `test_` prefix; `#[test]` already shows that it is a test.

### 5. Public API

- Public types, functions and fields must have `///` doc comments.
- Doc comments describe behavior, constraints and failure conditions; they do not explain
  language basics.

---

## Comments and Documentation

### 1. Comments

- A comment occupies its own line; trailing comments are forbidden.
- Comments end with a period.
- Comments explain "why", not restate what the code "does".
- Use backticks when referring to types, functions, keywords or paths.
- An outdated comment is equivalent to a bug and must be changed together with the code.

### 2. Division of documentation

- `AGENTS.md`: entry points and inviolable hard constraints.
- `GOALS.md`: product goals and non-goals.
- `PLANS.md`: phase breakdown and current tasks.
- `docs/history/completed-phases.md`: tasks, verification records and known limitations of
  completed phases.
- `CONTRIBUTING.md`: contribution process.
- `RELEASING.md`: release process.
- `docs/architecture.md`: layering and data flow.
- `docs/media-model.md`: media data model.
- `docs/ui-guidelines.md`: UI and desktop integration rules.
- `docs/media-rules.md`: media processing, time, Segment and export rules.
- `docs/coding-standards.md`: error handling, Rust style, dependencies, testing, performance.
- `docs/development.md`: commands, environment, permission boundaries, Git and completion
  criteria.
- `docs/research.md`: boundaries for researching external reference projects.
- `docs/open-questions.md`: unresolved questions.
- `docs/decisions/`: architecture decision records.
- `docs/investigations/`: investigation records.

Specific rules are governed by the corresponding document; where they conflict with the hard
constraints in `AGENTS.md`, `AGENTS.md` prevails.

Each document exists in two languages: the table above lists the English primary files, and the
Chinese original is kept next to it as `<name>_ZH.md` (`README.md` pairs with `README_ZH.md`).
When editing a document, update both languages and keep headings, code blocks, tables and list
structure aligned one-to-one; code, paths, commands, identifiers and proper nouns are not
translated.

### 3. Writing requirements

- Concise; do not explain basic concepts; assume the reader is technically proficient.
- Do not use marketing-style language.
- Keep proper nouns in their original form: `stream copy`, `time base`, `keyframe`, `PTS`,
  `DTS`.

---

## Dependency Management

Before adding a crate, the following must be considered:

1. Whether the standard library can already solve the problem.
2. Whether existing project dependencies already provide the capability.
3. Whether the crate is still maintained.
4. The crate's license.
5. The number of transitive dependencies.
6. Compile time.
7. Binary size.
8. Whether it introduces a new runtime.
9. Whether it introduces new architecture constraints.

It is forbidden to add a dependency directly just because:

> "There seems to be a crate for this feature."

When adding a dependency, the reason for the choice and the rejected alternatives must be
stated clearly in the change description.

---

## Testing

When modifying core media logic, tests must be added first.

Tests include at least:

### Unit tests

Test:

- time computation
- Segment
- timeline
- FFmpeg argument generation
- FFprobe parsing
- stream selection
- progress parsing

Unit tests do not involve the file system, external processes or the GUI.

### Integration tests

Test:

- FFprobe
- FFmpeg
- export
- multiple segments
- stream copy
- cancelling tasks

Integration tests should not depend on the log wording of a specific FFmpeg version. When
version behavior needs to be distinguished, detect the version explicitly in the test and
explain.

### Regression tests

When a specific media file causes a bug:

```text
Bug
 ↓
Minimal reproduction
 ↓
Test
 ↓
Fix
 ↓
Regression test
```

Do not fix only the current file without leaving a test behind.

### UI tests

Do not do pixel-level snapshot tests.

Test only pure logic:

- timeline coordinate conversion
- selection set operations
- command argument construction

### Losslessness must be asserted

Integration tests related to export must not assert only that "the command exited
successfully".

At minimum, use FFprobe to compare input and output:

- same codec
- same time base
- same resolution / sample rate / channel count
- duration within the allowed tolerance

This proves that the output was not re-encoded.

---

## Test Media

Test media should:

- be as small as possible
- be reproducible
- have a clear purpose
- be legally distributable
- be as stable as possible

Test assets should cover typical cases:

- H.264 + AAC / MP4
- H.265 + AAC / MP4
- MKV
- MPEG-TS
- VFR
- multiple audio tracks
- subtitles
- chapters
- non-zero start time

Detailed asset descriptions go in:

```text
tests/fixtures/
```

Do not commit media files of unknown origin, excessive size or unclear copyright.

---

## Performance

This project pursues:

- fast startup
- low idle CPU
- low memory usage
- fast media analysis
- smooth timeline
- smooth preview

Do not keep background tasks running in order to "look advanced".

Expensive operations such as waveforms and scene detection should be:

- executed on demand
- executed in the background
- cancellable
- cacheable

---

## Handling Performance Problems

Do not optimize based on gut feeling.

The correct process:

```text
Find the problem
 ↓
Reproduce
 ↓
Measure
 ↓
Locate the bottleneck
 ↓
Minimal change
 ↓
Measure again
```

Forbidden:

> "This place might be slow, so I'll add a cache first."

---

## Architecture Changes

If a feature requires large-scale architecture changes:

The following must be explained first:

1. Why the current architecture cannot support it.
2. Which boundary has a problem.
3. What solutions exist.
4. Why the current solution was chosen.
5. What complexity the change adds.

Major architecture decisions are recorded in:

```text
docs/decisions/
```

---

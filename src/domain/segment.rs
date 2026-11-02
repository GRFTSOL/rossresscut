//! Segment and timeline domain model.
//!
//! A segment is a half-open source-time range `[start, end)`. A timeline is an
//! ordered edit list (EDL): the vector order is the output order, and users may
//! reorder segments independently of their source times. This layer is
//! independent of the UI and of FFprobe output.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::time::{Duration, Timestamp};

/// Stable identity of a segment inside one timeline.
///
/// IDs are assigned by [`Timeline`] and are never reused while the timeline
/// lives. They are intentionally small and project-local; no UUID or database
/// machinery is involved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct SegmentId(u64);

impl SegmentId {
    /// Creates an ID from a raw value.
    ///
    /// Normal callers receive IDs from [`Timeline`]; this constructor exists
    /// for tests and future project loading.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw identifier.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A user-visible source-time range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Segment {
    id: SegmentId,
    start: Timestamp,
    end: Timestamp,
    enabled: bool,
}

impl Segment {
    /// Creates an enabled segment.
    ///
    /// Returns `None` unless `start < end`.
    pub fn new(id: SegmentId, start: Timestamp, end: Timestamp) -> Option<Self> {
        (start < end).then_some(Self {
            id,
            start,
            end,
            enabled: true,
        })
    }

    /// Returns the stable segment identity.
    pub const fn id(self) -> SegmentId {
        self.id
    }

    /// Returns the inclusive start of the segment on the source timeline.
    pub const fn start(self) -> Timestamp {
        self.start
    }

    /// Returns the exclusive end of the segment on the source timeline.
    pub const fn end(self) -> Timestamp {
        self.end
    }

    /// Returns whether the segment participates in export.
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    /// Returns the segment length.
    pub fn duration(self) -> Duration {
        self.end.duration_since(self.start).unwrap_or_default()
    }

    /// Returns whether `time` falls inside `[start, end)`.
    pub fn contains(self, time: Timestamp) -> bool {
        self.start <= time && time < self.end
    }

    /// Returns a copy with a different start, preserving ID and `enabled`.
    ///
    /// Returns `None` unless the new range is valid.
    pub fn with_start(self, start: Timestamp) -> Option<Self> {
        Self::new(self.id, start, self.end).map(|segment| segment.with_enabled(self.enabled))
    }

    /// Returns a copy with a different end, preserving ID and `enabled`.
    ///
    /// Returns `None` unless the new range is valid.
    pub fn with_end(self, end: Timestamp) -> Option<Self> {
        Self::new(self.id, self.start, end).map(|segment| segment.with_enabled(self.enabled))
    }

    /// Returns a copy with a different enabled state.
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Errors produced by timeline edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineError {
    /// The requested range is empty or reversed.
    InvalidRange,
    /// The segment ID is not part of this timeline.
    NotFound,
    /// The segment ID counter overflowed.
    IdExhausted,
}

impl fmt::Display for TimelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRange => "segment start must be before its end",
            Self::NotFound => "segment not found in the timeline",
            Self::IdExhausted => "segment identifier space exhausted",
        })
    }
}

impl std::error::Error for TimelineError {}

/// An ordered edit list of the segments belonging to one source media file.
///
/// `segments` order is the user-visible output order. It is not sorted by
/// source time unless the user explicitly calls [`Timeline::sort_by_start`].
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    source: Option<PathBuf>,
    segments: Vec<Segment>,
    next_id: u64,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            source: None,
            segments: Vec::new(),
            next_id: 1,
        }
    }
}

impl Timeline {
    /// Creates an empty timeline for `source`.
    pub fn new(source: PathBuf) -> Self {
        Self {
            source: Some(source),
            ..Self::default()
        }
    }

    /// Returns the source media path, when a media file is bound.
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// Returns the segments in output order.
    ///
    /// Export code must iterate this order. It must not re-sort by source time;
    /// `sort_by_start` is an explicit user command, not an export invariant.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Returns the number of segments.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Returns whether there are no segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Rebinds the timeline to `source` and removes all segments.
    pub fn reset(&mut self, source: PathBuf) {
        self.source = Some(source);
        self.segments.clear();
        self.next_id = 1;
    }

    /// Rebinds the timeline to `source` while keeping the segments.
    ///
    /// Used when a saved project points at a media file that was moved or
    /// renamed and the user picks a replacement.
    pub fn rebind(&mut self, source: PathBuf) {
        self.source = Some(source);
    }

    /// Returns the next identifier that [`Timeline::push`] would hand out.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Rebuilds a timeline from saved parts, rejecting inconsistent input.
    ///
    /// Returns `None` when the source is empty, when an identifier is not below
    /// `next_id`, or when two segments share an identifier. Callers must build
    /// each [`Segment`] through [`Segment::new`] so `start < end` still holds.
    pub fn restore(source: PathBuf, segments: Vec<Segment>, next_id: u64) -> Option<Self> {
        if source.as_os_str().is_empty() || next_id == 0 {
            return None;
        }

        let mut seen = std::collections::BTreeSet::new();
        for segment in &segments {
            if segment.id.get() >= next_id || !seen.insert(segment.id) {
                return None;
            }
        }

        Some(Self {
            source: Some(source),
            segments,
            next_id,
        })
    }

    /// Removes all segments and the source binding.
    pub fn clear(&mut self) {
        self.source = None;
        self.segments.clear();
        self.next_id = 1;
    }

    /// Removes all segments while keeping the source binding and ID counter.
    pub fn clear_segments(&mut self) {
        self.segments.clear();
    }

    /// Appends a segment and returns its new stable ID.
    ///
    /// The new segment becomes the last item in the output order.
    pub fn push(&mut self, start: Timestamp, end: Timestamp) -> Result<SegmentId, TimelineError> {
        let id = SegmentId::new(self.next_id);
        let Some(segment) = Segment::new(id, start, end) else {
            return Err(TimelineError::InvalidRange);
        };
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(TimelineError::IdExhausted)?;

        self.next_id = next_id;
        self.segments.push(segment);
        Ok(id)
    }

    /// Returns a segment by stable ID.
    pub fn get(&self, id: SegmentId) -> Option<&Segment> {
        self.segments.iter().find(|segment| segment.id == id)
    }

    /// Returns the output-order index of a segment.
    pub fn index_of(&self, id: SegmentId) -> Option<usize> {
        self.segments.iter().position(|segment| segment.id == id)
    }

    /// Removes and returns a segment by stable ID.
    pub fn remove(&mut self, id: SegmentId) -> Option<Segment> {
        let index = self.index_of(id)?;
        Some(self.segments.remove(index))
    }

    /// Updates the source-time bounds of a segment, preserving its ID and
    /// enabled state.
    ///
    /// Returns the updated segment.
    pub fn update(
        &mut self,
        id: SegmentId,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<Segment, TimelineError> {
        let index = self.index_of(id).ok_or(TimelineError::NotFound)?;
        let enabled = self.segments[index].enabled;
        let segment = Segment::new(id, start, end).ok_or(TimelineError::InvalidRange)?;
        let segment = segment.with_enabled(enabled);
        self.segments[index] = segment;
        Ok(segment)
    }

    /// Sets the enabled flag of a segment.
    ///
    /// Returns `false` when the ID is unknown.
    pub fn set_enabled(&mut self, id: SegmentId, enabled: bool) -> bool {
        let Some(segment) = self.segments.iter_mut().find(|segment| segment.id == id) else {
            return false;
        };
        *segment = segment.with_enabled(enabled);
        true
    }

    /// Moves a segment to `target_index` in the output order.
    ///
    /// Returns `false` when the ID or target index is invalid. `target_index`
    /// is the final index after the move.
    pub fn move_to(&mut self, id: SegmentId, target_index: usize) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        if target_index >= self.segments.len() {
            return false;
        }
        let segment = self.segments.remove(from);
        self.segments.insert(target_index, segment);
        true
    }

    /// Moves a segment one position earlier in the output order.
    pub fn move_up(&mut self, id: SegmentId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let Some(target) = index.checked_sub(1) else {
            return false;
        };
        self.move_to(id, target)
    }

    /// Moves a segment one position later in the output order.
    pub fn move_down(&mut self, id: SegmentId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.move_to(id, index + 1)
    }

    /// Sorts segments by source start time, keeping the output list stable for
    /// equal starts. This is an explicit user operation, not an invariant.
    pub fn sort_by_start(&mut self) {
        self.segments.sort_by_key(|segment| segment.start());
    }

    /// Reverses the selected areas on the source timeline.
    ///
    /// Every time range that was not covered by a segment becomes a segment,
    /// and every covered range is removed. Overlapping source ranges are
    /// unioned first. New IDs are assigned in source order.
    ///
    /// Returns the number of segments after inversion.
    pub fn invert(&mut self, duration: Duration) -> Result<usize, TimelineError> {
        let Ok(total) = i64::try_from(duration.micros()) else {
            return Err(TimelineError::InvalidRange);
        };
        if total <= 0 {
            return Err(TimelineError::InvalidRange);
        }

        let mut ranges: Vec<(i64, i64)> = self
            .segments
            .iter()
            .map(|segment| (segment.start().micros(), segment.end().micros()))
            .collect();
        ranges.sort_unstable();

        let mut covered: Vec<(i64, i64)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            let start = start.clamp(0, total);
            let end = end.clamp(0, total);
            if start >= end {
                continue;
            }
            if let Some(last) = covered.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                continue;
            }
            covered.push((start, end));
        }

        let mut gaps: Vec<(i64, i64)> = Vec::with_capacity(covered.len() + 1);
        let mut cursor = 0;
        for (start, end) in covered {
            if start > cursor {
                gaps.push((cursor, start));
            }
            cursor = cursor.max(end);
        }
        if cursor < total {
            gaps.push((cursor, total));
        }

        self.segments.clear();
        for (start, end) in gaps {
            self.push(Timestamp::from_micros(start), Timestamp::from_micros(end))?;
        }
        Ok(self.segments.len())
    }

    /// Swaps two output positions. Returns `false` when either index is out of
    /// range.
    pub fn swap(&mut self, left: usize, right: usize) -> bool {
        if left >= self.segments.len() || right >= self.segments.len() {
            return false;
        }
        self.segments.swap(left, right);
        true
    }

    /// Merges segments whose source ranges overlap, preserving timeline order.
    ///
    /// Overlap is transitive: a segment that overlaps several existing merged
    /// ranges joins them into one. The merged segment keeps the first
    /// overlapping segment's ID and enabled flag and spans from the smallest
    /// start to the largest end. Returns the number of segments removed.
    pub fn merge_overlaps(&mut self) -> usize {
        let mut merged = 0;
        let mut result: Vec<Segment> = Vec::with_capacity(self.segments.len());
        for segment in self.segments.iter().copied() {
            let overlapping: Vec<usize> = result
                .iter()
                .enumerate()
                .filter(|(_, other)| segment.start() < other.end() && other.start() < segment.end())
                .map(|(index, _)| index)
                .collect();
            if overlapping.is_empty() {
                result.push(segment);
                continue;
            }

            let first = overlapping[0];
            let base = result[first];
            let mut start = base.start().min(segment.start());
            let mut end = base.end().max(segment.end());
            for index in overlapping.iter().skip(1) {
                start = start.min(result[*index].start());
                end = end.max(result[*index].end());
            }
            if let Some(updated) = Segment::new(base.id(), start, end) {
                result[first] = updated.with_enabled(base.enabled());
            }
            for index in overlapping.iter().skip(1).rev() {
                result.remove(*index);
            }
            merged += overlapping.len();
        }
        self.segments = result;
        merged
    }

    /// Replaces the segment list, preserving the IDs carried by `segments`.
    ///
    /// Used by undo/redo. The next ID moves past the highest restored ID so new
    /// segments never collide with restored ones.
    pub fn replace_segments(&mut self, segments: Vec<Segment>) {
        self.next_id = segments
            .iter()
            .map(|segment| segment.id.get())
            .max()
            .map_or(1, |max| max.saturating_add(1));
        self.segments = segments;
    }

    /// Splits `id` at `at` and returns the new second segment's ID.
    ///
    /// The original segment keeps its ID for the first part. Both parts keep
    /// the enabled flag and the second part is inserted immediately after the
    /// first, preserving timeline order.
    ///
    /// Returns `InvalidRange` unless `at` is strictly inside the segment.
    pub fn split(&mut self, id: SegmentId, at: Timestamp) -> Result<SegmentId, TimelineError> {
        let index = self.index_of(id).ok_or(TimelineError::NotFound)?;
        let segment = self.segments[index];
        if at <= segment.start || at >= segment.end {
            return Err(TimelineError::InvalidRange);
        }

        let second_id = SegmentId::new(self.next_id);
        let first = Segment::new(id, segment.start, at).ok_or(TimelineError::InvalidRange)?;
        let second = Segment::new(second_id, at, segment.end).ok_or(TimelineError::InvalidRange)?;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(TimelineError::IdExhausted)?;

        self.next_id = next_id;
        self.segments[index] = first.with_enabled(segment.enabled);
        self.segments
            .insert(index + 1, second.with_enabled(segment.enabled));
        Ok(second_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(micros: i64) -> Timestamp {
        Timestamp::from_micros(micros)
    }

    fn segment(id: u64, start: i64, end: i64) -> Segment {
        Segment::new(SegmentId::new(id), timestamp(start), timestamp(end))
            .expect("test segment is valid")
    }

    #[test]
    fn rejects_empty_or_reversed_ranges() {
        assert_eq!(
            Segment::new(SegmentId::new(1), timestamp(1), timestamp(1)),
            None
        );
        assert_eq!(
            Segment::new(SegmentId::new(1), timestamp(2), timestamp(1)),
            None
        );
    }

    #[test]
    fn reports_duration_and_containment() {
        let segment = segment(1, 1_000_000, 3_000_000);

        assert_eq!(segment.id(), SegmentId::new(1));
        assert_eq!(segment.duration(), Duration::from_micros(2_000_000));
        assert!(segment.contains(timestamp(1_000_000)));
        assert!(segment.contains(timestamp(2_999_999)));
        assert!(!segment.contains(timestamp(3_000_000)));
        assert!(!segment.contains(timestamp(999_999)));
    }

    #[test]
    fn preserves_identity_and_enabled_state_when_editing_bounds() {
        let original = segment(7, 1_000_000, 2_000_000).with_enabled(false);

        assert_eq!(
            original.with_start(timestamp(500_000)),
            Some(segment(7, 500_000, 2_000_000).with_enabled(false))
        );
        assert_eq!(
            original.with_end(timestamp(3_000_000)),
            Some(segment(7, 1_000_000, 3_000_000).with_enabled(false))
        );
        assert_eq!(original.with_start(timestamp(2_000_000)), None);
    }

    #[test]
    fn push_appends_and_assigns_monotonic_ids() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));

        assert_eq!(
            timeline.push(timestamp(2_000_000), timestamp(3_000_000)),
            Ok(SegmentId::new(1))
        );
        assert_eq!(
            timeline.push(timestamp(0), timestamp(1_000_000)),
            Ok(SegmentId::new(2))
        );
        assert_eq!(
            timeline.push(timestamp(1_000_000), timestamp(2_000_000)),
            Ok(SegmentId::new(3))
        );

        let order: Vec<u64> = timeline
            .segments()
            .iter()
            .map(|segment| segment.id().get())
            .collect();
        assert_eq!(order, vec![1, 2, 3]);
        assert_eq!(timeline.len(), 3);
        assert!(!timeline.is_empty());
    }

    #[test]
    fn rejects_invalid_pushed_ranges() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));

        assert_eq!(
            timeline.push(timestamp(1_000_000), timestamp(1_000_000)),
            Err(TimelineError::InvalidRange)
        );
        assert!(timeline.is_empty());
        assert_eq!(
            timeline.push(timestamp(0), timestamp(1_000_000)),
            Ok(SegmentId::new(1))
        );
    }

    #[test]
    fn moves_segments_in_output_order() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        let first = timeline
            .push(timestamp(0), timestamp(1_000_000))
            .expect("valid");
        let second = timeline
            .push(timestamp(1_000_000), timestamp(2_000_000))
            .expect("valid");
        let third = timeline
            .push(timestamp(2_000_000), timestamp(3_000_000))
            .expect("valid");

        assert!(timeline.move_to(third, 0));
        assert_eq!(
            timeline
                .segments()
                .iter()
                .map(|segment| segment.id())
                .collect::<Vec<_>>(),
            vec![third, first, second]
        );

        assert!(timeline.move_down(third));
        assert_eq!(
            timeline
                .segments()
                .iter()
                .map(|segment| segment.id())
                .collect::<Vec<_>>(),
            vec![first, third, second]
        );

        assert!(timeline.move_up(second));
        assert_eq!(
            timeline
                .segments()
                .iter()
                .map(|segment| segment.id())
                .collect::<Vec<_>>(),
            vec![first, second, third]
        );

        assert!(!timeline.move_up(first));
        assert!(!timeline.move_down(third));
        assert!(!timeline.move_to(first, 9));
    }

    #[test]
    fn sort_by_start_is_explicit_and_preserves_ids() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        let late = timeline
            .push(timestamp(2_000_000), timestamp(3_000_000))
            .expect("valid");
        let early = timeline
            .push(timestamp(0), timestamp(1_000_000))
            .expect("valid");
        let middle = timeline
            .push(timestamp(1_000_000), timestamp(2_000_000))
            .expect("valid");

        assert_eq!(
            timeline
                .segments()
                .iter()
                .map(|segment| segment.id())
                .collect::<Vec<_>>(),
            vec![late, early, middle]
        );

        timeline.sort_by_start();

        assert_eq!(
            timeline
                .segments()
                .iter()
                .map(|segment| segment.id())
                .collect::<Vec<_>>(),
            vec![early, middle, late]
        );
    }

    #[test]
    fn updates_and_removes_by_stable_id() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        let first = timeline
            .push(timestamp(0), timestamp(1_000_000))
            .expect("valid");
        let second = timeline
            .push(timestamp(2_000_000), timestamp(3_000_000))
            .expect("valid");

        let updated = timeline
            .update(first, timestamp(0), timestamp(1_500_000))
            .expect("valid update");
        assert_eq!(updated.start(), timestamp(0));
        assert_eq!(updated.end(), timestamp(1_500_000));

        assert!(timeline.set_enabled(first, false));
        assert_eq!(
            timeline.get(first).map(|segment| segment.enabled()),
            Some(false)
        );
        assert!(!timeline.set_enabled(SegmentId::new(99), true));

        assert_eq!(
            timeline.remove(first).map(|segment| segment.id()),
            Some(first)
        );
        assert_eq!(timeline.remove(first), None);
        assert_eq!(timeline.segments().len(), 1);
        assert_eq!(timeline.segments()[0].id(), second);
    }

    #[test]
    fn does_not_reuse_ids_after_removal() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        let first = timeline
            .push(timestamp(0), timestamp(1_000_000))
            .expect("valid");
        let second = timeline
            .push(timestamp(1_000_000), timestamp(2_000_000))
            .expect("valid");

        assert_eq!(timeline.remove(first), Some(segment(1, 0, 1_000_000)));
        let third = timeline
            .push(timestamp(2_000_000), timestamp(3_000_000))
            .expect("valid");

        assert_ne!(third, first);
        assert_ne!(third, second);
        assert_eq!(third, SegmentId::new(3));
    }

    #[test]
    fn reset_and_clear_invalidate_the_source_binding() {
        let mut timeline = Timeline::new(PathBuf::from("first.mp4"));
        timeline
            .push(timestamp(0), timestamp(1_000_000))
            .expect("valid");

        timeline.reset(PathBuf::from("second.mp4"));
        assert_eq!(timeline.source(), Some(Path::new("second.mp4")));
        assert!(timeline.is_empty());

        timeline.clear();
        assert_eq!(timeline.source(), None);
        assert!(timeline.is_empty());
    }

    #[test]
    fn splits_a_segment_at_an_interior_time() {
        let mut timeline = Timeline::new(PathBuf::from("source.mp4"));
        let original = timeline
            .push(timestamp(1_000_000), timestamp(5_000_000))
            .expect("valid");

        let second = timeline
            .split(original, timestamp(3_000_000))
            .expect("split");

        assert_eq!(timeline.segments().len(), 2);
        assert_eq!(
            timeline.segments(),
            &[
                segment(original.get(), 1_000_000, 3_000_000),
                segment(second.get(), 3_000_000, 5_000_000),
            ]
        );
        assert_ne!(second, original);
    }

    #[test]
    fn rejects_splits_on_or_outside_the_bounds() {
        let mut timeline = Timeline::new(PathBuf::from("source.mp4"));
        let id = timeline
            .push(timestamp(1_000_000), timestamp(5_000_000))
            .expect("valid");

        assert_eq!(
            timeline.split(id, timestamp(1_000_000)),
            Err(TimelineError::InvalidRange)
        );
        assert_eq!(
            timeline.split(id, timestamp(5_000_000)),
            Err(TimelineError::InvalidRange)
        );
        assert_eq!(
            timeline.split(SegmentId::new(99), timestamp(3_000_000)),
            Err(TimelineError::NotFound)
        );
        assert_eq!(timeline.segments().len(), 1);
    }
    #[test]
    fn swaps_and_merges_without_renumbering() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        let a = timeline
            .push(timestamp(0), timestamp(10_000_000))
            .expect("A");
        let b = timeline
            .push(timestamp(20_000_000), timestamp(30_000_000))
            .expect("B");
        let c = timeline
            .push(timestamp(5_000_000), timestamp(15_000_000))
            .expect("C");

        assert!(timeline.swap(0, 2));
        let order: Vec<SegmentId> = timeline.segments().iter().map(|s| s.id()).collect();
        assert_eq!(order, vec![c, b, a]);

        timeline.sort_by_start();
        assert_eq!(timeline.merge_overlaps(), 1);
        let segments = timeline.segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id(), a);
        assert_eq!(segments[0].start(), timestamp(0));
        assert_eq!(segments[0].end(), timestamp(15_000_000));
        assert_eq!(segments[1].id(), b);

        let saved = timeline.segments().to_vec();
        timeline.clear_segments();
        assert!(timeline.is_empty());
        timeline.replace_segments(saved);
        assert_eq!(timeline.len(), 2);
        assert_eq!(
            timeline.push(timestamp(0), timestamp(1)),
            Ok(SegmentId::new(b.get() + 1))
        );
    }
    #[test]
    fn invert_swaps_covered_and_uncovered_ranges() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        timeline
            .push(timestamp(1_000_000), timestamp(3_000_000))
            .expect("first");
        timeline
            .push(timestamp(2_000_000), timestamp(4_000_000))
            .expect("overlapping");
        timeline
            .push(timestamp(5_000_000), timestamp(6_000_000))
            .expect("last");

        let duration = Duration::from_micros(7_000_000);
        assert_eq!(timeline.invert(duration), Ok(3));
        let ranges: Vec<(i64, i64)> = timeline
            .segments()
            .iter()
            .map(|segment| (segment.start().micros(), segment.end().micros()))
            .collect();
        assert_eq!(
            ranges,
            vec![
                (0, 1_000_000),
                (4_000_000, 5_000_000),
                (6_000_000, 7_000_000)
            ]
        );

        assert_eq!(timeline.invert(duration), Ok(2));
        let ranges: Vec<(i64, i64)> = timeline
            .segments()
            .iter()
            .map(|segment| (segment.start().micros(), segment.end().micros()))
            .collect();
        assert_eq!(ranges, vec![(1_000_000, 4_000_000), (5_000_000, 6_000_000)]);
    }

    #[test]
    fn merge_unions_transitively_overlapping_ranges() {
        let mut timeline = Timeline::new(PathBuf::from("sample.mp4"));
        timeline
            .push(timestamp(0), timestamp(10_000_000))
            .expect("A");
        timeline
            .push(timestamp(20_000_000), timestamp(30_000_000))
            .expect("B");
        timeline
            .push(timestamp(5_000_000), timestamp(25_000_000))
            .expect("C");

        assert_eq!(timeline.merge_overlaps(), 2);
        assert_eq!(timeline.segments().len(), 1);
        assert_eq!(timeline.segments()[0].start(), timestamp(0));
        assert_eq!(timeline.segments()[0].end(), timestamp(30_000_000));
    }
}

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

    /// Removes all segments and the source binding.
    pub fn clear(&mut self) {
        self.source = None;
        self.segments.clear();
        self.next_id = 1;
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
}

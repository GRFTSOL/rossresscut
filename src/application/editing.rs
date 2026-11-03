//! Timeline editing commands: markers, selection, clipboard and history.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::segment::{Segment, SegmentId, TimelineError};
use crate::domain::time::Timestamp;

use super::playback::clamp_position;
use super::{Application, Markers, SegmentCommandError, TimelineSnapshot};

const MAX_HISTORY: usize = 200;

impl Application {
    /// Returns the pending in point.
    pub fn in_point(&self) -> Option<Timestamp> {
        self.markers.in_point
    }

    /// Returns the pending out point.
    pub fn out_point(&self) -> Option<Timestamp> {
        self.markers.out_point
    }

    /// Returns the selected segment ID.
    pub fn selected_segment(&self) -> Option<SegmentId> {
        self.selected_segment
    }

    /// Selects a segment by stable ID, or clears the selection with `None`.
    pub fn select_segment(&mut self, id: Option<SegmentId>) {
        self.selected_segment = id.filter(|id| self.timeline.get(*id).is_some());
        self.selection.clear();
    }

    /// Returns whether `id` is part of the current selection.
    pub fn is_selected(&self, id: SegmentId) -> bool {
        self.selection.contains(&id) || self.selected_segment == Some(id)
    }

    /// Returns the selected IDs in timeline order.
    pub fn selected_ids(&self) -> Vec<SegmentId> {
        let mut ids: Vec<SegmentId> = if self.selection.is_empty() {
            self.selected_segment.into_iter().collect()
        } else {
            self.selection.iter().copied().collect()
        };
        ids.retain(|id| self.timeline.get(*id).is_some());
        ids.sort_by_key(|id| self.timeline.index_of(*id).unwrap_or(usize::MAX));
        ids
    }

    /// Selects every segment and makes the first one primary.
    pub fn select_all_segments(&mut self) {
        self.selection = self
            .timeline
            .segments()
            .iter()
            .map(|segment| segment.id())
            .collect();
        self.selected_segment = self.timeline.segments().first().map(|segment| segment.id());
    }

    /// Adds or removes `id` from the multi-selection.
    pub fn toggle_segment_selection(&mut self, id: SegmentId) {
        if self.timeline.get(id).is_none() {
            return;
        }
        if self.selection.remove(&id) {
            if self.selected_segment == Some(id) {
                self.selected_segment = self.selection.iter().next().copied();
            }
        } else {
            self.selection.insert(id);
            self.selected_segment = Some(id);
        }
    }

    /// Copies the selected segments into the internal clipboard.
    pub fn copy_selected_segments(&mut self) -> usize {
        let ids = self.selected_ids();
        self.clipboard = ids
            .iter()
            .filter_map(|id| self.timeline.get(*id).copied())
            .collect();
        self.clipboard.len()
    }

    /// Copies the selected segments and removes them.
    pub fn cut_selected_segments(&mut self) -> usize {
        let count = self.copy_selected_segments();
        if count > 0 {
            self.remove_selected_segments();
        }
        count
    }

    /// Appends clipboard segments as new segments with fresh IDs.
    pub fn paste_segments(&mut self) -> usize {
        if self.clipboard.is_empty() || self.media.is_none() {
            return 0;
        }
        self.checkpoint();
        let mut pasted = Vec::with_capacity(self.clipboard.len());
        for segment in self.clipboard.clone() {
            if let Ok(id) = self.timeline.push(segment.start(), segment.end()) {
                if !segment.enabled() {
                    self.timeline.set_enabled(id, false);
                }
                pasted.push(id);
            }
        }
        if pasted.is_empty() {
            return 0;
        }
        self.selection = pasted.iter().copied().collect();
        self.selected_segment = pasted.last().copied();
        pasted.len()
    }

    /// Undoes the last timeline change.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.history.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.redo_stack.push(current);
        self.restore(previous);
        true
    }

    /// Redoes the last undone timeline change.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.history.push(current);
        self.restore(next);
        true
    }

    /// Returns whether an undo step is available.
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Returns whether a redo step is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            segments: self.timeline.segments().to_vec(),
            selected: self.selected_segment,
            selection: self.selection.clone(),
        }
    }

    pub(super) fn checkpoint(&mut self) {
        self.history.push(self.snapshot());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        self.redo_stack.clear();
        self.timeline_revision += 1;
    }

    fn restore(&mut self, snapshot: TimelineSnapshot) {
        self.timeline.replace_segments(snapshot.segments);
        self.selection = snapshot.selection;
        self.selection.retain(|id| self.timeline.get(*id).is_some());
        self.selected_segment = snapshot
            .selected
            .filter(|id| self.timeline.get(*id).is_some());
    }

    fn next_random(&mut self) -> u64 {
        if self.rng_state == 0 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15);
            self.rng_state = (nanos ^ u64::from(std::process::id())) | 1;
        }
        let mut value = self.rng_state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.rng_state = value;
        value
    }

    /// Sets the in point, clamped to the media bounds.
    pub fn set_in_point(&mut self, point: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.markers.in_point = Some(clamp_position(point, media.duration));
    }

    /// Sets the out point, clamped to the media bounds.
    pub fn set_out_point(&mut self, point: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.markers.out_point = Some(clamp_position(point, media.duration));
    }

    /// Clears both pending markers.
    pub fn clear_markers(&mut self) {
        self.markers = Markers::default();
    }

    /// Creates a segment from the pending in and out points.
    ///
    /// On success the markers are cleared and the new segment is selected.
    /// The segment is appended to the output order.
    pub fn add_segment(&mut self) -> Result<SegmentId, SegmentCommandError> {
        if self.media.is_none() {
            return Err(SegmentCommandError::NoMedia);
        }
        let Some(start) = self.markers.in_point else {
            return Err(SegmentCommandError::MissingInPoint);
        };
        let Some(end) = self.markers.out_point else {
            return Err(SegmentCommandError::MissingOutPoint);
        };

        self.checkpoint();
        let id = self
            .timeline
            .push(start, end)
            .map_err(|error| match error {
                TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
                TimelineError::NotFound => SegmentCommandError::EmptyRange,
            })?;
        self.selected_segment = Some(id);
        self.selection.clear();
        self.markers = Markers::default();
        Ok(id)
    }

    /// Adds a cut point at `position`.
    ///
    /// The first point becomes the pending segment start. A later point at a
    /// greater position becomes the end and creates the segment immediately.
    /// A point at or before the pending start replaces it.
    pub fn add_cut_point(&mut self, position: Timestamp) -> Result<(), SegmentCommandError> {
        let Some(media) = self.media.as_ref() else {
            return Err(SegmentCommandError::NoMedia);
        };
        let position = clamp_position(position, media.duration);
        match self.markers.in_point {
            None => {
                self.markers.in_point = Some(position);
                self.markers.out_point = None;
                Ok(())
            }
            Some(start) if position > start => {
                self.markers.out_point = Some(position);
                self.add_segment().map(|_| ())
            }
            Some(_) => {
                self.markers.in_point = Some(position);
                self.markers.out_point = None;
                Ok(())
            }
        }
    }

    /// Adds a cut point, or moves the selected segment's start to `position`.
    ///
    /// The cut shortcut is contextual: with a selected segment and the
    /// playhead inside it, the same key moves the segment head instead of
    /// adding an unrelated marker.
    pub fn add_cut_point_or_set_start(
        &mut self,
        position: Timestamp,
    ) -> Result<(), SegmentCommandError> {
        // A pending cut point takes precedence: the user is already building a
        // new segment, so the same key continues that flow even when another
        // segment is selected.
        if self.markers.in_point.is_some() {
            return self.add_cut_point(position);
        }
        let selected = self
            .selected_segment
            .and_then(|id| self.timeline.get(id).copied());
        match selected {
            Some(segment) if position < segment.end() => self.set_selected_segment_start(position),
            _ => self.add_cut_point(position),
        }
    }

    /// Sets the selected segment's end, or closes the pending cut point.
    ///
    /// With a pending in point the playhead becomes the out point and the
    /// segment is created, which is the second half of the cut workflow.
    /// Without a selection or a pending point the command does nothing.
    pub fn set_end_or_complete_cut_point(
        &mut self,
        position: Timestamp,
    ) -> Result<(), SegmentCommandError> {
        // Finish the pending point before touching the selected segment.
        if let Some(start) = self.markers.in_point {
            let Some(media) = self.media.as_ref() else {
                return Err(SegmentCommandError::NoMedia);
            };
            let position = clamp_position(position, media.duration);
            if position <= start {
                return Err(SegmentCommandError::EmptyRange);
            }
            self.markers.out_point = Some(position);
            return self.add_segment().map(|_| ());
        }
        if self.selected_segment.is_some() {
            return self.set_selected_segment_end(position);
        }
        Ok(())
    }

    /// Removes pending cut points, or the selected segment when none is
    /// pending.
    pub fn remove_cut_point(&mut self) {
        if self.markers.in_point.is_some() || self.markers.out_point.is_some() {
            self.clear_markers();
        } else {
            self.remove_selected_segment();
        }
    }

    /// Splits the selected segment at the current playback position.
    ///
    /// The new second part becomes selected.
    pub fn split_selected_segment(&mut self) -> Result<SegmentId, SegmentCommandError> {
        let id = self
            .selected_segment
            .ok_or(SegmentCommandError::NoSelection)?;
        self.checkpoint();
        let second =
            self.timeline
                .split(id, self.playback.position)
                .map_err(|error| match error {
                    TimelineError::NotFound => SegmentCommandError::NoSelection,
                    TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                    TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
                })?;
        self.selected_segment = Some(second);
        self.selection.clear();
        Ok(second)
    }

    /// Selects the previous segment in timeline order.
    pub fn select_previous_segment(&mut self) {
        let segments = self.timeline.segments();
        if segments.is_empty() {
            return;
        }
        let index = self
            .selected_segment
            .and_then(|id| self.timeline.index_of(id))
            .and_then(|index| index.checked_sub(1))
            .unwrap_or(0);
        self.selected_segment = Some(segments[index].id());
    }

    /// Selects the next segment in timeline order.
    pub fn select_next_segment(&mut self) {
        let segments = self.timeline.segments();
        if segments.is_empty() {
            return;
        }
        let index = self
            .selected_segment
            .and_then(|id| self.timeline.index_of(id))
            .map(|index| (index + 1).min(segments.len() - 1))
            .unwrap_or(segments.len() - 1);
        self.selected_segment = Some(segments[index].id());
    }

    /// Removes the selected segments.
    ///
    /// The primary selection moves to the next segment in output order, or the
    /// previous one when the removed range was last.
    pub fn remove_selected_segment(&mut self) -> Option<Segment> {
        self.remove_selected_segments()
    }

    fn remove_selected_segments(&mut self) -> Option<Segment> {
        let ids = self.selected_ids();
        let first_id = *ids.first()?;
        let first = self.timeline.get(first_id).copied()?;
        let index = self.timeline.index_of(first_id)?;
        self.checkpoint();
        for id in &ids {
            self.timeline.remove(*id);
        }
        self.selection.clear();
        self.selected_segment = self
            .timeline
            .segments()
            .get(index)
            .or_else(|| {
                index
                    .checked_sub(1)
                    .and_then(|index| self.timeline.segments().get(index))
            })
            .map(|segment| segment.id());
        Some(first)
    }

    /// Sets the enabled flag of a segment.
    pub fn set_segment_enabled(&mut self, id: SegmentId, enabled: bool) -> bool {
        if self.timeline.get(id).is_none() {
            return false;
        }
        self.checkpoint();
        self.timeline.set_enabled(id, enabled)
    }

    /// Moves the selected segment one position earlier in output order.
    pub fn move_selected_segment_up(&mut self) -> bool {
        let Some(id) = self.selected_segment else {
            return false;
        };
        if !self.timeline.index_of(id).is_some_and(|index| index > 0) {
            return false;
        }
        self.checkpoint();
        self.timeline.move_up(id)
    }

    /// Moves the selected segment one position later in output order.
    pub fn move_selected_segment_down(&mut self) -> bool {
        let Some(id) = self.selected_segment else {
            return false;
        };
        let Some(index) = self.timeline.index_of(id) else {
            return false;
        };
        if index + 1 >= self.timeline.len() {
            return false;
        }
        self.checkpoint();
        self.timeline.move_down(id)
    }

    /// Moves the selected segment's start to `start`.
    pub fn set_selected_segment_start(
        &mut self,
        start: Timestamp,
    ) -> Result<(), SegmentCommandError> {
        let id = self
            .selected_segment
            .ok_or(SegmentCommandError::NoSelection)?;
        let Some(segment) = self.timeline.get(id).copied() else {
            return Err(SegmentCommandError::NoSelection);
        };
        let start = clamp_position(start, self.duration());
        self.checkpoint();
        self.timeline
            .update(id, start, segment.end())
            .map(|_| ())
            .map_err(|error| match error {
                TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                TimelineError::NotFound => SegmentCommandError::NoSelection,
                TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
            })
    }

    /// Moves the selected segment's end to `end`.
    pub fn set_selected_segment_end(&mut self, end: Timestamp) -> Result<(), SegmentCommandError> {
        let id = self
            .selected_segment
            .ok_or(SegmentCommandError::NoSelection)?;
        let Some(segment) = self.timeline.get(id).copied() else {
            return Err(SegmentCommandError::NoSelection);
        };
        let end = clamp_position(end, self.duration());
        self.checkpoint();
        self.timeline
            .update(id, segment.start(), end)
            .map(|_| ())
            .map_err(|error| match error {
                TimelineError::InvalidRange => SegmentCommandError::EmptyRange,
                TimelineError::NotFound => SegmentCommandError::NoSelection,
                TimelineError::IdExhausted => SegmentCommandError::IdExhausted,
            })
    }

    /// Sorts segments by source start time. Selection is preserved because it
    /// is tracked by stable ID.
    pub fn sort_segments(&mut self) {
        if self.timeline.len() < 2 {
            return;
        }
        self.checkpoint();
        self.timeline.sort_by_start();
    }

    /// Reverses the selected and unselected areas on the source timeline.
    ///
    /// Segments become gaps and gaps become segments. New IDs are assigned;
    /// the selection moves to the first new segment.
    pub fn invert_segments(&mut self) -> usize {
        let Some(duration) = self.media.as_ref().and_then(|media| media.duration) else {
            return 0;
        };
        self.checkpoint();
        match self.timeline.invert(duration) {
            Ok(count) => {
                self.selection.clear();
                self.selected_segment =
                    self.timeline.segments().first().map(|segment| segment.id());
                count
            }
            Err(error) => {
                log::warn!("inverting segments failed: {error}");
                0
            }
        }
    }

    /// Shuffles the timeline order with the application PRNG.
    pub fn shuffle_segments(&mut self) {
        let len = self.timeline.len();
        if len < 2 {
            return;
        }
        self.checkpoint();
        for index in (1..len).rev() {
            let target = (self.next_random() % (index as u64 + 1)) as usize;
            self.timeline.swap(index, target);
        }
    }

    /// Merges overlapping segments in timeline order.
    pub fn merge_overlapping_segments(&mut self) -> usize {
        if self.timeline.len() < 2 {
            return 0;
        }
        self.checkpoint();
        let merged = self.timeline.merge_overlaps();
        self.selection.retain(|id| self.timeline.get(*id).is_some());
        self.selected_segment = self
            .selected_segment
            .filter(|id| self.timeline.get(*id).is_some());
        merged
    }

    /// Removes every segment without clearing the source binding.
    pub fn clear_segments(&mut self) {
        if self.timeline.is_empty() {
            return;
        }
        self.checkpoint();
        self.timeline.clear_segments();
        self.selection.clear();
        self.selected_segment = None;
    }

    /// Appends `count` random segments that never overlap.
    ///
    /// The timeline is split into `count` slots and each segment is placed
    /// inside its own slot, with a length between 3% and 10% of the media
    /// duration, capped to the slot size. Returns how many were created.
    pub fn add_random_segments(&mut self, count: usize) -> Result<usize, SegmentCommandError> {
        let Some(media) = self.media.as_ref() else {
            return Err(SegmentCommandError::NoMedia);
        };
        let duration = media.duration.map_or(0, |duration| duration.micros());
        if count == 0 || duration < 1_000_000 {
            return Err(SegmentCommandError::EmptyRange);
        }

        let count = count.min(100);
        let slot = duration / count as u64;
        if slot < 100_000 {
            return Err(SegmentCommandError::EmptyRange);
        }

        self.checkpoint();
        let mut created = 0;
        let mut last = None;
        for index in 0..count {
            let slot_start = slot * index as u64;
            let slot_len = if index + 1 == count {
                duration - slot_start
            } else {
                slot
            };

            // 3.0%..10.0% of the whole timeline, capped to the slot.
            let permille = 30 + self.next_random() % 71;
            let target = duration.saturating_mul(permille) / 1000;
            let max_len = (slot_len * 85 / 100).max(1);
            let len = target.clamp(1, max_len);
            let room = slot_len - len;
            let start = slot_start
                + if room > 0 {
                    self.next_random() % (room + 1)
                } else {
                    0
                };
            let end = start + len;
            if start >= end || end > duration {
                continue;
            }

            let Ok(id) = self.timeline.push(
                Timestamp::from_micros(start as i64),
                Timestamp::from_micros(end as i64),
            ) else {
                continue;
            };
            last = Some(id);
            created += 1;
        }

        if created == 0 {
            return Err(SegmentCommandError::EmptyRange);
        }
        self.selection.clear();
        self.selected_segment = last;
        Ok(created)
    }
}

//! Chapter titles for the export.
//!
//! Titles are keyed by [`SegmentId`] so reordering or renaming segments keeps
//! each chapter attached to the right edit. They are not stored in project
//! files yet.

use crate::domain::media::MediaFile;
use crate::domain::segment::SegmentId;
use crate::domain::time::Timestamp;

use super::Application;

impl Application {
    /// Returns the custom title of a segment, when one was set.
    pub fn chapter_title(&self, segment: SegmentId) -> Option<&str> {
        self.chapter_titles.get(&segment).map(String::as_str)
    }

    /// Sets the export chapter title of a segment. Blank titles remove it.
    pub fn set_chapter_title(&mut self, segment: SegmentId, title: String) {
        let trimmed = title.trim();
        let next = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        if self.chapter_titles.get(&segment).map(String::as_str) == next.as_deref() {
            return;
        }
        match next {
            Some(title) => {
                self.chapter_titles.insert(segment, title);
            }
            None => {
                self.chapter_titles.remove(&segment);
            }
        }
        self.timeline_revision += 1;
    }

    /// Creates one segment per container chapter, named after the chapter.
    ///
    /// Chapter times are absolute container times, so they are shifted by the
    /// container start time and clamped to the media duration like every other
    /// timeline position. Empty or reversed chapters are skipped.
    pub(super) fn segments_from_container_chapters(&mut self, media: &MediaFile) {
        if media.chapters.is_empty() {
            return;
        }
        let offset = media.start_time.map_or(0, Timestamp::micros);
        let end_limit = media
            .duration
            .map(|duration| i64::try_from(duration.micros()).unwrap_or(i64::MAX))
            .map(Timestamp::from_micros);

        let mut added = false;
        for chapter in &media.chapters {
            let start = normalize(chapter.start, offset);
            let end = normalize(chapter.end, offset);
            let end = end_limit.map_or(end, |limit| end.min(limit));
            if start >= end {
                continue;
            }
            let Ok(id) = self.timeline.push(start, end) else {
                continue;
            };
            if let Some(title) = chapter
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
            {
                self.chapter_titles.insert(id, title.to_owned());
            }
            added = true;
        }
        if added {
            self.timeline_revision += 1;
        }
    }

    /// Returns the title used when exporting the chapter at `index`.
    pub(super) fn export_chapter_title(&self, segment: SegmentId, index: usize) -> String {
        self.chapter_title(segment)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Segment {}", index + 1))
    }

    /// Drops every custom title.
    pub(super) fn reset_chapter_titles(&mut self) {
        self.chapter_titles.clear();
    }
}

fn normalize(time: Timestamp, offset: i64) -> Timestamp {
    Timestamp::from_micros(time.micros().saturating_sub(offset).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_clears_titles_by_segment_id() {
        let mut application = Application::new();
        let id = SegmentId::new(7);

        assert!(application.chapter_title(id).is_none());
        assert_eq!(application.export_chapter_title(id, 2), "Segment 3");

        application.set_chapter_title(id, "  Intro  ".to_owned());
        assert_eq!(application.chapter_title(id), Some("Intro"));
        assert_eq!(application.export_chapter_title(id, 2), "Intro");

        application.set_chapter_title(id, "   ".to_owned());
        assert!(application.chapter_title(id).is_none());
        assert_eq!(application.export_chapter_title(id, 2), "Segment 3");
    }
}

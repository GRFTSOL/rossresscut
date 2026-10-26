//! Chapter metadata for exports.
//!
//! FFmpeg reads chapters from an `ffmetadata` input. The writer here produces
//! the minimal subset needed for segment chapters.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::time::Duration;

/// One chapter on the output timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterSpec {
    /// Chapter start on the output timeline.
    pub start: Duration,
    /// Chapter end on the output timeline.
    pub end: Duration,
    /// Chapter title.
    pub title: String,
}

/// A temporary ffmetadata file removed on drop.
pub(super) struct TempMetadata {
    path: PathBuf,
}

impl TempMetadata {
    /// Writes `chapters` to a unique temporary file.
    pub(super) fn create(chapters: &[ChapterSpec]) -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let path = std::env::temp_dir().join(format!(
            "rossresscut-chapters-{}-{}.txt",
            std::process::id(),
            stamp.as_nanos()
        ));
        fs::write(&path, render_ffmetadata(chapters))?;
        Ok(Self { path })
    }

    /// Returns the metadata file path.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempMetadata {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            log::warn!(
                "could not remove chapter metadata {}: {error}",
                self.path.display()
            );
        }
    }
}

/// Renders chapters in FFmpeg's `ffmetadata` format.
pub(super) fn render_ffmetadata(chapters: &[ChapterSpec]) -> String {
    let mut text = String::from(";FFMETADATA1\n");
    for chapter in chapters {
        text.push_str("[CHAPTER]\n");
        text.push_str("TIMEBASE=1/1000000\n");
        let _ = writeln!(text, "START={}", chapter.start.micros());
        let _ = writeln!(text, "END={}", chapter.end.micros());
        let _ = writeln!(text, "title={}", escape_value(&chapter.title));
    }
    text
}

fn escape_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '=' | ';' | '#' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\n' | '\r' => escaped.push(' '),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_chapters_in_microseconds() {
        let chapters = vec![
            ChapterSpec {
                start: Duration::from_micros(0),
                end: Duration::from_micros(5_000_000),
                title: "Segment 1".to_owned(),
            },
            ChapterSpec {
                start: Duration::from_micros(5_000_000),
                end: Duration::from_micros(8_000_000),
                title: "Segment 2".to_owned(),
            },
        ];
        let text = render_ffmetadata(&chapters);
        assert!(text.starts_with(";FFMETADATA1\n"));
        assert!(text.contains("START=0\n"));
        assert!(text.contains("END=5000000\n"));
        assert!(text.contains("title=Segment 1\n"));
        assert_eq!(text.matches("[CHAPTER]").count(), 2);
    }

    #[test]
    fn escapes_metadata_special_characters() {
        assert_eq!(escape_value("a=b;c#d\\e"), "a\\=b\\;c\\#d\\\\e");
        assert_eq!(escape_value("line\nbreak"), "line break");
    }
}

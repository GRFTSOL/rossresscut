//! Text subtitle extraction and cropping.
//!
//! FFmpeg's `-ss` input seek does not rewrite subtitle packet timestamps, so a
//! stream copy keeps packets outside the requested range and stretches the
//! output container. This module extracts the whole track as SRT, crops it on
//! the media timeline and feeds the result back to the muxer.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::domain::time::Timestamp;
use crate::media::process;

/// Errors produced while extracting subtitles.
#[derive(Debug)]
pub enum SubtitleError {
    /// The FFmpeg executable could not be started.
    Spawn {
        /// Media file that was being read.
        path: PathBuf,
        /// Operating-system error from process creation.
        source: std::io::Error,
    },
    /// FFmpeg exited with a non-success status.
    Failed {
        /// Media file that was being read.
        path: PathBuf,
        /// Human-readable exit status.
        status: String,
        /// Captured standard error, possibly empty.
        stderr: String,
    },
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { path, source } => write!(
                formatter,
                "starting ffmpeg for subtitles of {}: {source}",
                path.display()
            ),
            Self::Failed {
                path,
                status,
                stderr,
            } => {
                write!(
                    formatter,
                    "ffmpeg failed to extract subtitles from {} ({status})",
                    path.display()
                )?;
                let detail = stderr.trim();
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SubtitleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::Failed { .. } => None,
        }
    }
}

/// One subtitle cue with absolute timestamps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    /// Display start.
    pub start: Timestamp,
    /// Display end.
    pub end: Timestamp,
    /// Cue text, possibly multiline.
    pub text: String,
}

/// Extracts `stream_index` as SRT text.
///
/// This blocks until FFmpeg finishes and must run outside the UI thread.
pub fn extract(path: &Path, stream_index: usize) -> Result<String, SubtitleError> {
    let output = process::command("ffmpeg")
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(path)
        .arg("-map")
        .arg(format!("0:{stream_index}"))
        .arg("-c:s")
        .arg("srt")
        .arg("-f")
        .arg("srt")
        .arg("pipe:1")
        .output()
        .map_err(|source| SubtitleError::Spawn {
            path: path.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(SubtitleError::Failed {
            path: path.to_path_buf(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Keeps the cues that overlap `[start, end)` and rebases them to zero.
///
/// When `keep_partial` is false, a cue that crosses either boundary is dropped
/// entirely. When it is true, the cue is clipped to the range instead.
pub fn crop(srt: &str, start: Timestamp, end: Timestamp, keep_partial: bool) -> String {
    if end <= start {
        return String::new();
    }

    let mut kept = Vec::new();
    for mut cue in parse(srt) {
        if cue.end <= start || cue.start >= end {
            continue;
        }
        if !keep_partial && (cue.start < start || cue.end > end) {
            continue;
        }

        cue.start = Timestamp::from_micros((cue.start.max(start).micros() - start.micros()).max(0));
        cue.end = Timestamp::from_micros((cue.end.min(end).micros() - start.micros()).max(0));
        if cue.end <= cue.start {
            continue;
        }
        kept.push(cue);
    }

    render(&kept)
}

/// Parses SRT text into cues. Malformed blocks are skipped.
pub fn parse(srt: &str) -> Vec<Cue> {
    let mut cues = Vec::new();

    for block in srt.split("\n\n") {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let Some(first) = lines.next() else {
            continue;
        };
        let timing = if first.contains("-->") {
            first
        } else {
            match lines.next() {
                Some(next) => next,
                None => continue,
            }
        };
        if !timing.contains("-->") {
            continue;
        }

        let Some((left, right)) = timing.split_once("-->") else {
            continue;
        };
        let Some(start) = parse_time(left.trim()) else {
            continue;
        };
        let end_text = right.split_whitespace().next().unwrap_or_default();
        let Some(end) = parse_time(end_text) else {
            continue;
        };
        if end <= start {
            continue;
        }

        cues.push(Cue {
            start,
            end,
            text: lines.collect::<Vec<_>>().join("\n"),
        });
    }

    cues
}

/// Renders cues as SRT text with sequential numbering.
pub fn render(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (index, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            format_time(cue.start),
            format_time(cue.end),
            cue.text
        ));
    }
    out
}

fn parse_time(text: &str) -> Option<Timestamp> {
    let (clock, millis) = text.split_once(',')?;
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours: i64 = parts[0].trim().parse().ok()?;
    let minutes: i64 = parts[1].trim().parse().ok()?;
    let seconds: i64 = parts[2].trim().parse().ok()?;
    let millis: i64 = millis.trim().parse().ok()?;
    if hours < 0 || minutes < 0 || seconds < 0 || millis < 0 {
        return None;
    }
    let total_millis = ((hours * 3600 + minutes * 60 + seconds) * 1000) + millis;
    Some(Timestamp::from_micros(total_millis * 1000))
}

fn format_time(time: Timestamp) -> String {
    let micros = time.micros().max(0);
    let total_millis = micros / 1000;
    let millis = total_millis % 1000;
    let total_seconds = total_millis / 1000;
    let seconds = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str =
        "1\n00:00:00,000 --> 00:00:00,900\nHello\n\n2\n00:00:01,000 --> 00:00:01,900\nWorld\n";

    #[test]
    fn parses_cues() {
        let cues = parse(SRT);

        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Timestamp::ZERO);
        assert_eq!(cues[0].end, Timestamp::from_micros(900_000));
        assert_eq!(cues[0].text, "Hello");
    }

    #[test]
    fn cropping_drops_partial_cues_by_default() {
        let cropped = crop(
            SRT,
            Timestamp::from_micros(500_000),
            Timestamp::from_micros(1_500_000),
            false,
        );

        assert!(cropped.is_empty(), "{cropped}");
    }

    #[test]
    fn cropping_keeps_partial_cues_when_asked() {
        let cropped = crop(
            SRT,
            Timestamp::from_micros(500_000),
            Timestamp::from_micros(1_500_000),
            true,
        );

        let cues = parse(&cropped);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Timestamp::ZERO);
        assert_eq!(cues[0].end, Timestamp::from_micros(400_000));
        assert_eq!(cues[0].text, "Hello");
        assert_eq!(cues[1].start, Timestamp::from_micros(500_000));
        assert_eq!(cues[1].end, Timestamp::from_micros(1_000_000));
        assert_eq!(cues[1].text, "World");
    }

    #[test]
    fn cropping_rebases_and_renumbers() {
        let cropped = crop(
            SRT,
            Timestamp::from_micros(1_000_000),
            Timestamp::from_micros(2_000_000),
            false,
        );

        assert!(cropped.starts_with("1\n00:00:00,000 --> 00:00:00,900\nWorld"));
    }

    #[test]
    fn malformed_blocks_are_skipped() {
        let text = "1\nnot a timing line\nHello\n\n2\n00:00:01,000 --> 00:00:02,000\nWorld\n";

        let cues = parse(text);

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "World");
    }

    #[test]
    fn timestamps_round_trip() {
        for micros in [0, 1_000, 999_000, 3_600_000_000] {
            let time = Timestamp::from_micros(micros);
            assert_eq!(parse_time(&format_time(time)), Some(time));
        }
    }
}

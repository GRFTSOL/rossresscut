//! Parsing of `ffmpeg -progress pipe:1` output.
//!
//! FFmpeg writes one `key=value` pair per line and repeats blocks while the
//! export runs. Only the fields needed for a progress bar are recognised here.

use crate::domain::time::Timestamp;

/// One recognised progress line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressUpdate {
    /// The muxer has written up to this output timestamp.
    OutTime(Timestamp),
    /// FFmpeg reported `progress=end`.
    Ended,
}

/// Parses one line of `ffmpeg -progress` output.
///
/// Returns `None` for unrelated fields, malformed values, and negative times.
pub fn parse_progress_line(line: &str) -> Option<ProgressUpdate> {
    let (key, value) = line.trim().split_once('=')?;
    match key.trim() {
        "out_time_us" => parse_micros(value).map(ProgressUpdate::OutTime),
        // `out_time_ms` is documented as milliseconds but FFmpeg has always
        // emitted microseconds for this field. Treat it as microseconds.
        "out_time_ms" => parse_micros(value).map(ProgressUpdate::OutTime),
        "progress" => (value.trim() == "end").then_some(ProgressUpdate::Ended),
        _ => None,
    }
}

fn parse_micros(value: &str) -> Option<Timestamp> {
    let micros: i64 = value.trim().parse().ok()?;
    (micros >= 0).then(|| Timestamp::from_micros(micros))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_out_time_fields() {
        assert_eq!(
            parse_progress_line("out_time_us=1500000"),
            Some(ProgressUpdate::OutTime(Timestamp::from_micros(1_500_000)))
        );
        assert_eq!(
            parse_progress_line("out_time_ms=2500000"),
            Some(ProgressUpdate::OutTime(Timestamp::from_micros(2_500_000)))
        );
    }

    #[test]
    fn recognises_end_only() {
        assert_eq!(
            parse_progress_line("progress=end"),
            Some(ProgressUpdate::Ended)
        );
        assert_eq!(parse_progress_line("progress=continue"), None);
    }

    #[test]
    fn ignores_unrelated_and_malformed_lines() {
        assert_eq!(parse_progress_line("frame=42"), None);
        assert_eq!(parse_progress_line(""), None);
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("out_time_us=-1"), None);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            parse_progress_line("  out_time_us=1000000  "),
            Some(ProgressUpdate::OutTime(Timestamp::from_micros(1_000_000)))
        );
    }
}

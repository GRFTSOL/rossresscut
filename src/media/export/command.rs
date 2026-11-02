//! FFmpeg argument construction for lossless segment export.
//!
//! The export is a stream copy: FFmpeg remuxes packets without re-encoding.
//! Seeking uses `-ss` before `-i`, so the actual start falls back to the
//! previous keyframe. This is the documented lossless-cut behaviour and must
//! stay visible to the user.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::domain::time::{Duration, Timestamp};

use super::chapters::ChapterSpec;

/// A validated request for one lossless segment export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRequest {
    source: PathBuf,
    start: Timestamp,
    end: Timestamp,
    output: PathBuf,
    chapters: Vec<ChapterSpec>,
    title: Option<String>,
}

impl ExportRequest {
    /// Creates a request for `[start, end)` written to `output`.
    ///
    /// Returns `None` when the range is empty, reversed, or when the output
    /// path is the source path.
    pub fn new(
        source: impl Into<PathBuf>,
        start: Timestamp,
        end: Timestamp,
        output: impl Into<PathBuf>,
    ) -> Option<Self> {
        let source = source.into();
        let output = output.into();
        if start >= end || source == output {
            return None;
        }
        Some(Self {
            source,
            start,
            end,
            output,
            chapters: Vec::new(),
            title: None,
        })
    }

    /// Attaches chapters written to the output container.
    pub fn with_chapters(mut self, chapters: Vec<ChapterSpec>) -> Self {
        self.chapters = chapters;
        self
    }

    /// Sets the output title metadata. Empty titles are ignored.
    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title.filter(|title| !title.trim().is_empty());
        self
    }

    /// Returns the output title metadata, when set.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the chapters to write.
    pub fn chapters(&self) -> &[ChapterSpec] {
        &self.chapters
    }

    /// Returns the media file being exported.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Returns the segment start on the source timeline.
    pub const fn start(&self) -> Timestamp {
        self.start
    }

    /// Returns the segment end on the source timeline.
    pub const fn end(&self) -> Timestamp {
        self.end
    }

    /// Returns the requested output path.
    pub fn output(&self) -> &Path {
        &self.output
    }

    /// Returns the requested segment length.
    pub fn duration(&self) -> Duration {
        self.end.duration_since(self.start).unwrap_or_default()
    }
}

/// Builds the FFmpeg argument list for a stream-copy export.
///
/// `-ss` is placed before `-i` for a fast keyframe seek. `-t` is placed after
/// all inputs and uses the requested segment length. `-c copy` applies to every
/// mapped stream. When `metadata` is provided it is a `ffmetadata` input at
/// index 1 and its chapters are mapped to the output.
pub fn build_args(request: &ExportRequest, metadata: Option<&Path>) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];

    let cuts_start = request.start.micros() > 0;
    if cuts_start {
        args.push("-ss".into());
        args.push(format_timestamp(request.start).into());
    }

    args.push("-i".into());
    args.push(request.source.as_os_str().to_owned());

    if let Some(metadata) = metadata {
        args.push("-i".into());
        args.push(metadata.as_os_str().to_owned());
    }

    args.push("-t".into());
    args.push(format_duration(request.duration()).into());

    args.push("-map".into());
    args.push("0".into());
    args.push("-c".into());
    args.push("copy".into());

    if metadata.is_some() {
        args.push("-map_chapters".into());
        args.push("1".into());
    }

    if let Some(title) = &request.title {
        args.push("-metadata".into());
        args.push(format!("title={title}").into());
    }

    if cuts_start {
        // Rebase the output to zero so the saved segment starts at 0.
        args.push("-avoid_negative_ts".into());
        args.push("make_zero".into());
    }

    args.push("-ignore_unknown".into());
    args.push("-y".into());
    args.push(request.output.as_os_str().to_owned());

    args
}

/// Formats a timestamp as decimal seconds with microsecond precision.
fn format_timestamp(timestamp: Timestamp) -> String {
    let micros = timestamp.micros();
    let sign = if micros < 0 { "-" } else { "" };
    let absolute = micros.unsigned_abs();
    let seconds = absolute / 1_000_000;
    let fraction = absolute % 1_000_000;
    format!("{sign}{seconds}.{fraction:06}")
}

/// Formats a duration as decimal seconds with microsecond precision.
fn format_duration(duration: Duration) -> String {
    let micros = duration.micros();
    let seconds = micros / 1_000_000;
    let fraction = micros % 1_000_000;
    format!("{seconds}.{fraction:06}")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn request(start_micros: i64, end_micros: i64) -> ExportRequest {
        let start = Timestamp::from_micros(start_micros);
        let end = Timestamp::from_micros(end_micros);
        ExportRequest::new("input.mp4", start, end, "output.mp4").expect("valid request")
    }

    fn args(request: &ExportRequest) -> Vec<String> {
        build_args(request, None)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn position(values: &[String], needle: &str) -> usize {
        values
            .iter()
            .position(|value| value == needle)
            .expect("argument present")
    }

    #[test]
    fn places_input_seek_before_input_and_duration_after() {
        let values = args(&request(5_000_000, 8_500_000));
        let seek = position(&values, "-ss");
        let input = position(&values, "-i");
        let duration = position(&values, "-t");

        assert!(seek < input);
        assert!(duration > input);
        assert_eq!(values[seek + 1], "5.000000");
        assert_eq!(values[duration + 1], "3.500000");
    }

    #[test]
    fn writes_the_requested_title_and_ignores_blank_ones() {
        let titled = request(0, 1_000_000).with_title(Some("Episode 1".to_owned()));
        let values = args(&titled);
        assert_eq!(
            values[position(&values, "-metadata") + 1],
            "title=Episode 1"
        );

        let blank = request(0, 1_000_000).with_title(Some("   ".to_owned()));
        assert!(blank.title().is_none());
        let values = args(&blank);
        assert!(!values.iter().any(|value| value == "-metadata"));
    }

    #[test]
    fn copies_every_stream_without_reencoding() {
        let values = args(&request(1_000_000, 2_000_000));

        assert_eq!(values[position(&values, "-map") + 1], "0");
        assert_eq!(values[position(&values, "-c") + 1], "copy");
        assert!(!values.iter().any(|value| value == "-c:v"));
        assert!(!values.iter().any(|value| value == "libx264"));
    }

    #[test]
    fn rebases_to_zero_only_when_cutting_the_start() {
        let values = args(&request(1_000_000, 2_000_000));
        assert!(values.iter().any(|value| value == "-avoid_negative_ts"));

        let values = args(&request(0, 2_000_000));
        assert!(!values.iter().any(|value| value == "-ss"));
        assert!(!values.iter().any(|value| value == "-avoid_negative_ts"));
    }

    #[test]
    fn rejects_empty_reversed_and_same_path_requests() {
        let start = Timestamp::from_micros(2_000_000);
        let end = Timestamp::from_micros(1_000_000);
        assert!(ExportRequest::new("in.mp4", start, end, "out.mp4").is_none());

        let same = Timestamp::from_micros(3_000_000);
        assert!(ExportRequest::new("in.mp4", start, same, "in.mp4").is_none());
    }

    #[test]
    fn formats_microsecond_precision() {
        assert_eq!(
            format_timestamp(Timestamp::from_micros(1_500_001)),
            "1.500001"
        );
        assert_eq!(
            format_duration(Duration::from_micros(3_000_000)),
            "3.000000"
        );
        assert_eq!(format_timestamp(Timestamp::from_micros(0)), "0.000000");
    }

    #[test]
    fn output_path_is_last_argument() {
        let values = args(&request(1_000_000, 2_000_000));
        assert_eq!(values.last().map(String::as_str), Some("output.mp4"));
        assert_eq!(
            build_args(&request(1_000_000, 2_000_000), None)
                .last()
                .map(OsString::as_os_str),
            Some(OsStr::new("output.mp4"))
        );
    }
}

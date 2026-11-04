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

/// Subtitles to mux into the export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitlePlan {
    /// Complete SRT text of the source track.
    pub srt: String,
    /// Whether cues crossing the segment boundaries are kept.
    pub keep_partial: bool,
}

impl SubtitlePlan {
    /// Returns the SRT to mux for `[start, end)`.
    pub fn crop_for(&self, start: Timestamp, end: Timestamp) -> String {
        crate::media::subtitle::crop(&self.srt, start, end, self.keep_partial)
    }
}

/// Streams to map into the output, by absolute media stream index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamSelection {
    /// Video stream indices, in media order.
    pub video: Vec<usize>,
    /// Audio stream indices, in media order.
    pub audio: Vec<usize>,
    /// Subtitle stream indices, in media order.
    pub subtitle: Vec<usize>,
}

impl StreamSelection {
    /// Returns whether every list is empty.
    pub fn is_empty(&self) -> bool {
        self.video.is_empty() && self.audio.is_empty() && self.subtitle.is_empty()
    }
}

/// Source color properties copied to a smart-cut re-encode.
///
/// FFmpeg usually propagates these from the decoded frames, but passing them
/// explicitly keeps 10-bit/HDR output stable across encoder and muxer versions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoColorSpec {
    /// Source pixel format, for example `yuv420p10le`.
    pub pixel_format: Option<String>,
    /// Color range, for example `tv` or `pc`.
    pub range: Option<String>,
    /// Color space, for example `bt2020nc`.
    pub space: Option<String>,
    /// Transfer characteristics, for example `smpte2084`.
    pub transfer: Option<String>,
    /// Color primaries, for example `bt2020`.
    pub primaries: Option<String>,
}

/// Video re-encode settings for a smart-cut head.
///
/// The encoder is a software encoder: exporting must not depend on the local
/// GPU or on a hardware encoder being compiled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoEncodeSpec {
    /// FFmpeg encoder name, for example `libx264`.
    pub encoder: String,
    /// Target video bit rate in bits per second.
    pub bit_rate: u64,
    /// Source B-frame count, copied so the encoder keeps the same structure.
    pub b_frames: Option<u32>,
    /// Source video timescale, when it can be expressed as an integer.
    pub timescale: Option<u64>,
    /// Source pixel format and color tags, copied to avoid HDR loss.
    pub color: VideoColorSpec,
}

/// A validated request for one lossless segment export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRequest {
    source: PathBuf,
    start: Timestamp,
    end: Timestamp,
    seek: Timestamp,
    output: PathBuf,
    chapters: Vec<ChapterSpec>,
    title: Option<String>,
    subtitle: Option<SubtitlePlan>,
    streams: Option<StreamSelection>,
    encode: Option<VideoEncodeSpec>,
    rebase_negative: bool,
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
            seek: start,
            output,
            chapters: Vec::new(),
            title: None,
            subtitle: None,
            streams: None,
            encode: None,
            rebase_negative: true,
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

    /// Attaches cropped subtitles to the output.
    pub fn with_subtitle(mut self, subtitle: Option<SubtitlePlan>) -> Self {
        self.subtitle = subtitle;
        self
    }

    /// Returns the subtitle plan, when set.
    pub fn subtitle(&self) -> Option<&SubtitlePlan> {
        self.subtitle.as_ref()
    }

    /// Restricts the output to the listed streams. `None` keeps every stream.
    pub fn with_streams(mut self, streams: Option<StreamSelection>) -> Self {
        self.streams = streams;
        self
    }

    /// Returns the stream selection, when restricted.
    pub fn streams(&self) -> Option<&StreamSelection> {
        self.streams.as_ref()
    }

    /// Re-encodes the video stream instead of copying it.
    pub fn with_encode(mut self, encode: Option<VideoEncodeSpec>) -> Self {
        self.encode = encode;
        self
    }

    /// Returns the video re-encode settings, when set.
    pub fn encode(&self) -> Option<&VideoEncodeSpec> {
        self.encode.as_ref()
    }

    /// Controls whether a cut start is rebased to zero.
    ///
    /// Parts that are later concatenated must keep their timestamps, otherwise
    /// the join gains a one-frame hole.
    pub fn with_rebase_negative(mut self, rebase: bool) -> Self {
        self.rebase_negative = rebase;
        self
    }

    /// Returns whether a cut start is rebased to zero.
    pub const fn rebase_negative(&self) -> bool {
        self.rebase_negative
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

    /// Sets the input seek target while keeping `start` as the content start.
    ///
    /// Stream copy can only start at a seekable point, which is often earlier
    /// than `start`. Seeking to a later target can make the demuxer land on
    /// `start` exactly, so the copy piece does not take other content with it.
    pub fn with_seek(mut self, seek: Timestamp) -> Self {
        self.seek = seek;
        self
    }

    /// Returns the input seek target used for `-ss`.
    pub const fn seek(&self) -> Timestamp {
        self.seek
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
pub fn build_args(
    request: &ExportRequest,
    metadata: Option<&Path>,
    subtitle: Option<&Path>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];

    let cuts_start = request.seek().micros() > 0;
    if cuts_start {
        args.push("-ss".into());
        args.push(format_timestamp(request.seek()).into());
    }

    args.push("-i".into());
    args.push(request.source.as_os_str().to_owned());

    if let Some(metadata) = metadata {
        args.push("-i".into());
        args.push(metadata.as_os_str().to_owned());
    }

    if let Some(subtitle) = subtitle {
        args.push("-i".into());
        args.push(subtitle.as_os_str().to_owned());
    }

    if request.encode().is_some() {
        // Without this the input seek offset leaks into the encoded part and
        // the concatenated output starts with a hole.
        args.push("-ss".into());
        args.push("0".into());
    }

    args.push("-t".into());
    args.push(
        format_duration(
            request
                .end()
                .duration_since(request.seek())
                .unwrap_or_else(|| request.duration()),
        )
        .into(),
    );

    let subtitle_replaced = request.subtitle().is_some();
    let subtitle_included = subtitle.is_some()
        && match request.streams() {
            Some(selection) => !selection.subtitle.is_empty(),
            None => true,
        };
    match request.streams() {
        Some(selection) => {
            for index in &selection.video {
                args.push("-map".into());
                args.push(format!("0:{index}").into());
            }
            for index in &selection.audio {
                args.push("-map".into());
                args.push(format!("0:{index}").into());
            }
            if subtitle_replaced {
                // Subtitle output follows the selection: only keep the cropped
                // track when the user asked for a subtitle stream.
                if subtitle.is_some() && !selection.subtitle.is_empty() {
                    args.push("-map".into());
                    args.push(format!("{}:s", 1 + usize::from(metadata.is_some())).into());
                }
            } else {
                for index in &selection.subtitle {
                    args.push("-map".into());
                    args.push(format!("0:{index}").into());
                }
            }
        }
        None if subtitle_replaced => {
            args.push("-map".into());
            args.push("0:v?".into());
            args.push("-map".into());
            args.push("0:a?".into());
            if subtitle.is_some() {
                args.push("-map".into());
                args.push(format!("{}:s", 1 + usize::from(metadata.is_some())).into());
            }
        }
        None => {
            args.push("-map".into());
            args.push("0".into());
        }
    }

    args.push("-c".into());
    args.push("copy".into());

    if let Some(encode) = request.encode() {
        args.push("-c:v".into());
        args.push(encode.encoder.clone().into());
        args.push("-b:v".into());
        args.push(encode.bit_rate.to_string().into());
        if let Some(b_frames) = encode.b_frames.filter(|count| *count > 0) {
            args.push("-bf".into());
            args.push(b_frames.to_string().into());
        }
        if let Some(timescale) = encode.timescale {
            args.push("-video_track_timescale".into());
            args.push(timescale.to_string().into());
        }
        append_color_args(&mut args, &encode.color);
    }

    if subtitle_included {
        args.push("-c:s".into());
        args.push(subtitle_codec(&request.output).into());
    }

    if metadata.is_some() {
        args.push("-map_chapters".into());
        args.push("1".into());
    }

    if let Some(title) = &request.title {
        args.push("-metadata".into());
        args.push(format!("title={title}").into());
    }

    if cuts_start && request.rebase_negative() {
        // Rebase the output to zero so the saved segment starts at 0.
        args.push("-avoid_negative_ts".into());
        args.push("make_zero".into());
    }

    args.push("-ignore_unknown".into());
    args.push("-y".into());
    args.push(request.output.as_os_str().to_owned());

    args
}

/// Appends the source pixel format and color tags used when re-encoding.
fn append_color_args(args: &mut Vec<OsString>, color: &VideoColorSpec) {
    for (option, value) in [
        ("-pix_fmt", &color.pixel_format),
        ("-color_range", &color.range),
        ("-colorspace", &color.space),
        ("-color_trc", &color.transfer),
        ("-color_primaries", &color.primaries),
    ] {
        if let Some(value) = value {
            args.push(option.into());
            args.push(value.clone().into());
        }
    }
}

/// Picks a subtitle codec the output container can store.
fn subtitle_codec(output: &Path) -> &'static str {
    match output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp4" | "m4v" | "mov") => "mov_text",
        _ => "srt",
    }
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
        build_args(request, None, None)
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
    fn seeks_to_a_later_target_to_reach_the_content_start() {
        let request = request(1_000_000, 5_000_000).with_seek(Timestamp::from_micros(2_000_000));
        let values = args(&request);

        assert_eq!(values[position(&values, "-ss") + 1], "2.000000");
        assert_eq!(values[position(&values, "-t") + 1], "3.000000");
        assert_eq!(request.start(), Timestamp::from_micros(1_000_000));
        assert_eq!(request.duration(), Duration::from_micros(4_000_000));
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
    fn maps_only_the_selected_streams() {
        let request = request(0, 1_000_000).with_streams(Some(StreamSelection {
            video: vec![0],
            audio: vec![2],
            subtitle: vec![3],
        }));
        let values = args(&request);

        assert!(values.iter().any(|value| value == "0:0"));
        assert!(values.iter().any(|value| value == "0:2"));
        assert!(values.iter().any(|value| value == "0:3"));
        assert!(!values.iter().any(|value| value == "0:1"));
        assert!(!values.iter().any(|value| value == "0"));
    }

    #[test]
    fn subtitle_selection_gates_the_cropped_track() {
        let plan = SubtitlePlan {
            srt: "1\n00:00:00,000 --> 00:00:00,500\nHi\n\n".to_owned(),
            keep_partial: true,
        };
        let subtitle_path = std::path::Path::new("/tmp/cropped.srt");

        let with_subtitle = request(0, 1_000_000)
            .with_subtitle(Some(plan.clone()))
            .with_streams(Some(StreamSelection {
                video: vec![0],
                audio: vec![],
                subtitle: vec![3],
            }));
        let values: Vec<String> = build_args(&with_subtitle, None, Some(subtitle_path))
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(values.iter().any(|value| value == "1:s"));
        assert!(values.iter().any(|value| value == "-c:s"));

        let without_subtitle = request(0, 1_000_000)
            .with_subtitle(Some(plan))
            .with_streams(Some(StreamSelection {
                video: vec![0],
                audio: vec![],
                subtitle: vec![],
            }));
        let values: Vec<String> = build_args(&without_subtitle, None, Some(subtitle_path))
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(!values.iter().any(|value| value == "1:s"));
        assert!(!values.iter().any(|value| value == "-c:s"));
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
    fn encodes_the_video_head_and_keeps_the_other_streams_copied() {
        let request = request(5_500_000, 6_000_000)
            .with_encode(Some(VideoEncodeSpec {
                encoder: "libx264".to_owned(),
                bit_rate: 1_000_000,
                b_frames: Some(3),
                timescale: Some(15_360),
                color: VideoColorSpec::default(),
            }))
            .with_rebase_negative(false);
        let values = args(&request);

        assert_eq!(values[position(&values, "-c") + 1], "copy");
        assert_eq!(values[position(&values, "-c:v") + 1], "libx264");
        assert_eq!(values[position(&values, "-b:v") + 1], "1000000");
        assert_eq!(values[position(&values, "-bf") + 1], "3");
        assert_eq!(
            values[position(&values, "-video_track_timescale") + 1],
            "15360"
        );
        assert!(!values.iter().any(|value| value == "-avoid_negative_ts"));
    }

    #[test]
    fn copies_colour_tags_when_reencoding() {
        let request = request(5_500_000, 6_000_000).with_encode(Some(VideoEncodeSpec {
            encoder: "libx265".to_owned(),
            bit_rate: 1_000_000,
            b_frames: None,
            timescale: None,
            color: VideoColorSpec {
                pixel_format: Some("yuv420p10le".to_owned()),
                range: Some("tv".to_owned()),
                space: Some("bt2020nc".to_owned()),
                transfer: Some("smpte2084".to_owned()),
                primaries: Some("bt2020".to_owned()),
            },
        }));
        let values = args(&request);

        assert_eq!(values[position(&values, "-pix_fmt") + 1], "yuv420p10le");
        assert_eq!(values[position(&values, "-color_range") + 1], "tv");
        assert_eq!(values[position(&values, "-colorspace") + 1], "bt2020nc");
        assert_eq!(values[position(&values, "-color_trc") + 1], "smpte2084");
        assert_eq!(values[position(&values, "-color_primaries") + 1], "bt2020");
    }

    #[test]
    fn resets_the_output_timestamps_after_the_input_seek() {
        let request = request(5_500_000, 6_000_000).with_encode(Some(VideoEncodeSpec {
            encoder: "libx264".to_owned(),
            bit_rate: 1_000_000,
            b_frames: None,
            timescale: None,
            color: VideoColorSpec::default(),
        }));
        let values = args(&request);
        let seeks: Vec<usize> = values
            .iter()
            .enumerate()
            .filter(|(_, value)| *value == "-ss")
            .map(|(index, _)| index)
            .collect();

        assert_eq!(seeks.len(), 2);
        assert!(seeks[1] > position(&values, "-i"));
        assert_eq!(values[seeks[1] + 1], "0");
        assert!(!values.iter().any(|value| value == "-bf"));
        assert!(!values.iter().any(|value| value == "-video_track_timescale"));
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
            build_args(&request(1_000_000, 2_000_000), None, None)
                .last()
                .map(OsString::as_os_str),
            Some(OsStr::new("output.mp4"))
        );
    }
}

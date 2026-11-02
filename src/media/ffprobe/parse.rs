//! Conversion from FFprobe's raw JSON to the domain media model.

use std::path::Path;

use crate::domain::media::{
    AudioStream, Chapter, Disposition, MediaFile, MediaFormat, MediaStream, StreamInfo,
    SubtitleStream, VideoStream,
};
use crate::domain::time::{Duration, FrameRate, TimeBase, Timestamp};

use super::ProbeError;
use super::raw::{ProbeOutput, RawChapter, RawDisposition, RawFormat, RawStream, Scalar};

pub(super) fn parse_output(json: &str, path: &Path) -> Result<MediaFile, ProbeError> {
    let output: ProbeOutput =
        serde_json::from_str(json).map_err(|source| ProbeError::json(path, source))?;

    let raw_format = output
        .format
        .as_ref()
        .ok_or_else(|| ProbeError::invalid_data(path, "missing format section"))?;
    let format = parse_format(raw_format, path)?;

    let streams = output
        .streams
        .iter()
        .enumerate()
        .map(|(position, raw)| parse_stream(raw, position, path))
        .collect::<Result<Vec<_>, _>>()?;

    let chapters = output
        .chapters
        .iter()
        .enumerate()
        .map(|(position, raw)| parse_chapter(raw, position, path))
        .collect::<Result<Vec<_>, _>>()?;

    let duration = format.duration;
    let start_time = format.start_time;

    Ok(MediaFile {
        path: path.to_path_buf(),
        format,
        duration,
        start_time,
        streams,
        chapters,
    })
}

fn parse_format(raw: &RawFormat, path: &Path) -> Result<MediaFormat, ProbeError> {
    let name = raw
        .format_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ProbeError::invalid_data(path, "format.format_name is missing"))?;

    Ok(MediaFormat {
        name: name.to_owned(),
        long_name: raw.format_long_name.clone(),
        duration: parse_optional_duration(raw.duration.as_ref(), "format.duration", path)?,
        size: parse_optional_u64(raw.size.as_ref(), "format.size", path)?,
        bit_rate: parse_optional_u64(raw.bit_rate.as_ref(), "format.bit_rate", path)?,
        start_time: parse_optional_timestamp(raw.start_time.as_ref(), "format.start_time", path)?,
    })
}

fn parse_stream(raw: &RawStream, position: usize, path: &Path) -> Result<MediaStream, ProbeError> {
    let raw_index = raw.index.ok_or_else(|| {
        ProbeError::invalid_data(path, format!("stream #{position} has no index"))
    })?;
    let index = usize::try_from(raw_index).map_err(|_| {
        ProbeError::invalid_data(
            path,
            format!("stream #{position} has a negative index: {raw_index}"),
        )
    })?;
    let codec_type = raw.codec_type.as_deref().ok_or_else(|| {
        ProbeError::invalid_data(path, format!("stream #{index} has no codec_type"))
    })?;

    let info = StreamInfo {
        index,
        codec_name: raw.codec_name.clone(),
        codec_long_name: raw.codec_long_name.clone(),
        profile: raw.profile.clone(),
        language: raw.tags.as_ref().and_then(|tags| tags.language.clone()),
        title: raw.tags.as_ref().and_then(|tags| tags.title.clone()),
        disposition: parse_disposition(raw.disposition.as_ref()),
        bit_rate: parse_optional_u64(
            raw.bit_rate.as_ref(),
            &format!("stream #{index} bit_rate"),
            path,
        )?,
        time_base: parse_optional_time_base(
            raw.time_base.as_deref(),
            &format!("stream #{index} time_base"),
            path,
        )?,
        start_time: parse_optional_timestamp(
            raw.start_time.as_ref(),
            &format!("stream #{index} start_time"),
            path,
        )?,
        duration: parse_optional_duration(
            raw.duration.as_ref(),
            &format!("stream #{index} duration"),
            path,
        )?,
    };

    match codec_type {
        "video" => Ok(MediaStream::Video(VideoStream {
            info,
            width: raw.width,
            height: raw.height,
            pixel_format: raw.pix_fmt.clone(),
            frame_rate: parse_optional_frame_rate(
                raw.r_frame_rate.as_deref(),
                &format!("stream #{index} r_frame_rate"),
                path,
            )?,
            average_frame_rate: parse_optional_frame_rate(
                raw.avg_frame_rate.as_deref(),
                &format!("stream #{index} avg_frame_rate"),
                path,
            )?,
            has_b_frames: parse_optional_u32(
                raw.has_b_frames.as_ref(),
                &format!("stream #{index} has_b_frames"),
                path,
            )?,
            color_range: raw.color_range.clone(),
            color_space: raw.color_space.clone(),
            color_transfer: raw.color_transfer.clone(),
            color_primaries: raw.color_primaries.clone(),
        })),
        "audio" => Ok(MediaStream::Audio(AudioStream {
            info,
            sample_rate: parse_optional_u32(
                raw.sample_rate.as_ref(),
                &format!("stream #{index} sample_rate"),
                path,
            )?,
            channels: raw.channels,
            channel_layout: raw.channel_layout.clone(),
            sample_format: raw.sample_fmt.clone(),
        })),
        "subtitle" => Ok(MediaStream::Subtitle(SubtitleStream { info })),
        _ => Ok(MediaStream::Other(info)),
    }
}

fn parse_chapter(raw: &RawChapter, position: usize, path: &Path) -> Result<Chapter, ProbeError> {
    let id = raw
        .id
        .ok_or_else(|| ProbeError::invalid_data(path, format!("chapter #{position} has no id")))?;
    let time_base_text = raw.time_base.as_deref().ok_or_else(|| {
        ProbeError::invalid_data(path, format!("chapter #{position} has no time_base"))
    })?;
    let time_base = TimeBase::parse(time_base_text).ok_or_else(|| {
        ProbeError::invalid_data(
            path,
            format!("chapter #{position} has an invalid time_base: {time_base_text}"),
        )
    })?;
    let start_ticks = raw.start.ok_or_else(|| {
        ProbeError::invalid_data(path, format!("chapter #{position} has no start"))
    })?;
    let end_ticks = raw
        .end
        .ok_or_else(|| ProbeError::invalid_data(path, format!("chapter #{position} has no end")))?;
    let start = Timestamp::from_ticks(start_ticks, time_base).ok_or_else(|| {
        ProbeError::invalid_data(
            path,
            format!("chapter #{position} start cannot be represented: {start_ticks} ticks"),
        )
    })?;
    let end = Timestamp::from_ticks(end_ticks, time_base).ok_or_else(|| {
        ProbeError::invalid_data(
            path,
            format!("chapter #{position} end cannot be represented: {end_ticks} ticks"),
        )
    })?;

    if end < start {
        return Err(ProbeError::invalid_data(
            path,
            format!("chapter #{position} ends before it starts"),
        ));
    }

    Ok(Chapter {
        id,
        start,
        end,
        title: raw.tags.as_ref().and_then(|tags| tags.title.clone()),
    })
}

fn parse_disposition(raw: Option<&RawDisposition>) -> Disposition {
    match raw {
        Some(raw) => Disposition {
            default: raw.default != 0,
            forced: raw.forced != 0,
            hearing_impaired: raw.hearing_impaired != 0,
            visual_impaired: raw.visual_impaired != 0,
            attached_pic: raw.attached_pic != 0,
        },
        None => Disposition::default(),
    }
}

fn parse_optional_timestamp(
    value: Option<&Scalar>,
    field: &str,
    path: &Path,
) -> Result<Option<Timestamp>, ProbeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value.text();
    let text = text.trim();
    if text.is_empty() || is_unavailable(text) {
        return Ok(None);
    }

    Timestamp::from_seconds(text).map(Some).ok_or_else(|| {
        ProbeError::invalid_data(path, format!("{field} is not a valid timestamp: {text}"))
    })
}

fn parse_optional_duration(
    value: Option<&Scalar>,
    field: &str,
    path: &Path,
) -> Result<Option<Duration>, ProbeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value.text();
    let text = text.trim();
    if text.is_empty() || is_unavailable(text) {
        return Ok(None);
    }

    Duration::from_seconds(text).map(Some).ok_or_else(|| {
        ProbeError::invalid_data(path, format!("{field} is not a valid duration: {text}"))
    })
}

fn parse_optional_u64(
    value: Option<&Scalar>,
    field: &str,
    path: &Path,
) -> Result<Option<u64>, ProbeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value.text();
    let text = text.trim();
    if text.is_empty() || is_unavailable(text) {
        return Ok(None);
    }

    text.parse::<u64>().map(Some).map_err(|_| {
        ProbeError::invalid_data(path, format!("{field} is not an unsigned integer: {text}"))
    })
}

fn parse_optional_u32(
    value: Option<&Scalar>,
    field: &str,
    path: &Path,
) -> Result<Option<u32>, ProbeError> {
    match parse_optional_u64(value, field, path)? {
        Some(number) => u32::try_from(number).map(Some).map_err(|_| {
            ProbeError::invalid_data(path, format!("{field} is out of range: {number}"))
        }),
        None => Ok(None),
    }
}

fn parse_optional_time_base(
    value: Option<&str>,
    field: &str,
    path: &Path,
) -> Result<Option<TimeBase>, ProbeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || is_unavailable(value) {
        return Ok(None);
    }

    TimeBase::parse(value).map(Some).ok_or_else(|| {
        ProbeError::invalid_data(path, format!("{field} is not a valid time base: {value}"))
    })
}

fn parse_optional_frame_rate(
    value: Option<&str>,
    field: &str,
    path: &Path,
) -> Result<Option<FrameRate>, ProbeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || is_unavailable(value) || value == "0/0" {
        return Ok(None);
    }

    FrameRate::parse(value).map(Some).ok_or_else(|| {
        ProbeError::invalid_data(path, format!("{field} is not a valid frame rate: {value}"))
    })
}

fn is_unavailable(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("N/A")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"
    {
      "format": {
        "format_name": "matroska,webm",
        "format_long_name": "Matroska / WebM",
        "duration": "2.000000",
        "size": "12345",
        "bit_rate": "49380",
        "start_time": "0.000000"
      },
      "streams": [
        {
          "index": 0,
          "codec_type": "video",
          "codec_name": "h264",
          "codec_long_name": "H.264 / AVC",
          "profile": "High",
          "width": 160,
          "height": 120,
          "pix_fmt": "yuv420p",
          "r_frame_rate": "10/1",
          "avg_frame_rate": "10/1",
          "time_base": "1/1000",
          "start_time": "0.000000",
          "duration": "2.000000",
          "tags": { "language": "und", "title": "Main" },
          "disposition": { "default": 1 }
        },
        {
          "index": 1,
          "codec_type": "audio",
          "codec_name": "aac",
          "sample_rate": "44100",
          "channels": 1,
          "channel_layout": "mono",
          "sample_fmt": "fltp",
          "bit_rate": "32000",
          "time_base": "1/1000",
          "start_time": "0.000000",
          "duration": "N/A",
          "tags": { "language": "eng" },
          "disposition": { "default": 1 }
        },
        {
          "index": 2,
          "codec_type": "subtitle",
          "codec_name": "subrip",
          "time_base": "1/1000",
          "start_time": "0.000000",
          "duration": "1.900000",
          "tags": { "language": "eng" }
        },
        {
          "index": 3,
          "codec_type": "data",
          "codec_name": "bin_data",
          "time_base": "1/1000"
        }
      ],
      "chapters": [
        {
          "id": 1,
          "time_base": "1/1000",
          "start": 0,
          "end": 1000,
          "tags": { "title": "Opening" }
        }
      ]
    }
    "#;

    fn sample_media() -> MediaFile {
        match parse_output(SAMPLE_JSON, Path::new("fixture.mkv")) {
            Ok(media) => media,
            Err(error) => panic!("sample JSON should parse: {error}"),
        }
    }

    #[test]
    fn maps_format_and_chapters() {
        let media = sample_media();

        assert_eq!(media.format.name, "matroska,webm");
        assert_eq!(media.format.long_name.as_deref(), Some("Matroska / WebM"));
        assert_eq!(media.format.size, Some(12_345));
        assert_eq!(media.format.bit_rate, Some(49_380));
        assert_eq!(media.duration, Some(Duration::from_micros(2_000_000)));
        assert_eq!(media.chapters.len(), 1);
        assert_eq!(media.chapters[0].id, 1);
        assert_eq!(media.chapters[0].start, Timestamp::ZERO);
        assert_eq!(media.chapters[0].end, Timestamp::from_micros(1_000_000));
        assert_eq!(media.chapters[0].title.as_deref(), Some("Opening"));
    }

    #[test]
    fn maps_stream_kinds_and_shared_fields() {
        let media = sample_media();
        assert_eq!(media.streams.len(), 4);

        match &media.streams[0] {
            MediaStream::Video(video) => {
                assert_eq!(video.info.index, 0);
                assert_eq!(video.info.codec_name.as_deref(), Some("h264"));
                assert_eq!(video.info.language.as_deref(), Some("und"));
                assert!(video.info.disposition.default);
                assert_eq!(video.width, Some(160));
                assert_eq!(video.height, Some(120));
                assert_eq!(video.pixel_format.as_deref(), Some("yuv420p"));
                assert_eq!(
                    video.frame_rate.map(|rate| rate.to_string()).as_deref(),
                    Some("10/1")
                );
            }
            other => panic!("expected a video stream, got {other:?}"),
        }

        match &media.streams[1] {
            MediaStream::Audio(audio) => {
                assert_eq!(audio.info.codec_name.as_deref(), Some("aac"));
                assert_eq!(audio.sample_rate, Some(44_100));
                assert_eq!(audio.channels, Some(1));
                assert_eq!(audio.channel_layout.as_deref(), Some("mono"));
                assert_eq!(audio.info.duration, None);
            }
            other => panic!("expected an audio stream, got {other:?}"),
        }

        match &media.streams[2] {
            MediaStream::Subtitle(subtitle) => {
                assert_eq!(subtitle.info.codec_name.as_deref(), Some("subrip"));
                assert_eq!(subtitle.info.language.as_deref(), Some("eng"));
            }
            other => panic!("expected a subtitle stream, got {other:?}"),
        }

        assert!(matches!(media.streams[3], MediaStream::Other(_)));
    }

    #[test]
    fn rejects_missing_format_section() {
        let json = r#"{ "streams": [], "chapters": [] }"#;

        match parse_output(json, Path::new("broken.mp4")) {
            Err(ProbeError::InvalidData { detail, .. }) => {
                assert!(detail.contains("format"));
            }
            other => panic!("expected invalid data, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_duration() {
        let json = r#"
        {
          "format": { "format_name": "mp4", "duration": "not-a-number" },
          "streams": [],
          "chapters": []
        }
        "#;

        match parse_output(json, Path::new("broken.mp4")) {
            Err(ProbeError::InvalidData { detail, .. }) => {
                assert!(detail.contains("format.duration"));
            }
            other => panic!("expected invalid data, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_json() {
        match parse_output("not json", Path::new("broken.mp4")) {
            Err(ProbeError::Json { .. }) => {}
            other => panic!("expected a JSON error, got {other:?}"),
        }
    }
}

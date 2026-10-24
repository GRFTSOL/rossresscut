//! Raw FFprobe JSON representation.
//!
//! These types are private to the FFprobe parsing boundary. JSON values never
//! escape into the rest of the application.

use serde::Deserialize;

/// Top-level `ffprobe -print_format json` output.
#[derive(Debug, Deserialize)]
pub(super) struct ProbeOutput {
    pub format: Option<RawFormat>,
    #[serde(default)]
    pub streams: Vec<RawStream>,
    #[serde(default)]
    pub chapters: Vec<RawChapter>,
}

/// A JSON scalar that FFprobe emits either as a string or as a number.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Scalar {
    String(String),
    Number(serde_json::Number),
}

impl Scalar {
    pub(super) fn text(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::String(value) => std::borrow::Cow::Borrowed(value),
            Self::Number(value) => std::borrow::Cow::Owned(value.to_string()),
        }
    }
}

/// Container section of the FFprobe output.
#[derive(Debug, Deserialize)]
pub(super) struct RawFormat {
    pub format_name: Option<String>,
    pub format_long_name: Option<String>,
    pub duration: Option<Scalar>,
    pub size: Option<Scalar>,
    pub bit_rate: Option<Scalar>,
    pub start_time: Option<Scalar>,
}

/// Stream section of the FFprobe output.
#[derive(Debug, Deserialize)]
pub(super) struct RawStream {
    pub index: Option<i64>,
    pub codec_type: Option<String>,
    pub codec_name: Option<String>,
    pub codec_long_name: Option<String>,
    pub profile: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub pix_fmt: Option<String>,
    pub r_frame_rate: Option<String>,
    pub avg_frame_rate: Option<String>,
    pub sample_rate: Option<Scalar>,
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
    pub sample_fmt: Option<String>,
    pub bit_rate: Option<Scalar>,
    pub time_base: Option<String>,
    pub start_time: Option<Scalar>,
    pub duration: Option<Scalar>,
    pub color_range: Option<String>,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub disposition: Option<RawDisposition>,
    pub tags: Option<RawTags>,
}

/// Stream disposition flags as emitted by FFprobe (zero or one).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct RawDisposition {
    pub default: i64,
    pub forced: i64,
    pub hearing_impaired: i64,
    pub visual_impaired: i64,
    pub attached_pic: i64,
}

/// Stream and chapter tags used by the media model.
#[derive(Debug, Deserialize)]
pub(super) struct RawTags {
    pub language: Option<String>,
    pub title: Option<String>,
}

/// Chapter section of the FFprobe output.
///
/// Chapter times are integer ticks in `time_base`, not decimal seconds.
#[derive(Debug, Deserialize)]
pub(super) struct RawChapter {
    pub id: Option<i64>,
    pub time_base: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub tags: Option<RawTags>,
}

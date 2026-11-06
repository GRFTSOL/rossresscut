//! Media data model produced from FFprobe metadata.
//!
//! These types are independent of FFprobe's JSON shape and of the UI. Fields
//! that FFprobe does not provide are omitted rather than synthesized.

use std::path::PathBuf;

use serde::Serialize;

use super::time::{Duration, FrameRate, TimeBase, Timestamp};

/// A media file and the metadata known about it.
#[derive(Clone, Debug, Serialize)]
pub struct MediaFile {
    /// Path that was probed.
    pub path: PathBuf,
    /// Container-level information.
    pub format: MediaFormat,
    /// Container duration, when FFprobe reports one.
    pub duration: Option<Duration>,
    /// Container start time, when FFprobe reports one.
    pub start_time: Option<Timestamp>,
    /// Streams in FFprobe order.
    pub streams: Vec<MediaStream>,
    /// Chapters in container order.
    pub chapters: Vec<Chapter>,
}

/// Container-level media information.
#[derive(Clone, Debug, Serialize)]
pub struct MediaFormat {
    /// Short container name, for example `mov,mp4,m4a,3gp,3g2,mj2`.
    pub name: String,
    /// Human-readable container name.
    pub long_name: Option<String>,
    /// Container duration.
    pub duration: Option<Duration>,
    /// Container size in bytes.
    pub size: Option<u64>,
    /// Overall bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// Container start time.
    pub start_time: Option<Timestamp>,
}

/// Fields shared by every media stream.
#[derive(Clone, Debug, Serialize)]
pub struct StreamInfo {
    /// Zero-based stream index as reported by FFprobe.
    pub index: usize,
    /// Short codec name, for example `h264` or `aac`.
    pub codec_name: Option<String>,
    /// Human-readable codec name.
    pub codec_long_name: Option<String>,
    /// Codec profile, for example `Constrained Baseline`.
    pub profile: Option<String>,
    /// Language tag from stream metadata.
    pub language: Option<String>,
    /// Title from stream metadata.
    pub title: Option<String>,
    /// Stream disposition flags.
    pub disposition: Disposition,
    /// Stream bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// Time base used by stream timestamps.
    pub time_base: Option<TimeBase>,
    /// Stream start time.
    pub start_time: Option<Timestamp>,
    /// Stream duration.
    pub duration: Option<Duration>,
}

/// A stream in the container, categorized by its media type.
#[derive(Clone, Debug, Serialize)]
pub enum MediaStream {
    /// A video stream.
    Video(VideoStream),
    /// An audio stream.
    Audio(AudioStream),
    /// A subtitle stream.
    Subtitle(SubtitleStream),
    /// Any stream type the application does not model yet.
    Other(StreamInfo),
}

impl MediaStream {
    /// Returns the fields shared by all stream kinds.
    pub fn info(&self) -> &StreamInfo {
        match self {
            Self::Video(stream) => &stream.info,
            Self::Audio(stream) => &stream.info,
            Self::Subtitle(stream) => &stream.info,
            Self::Other(info) => info,
        }
    }
}

/// A video stream.
#[derive(Clone, Debug, Serialize)]
pub struct VideoStream {
    /// Fields shared by all streams.
    pub info: StreamInfo,
    /// Frame width in pixels.
    pub width: Option<u32>,
    /// Frame height in pixels.
    pub height: Option<u32>,
    /// Pixel format, for example `yuv420p`.
    pub pixel_format: Option<String>,
    /// Nominal frame rate reported by FFprobe.
    pub frame_rate: Option<FrameRate>,
    /// Average frame rate reported by FFprobe.
    pub average_frame_rate: Option<FrameRate>,
    /// Number of B-frames reported by FFprobe, used when re-encoding the head.
    pub has_b_frames: Option<u32>,
    /// Color range, for example `tv` or `pc`.
    pub color_range: Option<String>,
    /// Color space, for example `bt709`.
    pub color_space: Option<String>,
    /// Color transfer characteristics.
    pub color_transfer: Option<String>,
    /// Color primaries.
    pub color_primaries: Option<String>,
    /// HDR10 mastering display metadata, in x265 `master-display` syntax.
    pub mastering_display: Option<String>,
    /// HDR10 content light level metadata, in x265 `max-cll` syntax.
    pub content_light: Option<String>,
}

/// An audio stream.
#[derive(Clone, Debug, Serialize)]
pub struct AudioStream {
    /// Fields shared by all streams.
    pub info: StreamInfo,
    /// Sample rate in hertz.
    pub sample_rate: Option<u32>,
    /// Number of audio channels.
    pub channels: Option<u32>,
    /// Channel layout, for example `mono` or `stereo`.
    pub channel_layout: Option<String>,
    /// Sample format, for example `fltp`.
    pub sample_format: Option<String>,
}

/// A subtitle stream.
#[derive(Clone, Debug, Serialize)]
pub struct SubtitleStream {
    /// Fields shared by all streams.
    pub info: StreamInfo,
}

/// Stream disposition flags that affect playback and editing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Disposition {
    /// Stream is selected by default.
    pub default: bool,
    /// Stream is forced.
    pub forced: bool,
    /// Stream targets hearing-impaired viewers.
    pub hearing_impaired: bool,
    /// Stream targets visually-impaired viewers.
    pub visual_impaired: bool,
    /// Stream is cover art rather than playable video.
    pub attached_pic: bool,
}

/// A chapter mark in the container.
#[derive(Clone, Debug, Serialize)]
pub struct Chapter {
    /// Chapter identifier reported by FFprobe.
    pub id: i64,
    /// Inclusive start of the chapter.
    pub start: Timestamp,
    /// Exclusive end of the chapter.
    pub end: Timestamp,
    /// Chapter title.
    pub title: Option<String>,
}

/// Keyframe timestamps for one video stream, ordered and normalized to the
/// media timeline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct KeyframeIndex {
    times: Vec<Timestamp>,
}

impl KeyframeIndex {
    /// Builds an index from timestamps in any order, sorting and deduplicating
    /// them.
    pub fn new(mut times: Vec<Timestamp>) -> Self {
        times.sort_unstable();
        times.dedup();
        Self { times }
    }

    /// Returns the keyframes in ascending order.
    pub fn times(&self) -> &[Timestamp] {
        &self.times
    }

    /// Returns the number of known keyframes.
    pub fn len(&self) -> usize {
        self.times.len()
    }

    /// Returns whether no keyframes are known.
    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    /// Returns the latest keyframe strictly before `position`.
    pub fn previous(&self, position: Timestamp) -> Option<Timestamp> {
        let index = self.times.partition_point(|time| *time < position);
        self.times.get(index.checked_sub(1)?).copied()
    }

    /// Returns the earliest keyframe at or after `position`.
    pub fn at_or_after(&self, position: Timestamp) -> Option<Timestamp> {
        let index = self.times.partition_point(|time| *time < position);
        self.times.get(index).copied()
    }

    /// Returns the earliest keyframe strictly after `position`.
    pub fn next(&self, position: Timestamp) -> Option<Timestamp> {
        let index = self.times.partition_point(|time| *time <= position);
        self.times.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> KeyframeIndex {
        KeyframeIndex::new(vec![
            Timestamp::from_micros(2_000_000),
            Timestamp::ZERO,
            Timestamp::from_micros(1_000_000),
            Timestamp::from_micros(1_000_000),
        ])
    }

    #[test]
    fn sorts_and_deduplicates_keyframes() {
        let index = index();

        assert_eq!(index.len(), 3);
        assert_eq!(
            index.times(),
            &[
                Timestamp::ZERO,
                Timestamp::from_micros(1_000_000),
                Timestamp::from_micros(2_000_000),
            ]
        );
    }

    #[test]
    fn navigates_strictly_between_keyframes() {
        let index = index();

        assert_eq!(index.previous(Timestamp::ZERO), None);
        assert_eq!(
            index.previous(Timestamp::from_micros(1_500_000)),
            Some(Timestamp::from_micros(1_000_000))
        );
        assert_eq!(
            index.previous(Timestamp::from_micros(1_000_000)),
            Some(Timestamp::ZERO)
        );
        assert_eq!(
            index.next(Timestamp::ZERO),
            Some(Timestamp::from_micros(1_000_000))
        );
        assert_eq!(
            index.next(Timestamp::from_micros(1_000_000)),
            Some(Timestamp::from_micros(2_000_000))
        );
        assert_eq!(index.next(Timestamp::from_micros(2_000_000)), None);
    }

    #[test]
    fn empty_index_has_no_navigation_targets() {
        let index = KeyframeIndex::default();

        assert!(index.is_empty());
        assert_eq!(index.previous(Timestamp::ZERO), None);
        assert_eq!(index.next(Timestamp::ZERO), None);
    }
}

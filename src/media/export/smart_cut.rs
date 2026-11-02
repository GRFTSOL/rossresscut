//! Planning for smart-cut segments.
//!
//! A smart cut re-encodes only the part between the requested start and the
//! next keyframe, then stream-copies the rest. This module decides how one
//! requested range is split; it does not run FFmpeg.
//!
//! See `docs/decisions/0013-smart-cut.md` for the failure semantics.

use std::fmt;

use crate::domain::media::KeyframeIndex;
use crate::domain::time::{Duration, Timestamp};

/// One sub-range of a smart-cut segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartCutPiece {
    start: Timestamp,
    end: Timestamp,
    encode: bool,
}

impl SmartCutPiece {
    /// Returns the source start of this piece.
    pub const fn start(self) -> Timestamp {
        self.start
    }

    /// Returns the exclusive source end of this piece.
    pub const fn end(self) -> Timestamp {
        self.end
    }

    /// Returns whether the video stream must be re-encoded.
    pub const fn encode(self) -> bool {
        self.encode
    }

    /// Returns the requested length.
    pub fn duration(self) -> Duration {
        self.end.duration_since(self.start).unwrap_or_default()
    }
}

/// Why a segment cannot be smart cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartCutUnavailable {
    /// No keyframe exists at or after the requested start.
    NoKeyframeAfterStart,
    /// The frame rate is unknown, so a one-frame gap cannot be computed.
    UnknownFrameRate,
}

impl fmt::Display for SmartCutUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKeyframeAfterStart => {
                formatter.write_str("no keyframe exists after the segment start")
            }
            Self::UnknownFrameRate => formatter.write_str("the video frame rate is unknown"),
        }
    }
}

impl std::error::Error for SmartCutUnavailable {}

/// The pieces that make up one requested segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartCutPlan {
    pieces: Vec<SmartCutPiece>,
}

impl SmartCutPlan {
    /// Returns a plan that stream copies the whole range.
    pub fn copy_only(start: Timestamp, end: Timestamp) -> Self {
        Self {
            pieces: vec![SmartCutPiece {
                start,
                end,
                encode: false,
            }],
        }
    }

    /// Splits `[start, end)` into a re-encoded head and a copied remainder.
    ///
    /// Returns one copied piece when `start` already is a keyframe, and one
    /// encoded piece when the whole range sits between two keyframes.
    pub fn plan(
        start: Timestamp,
        end: Timestamp,
        keyframes: &KeyframeIndex,
        frame_duration: Option<Duration>,
    ) -> Result<Self, SmartCutUnavailable> {
        if start >= end {
            return Ok(Self {
                pieces: vec![SmartCutPiece {
                    start,
                    end,
                    encode: false,
                }],
            });
        }

        let next = keyframes
            .at_or_after(start)
            .ok_or(SmartCutUnavailable::NoKeyframeAfterStart)?;

        if next == start {
            return Ok(Self {
                pieces: vec![SmartCutPiece {
                    start,
                    end,
                    encode: false,
                }],
            });
        }

        if next >= end {
            return Ok(Self {
                pieces: vec![SmartCutPiece {
                    start,
                    end,
                    encode: true,
                }],
            });
        }

        let frame = frame_duration.ok_or(SmartCutUnavailable::UnknownFrameRate)?;
        let head_end = head_end(start, next, frame);
        Ok(Self {
            pieces: vec![
                SmartCutPiece {
                    start,
                    end: head_end,
                    encode: true,
                },
                SmartCutPiece {
                    start: next,
                    end,
                    encode: false,
                },
            ],
        })
    }

    /// Returns whether every piece can be stream copied.
    pub fn is_copy_only(&self) -> bool {
        self.pieces.iter().all(|piece| !piece.encode)
    }

    /// Returns the pieces in output order.
    pub fn pieces(&self) -> &[SmartCutPiece] {
        &self.pieces
    }

    /// Returns the sum of the piece lengths.
    pub fn total_duration(&self) -> Duration {
        self.pieces.iter().fold(Duration::ZERO, |total, piece| {
            total.checked_add(piece.duration()).unwrap_or(total)
        })
    }
}

/// Ends the encoded head one frame before the keyframe, but keeps at least one
/// frame. This avoids a duplicated frame at the join.
fn head_end(start: Timestamp, next: Timestamp, frame: Duration) -> Timestamp {
    let earliest = start.checked_add(frame).unwrap_or(start);
    let latest = next.checked_sub(frame).unwrap_or(start);
    earliest.max(latest).min(next)
}

/// Returns the software encoder matching a source video codec name.
///
/// Hardware encoders are deliberately not used: exporting stays reproducible
/// and independent of the local GPU.
pub fn encoder_for(codec: &str) -> Option<&'static str> {
    Some(match codec {
        "h264" => "libx264",
        "hevc" => "libx265",
        "av1" => "libsvtav1",
        "vp9" => "libvpx-vp9",
        "vp8" => "libvpx",
        "prores" => "prores_ks",
        "mpeg4" => "mpeg4",
        "mpeg2video" => "mpeg2video",
        "h263" => "h263",
        "flv1" => "flv",
        "mjpeg" => "mjpeg",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyframes(times: &[i64]) -> KeyframeIndex {
        KeyframeIndex::new(times.iter().copied().map(Timestamp::from_micros).collect())
    }

    fn frame() -> Duration {
        Duration::from_micros(33_333)
    }

    fn plan(
        start: i64,
        end: i64,
        keyframes: &KeyframeIndex,
    ) -> Result<SmartCutPlan, SmartCutUnavailable> {
        SmartCutPlan::plan(
            Timestamp::from_micros(start),
            Timestamp::from_micros(end),
            keyframes,
            Some(frame()),
        )
    }

    #[test]
    fn start_on_a_keyframe_is_a_plain_copy() {
        let keyframes = keyframes(&[0, 2_000_000, 4_000_000]);
        let plan = plan(2_000_000, 3_000_000, &keyframes).expect("planned");

        assert!(plan.is_copy_only());
        assert_eq!(plan.pieces().len(), 1);
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(2_000_000));
        assert!(!plan.pieces()[0].encode());
    }

    #[test]
    fn start_before_a_keyframe_encodes_the_head() {
        let keyframes = keyframes(&[0, 6_000_000]);
        let plan = plan(5_500_000, 9_000_000, &keyframes).expect("planned");

        assert!(!plan.is_copy_only());
        assert_eq!(plan.pieces().len(), 2);
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(5_500_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(5_966_667));
        assert!(plan.pieces()[0].encode());
        assert_eq!(plan.pieces()[1].start(), Timestamp::from_micros(6_000_000));
        assert_eq!(plan.pieces()[1].end(), Timestamp::from_micros(9_000_000));
        assert!(!plan.pieces()[1].encode());
    }

    #[test]
    fn a_range_inside_one_gop_is_encoded_whole() {
        let keyframes = keyframes(&[0, 6_000_000]);
        let plan = plan(5_100_000, 5_500_000, &keyframes).expect("planned");

        assert_eq!(plan.pieces().len(), 1);
        assert!(plan.pieces()[0].encode());
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(5_100_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(5_500_000));
    }

    #[test]
    fn encodes_one_frame_when_the_keyframe_is_immediately_next() {
        let keyframes = keyframes(&[0, 5_533_333]);
        let plan = plan(5_500_000, 9_000_000, &keyframes).expect("planned");

        assert_eq!(plan.pieces().len(), 2);
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(5_533_333));
        assert_eq!(plan.pieces()[1].start(), Timestamp::from_micros(5_533_333));
    }

    #[test]
    fn reports_a_missing_keyframe() {
        let keyframes = keyframes(&[0, 1_000_000]);
        assert_eq!(
            plan(2_000_000, 3_000_000, &keyframes),
            Err(SmartCutUnavailable::NoKeyframeAfterStart)
        );
    }

    #[test]
    fn requires_a_frame_duration_when_splitting() {
        let keyframes = keyframes(&[0, 6_000_000]);
        assert_eq!(
            SmartCutPlan::plan(
                Timestamp::from_micros(5_500_000),
                Timestamp::from_micros(9_000_000),
                &keyframes,
                None,
            ),
            Err(SmartCutUnavailable::UnknownFrameRate)
        );
    }

    #[test]
    fn maps_common_codecs_to_software_encoders() {
        assert_eq!(encoder_for("h264"), Some("libx264"));
        assert_eq!(encoder_for("hevc"), Some("libx265"));
        assert_eq!(encoder_for("av1"), Some("libsvtav1"));
        assert_eq!(encoder_for("prores"), Some("prores_ks"));
        assert_eq!(encoder_for("unknown-codec"), None);
    }
}

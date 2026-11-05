//! Planning for smart-cut segments.
//!
//! A smart cut re-encodes the part between the requested start and the next
//! keyframe, and the last GOP before the requested end, then stream-copies
//! everything in between. Both boundaries therefore land on the requested
//! frame. This module decides how one requested range is split; it does not
//! run FFmpeg.
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
    seek: Timestamp,
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

    /// Returns the `-ss` target that makes the piece start at [`Self::start`].
    ///
    /// Encoded pieces seek to their own start. Copy pieces may need a later
    /// target because a container can only seek to sparse cue points.
    pub const fn seek(self) -> Timestamp {
        self.seek
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

/// Where a stream-copy piece can start and the seek target that reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyBoundary {
    /// Content start of the copy piece on the source timeline.
    pub start: Timestamp,
    /// `-ss` target that makes the demuxer land on `start`.
    pub seek: Timestamp,
}

/// The pieces that make up one requested segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartCutPlan {
    pieces: Vec<SmartCutPiece>,
    /// Source average bit rate for each piece, when probed.
    bit_rates: Vec<Option<u64>>,
}

impl SmartCutPlan {
    /// Returns a plan that stream copies the whole range.
    pub fn copy_only(start: Timestamp, end: Timestamp) -> Self {
        Self {
            pieces: vec![SmartCutPiece {
                start,
                end,
                seek: start,
                encode: false,
            }],
            bit_rates: vec![None],
        }
    }

    /// Splits `[start, end)` into re-encoded head/tail pieces and a copied
    /// middle.
    ///
    /// This assumes the first keyframe is a valid seek target. Use
    /// [`Self::plan_with_seek`] when the container may only seek to an earlier
    /// point.
    pub fn plan(
        start: Timestamp,
        end: Timestamp,
        keyframes: &KeyframeIndex,
        frame_duration: Option<Duration>,
    ) -> Result<Self, SmartCutUnavailable> {
        if start >= end {
            return Ok(Self::copy_only(start, end));
        }

        let next = keyframes
            .at_or_after(start)
            .ok_or(SmartCutUnavailable::NoKeyframeAfterStart)?;
        let copy = CopyBoundary {
            start: next,
            seek: next,
        };
        Self::plan_with_seek(start, end, copy, keyframes, frame_duration)
    }

    /// Splits `[start, end)` using a probed copy boundary.
    ///
    /// `copy` says where the copy piece really begins and which `-ss` value
    /// reaches it. Both boundaries of the requested range are made
    /// frame-exact: the head covers `[start, copy.start)` and the tail covers
    /// the last GOP before `end`. A range without a copyable section between
    /// them is encoded whole.
    pub fn plan_with_seek(
        start: Timestamp,
        end: Timestamp,
        copy: CopyBoundary,
        keyframes: &KeyframeIndex,
        frame_duration: Option<Duration>,
    ) -> Result<Self, SmartCutUnavailable> {
        Self::plan_with_frame_durations(
            start,
            end,
            copy,
            keyframes,
            frame_duration,
            frame_duration,
            2,
        )
    }

    /// Splits `[start, end)` with separate head and tail frame durations.
    ///
    /// VFR sources can have different local frame durations at the two
    /// boundaries. The head uses one frame of `head_frame`, while the copy tail
    /// ends `tail_margin_frames` units of `tail_frame` before the tail keyframe.
    pub fn plan_with_frame_durations(
        start: Timestamp,
        end: Timestamp,
        copy: CopyBoundary,
        keyframes: &KeyframeIndex,
        head_frame: Option<Duration>,
        tail_frame: Option<Duration>,
        tail_margin_frames: u32,
    ) -> Result<Self, SmartCutUnavailable> {
        if start >= end {
            return Ok(Self::copy_only(start, end));
        }
        if copy.start < start || copy.start >= end {
            return Ok(Self::encoded(start, end));
        }

        let head_frame = head_frame
            .or(tail_frame)
            .ok_or(SmartCutUnavailable::UnknownFrameRate)?;
        let tail_frame = tail_frame.unwrap_or(head_frame);

        // A stream-copy cut can end on the keyframe itself and keep a fringe
        // from the next GOP whose out-of-order B-frames are then missing. End
        // the copy early, then re-encode from the keyframe so the join and the
        // segment end are both exact. Sources without B-frames need only one
        // frame of margin, which also keeps the audio seam tighter.
        let margin = if tail_margin_frames >= 2 {
            tail_frame.checked_add(tail_frame).unwrap_or(tail_frame)
        } else {
            tail_frame
        };
        let anchor = if keyframes.at_or_after(end) == Some(end) {
            end
        } else {
            keyframes.previous(end).unwrap_or(copy.start)
        };
        if anchor <= copy.start {
            return Ok(Self::encoded(start, end));
        }
        let copy_end = if anchor == end {
            // Ending on a keyframe needs no encoded tail; keep the copy exact.
            end
        } else {
            anchor.checked_sub(margin).unwrap_or(start)
        };
        if copy_end <= copy.start || copy.seek >= copy_end {
            return Ok(Self::encoded(start, end));
        }

        let mut pieces = Vec::with_capacity(3);
        if copy.start > start {
            pieces.push(SmartCutPiece {
                start,
                end: head_end(start, copy.start, head_frame),
                seek: start,
                encode: true,
            });
        }
        pieces.push(SmartCutPiece {
            start: copy.start,
            end: copy_end,
            seek: copy.seek,
            encode: false,
        });
        if copy_end < end {
            pieces.push(SmartCutPiece {
                start: copy_end,
                end,
                seek: copy_end,
                encode: true,
            });
        }
        let bit_rates = vec![None; pieces.len()];
        Ok(Self { pieces, bit_rates })
    }

    /// Returns a plan that re-encodes the whole range.
    pub fn encoded(start: Timestamp, end: Timestamp) -> Self {
        Self {
            pieces: vec![SmartCutPiece {
                start,
                end,
                seek: start,
                encode: true,
            }],
            bit_rates: vec![None],
        }
    }

    /// Returns whether every piece can be stream copied.
    pub fn is_copy_only(&self) -> bool {
        self.pieces.iter().all(|piece| !piece.encode)
    }

    /// Returns the pieces in output order.
    pub fn pieces(&self) -> &[SmartCutPiece] {
        &self.pieces
    }

    /// Returns the probed source bit rate of one piece.
    pub fn bit_rate(&self, index: usize) -> Option<u64> {
        self.bit_rates.get(index).copied().flatten()
    }

    /// Stores the probed source bit rates, one per piece.
    pub(super) fn set_bit_rates(&mut self, bit_rates: Vec<Option<u64>>) {
        if bit_rates.len() == self.pieces.len() {
            self.bit_rates = bit_rates;
        }
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
    fn start_and_end_on_keyframes_are_a_plain_copy() {
        let keyframes = keyframes(&[0, 2_000_000, 4_000_000]);
        let plan = plan(2_000_000, 4_000_000, &keyframes).expect("planned");

        assert!(plan.is_copy_only());
        assert_eq!(plan.pieces().len(), 1);
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(2_000_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(4_000_000));
        assert!(!plan.pieces()[0].encode());
    }

    #[test]
    fn start_before_a_keyframe_encodes_the_head() {
        let keyframes = keyframes(&[0, 6_000_000, 9_000_000]);
        let plan = plan(5_500_000, 10_000_000, &keyframes).expect("planned");

        assert!(!plan.is_copy_only());
        assert_eq!(plan.pieces().len(), 3);
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(5_500_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(5_966_667));
        assert!(plan.pieces()[0].encode());
        assert_eq!(plan.pieces()[1].start(), Timestamp::from_micros(6_000_000));
        assert_eq!(plan.pieces()[1].end(), Timestamp::from_micros(8_933_334));
        assert!(!plan.pieces()[1].encode());
        assert_eq!(plan.pieces()[2].start(), Timestamp::from_micros(8_933_334));
        assert_eq!(plan.pieces()[2].end(), Timestamp::from_micros(10_000_000));
        assert!(plan.pieces()[2].encode());
    }

    #[test]
    fn encodes_the_tail_so_the_end_is_frame_exact() {
        let keyframes = keyframes(&[0, 6_000_000, 9_000_000]);
        let plan = plan(6_000_000, 10_000_000, &keyframes).expect("planned");

        assert_eq!(plan.pieces().len(), 2);
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(6_000_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(8_933_334));
        assert!(!plan.pieces()[0].encode());
        assert_eq!(plan.pieces()[1].start(), Timestamp::from_micros(8_933_334));
        assert_eq!(plan.pieces()[1].end(), Timestamp::from_micros(10_000_000));
        assert!(plan.pieces()[1].encode());
    }

    #[test]
    fn a_keyframe_end_keeps_the_copy() {
        let keyframes = keyframes(&[0, 6_000_000, 9_000_000]);
        let plan = plan(6_000_000, 9_000_000, &keyframes).expect("planned");

        assert!(plan.is_copy_only());
        assert_eq!(plan.pieces().len(), 1);
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(6_000_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(9_000_000));
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
    fn a_range_without_a_copyable_section_is_encoded_whole() {
        let keyframes = keyframes(&[0, 6_000_000]);
        // The encoded head and tail would meet at 6.0s without a copy piece.
        let plan = plan(5_500_000, 8_000_000, &keyframes).expect("planned");

        assert_eq!(plan.pieces().len(), 1);
        assert!(plan.pieces()[0].encode());
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(5_500_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(8_000_000));
    }

    #[test]
    fn encodes_one_frame_when_the_keyframe_is_immediately_next() {
        let keyframes = keyframes(&[0, 5_533_333]);
        let plan = plan(5_500_000, 9_000_000, &keyframes).expect("planned");

        // There is no copyable section after the head, so the range is encoded
        // whole instead of splitting it at the keyframe.
        assert_eq!(plan.pieces().len(), 1);
        assert!(plan.pieces()[0].encode());
        assert_eq!(plan.pieces()[0].start(), Timestamp::from_micros(5_500_000));
        assert_eq!(plan.pieces()[0].end(), Timestamp::from_micros(9_000_000));
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

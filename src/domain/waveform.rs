//! Downsampled audio peaks used to draw the timeline waveform.
//!
//! The waveform is derived data: it is never edited and never participates in
//! export. It only describes the amplitude envelope of one audio stream so the
//! timeline can show where silence is.

use crate::domain::time::{Duration, Timestamp};

/// One min/max pair covering a fixed slice of the audio track.
///
/// Values are normalised PCM samples in `i16` range. `min` is always less than
/// or equal to `max`; both are zero for digital silence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peak {
    min: i16,
    max: i16,
}

impl Peak {
    /// Creates a peak from a min/max pair, ordering the two values.
    pub fn new(first: i16, second: i16) -> Self {
        Self {
            min: first.min(second),
            max: first.max(second),
        }
    }

    /// Lowest sample in the slice.
    pub fn min(self) -> i16 {
        self.min
    }

    /// Highest sample in the slice.
    pub fn max(self) -> i16 {
        self.max
    }

    /// Combines two peaks covering adjacent slices.
    pub fn merge(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// Largest absolute amplitude, in `0.0..=1.0`.
    pub fn amplitude(self) -> f32 {
        let largest = self.max.unsigned_abs().max(self.min.unsigned_abs());
        f32::from(largest) / f32::from(i16::MAX)
    }
}

/// Audio peaks for one stream, sampled at a fixed rate.
///
/// The waveform starts at [`Waveform::start`], which is the stream's own start
/// time and can be later than zero. `peaks[i]` covers
/// `start + i * duration / len` up to `start + (i + 1) * duration / len`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waveform {
    start: Timestamp,
    duration: Duration,
    peaks: Vec<Peak>,
}

impl Waveform {
    /// Builds a waveform, or returns `None` for empty input.
    pub fn new(start: Timestamp, duration: Duration, peaks: Vec<Peak>) -> Option<Self> {
        if peaks.is_empty() || duration.micros() == 0 {
            return None;
        }
        Some(Self {
            start,
            duration,
            peaks,
        })
    }

    /// Source time where the first peak begins.
    pub fn start(&self) -> Timestamp {
        self.start
    }

    /// Source time where the last peak ends.
    pub fn end(&self) -> Timestamp {
        self.start
            .checked_add(self.duration)
            .unwrap_or(Timestamp::from_micros(i64::MAX))
    }

    /// Time span the peaks cover.
    ///
    /// This is the decoded audio length, which can be shorter than the
    /// container duration when the audio stream ends early.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Raw peaks at the extraction resolution.
    pub fn peaks(&self) -> &[Peak] {
        &self.peaks
    }

    /// Number of peaks stored.
    pub fn len(&self) -> usize {
        self.peaks.len()
    }

    /// Returns whether no peaks are stored.
    pub fn is_empty(&self) -> bool {
        self.peaks.is_empty()
    }

    /// Aggregates the peaks into exactly `width` columns.
    ///
    /// Every column spans a contiguous slice of `peaks`, so a narrow or a wide
    /// timeline both preserve silence gaps. Columns are returned in time order.
    pub fn columns(&self, width: usize) -> Vec<Peak> {
        let total = self.peaks.len();
        if width == 0 || total == 0 {
            return Vec::new();
        }

        let mut columns = Vec::with_capacity(width);
        for column in 0..width {
            let start = column * total / width;
            let end = (((column + 1) * total / width).max(start + 1)).min(total);
            let mut peak = self.peaks[start];
            for next in &self.peaks[start + 1..end] {
                peak = peak.merge(*next);
            }
            columns.push(peak);
        }
        columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waveform(values: &[i16]) -> Waveform {
        let peaks = values
            .iter()
            .map(|value| Peak::new(-value.abs(), *value))
            .collect();
        Waveform::new(
            Timestamp::ZERO,
            Duration::from_micros(values.len() as u64 * 5_000),
            peaks,
        )
        .expect("waveform")
    }

    #[test]
    fn rejects_empty_input() {
        assert!(Waveform::new(Timestamp::ZERO, Duration::from_micros(1_000), Vec::new()).is_none());
        assert!(Waveform::new(Timestamp::ZERO, Duration::ZERO, vec![Peak::new(0, 0)]).is_none());
    }

    #[test]
    fn keeps_the_stream_start_time() {
        let start = Timestamp::from_micros(1_500_000);
        let waveform = Waveform::new(
            start,
            Duration::from_micros(2_000_000),
            vec![Peak::new(0, 0)],
        )
        .expect("waveform");

        assert_eq!(waveform.start(), start);
        assert_eq!(waveform.end(), Timestamp::from_micros(3_500_000));
    }

    #[test]
    fn merges_min_and_max() {
        let merged = Peak::new(-3, 5).merge(Peak::new(-7, 2));
        assert_eq!(merged, Peak::new(-7, 5));
    }

    #[test]
    fn amplitude_uses_the_largest_absolute_sample() {
        assert_eq!(Peak::new(-i16::MAX, 10).amplitude(), 1.0);
        assert_eq!(Peak::new(0, 0).amplitude(), 0.0);
    }

    #[test]
    fn columns_preserve_every_slice() {
        let waveform = waveform(&[1, 2, 3, 4, 5, 6, 7, 8]);

        let columns = waveform.columns(2);
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0], Peak::new(-4, 4));
        assert_eq!(columns[1], Peak::new(-8, 8));

        let columns = waveform.columns(8);
        assert_eq!(columns.len(), 8);
        assert_eq!(columns[3], Peak::new(-4, 4));
    }

    #[test]
    fn widening_beyond_the_peak_count_repeats_slices() {
        let waveform = waveform(&[1, 2]);

        let columns = waveform.columns(6);

        assert_eq!(columns.len(), 6);
        assert_eq!(columns[0], Peak::new(-1, 1));
        assert_eq!(columns[5], Peak::new(-2, 2));
    }

    #[test]
    fn zero_width_produces_no_columns() {
        assert!(waveform(&[1, 2]).columns(0).is_empty());
    }
}

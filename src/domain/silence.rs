//! Silence detection over the extracted waveform.
//!
//! Cutting happens at the midpoint of each long enough silent span, so the
//! silent parts are split evenly between the neighbouring audible segments.
//! Ranges that contain no sound at all are dropped.

use crate::domain::time::{Duration, Timestamp};
use crate::domain::waveform::Waveform;

/// Detection parameters chosen by the user.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SilenceOptions {
    /// Peaks at or below this level count as silence, in dBFS.
    pub threshold_db: f32,
    /// Silent spans shorter than this are ignored.
    pub min_duration: Duration,
}

impl Default for SilenceOptions {
    fn default() -> Self {
        Self {
            threshold_db: -60.0,
            min_duration: Duration::from_micros(1_000_000),
        }
    }
}

/// A silent span on the source timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SilenceSpan {
    start: Timestamp,
    end: Timestamp,
}

impl SilenceSpan {
    /// Returns the span start.
    pub fn start(self) -> Timestamp {
        self.start
    }

    /// Returns the span end.
    pub fn end(self) -> Timestamp {
        self.end
    }

    /// Returns the point where the span is split between two segments.
    pub fn midpoint(self) -> Timestamp {
        let micros = i128::from(self.start.micros()) + i128::from(self.end.micros());
        Timestamp::from_micros((micros / 2) as i64)
    }
}

/// Converts a dBFS threshold into a linear amplitude in `0.0..=1.0`.
pub fn threshold_amplitude(threshold_db: f32) -> f32 {
    if threshold_db <= -100.0 {
        return 0.0;
    }
    10f32.powf(threshold_db / 20.0).clamp(0.0, 1.0)
}

/// Returns every silent span at least `min_duration` long.
pub fn detect(waveform: &Waveform, options: SilenceOptions) -> Vec<SilenceSpan> {
    let threshold = threshold_amplitude(options.threshold_db);
    let mut spans = Vec::new();
    let mut silent_start: Option<usize> = None;

    for (index, peak) in waveform.peaks().iter().enumerate() {
        if peak.amplitude() <= threshold {
            silent_start.get_or_insert(index);
        } else if let Some(start) = silent_start.take() {
            push_span(waveform, start, index, options, &mut spans);
        }
    }
    if let Some(start) = silent_start {
        push_span(waveform, start, waveform.len(), options, &mut spans);
    }

    spans
}

fn push_span(
    waveform: &Waveform,
    start_index: usize,
    end_index: usize,
    options: SilenceOptions,
    spans: &mut Vec<SilenceSpan>,
) {
    let start = time_at(waveform, start_index);
    let end = time_at(waveform, end_index);
    let Some(length) = end.duration_since(start) else {
        return;
    };
    if length >= options.min_duration {
        spans.push(SilenceSpan { start, end });
    }
}

/// Maps a peak index onto the source timeline.
fn time_at(waveform: &Waveform, index: usize) -> Timestamp {
    let total = i128::from(waveform.duration().micros());
    let len = waveform.len() as i128;
    let offset = total * index as i128 / len;
    Timestamp::from_micros(waveform.start().micros() + offset as i64)
}

/// Returns the audible ranges of the whole media, in timeline order.
///
/// Boundaries are the media start, the midpoint of every detected silence and
/// the media end. Ranges with no peak above the threshold are dropped, which
/// removes leading and trailing silence.
pub fn audible_ranges(
    waveform: &Waveform,
    duration: Duration,
    options: SilenceOptions,
) -> Vec<(Timestamp, Timestamp)> {
    let threshold = threshold_amplitude(options.threshold_db);
    let spans = detect(waveform, options);

    let mut boundaries = Vec::with_capacity(spans.len() + 2);
    boundaries.push(Timestamp::ZERO);
    boundaries.extend(spans.iter().map(|span| span.midpoint()));
    boundaries.push(Timestamp::from_micros(
        i64::try_from(duration.micros()).unwrap_or(i64::MAX),
    ));
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter(|window| window[1] > window[0])
        .filter(|window| has_audio(waveform, window[0], window[1], threshold))
        .map(|window| (window[0], window[1]))
        .collect()
}

/// Returns whether any peak inside `[start, end)` is above `threshold`.
fn has_audio(waveform: &Waveform, start: Timestamp, end: Timestamp, threshold: f32) -> bool {
    let (first, last) = peak_range(waveform, start, end);
    waveform.peaks()[first..last]
        .iter()
        .any(|peak| peak.amplitude() > threshold)
}

/// Maps a time range onto the inclusive-exclusive peak index range.
fn peak_range(waveform: &Waveform, start: Timestamp, end: Timestamp) -> (usize, usize) {
    let len = waveform.len() as i128;
    let total = i128::from(waveform.duration().micros());
    let base = i128::from(waveform.start().micros());

    let index = |time: Timestamp| -> usize {
        let offset = (i128::from(time.micros()) - base).clamp(0, total);
        ((offset * len) / total).clamp(0, len) as usize
    };

    (index(start), index(end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::waveform::{Peak, Waveform};

    /// Builds a waveform with `loud` peaks followed by `silent` peaks and so on.
    fn build(pattern: &[(&str, usize)]) -> Waveform {
        let mut peaks = Vec::new();
        for (kind, count) in pattern {
            let value = if *kind == "loud" { 20_000 } else { 0 };
            peaks.extend(std::iter::repeat_n(Peak::new(-value, value), *count));
        }
        let duration = Duration::from_micros(peaks.len() as u64 * 5_000);
        Waveform::new(Timestamp::ZERO, duration, peaks).expect("waveform")
    }

    #[test]
    fn threshold_translation_is_exact() {
        assert!((threshold_amplitude(-60.0) - 0.001).abs() < 1e-6);
        assert_eq!(threshold_amplitude(-200.0), 0.0);
        assert_eq!(threshold_amplitude(0.0), 1.0);
    }

    #[test]
    fn finds_a_silent_span_and_ignores_short_gaps() {
        // 200 peaks = 1 s.
        let waveform = build(&[("loud", 800), ("silent", 200), ("loud", 800)]);

        let spans = detect(
            &waveform,
            SilenceOptions {
                threshold_db: -60.0,
                min_duration: Duration::from_micros(500_000),
            },
        );

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start(), Timestamp::from_micros(4_000_000));
        assert_eq!(spans[0].end(), Timestamp::from_micros(5_000_000));
    }

    #[test]
    fn short_silences_are_ignored() {
        let waveform = build(&[("loud", 800), ("silent", 20), ("loud", 800)]);

        let spans = detect(&waveform, SilenceOptions::default());

        assert!(spans.is_empty());
    }

    #[test]
    fn cuts_at_the_silence_midpoint() {
        let waveform = build(&[("loud", 400), ("silent", 400), ("loud", 400)]);
        let duration = waveform.duration();

        let ranges = audible_ranges(&waveform, duration, SilenceOptions::default());

        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].0, Timestamp::ZERO);
        assert_eq!(ranges[0].1, Timestamp::from_micros(3_000_000));
        assert_eq!(ranges[1].0, Timestamp::from_micros(3_000_000));
        assert_eq!(ranges[1].1, Timestamp::from_micros(6_000_000));
    }

    #[test]
    fn drops_leading_and_trailing_silence() {
        let waveform = build(&[("silent", 400), ("loud", 400), ("silent", 400)]);
        let duration = waveform.duration();

        let ranges = audible_ranges(&waveform, duration, SilenceOptions::default());

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].0, Timestamp::from_micros(1_000_000));
        assert_eq!(ranges[0].1, Timestamp::from_micros(5_000_000));
    }

    #[test]
    fn a_fully_silent_track_produces_nothing() {
        let waveform = build(&[("silent", 400)]);

        let ranges = audible_ranges(&waveform, waveform.duration(), SilenceOptions::default());

        assert!(ranges.is_empty());
    }
}

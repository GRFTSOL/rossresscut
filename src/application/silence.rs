//! Automatic segment creation from detected silence.

use std::fmt;

pub use crate::domain::silence::SilenceOptions;

use crate::domain::silence;
use crate::domain::time::Timestamp;

use super::Application;

/// Errors produced while deriving segments from the waveform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SilenceError {
    /// No waveform is available, typically because the media has no audio.
    NoWaveform,
    /// The media duration is unknown.
    NoDuration,
    /// Every detected range was silent.
    NoAudibleRange,
}

impl fmt::Display for SilenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoWaveform => "no audio waveform is available",
            Self::NoDuration => "the media duration is unknown",
            Self::NoAudibleRange => "no audible range was detected",
        })
    }
}

impl std::error::Error for SilenceError {}

impl Application {
    /// Replaces the timeline with segments split at silence midpoints.
    ///
    /// Existing segments are discarded; the previous state stays available
    /// through undo.
    pub fn segments_from_silence(
        &mut self,
        options: SilenceOptions,
    ) -> Result<usize, SilenceError> {
        let ranges = self.silence_ranges(options)?;

        self.checkpoint();
        self.timeline.clear_segments();
        for (start, end) in &ranges {
            if let Err(error) = self.timeline.push(*start, *end) {
                log::warn!("silence detection produced an invalid range: {error}");
            }
        }
        self.selection.clear();
        self.selected_segment = self.timeline.segments().first().map(|segment| segment.id());

        Ok(ranges.len())
    }

    /// Returns how many segments [`Self::segments_from_silence`] would create.
    pub fn count_silence_segments(&self, options: SilenceOptions) -> Result<usize, SilenceError> {
        self.silence_ranges(options).map(|ranges| ranges.len())
    }

    fn silence_ranges(
        &self,
        options: SilenceOptions,
    ) -> Result<Vec<(Timestamp, Timestamp)>, SilenceError> {
        let Some(waveform) = self.waveform.as_ref() else {
            return Err(SilenceError::NoWaveform);
        };
        let Some(duration) = self.media.as_ref().and_then(|media| media.duration) else {
            return Err(SilenceError::NoDuration);
        };

        let ranges = silence::audible_ranges(waveform, duration, options);
        if ranges.is_empty() {
            return Err(SilenceError::NoAudibleRange);
        }
        Ok(ranges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::waveform::{Peak, Waveform};

    fn application_with_waveform(loud: usize, silent: usize) -> Application {
        use crate::domain::segment::Timeline;
        use std::path::PathBuf;

        let mut application = Application::new();
        application.timeline = Timeline::new(PathBuf::from("/media/clip.mp4"));

        let mut peaks = Vec::new();
        peaks.extend(std::iter::repeat_n(Peak::new(-20_000, 20_000), loud));
        peaks.extend(std::iter::repeat_n(Peak::new(0, 0), silent));
        peaks.extend(std::iter::repeat_n(Peak::new(-20_000, 20_000), loud));

        let duration = crate::domain::time::Duration::from_micros(peaks.len() as u64 * 5_000);
        application.waveform = Waveform::new(Timestamp::ZERO, duration, peaks);
        application.media = Some(fake_media(duration));
        application
    }

    fn fake_media(duration: crate::domain::time::Duration) -> crate::domain::media::MediaFile {
        crate::domain::media::MediaFile {
            path: std::path::PathBuf::from("/media/clip.mp4"),
            format: crate::domain::media::MediaFormat {
                name: "mp4".to_owned(),
                long_name: None,
                duration: Some(duration),
                size: None,
                bit_rate: None,
                start_time: None,
            },
            duration: Some(duration),
            start_time: None,
            streams: Vec::new(),
            chapters: Vec::new(),
        }
    }

    #[test]
    fn replaces_segments_with_audible_ranges() {
        let mut application = application_with_waveform(400, 400);

        let count = application
            .segments_from_silence(SilenceOptions::default())
            .expect("segments");

        assert_eq!(count, 2);
        assert_eq!(application.timeline().len(), 2);
        assert_eq!(
            application.timeline().segments()[0].end(),
            Timestamp::from_micros(3_000_000)
        );
    }

    #[test]
    fn requires_a_waveform() {
        let mut application = Application::new();

        let error = application
            .segments_from_silence(SilenceOptions::default())
            .expect_err("no waveform");

        assert_eq!(error, SilenceError::NoWaveform);
    }

    #[test]
    fn reports_an_all_silent_track() {
        let mut application = application_with_waveform(0, 800);

        let error = application
            .segments_from_silence(SilenceOptions::default())
            .expect_err("no audible range");

        assert_eq!(error, SilenceError::NoAudibleRange);
    }
}

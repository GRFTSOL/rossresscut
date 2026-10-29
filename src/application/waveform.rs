//! Background waveform extraction.
//!
//! The waveform is derived data: it is replaced whenever media changes and is
//! dropped when media closes. Decoding always happens on a worker thread.

use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::domain::media::{MediaFile, MediaStream};
use crate::domain::time::Timestamp;
use crate::domain::waveform::Waveform;
use crate::media::waveform;

use super::{Application, WaveformJob};

impl Application {
    /// Returns the waveform for the current audio stream, once extracted.
    pub fn waveform(&self) -> Option<&Waveform> {
        self.waveform.as_ref()
    }

    /// Returns whether the current media has an audio stream.
    ///
    /// The timeline reserves space for the waveform based on this, not on
    /// [`Self::waveform`], so its height does not jump when extraction ends.
    pub fn has_audio(&self) -> bool {
        self.media
            .as_ref()
            .is_some_and(|media| audio_stream(media).is_some())
    }

    /// Starts waveform extraction for `media` on a worker thread.
    pub(super) fn start_waveform(&mut self, media: &MediaFile) {
        self.waveform = None;
        self.waveform_job = None;

        let Some((index, start)) = audio_stream(media) else {
            return;
        };
        let path = media.path.clone();
        let (sender, receiver) = mpsc::channel();

        let spawn_result = thread::Builder::new()
            .name("waveform".to_owned())
            .spawn(move || {
                let result = waveform::extract(&path, index, start);
                if sender.send(result).is_err() {
                    log::debug!(
                        "discarding waveform for {}: the request was superseded",
                        path.display()
                    );
                }
            });

        match spawn_result {
            Ok(_handle) => self.waveform_job = Some(WaveformJob { receiver }),
            Err(error) => log::warn!("failed to start the waveform worker: {error}"),
        }
    }

    /// Polls the waveform worker without blocking.
    pub(super) fn poll_waveform(&mut self) {
        let Some(job) = self.waveform_job.take() else {
            return;
        };

        match job.receiver.try_recv() {
            Ok(Ok(waveform)) => {
                log::info!("extracted {} waveform peaks", waveform.len());
                self.waveform = Some(waveform);
            }
            Ok(Err(error)) => log::warn!("{error}"),
            Err(TryRecvError::Empty) => self.waveform_job = Some(job),
            Err(TryRecvError::Disconnected) => {
                log::warn!("the waveform worker stopped without a result");
            }
        }
    }

    /// Drops any waveform state.
    pub(super) fn reset_waveform(&mut self) {
        self.waveform = None;
        self.waveform_job = None;
    }
}

/// Returns the index and normalized start time of the first audio stream.
///
/// Cover art is skipped: it is carried on an audio stream but has no samples.
/// The start time is relative to the container start, matching the normalized
/// keyframe timeline, so a stream that begins late leaves a gap on the left.
fn audio_stream(media: &MediaFile) -> Option<(usize, Timestamp)> {
    let container_start = media.start_time.map_or(0, Timestamp::micros);
    media.streams.iter().find_map(|stream| match stream {
        MediaStream::Audio(audio) if !audio.info.disposition.attached_pic => {
            let stream_start = audio.info.start_time.map_or(0, Timestamp::micros);
            let start = Timestamp::from_micros((stream_start - container_start).max(0));
            Some((audio.info.index, start))
        }
        _ => None,
    })
}

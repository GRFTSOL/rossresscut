//! Background waveform extraction.
//!
//! The waveform is derived data: it is replaced whenever media changes and is
//! dropped when media closes. Decoding always happens on a worker thread.

use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::domain::media::{MediaFile, MediaStream};
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
            .is_some_and(|media| audio_stream_index(media).is_some())
    }

    /// Starts waveform extraction for `media` on a worker thread.
    pub(super) fn start_waveform(&mut self, media: &MediaFile) {
        self.waveform = None;
        self.waveform_job = None;

        let Some(index) = audio_stream_index(media) else {
            return;
        };
        let path = media.path.clone();
        let (sender, receiver) = mpsc::channel();

        let spawn_result = thread::Builder::new()
            .name("waveform".to_owned())
            .spawn(move || {
                let result = waveform::extract(&path, index);
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

/// Returns the index of the first audio stream that is not cover art.
fn audio_stream_index(media: &MediaFile) -> Option<usize> {
    media.streams.iter().find_map(|stream| match stream {
        MediaStream::Audio(audio) if !audio.info.disposition.attached_pic => Some(audio.info.index),
        _ => None,
    })
}

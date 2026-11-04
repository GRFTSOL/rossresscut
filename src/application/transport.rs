//! Playback transport commands: play, pause, seek, scrub and stepping.

use crate::domain::media::MediaStream;
use crate::domain::time::{Duration, Timestamp};

use super::Application;
use super::playback::clamp_position;

impl Application {
    /// Starts or resumes playback.
    pub fn play(&mut self) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.playback.play(media);
    }

    /// Pauses playback at the current position.
    pub fn pause(&mut self) {
        let duration = self.duration();
        self.playback.pause(duration);
    }

    /// Toggles between play and pause.
    pub fn toggle_playback(&mut self) {
        if self.playback.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Seeks to `position`, clamped to the media bounds.
    pub fn seek(&mut self, position: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        let target = clamp_position(position, media.duration);
        self.playback.seek(target, media);
    }

    /// Requests a transient scrub preview at `position`.
    ///
    /// Scrub previews use a fast keyframe seek and may show a frame from before
    /// `position`. Use [`Self::seek`] for an accurate preview and playback
    /// position.
    pub fn scrub(&mut self, position: Timestamp) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        let target = clamp_position(position, media.duration);
        self.playback.scrub(target, media);
    }

    /// Pauses playback and moves to the previous or next keyframe.
    ///
    /// The keyframe index is built in the background. Until it is available,
    /// this falls back to a one-second step so navigation still responds.
    /// Nothing happens when there is no keyframe in the requested direction.
    pub fn step_keyframe(&mut self, forward: bool) {
        let Some(duration) = self.media.as_ref().map(|media| media.duration) else {
            return;
        };
        let Some(index) = self.keyframes.as_ref() else {
            self.skip(if forward { 1_000_000 } else { -1_000_000 });
            return;
        };

        let position = self.playback.position;
        let target = if forward {
            index.next(position)
        } else {
            index.previous(position)
        };
        let Some(target) = target else {
            return;
        };
        let target = clamp_position(target, duration);

        if self.playback.playing {
            self.pause();
        }
        let Some(media) = self.media.as_ref() else {
            return;
        };
        self.playback.seek(target, media);
    }

    /// Seeks to the start of the media.
    pub fn seek_to_start(&mut self) {
        self.seek(Timestamp::ZERO);
    }

    /// Seeks to the end of the media.
    pub fn seek_to_end(&mut self) {
        if let Some(duration) = self.duration() {
            self.seek(Timestamp::from_micros(
                i64::try_from(duration.micros()).unwrap_or(i64::MAX),
            ));
        }
    }

    /// Seeks to the selected segment's start.
    pub fn seek_to_selected_segment_start(&mut self) {
        let Some(segment) = self
            .selected_segment
            .and_then(|id| self.timeline.get(id).copied())
        else {
            return;
        };
        self.seek(segment.start());
    }

    /// Seeks to the selected segment's end.
    pub fn seek_to_selected_segment_end(&mut self) {
        let Some(segment) = self
            .selected_segment
            .and_then(|id| self.timeline.get(id).copied())
        else {
            return;
        };
        self.seek(segment.end());
    }

    /// Pauses playback and moves by one source frame.
    ///
    /// The mpv backend uses mpv's native `frame-step` / `frame-back-step`, so
    /// the step follows decoded frame boundaries. The external FFmpeg fallback
    /// uses the nominal frame duration, which is approximate for VFR sources.
    pub fn step_frame(&mut self, forward: bool) {
        if self.playback.playing {
            self.pause();
        }
        #[cfg(feature = "mpv-preview")]
        if let Some(mpv) = self.playback.mpv.as_ref() {
            match mpv.frame_step(forward) {
                Ok(()) => return,
                Err(error) => {
                    log::warn!("mpv frame step failed: {error}; using a time-based step");
                }
            }
        }
        let micros = i64::try_from(self.frame_duration().micros()).unwrap_or(33_333);
        self.skip(if forward { micros } else { -micros });
    }

    fn frame_duration(&self) -> Duration {
        let rate = self.media.as_ref().and_then(|media| {
            media.streams.iter().find_map(|stream| match stream {
                MediaStream::Video(video) => video.frame_rate,
                _ => None,
            })
        });
        rate.and_then(|rate| {
            let numerator = u128::from(rate.numerator());
            let denominator = u128::from(rate.denominator());
            let micros = denominator.checked_mul(1_000_000)? / numerator;
            u64::try_from(micros).ok().map(Duration::from_micros)
        })
        .unwrap_or(Duration::from_micros(33_333))
    }

    /// Moves the playback position by `delta_micros` and clamps it to the media.
    pub fn skip(&mut self, delta_micros: i64) {
        let Some(media) = self.media.as_ref() else {
            return;
        };
        let Some(position) = self.playback.position.micros().checked_add(delta_micros) else {
            log::warn!("relative seek overflowed; ignoring it");
            return;
        };
        let target = clamp_position(Timestamp::from_micros(position), media.duration);
        self.playback.seek(target, media);
    }
}

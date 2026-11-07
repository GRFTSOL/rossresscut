//! Background scene-change scanning.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::domain::time::Timestamp;
use crate::media::scene;

use super::{Application, SceneJob};

/// State of the scene scan started from the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneScan {
    /// FFmpeg is still decoding the video; the value is a 0-99 percentage.
    Running(u8),
    /// Detection finished; timestamps are normalized to the media timeline.
    Ready(Vec<Timestamp>),
    /// Detection failed with a user-facing message.
    Failed(String),
}

/// Messages sent from the scene-scan worker.
pub(super) enum SceneEvent {
    /// Decode progress in percent, monotonic and capped at 99.
    Progress(u8),
    /// The worker finished; the result is the detected cuts.
    Finished(Result<Vec<Timestamp>, scene::SceneError>),
}

/// Errors produced when turning scene changes into segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneCutError {
    /// The scan has not finished yet.
    NotReady,
    /// The media has no video stream to scan.
    NoVideo,
    /// The media duration is unknown.
    NoDuration,
    /// No usable scene change was found.
    NoCuts,
}

impl fmt::Display for SceneCutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotReady => "the scene scan is not finished",
            Self::NoVideo => "the media has no video stream",
            Self::NoDuration => "the media duration is unknown",
            Self::NoCuts => "no scene change was detected",
        })
    }
}

impl std::error::Error for SceneCutError {}

impl Application {
    /// Starts a scene scan on a worker thread.
    ///
    /// Returns `false` when there is no video stream to scan.
    pub fn start_scene_scan(&mut self, threshold: f32) -> bool {
        let Some(media) = self.media.as_ref() else {
            return false;
        };
        if !media
            .streams
            .iter()
            .any(|stream| matches!(stream, crate::domain::media::MediaStream::Video(_)))
        {
            return false;
        }

        // Restarting must abort the previous FFmpeg process, not just drop its
        // result channel.
        let path = media.path.clone();
        let start_time = media.start_time;
        let duration = media.duration;
        self.cancel_scene_job();

        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        self.scene_scan = Some(SceneScan::Running(0));
        self.scene_job = None;

        let spawn_result = thread::Builder::new()
            .name("scene-scan".to_owned())
            .spawn(move || {
                let result = scene::detect(
                    &path,
                    threshold,
                    start_time,
                    duration,
                    &worker_cancel,
                    |percent| {
                        let _ = sender.send(SceneEvent::Progress(percent));
                    },
                );
                if !worker_cancel.load(Ordering::Relaxed)
                    && sender.send(SceneEvent::Finished(result)).is_err()
                {
                    log::debug!(
                        "discarding scene scan for {}: the request was superseded",
                        path.display()
                    );
                }
            });

        match spawn_result {
            Ok(_handle) => {
                self.scene_job = Some(SceneJob { receiver, cancel });
                true
            }
            Err(error) => {
                log::warn!("failed to start the scene scan worker: {error}");
                self.scene_scan = Some(SceneScan::Failed(error.to_string()));
                false
            }
        }
    }

    /// Returns the current scan state.
    pub fn scene_scan(&self) -> Option<&SceneScan> {
        self.scene_scan.as_ref()
    }

    /// Drops any scan state and stops the running FFmpeg process.
    pub fn reset_scene_scan(&mut self) {
        self.cancel_scene_scan();
    }

    /// Cancels a running scan and clears its state.
    ///
    /// The worker observes the flag between decoded frames and kills FFmpeg;
    /// dropping only the result channel would leave the process running.
    pub fn cancel_scene_scan(&mut self) {
        self.cancel_scene_job();
        self.scene_scan = None;
    }

    fn cancel_scene_job(&mut self) {
        if let Some(job) = self.scene_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Polls the scan worker without blocking.
    ///
    /// Returns whether the visible scan state changed, so the UI repaints while
    /// the percentage advances.
    pub(super) fn poll_scene_scan(&mut self) -> bool {
        let Some(job) = self.scene_job.take() else {
            return false;
        };
        let mut changed = false;

        loop {
            match job.receiver.try_recv() {
                Ok(SceneEvent::Progress(percent)) => {
                    if let Some(SceneScan::Running(current)) = self.scene_scan.as_mut()
                        && *current != percent
                    {
                        *current = percent;
                        changed = true;
                    }
                }
                Ok(SceneEvent::Finished(Ok(cuts))) => {
                    log::info!("detected {} scene changes", cuts.len());
                    self.scene_scan = Some(SceneScan::Ready(cuts));
                    return true;
                }
                Ok(SceneEvent::Finished(Err(error))) => {
                    log::warn!("{error}");
                    self.scene_scan = Some(SceneScan::Failed(error.to_string()));
                    return true;
                }
                Err(TryRecvError::Empty) => {
                    self.scene_job = Some(job);
                    return changed;
                }
                Err(TryRecvError::Disconnected) => {
                    self.scene_scan = Some(SceneScan::Failed(
                        "the scene scan worker stopped without a result".to_owned(),
                    ));
                    return true;
                }
            }
        }
    }

    /// Replaces the timeline with one segment per detected scene.
    pub fn segments_from_scene_changes(&mut self) -> Result<usize, SceneCutError> {
        let Some(SceneScan::Ready(cuts)) = self.scene_scan.as_ref() else {
            return Err(SceneCutError::NotReady);
        };
        let Some(duration) = self.media.as_ref().and_then(|media| media.duration) else {
            return Err(SceneCutError::NoDuration);
        };
        let end = Timestamp::from_micros(i64::try_from(duration.micros()).unwrap_or(i64::MAX));

        let mut boundaries = vec![Timestamp::ZERO];
        boundaries.extend(
            cuts.iter()
                .copied()
                .filter(|cut| *cut > Timestamp::ZERO && *cut < end),
        );
        boundaries.push(end);
        boundaries.sort_unstable();
        boundaries.dedup();

        let ranges: Vec<_> = boundaries
            .windows(2)
            .filter(|window| window[1] > window[0])
            .map(|window| (window[0], window[1]))
            .collect();
        if ranges.len() < 2 {
            return Err(SceneCutError::NoCuts);
        }

        self.checkpoint();
        self.timeline.clear_segments();
        for (start, finish) in &ranges {
            if let Err(error) = self.timeline.push(*start, *finish) {
                log::warn!("scene detection produced an invalid range: {error}");
            }
        }
        self.selection.clear();
        self.selected_segment = self.timeline.segments().first().map(|segment| segment.id());

        Ok(ranges.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::{MediaFile, MediaFormat};
    use crate::domain::segment::Timeline;
    use crate::domain::time::Duration;
    use std::path::PathBuf;

    fn application_with_video() -> Application {
        let duration = Duration::from_micros(10_000_000);
        let mut application = Application::new();
        application.timeline = Timeline::new(PathBuf::from("/media/clip.mp4"));
        application.media = Some(MediaFile {
            path: PathBuf::from("/media/clip.mp4"),
            format: MediaFormat {
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
        });
        application
    }

    #[test]
    fn splits_at_every_cut() {
        let mut application = application_with_video();
        application.scene_scan = Some(SceneScan::Ready(vec![
            Timestamp::from_micros(2_000_000),
            Timestamp::from_micros(5_000_000),
        ]));

        let count = application.segments_from_scene_changes().expect("segments");

        assert_eq!(count, 3);
        assert_eq!(
            application.timeline().segments()[0].end(),
            Timestamp::from_micros(2_000_000)
        );
        assert_eq!(
            application.timeline().segments()[2].end(),
            Timestamp::from_micros(10_000_000)
        );
    }

    #[test]
    fn ignores_cuts_outside_the_media() {
        let mut application = application_with_video();
        application.scene_scan = Some(SceneScan::Ready(vec![
            Timestamp::ZERO,
            Timestamp::from_micros(10_000_000),
            Timestamp::from_micros(99_000_000),
        ]));

        let error = application
            .segments_from_scene_changes()
            .expect_err("no usable cuts");

        assert_eq!(error, SceneCutError::NoCuts);
    }

    #[test]
    fn requires_a_finished_scan() {
        let mut application = application_with_video();
        application.scene_scan = Some(SceneScan::Running(0));

        let error = application
            .segments_from_scene_changes()
            .expect_err("still running");

        assert_eq!(error, SceneCutError::NotReady);
    }

    #[test]
    fn updates_progress_and_finishes_from_the_worker_channel() {
        let mut application = application_with_video();
        let (sender, receiver) = mpsc::channel();
        application.scene_scan = Some(SceneScan::Running(0));
        application.scene_job = Some(SceneJob {
            receiver,
            cancel: Arc::new(AtomicBool::new(false)),
        });

        sender.send(SceneEvent::Progress(42)).expect("progress");
        application.poll_scene_scan();
        assert_eq!(application.scene_scan, Some(SceneScan::Running(42)));

        sender
            .send(SceneEvent::Finished(Ok(vec![Timestamp::from_micros(
                2_000_000,
            )])))
            .expect("finished");
        application.poll_scene_scan();
        assert_eq!(
            application.scene_scan,
            Some(SceneScan::Ready(vec![Timestamp::from_micros(2_000_000)]))
        );
        assert!(application.scene_job.is_none());
    }

    #[test]
    fn cancelling_signals_the_worker_and_clears_state() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (_sender, receiver) = mpsc::channel();
        let mut application = application_with_video();
        application.scene_scan = Some(SceneScan::Running(10));
        application.scene_job = Some(SceneJob {
            receiver,
            cancel: Arc::clone(&cancel),
        });

        application.cancel_scene_scan();

        assert!(cancel.load(Ordering::Relaxed));
        assert!(application.scene_job.is_none());
        assert!(application.scene_scan.is_none());
    }
}

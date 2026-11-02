//! Background scene-change scanning.

use std::fmt;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::domain::time::Timestamp;
use crate::media::scene;

use super::{Application, SceneJob};

/// State of the scene scan started from the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneScan {
    /// FFmpeg is still decoding the video.
    Running,
    /// Detection finished; timestamps are normalized to the media timeline.
    Ready(Vec<Timestamp>),
    /// Detection failed with a user-facing message.
    Failed(String),
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

        let path = media.path.clone();
        let start_time = media.start_time;
        let (sender, receiver) = mpsc::channel();
        self.scene_scan = Some(SceneScan::Running);
        self.scene_job = None;

        let spawn_result = thread::Builder::new()
            .name("scene-scan".to_owned())
            .spawn(move || {
                let result = scene::detect(&path, threshold, start_time);
                if sender.send(result).is_err() {
                    log::debug!(
                        "discarding scene scan for {}: the request was superseded",
                        path.display()
                    );
                }
            });

        match spawn_result {
            Ok(_handle) => {
                self.scene_job = Some(SceneJob { receiver });
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

    /// Drops any scan state.
    pub fn reset_scene_scan(&mut self) {
        self.scene_scan = None;
        self.scene_job = None;
    }

    /// Polls the scan worker without blocking.
    pub(super) fn poll_scene_scan(&mut self) {
        let Some(job) = self.scene_job.take() else {
            return;
        };

        match job.receiver.try_recv() {
            Ok(Ok(cuts)) => {
                log::info!("detected {} scene changes", cuts.len());
                self.scene_scan = Some(SceneScan::Ready(cuts));
            }
            Ok(Err(error)) => {
                log::warn!("{error}");
                self.scene_scan = Some(SceneScan::Failed(error.to_string()));
            }
            Err(TryRecvError::Empty) => self.scene_job = Some(job),
            Err(TryRecvError::Disconnected) => {
                self.scene_scan = Some(SceneScan::Failed(
                    "the scene scan worker stopped without a result".to_owned(),
                ));
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
        application.scene_scan = Some(SceneScan::Running);

        let error = application
            .segments_from_scene_changes()
            .expect_err("still running");

        assert_eq!(error, SceneCutError::NotReady);
    }
}

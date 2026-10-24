//! External FFmpeg video preview decoder.

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::domain::time::{Duration, Timestamp};

use super::{PreviewError, seconds_arg};

const PREVIEW_MAX_WIDTH: u32 = 960;
const PREVIEW_MAX_HEIGHT: u32 = 540;

/// A decoded RGB preview frame.
#[derive(Clone)]
pub struct VideoFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Tightly packed RGB24 pixels, `width * height * 3` bytes.
    pub rgb: Vec<u8>,
    /// Presentation time on the media timeline.
    pub timestamp: Timestamp,
}

/// Input for a preview video decoder.
pub struct PreviewVideoConfig {
    /// Media file to decode.
    pub path: PathBuf,
    /// Source video width in pixels.
    pub width: u32,
    /// Source video height in pixels.
    pub height: u32,
    /// Timeline position where decoding starts.
    pub start: Timestamp,
    /// Output frame rate used for preview timing.
    pub frame_rate: u32,
}

/// Result of polling a preview video decoder.
pub enum PreviewVideoResult {
    /// A decoded frame is available.
    Frame(VideoFrame),
    /// No frame is available yet.
    Pending,
    /// The decoder stopped.
    Ended {
        /// Decoding error, if it stopped because of a failure.
        error: Option<String>,
    },
}

/// A running FFmpeg preview decoder.
pub struct PreviewVideo {
    child: Arc<Mutex<Child>>,
    events: Option<Receiver<VideoEvent>>,
    reader: Option<JoinHandle<()>>,
}

enum VideoEvent {
    Frame(VideoFrame),
    Ended { error: Option<String> },
}

#[derive(Clone, Copy)]
struct ReaderConfig {
    width: u32,
    height: u32,
    frame_size: usize,
    start: Timestamp,
    frame_rate: u32,
}

impl PreviewVideo {
    /// Starts decoding `config.path` at `config.start`.
    pub fn start(config: PreviewVideoConfig) -> Result<Self, PreviewError> {
        if config.frame_rate == 0 {
            return Err(PreviewError::invalid_input(
                &config.path,
                "preview frame rate must be non-zero",
            ));
        }
        let Some((width, height)) = preview_dimensions(config.width, config.height) else {
            return Err(PreviewError::invalid_input(
                &config.path,
                "video dimensions must be non-zero",
            ));
        };
        let Some(frame_size) = frame_size(width, height) else {
            return Err(PreviewError::invalid_input(
                &config.path,
                "preview frame size is too large",
            ));
        };

        let mut child = command_for(&config, width, height)
            .spawn()
            .map_err(|source| PreviewError::spawn("ffmpeg", &config.path, source))?;

        let Some(stdout) = child.stdout.take() else {
            terminate_child(&mut child);
            return Err(PreviewError::invalid_input(
                &config.path,
                "ffmpeg did not expose stdout",
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_child(&mut child);
            return Err(PreviewError::invalid_input(
                &config.path,
                "ffmpeg did not expose stderr",
            ));
        };

        let child = Arc::new(Mutex::new(child));
        let (sender, receiver) = mpsc::sync_channel(4);
        let reader_config = ReaderConfig {
            width,
            height,
            frame_size,
            start: config.start,
            frame_rate: config.frame_rate,
        };
        let reader_child = Arc::clone(&child);
        let reader_path = config.path;
        let reader = thread::Builder::new()
            .name("preview-video".to_owned())
            .spawn(move || read_frames(stdout, stderr, reader_child, sender, reader_config))
            .map_err(|source| {
                let mut child = child
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                terminate_child(&mut child);
                PreviewError::worker("ffmpeg", &reader_path, source)
            })?;

        Ok(Self {
            child,
            events: Some(receiver),
            reader: Some(reader),
        })
    }

    /// Polls the decoder without blocking.
    pub fn next_result(&mut self) -> PreviewVideoResult {
        let Some(events) = self.events.as_ref() else {
            return PreviewVideoResult::Ended { error: None };
        };

        match events.try_recv() {
            Ok(VideoEvent::Frame(frame)) => PreviewVideoResult::Frame(frame),
            Ok(VideoEvent::Ended { error }) => PreviewVideoResult::Ended { error },
            Err(TryRecvError::Empty) => PreviewVideoResult::Pending,
            Err(TryRecvError::Disconnected) => PreviewVideoResult::Ended {
                error: Some("preview decoder stopped without reporting a result".to_owned()),
            },
        }
    }
}

impl Drop for PreviewVideo {
    fn drop(&mut self) {
        drop(self.events.take());

        {
            let mut child = self
                .child
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            terminate_child(&mut child);
        }

        if let Some(reader) = self.reader.take()
            && reader.join().is_err()
        {
            log::warn!("preview video reader thread panicked");
        }
    }
}

fn command_for(config: &PreviewVideoConfig, width: u32, height: u32) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-nostdin")
        .arg("-loglevel")
        .arg("error")
        .arg("-xerror")
        .arg("-ss")
        .arg(seconds_arg(config.start))
        .arg("-i")
        .arg(&config.path)
        .arg("-map")
        .arg("0:v:0")
        .arg("-an")
        .arg("-sn")
        .arg("-dn")
        .arg("-vf")
        .arg(format!(
            "fps={},scale={width}:{height}:flags=fast_bilinear",
            config.frame_rate
        ))
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("-f")
        .arg("rawvideo")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn read_frames(
    mut stdout: ChildStdout,
    mut stderr: ChildStderr,
    child: Arc<Mutex<Child>>,
    sender: SyncSender<VideoEvent>,
    config: ReaderConfig,
) {
    let mut buffer = vec![0_u8; config.frame_size];
    let mut frame_index: u64 = 0;
    let mut read_error: Option<String> = None;

    loop {
        match stdout.read(&mut buffer[..1]) {
            Ok(0) => break,
            Ok(_) => {
                if let Err(error) = stdout.read_exact(&mut buffer[1..]) {
                    read_error = Some(format!(
                        "preview decoder ended in the middle of a frame: {error}"
                    ));
                    break;
                }
                let Some(timestamp) = frame_timestamp(config.start, frame_index, config.frame_rate)
                else {
                    read_error = Some("preview frame timestamp overflowed".to_owned());
                    break;
                };
                let frame = VideoFrame {
                    width: config.width,
                    height: config.height,
                    rgb: buffer.clone(),
                    timestamp,
                };
                if sender.send(VideoEvent::Frame(frame)).is_err() {
                    log::debug!("preview frame consumer stopped; ending decoder");
                    return;
                }
                let Some(next_index) = frame_index.checked_add(1) else {
                    read_error = Some("preview frame counter overflowed".to_owned());
                    break;
                };
                frame_index = next_index;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                read_error = Some(format!("failed to read preview frames: {error}"));
                break;
            }
        }
    }

    let stderr_text = read_stderr(&mut stderr);
    let status = child
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .try_wait();
    let error = match (read_error, status) {
        (Some(read_error), _) => Some(join_detail(&read_error, &stderr_text)),
        (None, Ok(Some(status))) if !status.success() => Some(join_detail(
            &format!("ffmpeg exited with {status}"),
            &stderr_text,
        )),
        (None, Err(error)) => Some(join_detail(
            &format!("failed to check ffmpeg status: {error}"),
            &stderr_text,
        )),
        // FFmpeg is run with `-loglevel error`, so any stderr output is an
        // error even when the process has not been reaped yet.
        (None, _) if !stderr_text.is_empty() => {
            Some(format!("ffmpeg reported an error: {stderr_text}"))
        }
        (None, _) => None,
    };

    if sender.send(VideoEvent::Ended { error }).is_err() {
        log::debug!("preview consumer stopped before the decoder reported its result");
    }
}

fn read_stderr(stderr: &mut ChildStderr) -> String {
    let mut bytes = Vec::new();
    match stderr.read_to_end(&mut bytes) {
        Ok(_) => String::from_utf8_lossy(&bytes).trim().to_owned(),
        Err(error) => {
            let text = String::from_utf8_lossy(&bytes).trim().to_owned();
            if text.is_empty() {
                format!("failed to read ffmpeg stderr: {error}")
            } else {
                format!("{text}\nfailed to read ffmpeg stderr: {error}")
            }
        }
    }
}

fn join_detail(detail: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        detail.to_owned()
    } else {
        format!("{detail}: {stderr}")
    }
}

fn terminate_child(child: &mut Child) {
    if let Err(error) = child.kill()
        && error.kind() != io::ErrorKind::InvalidInput
    {
        log::warn!("failed to stop preview process: {error}");
    }
    if let Err(error) = child.wait() {
        log::warn!("failed to reap preview process: {error}");
    }
}

fn frame_timestamp(start: Timestamp, frame_index: u64, frame_rate: u32) -> Option<Timestamp> {
    let offset_micros = frame_index.checked_mul(1_000_000)? / u64::from(frame_rate);
    start.checked_add(Duration::from_micros(offset_micros))
}

fn frame_size(width: u32, height: u32) -> Option<usize> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    width.checked_mul(height)?.checked_mul(3)
}

fn preview_dimensions(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }

    let width_64 = u64::from(width);
    let height_64 = u64::from(height);
    let max_width = u64::from(PREVIEW_MAX_WIDTH);
    let max_height = u64::from(PREVIEW_MAX_HEIGHT);

    let (scaled_width, scaled_height) = if width_64 <= max_width && height_64 <= max_height {
        (width_64, height_64)
    } else if width_64 * max_height >= height_64 * max_width {
        (max_width, (height_64 * max_width / width_64).max(1))
    } else {
        ((width_64 * max_height / height_64).max(1), max_height)
    };

    Some((
        u32::try_from(scaled_width).ok()?,
        u32::try_from(scaled_height).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn config() -> PreviewVideoConfig {
        PreviewVideoConfig {
            path: PathBuf::from("fixture.mp4"),
            width: 160,
            height: 120,
            start: Timestamp::from_micros(1_500_000),
            frame_rate: 30,
        }
    }

    #[test]
    fn scales_preview_frames_without_exceeding_limits() {
        assert_eq!(preview_dimensions(160, 120), Some((160, 120)));
        assert_eq!(preview_dimensions(1920, 1080), Some((960, 540)));
        assert_eq!(preview_dimensions(1080, 1920), Some((303, 540)));
        assert_eq!(preview_dimensions(0, 120), None);
        assert_eq!(preview_dimensions(160, 0), None);
    }

    #[test]
    fn rejects_zero_dimensions() {
        let mut config = config();
        config.width = 0;

        assert!(matches!(
            PreviewVideo::start(config),
            Err(PreviewError::InvalidInput { .. })
        ));
    }

    #[test]
    fn computes_frame_timestamps() {
        let start = Timestamp::from_micros(1_500_000);

        assert_eq!(frame_timestamp(start, 0, 30), Some(start));
        assert_eq!(
            frame_timestamp(start, 30, 30),
            Some(Timestamp::from_micros(2_500_000))
        );
    }

    #[test]
    fn video_command_passes_each_argument_separately() {
        let command = command_for(&config(), 160, 120);
        let arguments: Vec<&OsStr> = command.get_args().collect();

        assert_eq!(
            arguments,
            vec![
                OsStr::new("-nostdin"),
                OsStr::new("-loglevel"),
                OsStr::new("error"),
                OsStr::new("-xerror"),
                OsStr::new("-ss"),
                OsStr::new("1.500000"),
                OsStr::new("-i"),
                OsStr::new("fixture.mp4"),
                OsStr::new("-map"),
                OsStr::new("0:v:0"),
                OsStr::new("-an"),
                OsStr::new("-sn"),
                OsStr::new("-dn"),
                OsStr::new("-vf"),
                OsStr::new("fps=30,scale=160:120:flags=fast_bilinear"),
                OsStr::new("-pix_fmt"),
                OsStr::new("rgb24"),
                OsStr::new("-f"),
                OsStr::new("rawvideo"),
                OsStr::new("pipe:1"),
            ]
        );
    }
}

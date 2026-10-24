//! External `ffplay` audio preview.

use std::io::{self, Read};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::domain::time::Timestamp;

use super::{PreviewError, seconds_arg};

/// Status of the audio preview process.
pub enum PreviewAudioStatus {
    /// Audio is still playing.
    Running,
    /// The audio process exited.
    Ended {
        /// Whether the process exited successfully.
        success: bool,
        /// Captured standard error, possibly empty.
        stderr: String,
    },
}

/// A running `ffplay` audio preview process.
pub struct PreviewAudio {
    child: Child,
    stderr: Arc<Mutex<String>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl PreviewAudio {
    /// Starts audio playback for `path` at `start`.
    pub fn start(path: &std::path::Path, start: Timestamp) -> Result<Self, PreviewError> {
        let mut child = command_for(path, start)
            .spawn()
            .map_err(|source| PreviewError::spawn("ffplay", path, source))?;

        let Some(stderr) = child.stderr.take() else {
            terminate_child(&mut child);
            return Err(PreviewError::invalid_input(
                path,
                "ffplay did not expose stderr",
            ));
        };

        let sink = Arc::new(Mutex::new(String::new()));
        let thread_sink = Arc::clone(&sink);
        let stderr_reader = thread::Builder::new()
            .name("preview-audio".to_owned())
            .spawn(move || read_stderr(stderr, thread_sink))
            .map_err(|source| {
                terminate_child(&mut child);
                PreviewError::worker("ffplay", path, source)
            })?;

        Ok(Self {
            child,
            stderr: sink,
            stderr_reader: Some(stderr_reader),
        })
    }

    /// Checks whether the audio process is still running without blocking.
    pub fn check(&mut self) -> PreviewAudioStatus {
        match self.child.try_wait() {
            Ok(None) => PreviewAudioStatus::Running,
            Ok(Some(status)) => {
                self.join_stderr();
                PreviewAudioStatus::Ended {
                    success: status.success(),
                    stderr: self.stderr_text(),
                }
            }
            Err(error) => {
                terminate_child(&mut self.child);
                self.join_stderr();
                PreviewAudioStatus::Ended {
                    success: false,
                    stderr: format!(
                        "failed to check ffplay status: {error}\n{}",
                        self.stderr_text()
                    )
                    .trim()
                    .to_owned(),
                }
            }
        }
    }

    fn join_stderr(&mut self) {
        if let Some(reader) = self.stderr_reader.take()
            && reader.join().is_err()
        {
            log::warn!("ffplay stderr reader thread panicked");
        }
    }

    fn stderr_text(&self) -> String {
        self.stderr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Drop for PreviewAudio {
    fn drop(&mut self) {
        terminate_child(&mut self.child);
        self.join_stderr();
    }
}

fn command_for(path: &std::path::Path, start: Timestamp) -> Command {
    let mut command = Command::new("ffplay");
    command
        .arg("-nodisp")
        .arg("-vn")
        .arg("-autoexit")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(seconds_arg(start))
        .arg("-i")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

fn terminate_child(child: &mut Child) {
    if let Err(error) = child.kill()
        && error.kind() != io::ErrorKind::InvalidInput
    {
        log::warn!("failed to stop ffplay: {error}");
    }
    if let Err(error) = child.wait() {
        log::warn!("failed to reap ffplay: {error}");
    }
}

fn read_stderr(mut stderr: ChildStderr, sink: Arc<Mutex<String>>) {
    let mut bytes = Vec::new();
    let read_result = stderr.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).trim().to_owned();
    let text = match read_result {
        Ok(_) => text,
        Err(error) => {
            if text.is_empty() {
                format!("failed to read ffplay stderr: {error}")
            } else {
                format!("{text}\nfailed to read ffplay stderr: {error}")
            }
        }
    };

    let mut sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *sink = text;
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn audio_command_passes_each_argument_separately() {
        let command = command_for(
            &PathBuf::from("fixture.mp4"),
            Timestamp::from_micros(2_000_000),
        );
        let arguments: Vec<&OsStr> = command.get_args().collect();

        assert_eq!(
            arguments,
            vec![
                OsStr::new("-nodisp"),
                OsStr::new("-vn"),
                OsStr::new("-autoexit"),
                OsStr::new("-loglevel"),
                OsStr::new("error"),
                OsStr::new("-ss"),
                OsStr::new("2.000000"),
                OsStr::new("-i"),
                OsStr::new("fixture.mp4"),
            ]
        );
    }
}

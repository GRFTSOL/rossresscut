//! Audio preview backed by the system audio device.
//!
//! FFmpeg decodes the selected audio stream into interleaved `f32` PCM at the
//! output device's channel count and sample rate. `cpal` writes those samples
//! to the device. The number of sample frames actually consumed by the device
//! drives the application's playback clock, which keeps video and audio in
//! sync without a separate player process.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration as StdDuration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

use crate::domain::time::{Duration, Timestamp};
use crate::media::process;

use super::{PreviewError, join_detail, read_stderr, seconds_arg, terminate_child};

/// Status of the audio preview.
pub enum PreviewAudioStatus {
    /// Audio is still playing.
    Running,
    /// Audio playback finished or failed.
    Ended {
        /// Whether playback ended without an error.
        success: bool,
        /// Captured standard error or device error, possibly empty.
        stderr: String,
    },
}

enum AudioEvent {
    Ended { error: Option<String> },
}

/// A running audio preview backed by the system audio device.
pub struct PreviewAudio {
    stream: Option<Stream>,
    decoder: Option<Child>,
    reader: Option<JoinHandle<()>>,
    events: Receiver<AudioEvent>,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    samples_played: Arc<AtomicU64>,
    stream_error: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    start: Timestamp,
    sample_rate: u32,
    decoder_ended: bool,
    decoder_error: Option<String>,
}

impl PreviewAudio {
    /// Starts audio playback for `path` at `start`.
    pub fn start(path: &Path, start: Timestamp) -> Result<Self, PreviewError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| PreviewError::audio_device("no default audio output device"))?;
        let supported = device.default_output_config().map_err(|error| {
            PreviewError::audio_device(format!(
                "failed to query the default audio output configuration: {error}"
            ))
        })?;

        let sample_format = supported.sample_format();
        if sample_format != SampleFormat::F32 {
            return Err(PreviewError::audio_device(format!(
                "unsupported output sample format {sample_format:?}; only f32 is supported"
            )));
        }

        let config = supported.config();
        let sample_rate = config.sample_rate;
        let channels = config.channels;
        if sample_rate == 0 || channels == 0 {
            return Err(PreviewError::audio_device(
                "the audio device reported an invalid configuration",
            ));
        }
        let capacity = (sample_rate as usize)
            .checked_mul(channels as usize)
            .ok_or_else(|| PreviewError::audio_device("audio buffer size overflowed"))?;

        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        let samples_played = Arc::new(AtomicU64::new(0));
        let stream_error = Arc::new(Mutex::new(None));
        let stream = build_stream(
            &device,
            config,
            Arc::clone(&buffer),
            Arc::clone(&samples_played),
            channels as usize,
            Arc::clone(&stream_error),
        )?;

        let mut child = command_for(path, start, channels, sample_rate)
            .spawn()
            .map_err(|source| PreviewError::spawn("ffmpeg", path, source))?;

        let Some(stdout) = child.stdout.take() else {
            terminate_child(&mut child);
            return Err(PreviewError::invalid_input(
                path,
                "ffmpeg did not expose audio stdout",
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_child(&mut child);
            return Err(PreviewError::invalid_input(
                path,
                "ffmpeg did not expose audio stderr",
            ));
        };

        let stop = Arc::new(AtomicBool::new(false));
        let (sender, events) = mpsc::channel();
        let reader_buffer = Arc::clone(&buffer);
        let reader_stop = Arc::clone(&stop);
        let reader = thread::Builder::new()
            .name("preview-audio".to_owned())
            .spawn(move || {
                read_audio(stdout, stderr, sender, reader_buffer, capacity, reader_stop);
            })
            .map_err(|source| {
                stop.store(true, Ordering::Relaxed);
                terminate_child(&mut child);
                PreviewError::worker("ffmpeg", path, source)
            })?;

        if let Err(error) = stream.play() {
            stop.store(true, Ordering::Relaxed);
            terminate_child(&mut child);
            if reader.join().is_err() {
                log::warn!("audio decoder reader thread panicked");
            }
            return Err(PreviewError::audio_device(format!(
                "failed to start audio output: {error}"
            )));
        }

        Ok(Self {
            stream: Some(stream),
            decoder: Some(child),
            reader: Some(reader),
            events,
            buffer,
            samples_played,
            stream_error,
            stop,
            start,
            sample_rate,
            decoder_ended: false,
            decoder_error: None,
        })
    }

    /// Returns the media position represented by the samples already played.
    pub fn position(&self) -> Timestamp {
        let frames = self.samples_played.load(Ordering::Relaxed);
        let Some(micros) = samples_to_micros(frames, self.sample_rate) else {
            return self.start;
        };
        self.start
            .checked_add(Duration::from_micros(micros))
            .unwrap_or(self.start)
    }

    /// Checks the audio decoder and output device without blocking.
    pub fn check(&mut self) -> PreviewAudioStatus {
        let stream_error = self
            .stream_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(error) = stream_error {
            return PreviewAudioStatus::Ended {
                success: false,
                stderr: error,
            };
        }

        if !self.decoder_ended {
            match self.events.try_recv() {
                Ok(AudioEvent::Ended { error }) => {
                    self.decoder_ended = true;
                    self.decoder_error = error;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.decoder_ended = true;
                    self.decoder_error =
                        Some("audio decoder stopped without reporting a result".to_owned());
                }
            }
        }

        if self.decoder_ended {
            if let Some(error) = self.decoder_error.clone() {
                return PreviewAudioStatus::Ended {
                    success: false,
                    stderr: error,
                };
            }

            let empty = self
                .buffer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty();
            if empty {
                return PreviewAudioStatus::Ended {
                    success: true,
                    stderr: String::new(),
                };
            }
        }

        PreviewAudioStatus::Running
    }
}

impl Drop for PreviewAudio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(mut child) = self.decoder.take() {
            terminate_child(&mut child);
        }
        if let Some(reader) = self.reader.take()
            && reader.join().is_err()
        {
            log::warn!("audio decoder reader thread panicked");
        }
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
    }
}

fn build_stream(
    device: &cpal::Device,
    config: StreamConfig,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    samples_played: Arc<AtomicU64>,
    channels: usize,
    stream_error: Arc<Mutex<Option<String>>>,
) -> Result<Stream, PreviewError> {
    let callback_buffer = Arc::clone(&buffer);
    let callback_samples = Arc::clone(&samples_played);
    let callback_error = Arc::clone(&stream_error);

    device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _info| {
                data.fill(0.0);
                let frames_requested = data.len() / channels;
                let mut queue = callback_buffer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let frames_available = queue.len() / channels;
                let frames = frames_requested.min(frames_available);
                let samples = frames * channels;

                for (output, sample) in data.iter_mut().zip(queue.drain(..samples)) {
                    *output = sample;
                }
                callback_samples.fetch_add(frames as u64, Ordering::Relaxed);
            },
            move |error| {
                let message = format!("audio output error: {error}");
                log::error!("{message}");
                let mut sink = callback_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if sink.is_none() {
                    *sink = Some(message);
                }
            },
            None,
        )
        .map_err(|error| {
            PreviewError::audio_device(format!("failed to create the audio output stream: {error}"))
        })
}

fn command_for(
    path: &Path,
    start: Timestamp,
    channels: u16,
    sample_rate: u32,
) -> process::LoggedCommand {
    let mut command = process::command("ffmpeg", process::CommandPurpose::Preview);
    command
        .arg("-nostdin")
        .arg("-loglevel")
        .arg("error")
        .arg("-xerror")
        .arg("-ss")
        .arg(seconds_arg(start))
        .arg("-i")
        .arg(path)
        .arg("-map")
        .arg("0:a:0")
        .arg("-vn")
        .arg("-sn")
        .arg("-dn")
        .arg("-ac")
        .arg(channels.to_string())
        .arg("-ar")
        .arg(sample_rate.to_string())
        .arg("-f")
        .arg("f32le")
        .arg("-acodec")
        .arg("pcm_f32le")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn read_audio(
    mut stdout: ChildStdout,
    mut stderr: ChildStderr,
    sender: mpsc::Sender<AudioEvent>,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    capacity: usize,
    stop: Arc<AtomicBool>,
) {
    let mut bytes = [0_u8; 8192];
    let mut carry: Vec<u8> = Vec::new();
    let mut error: Option<String> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        match stdout.read(&mut bytes) {
            Ok(0) => break,
            Ok(read) => {
                push_bytes(&bytes[..read], &mut carry, &buffer, capacity, &stop);
                if stop.load(Ordering::Relaxed) {
                    return;
                }
            }
            Err(read_error) if read_error.kind() == io::ErrorKind::Interrupted => {}
            Err(read_error) => {
                error = Some(format!("failed to read audio samples: {read_error}"));
                break;
            }
        }
    }

    if !carry.is_empty() {
        error = Some("audio decoder produced a partial sample".to_owned());
    }

    let stderr_text = read_stderr(&mut stderr);
    let error = match error {
        Some(error) => Some(join_detail(&error, &stderr_text)),
        None if !stderr_text.is_empty() => {
            Some(format!("ffmpeg reported an audio error: {stderr_text}"))
        }
        None => None,
    };

    if sender.send(AudioEvent::Ended { error }).is_err() {
        log::debug!("audio consumer stopped before the decoder ended");
    }
}

fn push_bytes(
    bytes: &[u8],
    carry: &mut Vec<u8>,
    buffer: &Arc<Mutex<VecDeque<f32>>>,
    capacity: usize,
    stop: &Arc<AtomicBool>,
) {
    carry.extend_from_slice(bytes);
    let complete = carry.len() - carry.len() % 4;
    let samples: Vec<f32> = carry[..complete]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();
    carry.drain(..complete);
    push_samples(&samples, buffer, capacity, stop);
}

fn push_samples(
    samples: &[f32],
    buffer: &Arc<Mutex<VecDeque<f32>>>,
    capacity: usize,
    stop: &Arc<AtomicBool>,
) {
    let mut offset = 0;

    while offset < samples.len() {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        let mut queue = buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let space = capacity.saturating_sub(queue.len());
        if space == 0 {
            drop(queue);
            thread::sleep(StdDuration::from_millis(5));
            continue;
        }

        let count = space.min(samples.len() - offset);
        queue.extend(samples[offset..offset + count].iter().copied());
        offset += count;
    }
}

fn samples_to_micros(frames: u64, sample_rate: u32) -> Option<u64> {
    if sample_rate == 0 {
        return None;
    }
    let micros = u128::from(frames).checked_mul(1_000_000)? / u128::from(sample_rate);
    u64::try_from(micros).ok()
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
            2,
            48_000,
        );
        let arguments: Vec<&OsStr> = command.get_args().collect();

        assert_eq!(
            arguments,
            vec![
                OsStr::new("-nostdin"),
                OsStr::new("-loglevel"),
                OsStr::new("error"),
                OsStr::new("-xerror"),
                OsStr::new("-ss"),
                OsStr::new("2.000000"),
                OsStr::new("-i"),
                OsStr::new("fixture.mp4"),
                OsStr::new("-map"),
                OsStr::new("0:a:0"),
                OsStr::new("-vn"),
                OsStr::new("-sn"),
                OsStr::new("-dn"),
                OsStr::new("-ac"),
                OsStr::new("2"),
                OsStr::new("-ar"),
                OsStr::new("48000"),
                OsStr::new("-f"),
                OsStr::new("f32le"),
                OsStr::new("-acodec"),
                OsStr::new("pcm_f32le"),
                OsStr::new("pipe:1"),
            ]
        );
    }

    #[test]
    fn converts_played_frames_to_microseconds() {
        assert_eq!(samples_to_micros(48_000, 48_000), Some(1_000_000));
        assert_eq!(samples_to_micros(24_000, 48_000), Some(500_000));
        assert_eq!(samples_to_micros(0, 48_000), Some(0));
        assert_eq!(samples_to_micros(1, 0), None);
    }
}

//! Waveform extraction through an external FFmpeg process.
//!
//! FFmpeg decodes the selected audio stream to mono `s16le` on stdout; this
//! module only computes min/max peaks from those samples. Nothing blocks the
//! UI thread: callers run [`extract`] on a worker thread.

use std::fmt;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use crate::domain::time::Duration;
use crate::domain::waveform::{Peak, Waveform};

/// Peak rate: one peak per 5 ms.
const PEAKS_PER_SECOND: u64 = 200;
/// Mono sample rate used for peak extraction.
const SAMPLE_RATE: u32 = 8_000;
/// Samples folded into one peak.
const SAMPLES_PER_PEAK: usize = (SAMPLE_RATE as u64 / PEAKS_PER_SECOND) as usize;

/// Errors produced while extracting a waveform.
#[derive(Debug)]
pub enum WaveformError {
    /// The FFmpeg executable could not be started.
    Spawn {
        /// Media file that was being scanned.
        path: PathBuf,
        /// Operating-system error from process creation.
        source: std::io::Error,
    },
    /// Reading the decoded samples failed.
    Read {
        /// Media file that was being scanned.
        path: PathBuf,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// FFmpeg exited with a non-success status.
    Failed {
        /// Media file that was being scanned.
        path: PathBuf,
        /// Human-readable exit status.
        status: String,
        /// Captured standard error, possibly empty.
        stderr: String,
    },
    /// FFmpeg produced no samples, typically because the stream has no audio.
    Empty {
        /// Media file that was being scanned.
        path: PathBuf,
    },
}

impl fmt::Display for WaveformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { path, source } => write!(
                formatter,
                "starting ffmpeg for the waveform of {}: {source}",
                path.display()
            ),
            Self::Read { path, source } => write!(
                formatter,
                "reading waveform samples for {}: {source}",
                path.display()
            ),
            Self::Failed {
                path,
                status,
                stderr,
            } => {
                let detail = stderr.trim();
                write!(
                    formatter,
                    "ffmpeg failed for the waveform of {} ({status})",
                    path.display()
                )?;
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::Empty { path } => write!(
                formatter,
                "ffmpeg decoded no audio samples for {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for WaveformError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Read { source, .. } => Some(source),
            Self::Failed { .. } | Self::Empty { .. } => None,
        }
    }
}

/// Decodes `stream_index` of `path` and returns its amplitude envelope.
///
/// This function blocks until FFmpeg finishes and must run outside the UI
/// thread.
pub fn extract(path: &Path, stream_index: usize) -> Result<Waveform, WaveformError> {
    let mut child =
        command_for(path, stream_index)
            .spawn()
            .map_err(|source| WaveformError::Spawn {
                path: path.to_path_buf(),
                source,
            })?;

    let stdout = child.stdout.take().ok_or_else(|| WaveformError::Read {
        path: path.to_path_buf(),
        source: std::io::Error::other("ffmpeg did not expose its standard output"),
    })?;
    let stderr = child.stderr.take();

    // Drain standard error on its own thread so a chatty FFmpeg cannot block
    // while the sample pipe is still being read.
    let stderr_reader = stderr.map(|stderr| thread::spawn(move || read_text(stderr)));

    let peaks = read_peaks(stdout, path);
    let status = child.wait();

    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();

    let peaks = peaks?;
    let status = status.map_err(|source| WaveformError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if !status.success() {
        return Err(WaveformError::Failed {
            path: path.to_path_buf(),
            status: status.to_string(),
            stderr,
        });
    }
    if peaks.is_empty() {
        return Err(WaveformError::Empty {
            path: path.to_path_buf(),
        });
    }

    let duration = Duration::from_micros(peaks.len() as u64 * 1_000_000 / PEAKS_PER_SECOND);
    Waveform::new(duration, peaks).ok_or_else(|| WaveformError::Empty {
        path: path.to_path_buf(),
    })
}

fn command_for(path: &Path, stream_index: usize) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-nostdin")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(path)
        .arg("-map")
        .arg(format!("0:{stream_index}"))
        .arg("-vn")
        .arg("-sn")
        .arg("-dn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg(SAMPLE_RATE.to_string())
        .arg("-f")
        .arg("s16le")
        .arg("-acodec")
        .arg("pcm_s16le")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Folds little-endian `i16` samples into min/max peaks.
fn read_peaks<R: Read>(reader: R, path: &Path) -> Result<Vec<Peak>, WaveformError> {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut buffer = vec![0u8; 64 * 1024];
    let read_error = |source| WaveformError::Read {
        path: path.to_path_buf(),
        source,
    };

    let mut peaks = Vec::new();
    let mut pending: Vec<u8> = Vec::with_capacity(2);
    let mut min = i16::MAX;
    let mut max = i16::MIN;
    let mut samples_in_peak = 0usize;

    loop {
        let read = reader.read(&mut buffer).map_err(read_error)?;
        if read == 0 {
            break;
        }

        let mut data = std::mem::take(&mut pending);
        data.extend_from_slice(&buffer[..read]);
        let usable = data.len() & !1;

        for pair in data[..usable].as_chunks::<2>().0 {
            let sample = i16::from_le_bytes(*pair);
            min = min.min(sample);
            max = max.max(sample);
            samples_in_peak += 1;
            if samples_in_peak == SAMPLES_PER_PEAK {
                peaks.push(Peak::new(min, max));
                min = i16::MAX;
                max = i16::MIN;
                samples_in_peak = 0;
            }
        }

        if usable < data.len() {
            pending = data[usable..].to_vec();
        }
    }

    if samples_in_peak > 0 {
        peaks.push(Peak::new(min, max));
    }

    Ok(peaks)
}

fn read_text(mut stream: impl Read) -> String {
    let mut text = Vec::new();
    if stream.read_to_end(&mut text).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&text).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn peaks_from(bytes: &[u8]) -> Vec<Peak> {
        read_peaks(Cursor::new(bytes.to_vec()), Path::new("test")).expect("peaks")
    }

    #[test]
    fn folds_samples_into_peaks_and_keeps_the_tail() {
        let mut bytes = Vec::new();
        for sample in [0i16, 100, -50, 20] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        let peaks = peaks_from(&bytes);

        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0], Peak::new(-50, 100));
    }

    #[test]
    fn splits_peaks_at_the_configured_sample_count() {
        let mut bytes = Vec::new();
        for sample in 0..(SAMPLES_PER_PEAK as i16 + 1) {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        let peaks = peaks_from(&bytes);

        assert_eq!(peaks.len(), 2);
        assert_eq!(
            peaks[1],
            Peak::new(SAMPLES_PER_PEAK as i16, SAMPLES_PER_PEAK as i16)
        );
    }

    #[test]
    fn ignores_a_trailing_partial_sample() {
        let peaks = peaks_from(&[0x01]);

        assert!(peaks.is_empty());
    }

    #[test]
    fn reads_all_text_from_a_stream() {
        assert_eq!(read_text(Cursor::new(b"boom")), "boom");
    }
}

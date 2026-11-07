//! Scene change detection through an external FFmpeg process.
//!
//! FFmpeg decodes the selected video stream and reports the timestamp of every
//! frame whose scene score exceeds the threshold. No frames are transferred to
//! this process; only timestamps are parsed from `showinfo` output.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::time::{Duration, Timestamp};
use crate::media::process;

/// Errors produced while scanning for scene changes.
#[derive(Debug)]
pub enum SceneError {
    /// The FFmpeg executable could not be started.
    Spawn {
        /// Media file that was being scanned.
        path: PathBuf,
        /// Operating-system error from process creation.
        source: std::io::Error,
    },
    /// FFmpeg exited with a non-success status.
    Failed {
        /// Media file that was being scanned.
        path: PathBuf,
        /// Human-readable exit status.
        status: String,
        /// Captured standard error tail, possibly empty.
        stderr: String,
    },
    /// Waiting for the FFmpeg process failed.
    Wait {
        /// Media file that was being scanned.
        path: PathBuf,
        /// Operating-system error from waiting for the process.
        source: std::io::Error,
    },
    /// The caller cancelled the scan.
    Cancelled,
}

impl fmt::Display for SceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { path, source } => write!(
                formatter,
                "starting ffmpeg for scene detection on {}: {source}",
                path.display()
            ),
            Self::Failed {
                path,
                status,
                stderr,
            } => {
                write!(
                    formatter,
                    "ffmpeg failed while scanning {} for scene changes ({status})",
                    path.display()
                )?;
                let detail = stderr.trim();
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::Wait { path, source } => write!(
                formatter,
                "waiting for ffmpeg scene detection on {}: {source}",
                path.display()
            ),
            Self::Cancelled => formatter.write_str("scene detection was cancelled"),
        }
    }
}

impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Wait { source, .. } => Some(source),
            Self::Failed { .. } | Self::Cancelled => None,
        }
    }
}

/// Scans `path` and returns scene-change timestamps on the media timeline.
///
/// `threshold` is FFmpeg's scene score in `0.0..=1.0`; higher values keep only
/// sharper cuts. `duration` is used to report decode progress as a percentage;
/// without it the callback is never called. Timestamps are normalized by
/// subtracting `start_time`, which matches the keyframe index and the preview
/// seek coordinates.
///
/// This function blocks until FFmpeg finishes and must run outside the UI
/// thread. `on_progress` runs on the calling thread.
pub fn detect<F: FnMut(u8)>(
    path: &Path,
    threshold: f32,
    start_time: Option<Timestamp>,
    duration: Option<Duration>,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<Vec<Timestamp>, SceneError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(SceneError::Cancelled);
    }

    let mut child = command_for(path, threshold)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| SceneError::Spawn {
            path: path.to_path_buf(),
            source,
        })?;

    let stderr = child.stderr.take().ok_or_else(|| SceneError::Spawn {
        path: path.to_path_buf(),
        source: std::io::Error::other("ffmpeg stderr pipe is unavailable"),
    })?;

    let offset = start_time.map_or(0, Timestamp::micros).max(0);
    let duration_micros = duration
        .map(|duration| i64::try_from(duration.micros()).unwrap_or(i64::MAX))
        .filter(|duration| *duration > 0);
    let mut cuts = Vec::new();
    let mut diagnostics = String::new();
    let mut last_percent = 0u8;

    for line in BufReader::new(stderr).lines() {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SceneError::Cancelled);
        }
        let Ok(line) = line else {
            continue;
        };
        if line.contains("showinfo@progress") {
            if let Some(percent) = progress_percent(&line, offset, duration_micros) {
                let percent = percent.max(last_percent);
                if percent > last_percent {
                    last_percent = percent;
                    on_progress(percent);
                }
            }
        } else if line.contains("showinfo@cuts") {
            if let Some(time) = showinfo_timestamp(&line) {
                cuts.push(normalize_cut(time, offset));
            }
        } else if diagnostics.len() < 16_384 {
            diagnostics.push_str(&line);
            diagnostics.push('\n');
        }
    }

    let status = child.wait().map_err(|source| SceneError::Wait {
        path: path.to_path_buf(),
        source,
    })?;
    if !status.success() {
        return Err(SceneError::Failed {
            path: path.to_path_buf(),
            status: status.to_string(),
            stderr: stderr_tail(&diagnostics),
        });
    }

    cuts.sort_unstable();
    cuts.dedup();
    Ok(cuts)
}

fn command_for(path: &Path, threshold: f32) -> Command {
    let threshold = threshold.clamp(0.01, 1.0);
    let mut command = process::command("ffmpeg");
    command
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-nostats")
        .arg("-loglevel")
        .arg("info")
        .arg("-i")
        .arg(path)
        .arg("-map")
        .arg("0:v:0")
        .arg("-an")
        .arg("-sn")
        .arg("-dn")
        .arg("-filter:v")
        .arg(format!(
            "showinfo@progress=checksum=0,select='gt(scene,{threshold:.3})',showinfo@cuts=checksum=0"
        ))
        .arg("-f")
        .arg("null")
        .arg("-");
    command
}

/// Extracts `pts_time:` from one `showinfo` line.
fn showinfo_timestamp(line: &str) -> Option<Timestamp> {
    let (_, rest) = line.split_once("pts_time:")?;
    let value = rest.split_whitespace().next()?;
    Timestamp::from_seconds(value)
}

/// Converts a progress line into a 0-99 percent.
///
/// 100 is reserved for completion, which the application represents with
/// [`crate::application::SceneScan::Ready`]. Returns `None` when the duration is
/// unknown or the line has no timestamp.
fn progress_percent(line: &str, offset: i64, duration_micros: Option<i64>) -> Option<u8> {
    let duration = duration_micros.filter(|duration| *duration > 0)?;
    let elapsed = (showinfo_timestamp(line)?.micros() - offset).max(0);
    let percent = i128::from(elapsed) * 100 / i128::from(duration);
    Some(u8::try_from(percent.clamp(0, 99)).unwrap_or(99))
}

fn normalize_cut(time: Timestamp, offset: i64) -> Timestamp {
    Timestamp::from_micros((time.micros() - offset).max(0))
}

fn stderr_tail(stderr: &str) -> String {
    const LIMIT: usize = 2000;
    let trimmed = stderr.trim();
    if trimmed.len() <= LIMIT {
        return trimmed.to_owned();
    }
    let start = trimmed.len() - LIMIT;
    trimmed.get(start..).unwrap_or(trimmed).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRESS: &str = "[showinfo@progress @ 0x55] n:  10 pts: 1000 pts_time:1.5 duration:1";
    const CUT: &str = "[showinfo@cuts @ 0x66] n:  20 pts: 2000 pts_time:3.25 duration:1";

    #[test]
    fn parses_showinfo_timestamps() {
        assert_eq!(
            showinfo_timestamp(PROGRESS),
            Some(Timestamp::from_micros(1_500_000))
        );
        assert_eq!(
            showinfo_timestamp(CUT),
            Some(Timestamp::from_micros(3_250_000))
        );
        assert_eq!(showinfo_timestamp("frame= 120 time=00:00:04.00"), None);
    }

    #[test]
    fn subtracts_the_container_start_and_clamps_at_zero() {
        assert_eq!(
            normalize_cut(Timestamp::from_micros(2_500_000), 1_400_000),
            Timestamp::from_micros(1_100_000)
        );
        assert_eq!(
            normalize_cut(Timestamp::from_micros(1_000_000), 1_400_000),
            Timestamp::ZERO
        );
    }

    #[test]
    fn computes_progress_percentages_and_reserves_one_hundred() {
        assert_eq!(progress_percent(PROGRESS, 0, Some(4_000_000)), Some(37));
        assert_eq!(
            progress_percent(PROGRESS, 500_000, Some(4_000_000)),
            Some(25)
        );
        // Past the end, and before completion, the bar stays at 99%.
        assert_eq!(progress_percent(PROGRESS, 0, Some(1_000_000)), Some(99));
        // Unknown durations cannot produce a percentage.
        assert_eq!(progress_percent(PROGRESS, 0, None), None);
    }

    #[test]
    fn distinguishes_progress_from_cut_lines() {
        assert!(PROGRESS.contains("showinfo@progress"));
        assert!(!PROGRESS.contains("showinfo@cuts"));
        assert!(CUT.contains("showinfo@cuts"));
        assert!(!CUT.contains("showinfo@progress"));
    }

    #[test]
    fn returns_immediately_when_already_cancelled() {
        let cancel = AtomicBool::new(true);
        let result = detect(Path::new("missing.mp4"), 0.4, None, None, &cancel, |_| {});

        assert!(matches!(result, Err(SceneError::Cancelled)));
    }
}

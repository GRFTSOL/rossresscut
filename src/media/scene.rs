//! Scene change detection through an external FFmpeg process.
//!
//! FFmpeg decodes the selected video stream and reports the timestamp of every
//! frame whose scene score exceeds the threshold. No frames are transferred to
//! this process; only timestamps are parsed from `showinfo` output.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::time::Timestamp;
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
        }
    }
}

impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::Failed { .. } => None,
        }
    }
}

/// Scans `path` and returns scene-change timestamps on the media timeline.
///
/// `threshold` is FFmpeg's scene score in `0.0..=1.0`; higher values keep only
/// sharper cuts. Timestamps are normalized by subtracting `start_time`, which
/// matches the keyframe index and the preview seek coordinates.
///
/// This function blocks until FFmpeg finishes and must run outside the UI
/// thread.
pub fn detect(
    path: &Path,
    threshold: f32,
    start_time: Option<Timestamp>,
) -> Result<Vec<Timestamp>, SceneError> {
    let output = command_for(path, threshold)
        .output()
        .map_err(|source| SceneError::Spawn {
            path: path.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SceneError::Failed {
            path: path.to_path_buf(),
            status: output.status.to_string(),
            stderr: stderr_tail(&stderr),
        });
    }

    let text = String::from_utf8_lossy(&output.stderr);
    Ok(parse_cuts(&text, start_time))
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
        .arg(format!("select='gt(scene,{threshold:.3})',showinfo"))
        .arg("-f")
        .arg("null")
        .arg("-");
    command
}

/// Extracts `pts_time:` values from `showinfo` lines.
fn parse_cuts(output: &str, start_time: Option<Timestamp>) -> Vec<Timestamp> {
    let offset = start_time.map_or(0, Timestamp::micros);
    let mut cuts = Vec::new();

    for line in output.lines() {
        let Some((_, rest)) = line.split_once("pts_time:") else {
            continue;
        };
        let value = rest.split_whitespace().next().unwrap_or_default();
        let Some(time) = Timestamp::from_seconds(value) else {
            continue;
        };
        cuts.push(Timestamp::from_micros((time.micros() - offset).max(0)));
    }

    cuts.sort_unstable();
    cuts.dedup();
    cuts
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

    #[test]
    fn parses_showinfo_timestamps() {
        let output = "\
[Parsed_showinfo_1 @ 0x55] n:   0 pts:     0 pts_time:0       duration:1
[Parsed_showinfo_1 @ 0x55] n:  10 pts:  1000 pts_time:1.5     duration:1
[Parsed_showinfo_1 @ 0x55] n:  20 pts:  2000 pts_time:3.25    duration:1
";

        let cuts = parse_cuts(output, None);

        assert_eq!(
            cuts,
            vec![
                Timestamp::ZERO,
                Timestamp::from_micros(1_500_000),
                Timestamp::from_micros(3_250_000),
            ]
        );
    }

    #[test]
    fn subtracts_the_container_start_and_clamps_at_zero() {
        let output = "\
[Parsed_showinfo_1 @ 0x55] n:   0 pts:     0 pts_time:1.0 duration:1
[Parsed_showinfo_1 @ 0x55] n:  10 pts:  1000 pts_time:2.5 duration:1
";

        let cuts = parse_cuts(output, Some(Timestamp::from_micros(1_400_000)));

        assert_eq!(
            cuts,
            vec![Timestamp::ZERO, Timestamp::from_micros(1_100_000),]
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        let output = "frame=  120 fps=0.0 q=-0.0 size=N/A time=00:00:04.00 bitrate=N/A";

        assert!(parse_cuts(output, None).is_empty());
    }
}

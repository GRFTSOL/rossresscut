//! Probing where an input seek actually lands.
//!
//! Stream copy can only start at a seekable point. Matroska often stores
//! sparse cue points, so `-ss <keyframe>` can land on an earlier keyframe and
//! copy content from before the requested position. The smart-cut planner
//! probes the landing point before choosing a copy boundary.

use std::path::Path;
use std::process::Stdio;

use crate::domain::time::Timestamp;
use crate::media::process;

use super::ProbeError;

/// Returns the first video packet timestamp after `ffmpeg -ss position`.
///
/// `None` means the probe produced no packet (an unreadable seek or an empty
/// stream). The seek target is not accounted for: the returned timestamp is on
/// the source timeline because the probe passes `-copyts`.
pub fn probe_seek_start(
    path: &Path,
    stream_index: usize,
    position: Timestamp,
) -> Result<Option<Timestamp>, ProbeError> {
    let mut ffmpeg = process::command("ffmpeg");
    ffmpeg
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(format_seconds(position))
        .arg("-copyts")
        .arg("-i")
        .arg(path)
        .arg("-map")
        .arg(format!("0:{stream_index}"))
        .arg("-frames:v")
        .arg("1")
        .arg("-c")
        .arg("copy")
        .arg("-f")
        .arg("matroska")
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = ffmpeg.spawn().map_err(|source| ProbeError::Spawn {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.wait();
        return Ok(None);
    };

    let output = process::command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
            "-i",
            "pipe:0",
        ])
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|source| ProbeError::Spawn {
            path: path.to_path_buf(),
            source,
        });

    // ffmpeg may exit with SIGPIPE when ffprobe stops after one packet.
    let _ = child.wait();
    let output = output?;
    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).map_err(|source| ProbeError::OutputNotUtf8 {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(stdout.lines().find_map(|line| {
        let field = line.trim().split(',').next()?;
        Timestamp::from_seconds(field)
    }))
}

/// Formats a timestamp as decimal seconds with microsecond precision.
fn format_seconds(timestamp: Timestamp) -> String {
    let micros = timestamp.micros();
    let sign = if micros < 0 { "-" } else { "" };
    let absolute = micros.unsigned_abs();
    let seconds = absolute / 1_000_000;
    let fraction = absolute % 1_000_000;
    format!("{sign}{seconds}.{fraction:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_microsecond_precision() {
        assert_eq!(
            format_seconds(Timestamp::from_micros(1_234_567)),
            "1.234567"
        );
        assert_eq!(format_seconds(Timestamp::from_micros(0)), "0.000000");
    }
}

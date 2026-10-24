//! Keyframe discovery through FFprobe packet metadata.
//!
//! Only packet headers are read, so the scan does not decode video. Timestamps
//! are normalized to the media timeline, which makes them directly comparable
//! with positions produced by the preview and seek paths.

use std::path::Path;
use std::process::Command;

use crate::domain::media::KeyframeIndex;
use crate::domain::time::Timestamp;

use super::ProbeError;

/// Reads the keyframe timestamps of `stream_index`.
///
/// `start_time` is the container start time. FFprobe reports packet timestamps
/// on the container timeline; subtracting `start_time` yields positions that
/// match the input `-ss` seeks used by the preview decoder. Keyframes before
/// `start_time` are clamped to zero.
pub fn probe_keyframes(
    path: &Path,
    stream_index: usize,
    start_time: Option<Timestamp>,
) -> Result<KeyframeIndex, ProbeError> {
    let output = command_for(path, stream_index)
        .output()
        .map_err(|source| ProbeError::Spawn {
            path: path.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(ProbeError::Failed {
            path: path.to_path_buf(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    let stdout = String::from_utf8(output.stdout).map_err(|source| ProbeError::OutputNotUtf8 {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(KeyframeIndex::new(parse_keyframes(&stdout, start_time)))
}

fn command_for(path: &Path, stream_index: usize) -> Command {
    let mut command = Command::new("ffprobe");
    command
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg(stream_index.to_string())
        .arg("-show_entries")
        .arg("packet=pts_time,flags")
        .arg("-of")
        .arg("csv=p=0")
        .arg(path);
    command
}

fn parse_keyframes(output: &str, start_time: Option<Timestamp>) -> Vec<Timestamp> {
    let offset = start_time.map_or(0, Timestamp::micros);

    output
        .lines()
        .filter_map(|line| {
            let (timestamp, flags) = line.trim().split_once(',')?;
            if !flags.contains('K') {
                return None;
            }
            let timestamp = Timestamp::from_seconds(timestamp)?;
            let micros = timestamp.micros().checked_sub(offset)?;
            Some(Timestamp::from_micros(micros.max(0)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn command_reads_only_packet_headers() {
        let command = command_for(Path::new("input file.mp4"), 3);
        let arguments: Vec<&OsStr> = command.get_args().collect();

        assert_eq!(command.get_program(), OsStr::new("ffprobe"));
        assert_eq!(
            arguments,
            vec![
                OsStr::new("-v"),
                OsStr::new("error"),
                OsStr::new("-select_streams"),
                OsStr::new("3"),
                OsStr::new("-show_entries"),
                OsStr::new("packet=pts_time,flags"),
                OsStr::new("-of"),
                OsStr::new("csv=p=0"),
                OsStr::new("input file.mp4"),
            ]
        );
    }

    #[test]
    fn keeps_only_keyframe_packets() {
        let output = "\
0.000000,K__
0.100000,___
1.000000,K__
1.100000,___
1.200000,___
2.000000,K__
";

        assert_eq!(
            parse_keyframes(output, None),
            vec![
                Timestamp::ZERO,
                Timestamp::from_micros(1_000_000),
                Timestamp::from_micros(2_000_000),
            ]
        );
    }

    #[test]
    fn normalizes_container_start_time() {
        let output = "1.423222,K__\n2.423222,K__\n";

        assert_eq!(
            parse_keyframes(output, Some(Timestamp::from_micros(1_423_222))),
            vec![Timestamp::ZERO, Timestamp::from_micros(1_000_000)]
        );
    }

    #[test]
    fn ignores_malformed_rows() {
        let output = "N/A,K__\nnot-a-time,K__\n0.500000,K__\n";

        assert_eq!(
            parse_keyframes(output, None),
            vec![Timestamp::from_micros(500_000)]
        );
    }
}

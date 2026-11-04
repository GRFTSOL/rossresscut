//! Packet timestamp bounds used to align concatenated parts.
//!
//! FFprobe reports stream durations as `N/A` for Matroska, so the export path
//! derives each part's usable length from the first and last packet timestamps.

use std::path::Path;
use std::process::Command;

use crate::domain::time::{Duration, Timestamp};

use super::ProbeError;

/// First and effective end timestamp of one stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketBounds {
    /// Timestamp of the first packet.
    pub first: Timestamp,
    /// End of the last packet (last PTS plus its duration).
    pub end: Timestamp,
}

/// Reads the first and last packet timestamps of `selector` (`v:0`, `a:0`, …).
///
/// Returns `Ok(None)` when the stream has no packets.
pub fn probe_packet_bounds(
    path: &Path,
    selector: &str,
) -> Result<Option<PacketBounds>, ProbeError> {
    let output = command_for(path, selector)
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

    Ok(parse_bounds(&stdout))
}

fn command_for(path: &Path, selector: &str) -> Command {
    let mut command = Command::new("ffprobe");
    command
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg(selector)
        .arg("-show_entries")
        .arg("packet=pts_time,duration_time")
        .arg("-of")
        .arg("csv=p=0")
        .arg(path);
    command
}

fn parse_bounds(output: &str) -> Option<PacketBounds> {
    let mut first = None;
    let mut previous = None;
    let mut end = None;

    for line in output.lines() {
        let mut fields = line.trim().split(',');
        let Some(pts) = fields.next().and_then(Timestamp::from_seconds) else {
            continue;
        };
        let duration = fields
            .next()
            .and_then(Timestamp::from_seconds)
            .map(|duration| Duration::from_micros(duration.micros().max(0) as u64));

        first.get_or_insert(pts);
        // Packets are listed in muxing order, so the largest presentation
        // timestamp is not necessarily on the last line when B-frames reorder
        // the stream. Track the furthest end instead.
        let packet_end = duration
            .or_else(|| previous.and_then(|previous| pts.duration_since(previous)))
            .and_then(|duration| pts.checked_add(duration))
            .unwrap_or(pts);
        end = Some(end.map_or(packet_end, |end: Timestamp| end.max(packet_end)));
        previous = Some(pts);
    }

    Some(PacketBounds {
        first: first?,
        end: end?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_and_last_packet_bounds() {
        let output = "0.016016,0.033333\n0.049349,0.033333\n0.082682,0.033333\n";
        let bounds = parse_bounds(output).expect("bounds");

        assert_eq!(bounds.first, Timestamp::from_micros(16_016));
        assert_eq!(bounds.end, Timestamp::from_micros(116_015));
    }

    #[test]
    fn falls_back_to_the_previous_packet_gap() {
        let output = "1.000000,\n1.040000,\n";
        let bounds = parse_bounds(output).expect("bounds");

        assert_eq!(bounds.first, Timestamp::from_micros(1_000_000));
        assert_eq!(bounds.end, Timestamp::from_micros(1_080_000));
    }

    #[test]
    fn uses_the_largest_packet_end_not_the_last_line() {
        // B-frames make the last muxed packet end before a previous one.
        let output = "0.000000,0.033333\n0.100000,0.033333\n0.033333,0.033333\n";
        let bounds = parse_bounds(output).expect("bounds");

        assert_eq!(bounds.end, Timestamp::from_micros(133_333));
    }

    #[test]
    fn returns_none_without_packets() {
        assert_eq!(parse_bounds(""), None);
        assert_eq!(parse_bounds("N/A,\n"), None);
    }
}

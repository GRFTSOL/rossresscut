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

/// Reads the average video bit rate of `selector` inside `[start, end)`.
///
/// `start` and `end` are absolute container timestamps. Returns `Ok(None)`
/// when the range has no packets.
pub fn probe_average_bit_rate(
    path: &Path,
    selector: &str,
    start: Timestamp,
    end: Timestamp,
) -> Result<Option<u64>, ProbeError> {
    if end <= start {
        return Ok(None);
    }
    let seconds = |time: Timestamp| format!("{:.6}", time.micros() as f64 / 1_000_000.0);
    let interval = format!("{}%{}", seconds(start), seconds(end));
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg(selector)
        .arg("-read_intervals")
        .arg(interval)
        .arg("-show_entries")
        .arg("packet=size")
        .arg("-of")
        .arg("csv=p=0")
        .arg(path)
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
    Ok(bit_rate_from_size(parse_total_size(&stdout), start, end))
}

fn parse_total_size(output: &str) -> u64 {
    output
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .fold(0_u64, u64::saturating_add)
}

fn bit_rate_from_size(bytes: u64, start: Timestamp, end: Timestamp) -> Option<u64> {
    let micros = end.micros().checked_sub(start.micros())?;
    let micros = u64::try_from(micros).ok()?;
    if micros == 0 || bytes == 0 {
        return None;
    }
    bytes
        .checked_mul(8)?
        .checked_mul(1_000_000)?
        .checked_div(micros)
}

/// Reads the largest local frame duration of `selector` around `center`.
///
/// The window is clamped to the start of the file. Returns `Ok(None)` when the
/// window has fewer than two packets. Used to pick smart-cut margins on VFR
/// sources, where the average frame rate is not the local frame duration.
pub fn probe_frame_duration(
    path: &Path,
    selector: &str,
    center: Timestamp,
    window: Duration,
) -> Result<Option<Duration>, ProbeError> {
    let start = Timestamp::from_micros(
        center
            .micros()
            .saturating_sub(i64::try_from(window.micros()).unwrap_or(i64::MAX))
            .max(0),
    );
    let end = Timestamp::from_micros(
        center
            .micros()
            .saturating_add(i64::try_from(window.micros()).unwrap_or(i64::MAX)),
    );
    let seconds = |time: Timestamp| format!("{:.6}", time.micros() as f64 / 1_000_000.0);
    let interval = format!("{}%{}", seconds(start), seconds(end));
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg(selector)
        .arg("-read_intervals")
        .arg(interval)
        .arg("-show_entries")
        .arg("packet=pts_time")
        .arg("-of")
        .arg("csv=p=0")
        .arg(path)
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
    Ok(parse_frame_duration(&stdout))
}

fn parse_frame_duration(output: &str) -> Option<Duration> {
    let mut timestamps: Vec<Timestamp> = output
        .lines()
        .filter_map(|line| Timestamp::from_seconds(line.trim()))
        .collect();
    timestamps.sort_unstable();
    timestamps
        .windows(2)
        .filter_map(|window| window[1].duration_since(window[0]))
        .filter(|delta| delta.micros() > 0 && delta.micros() <= 500_000)
        .max()
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
    fn sums_packet_sizes_and_converts_to_bits_per_second() {
        let total = parse_total_size("1000\n2500\n");
        assert_eq!(total, 3500);
        assert_eq!(
            bit_rate_from_size(total, Timestamp::ZERO, Timestamp::from_micros(1_000_000)),
            Some(28_000)
        );
        assert_eq!(
            bit_rate_from_size(0, Timestamp::ZERO, Timestamp::from_micros(1)),
            None
        );
    }

    #[test]
    fn picks_the_largest_local_frame_duration() {
        let output = "0.000000\n0.008333\n0.016667\n0.041667\n0.066667\n";
        assert_eq!(
            parse_frame_duration(output),
            Some(Duration::from_micros(25_000))
        );
        assert_eq!(parse_frame_duration("0.000000\n"), None);
    }

    #[test]
    fn returns_none_without_packets() {
        assert_eq!(parse_bounds(""), None);
        assert_eq!(parse_bounds("N/A,\n"), None);
        assert_eq!(parse_total_size(""), 0);
    }
}

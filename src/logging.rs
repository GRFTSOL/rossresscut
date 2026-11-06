//! Process-wide logging setup.
//!
//! Only the binary installs a logger. Library code emits records through the
//! `log` macros, which keeps tests and future non-GUI front ends in control of
//! where log output goes.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use env_logger::Env;

/// Installs the global logger.
///
/// The default filter keeps this crate at `info` and dependencies at `warn`,
/// which avoids forwarding runtime chatter from GUI dependencies to stderr.
/// The `RUST_LOG` environment variable overrides the default.
/// Returns an error if a logger has already been installed for this process.
pub fn init() -> Result<(), log::SetLoggerError> {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn,rossresscut=info"))
        .try_init()
}

/// Records unhandled panics in `crash.log` under the user state directory.
///
/// A GUI build has no console, so a panic after startup would otherwise leave
/// no trace: the process disappears and the user has nothing to report. The
/// report also goes to stderr for development shells and to the logger.
///
/// Nothing in the hook may panic; every write is best-effort.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let report = format!(
            "---- {} rossresscut {} ----\n{info}\n{}\n",
            format_utc(SystemTime::now()),
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture(),
        );
        log::error!("panic: {info}");
        if let Some(path) = crash_log_path() {
            let _ = append_report(&path, &report);
        }
        let _ = std::io::stderr().write_all(report.as_bytes());
    }));
}

/// Path of the crash log, for documentation and issue reports.
pub fn crash_log_path() -> Option<PathBuf> {
    state_directory().map(|directory| directory.join("crash.log"))
}

/// Appends `report` to `path`, creating the directory when needed.
fn append_report(path: &Path, report: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(report.as_bytes())
}

/// Per-user state directory for logs and crash reports.
///
/// Windows uses `%LOCALAPPDATA%\rossresscut`; other platforms follow the XDG
/// state directory (`$XDG_STATE_HOME` or `~/.local/state`).
fn state_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("rossresscut"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local").join("state"))
            })
            .map(|base| base.join("rossresscut"))
    }
}

/// Formats `time` as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// A crash report needs a readable timestamp, and adding a date/time crate for
/// one line is not worth it.
fn format_utc(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60,
    )
}

/// Converts days since 1970-01-01 into a civil date.
///
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_the_unix_epoch() {
        assert_eq!(format_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_a_known_timestamp() {
        let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(format_utc(time), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn formats_a_leap_day() {
        let time = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(format_utc(time), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn crash_log_lives_under_the_state_directory() {
        let Some(path) = crash_log_path() else {
            return;
        };
        assert!(path.ends_with(Path::new("rossresscut").join("crash.log")));
    }
}

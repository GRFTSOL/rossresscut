//! Process-wide logging setup.
//!
//! Only the binary installs a logger. Library code emits records through the
//! `log` macros, which keeps tests and future non-GUI front ends in control of
//! where log output goes.

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

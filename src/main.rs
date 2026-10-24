//! Binary entry point for the desktop application.

use std::path::PathBuf;
use std::process::ExitCode;

use rossresscut::error::AppError;
use rossresscut::logging;

fn main() -> ExitCode {
    if let Err(error) = logging::init() {
        eprintln!("warning: logging was already initialized: {error}");
    }

    log::info!("rossresscut {} starting", env!("CARGO_PKG_VERSION"));

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error}");
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), AppError> {
    let initial_path = std::env::args_os().nth(1).map(PathBuf::from);

    rossresscut::ui::run(initial_path)
        .map_err(|error| AppError::startup("starting the desktop window", error))
}

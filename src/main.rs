//! Binary entry point for the desktop application.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

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
    let initial_paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    rossresscut::ui::run(initial_paths)
        .map_err(|error| AppError::startup("starting the desktop window", error))
}

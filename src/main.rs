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
            report_startup_failure(&error);
            ExitCode::FAILURE
        }
    }
}

/// Shows a native dialog so a startup failure is visible without a terminal.
///
/// Windows GUI builds have no console, and eframe can fail before any window
/// exists (for example when the GPU surface cannot be created). Without this
/// the process would exit silently.
#[cfg(target_os = "windows")]
fn report_startup_failure(error: &AppError) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("rossresscut could not start")
        .set_description(format!(
            "{error}\n\nRun from a terminal with RUST_LOG=info for details."
        ))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Non-Windows builds keep using stderr, which the launching terminal shows.
#[cfg(not(target_os = "windows"))]
fn report_startup_failure(_error: &AppError) {}

fn run() -> Result<(), AppError> {
    let initial_paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    rossresscut::ui::run(initial_paths)
        .map_err(|error| AppError::startup("starting the desktop window", error))
}

//! External process construction shared by the media layer.
//!
//! Every FFmpeg and FFprobe invocation is created through [`command`] so the
//! platform setup lives in one place.

use std::process::Command;

/// Creates a [`Command`] for `program` with platform defaults applied.
///
/// The application is a Windows GUI binary without a console of its own, so a
/// child process would otherwise open one. Callers still pass every argument
/// separately; nothing is routed through a shell.
pub(crate) fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    hide_console(&mut command);
    command
}

/// Sets `CREATE_NO_WINDOW` so the child process cannot allocate a console
/// window. Other platforms have no equivalent, and the function is a no-op
/// there.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    /// `CREATE_NO_WINDOW`, from `winbase.h`.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

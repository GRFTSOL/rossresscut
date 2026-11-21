//! External process construction shared by the media layer.
//!
//! Every FFmpeg and FFprobe invocation is created through [`command`] so the
//! platform setup and the session command history live in one place.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{LazyLock, Mutex};

/// Why an external command was started.
///
/// The UI localizes these keys; the media layer only stores the stable name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandPurpose {
    /// Writing an export or batch output.
    Export,
    /// Scanning a video for scene changes.
    SceneDetection,
    /// Building the audio waveform.
    Waveform,
    /// Extracting a text subtitle track.
    Subtitle,
    /// Merging finished export files.
    Merge,
    /// Reading media metadata or packet timestamps.
    Probe,
    /// Building the keyframe index.
    Keyframes,
    /// Decoding a preview frame or audio block.
    Preview,
}

impl CommandPurpose {
    /// Preview decoders spawn once per seek; keeping them out of the history
    /// leaves room for the commands the user actually initiated.
    const fn records_history(self) -> bool {
        !matches!(self, Self::Preview)
    }
}

/// One FFmpeg command that was started in this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExecutedCommand {
    pub(crate) purpose: CommandPurpose,
    pub(crate) command_line: String,
}

/// Upper bound so a long session cannot grow the history without limit.
const HISTORY_LIMIT: usize = 500;

static HISTORY: LazyLock<Mutex<VecDeque<ExecutedCommand>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Returns the recorded FFmpeg commands, newest first.
pub(crate) fn history() -> Vec<ExecutedCommand> {
    match HISTORY.lock() {
        Ok(history) => history.iter().rev().cloned().collect(),
        Err(poisoned) => poisoned.into_inner().iter().rev().cloned().collect(),
    }
}

fn record(program: &str, purpose: CommandPurpose, args: &[String]) {
    // Only FFmpeg commands are shown; FFprobe is an internal detail.
    if program != "ffmpeg" || !purpose.records_history() {
        return;
    }
    let entry = ExecutedCommand {
        purpose,
        command_line: render_command(program, args),
    };
    let mut history = match HISTORY.lock() {
        Ok(history) => history,
        Err(poisoned) => poisoned.into_inner(),
    };
    if history.len() >= HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(entry);
}

/// Renders a command as a copyable shell line.
fn render_command(program: &str, args: &[String]) -> String {
    let mut text = String::from(program);
    for arg in args {
        text.push(' ');
        let needs_quotes = arg.is_empty()
            || arg
                .chars()
                .any(|character| character.is_whitespace() || "'\"\\$`".contains(character));
        if needs_quotes {
            text.push('\'');
            text.push_str(&arg.replace('\'', "'\\''"));
            text.push('\'');
        } else {
            text.push_str(arg);
        }
    }
    text
}

/// Creates a command for `program` with platform defaults applied.
///
/// The application is a Windows GUI binary without a console of its own, so a
/// child process would otherwise open one. Callers still pass every argument
/// separately; nothing is routed through a shell.
pub(crate) fn command(program: &'static str, purpose: CommandPurpose) -> LoggedCommand {
    let mut command = Command::new(program);
    hide_console(&mut command);
    LoggedCommand {
        inner: command,
        program,
        purpose,
        args: Vec::new(),
        recorded: false,
    }
}

/// A [`Command`] wrapper that records the argument list before it starts.
///
/// The wrapper keeps the argument methods on itself so a builder chain never
/// falls back to [`Command`] and starts the process without being recorded.
pub(crate) struct LoggedCommand {
    inner: Command,
    program: &'static str,
    purpose: CommandPurpose,
    args: Vec<String>,
    recorded: bool,
}

impl LoggedCommand {
    pub(crate) fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        let arg = arg.as_ref();
        self.args.push(arg.to_string_lossy().into_owned());
        self.inner.arg(arg);
        self
    }

    pub(crate) fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    pub(crate) fn stdin(&mut self, configuration: Stdio) -> &mut Self {
        self.inner.stdin(configuration);
        self
    }

    pub(crate) fn stdout(&mut self, configuration: Stdio) -> &mut Self {
        self.inner.stdout(configuration);
        self
    }

    pub(crate) fn stderr(&mut self, configuration: Stdio) -> &mut Self {
        self.inner.stderr(configuration);
        self
    }

    pub(crate) fn spawn(&mut self) -> io::Result<Child> {
        self.record_once();
        self.inner.spawn()
    }

    pub(crate) fn output(&mut self) -> io::Result<Output> {
        self.record_once();
        self.inner.output()
    }

    #[cfg(test)]
    pub(crate) fn get_program(&self) -> &OsStr {
        self.inner.get_program()
    }

    #[cfg(test)]
    pub(crate) fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.inner.get_args()
    }

    fn record_once(&mut self) {
        if self.recorded {
            return;
        }
        self.recorded = true;
        record(self.program, self.purpose, &self.args);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_shell_quoting() {
        let args = vec!["-i".to_owned(), "/tmp/a file.mkv".to_owned()];
        assert_eq!(
            render_command("ffmpeg", &args),
            "ffmpeg -i '/tmp/a file.mkv'"
        );
    }

    #[test]
    fn only_ffmpeg_is_recorded() {
        record("ffprobe", CommandPurpose::Probe, &["-v".to_owned()]);
        record("ffmpeg", CommandPurpose::Preview, &["-i".to_owned()]);
        record(
            "ffmpeg",
            CommandPurpose::SceneDetection,
            &["-i".to_owned(), "scene.mp4".to_owned()],
        );

        let newest = history().into_iter().next().expect("one recorded command");
        assert_eq!(newest.purpose, CommandPurpose::SceneDetection);
        assert_eq!(newest.command_line, "ffmpeg -i scene.mp4");
    }
}

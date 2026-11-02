//! Concatenate finished export files into one output.
//!
//! The inputs are already stream-copied segments, so merging only remuxes
//! them through the concat demuxer. No frame is decoded or re-encoded.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::concat::{TempDir, concat_line};

/// Errors produced while merging export files.
#[derive(Debug)]
pub enum MergeError {
    /// No input files were provided.
    Empty,
    /// The temporary working directory could not be created.
    WorkDir(std::io::Error),
    /// The concat list could not be written.
    List {
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// FFmpeg could not be started.
    Spawn(std::io::Error),
    /// FFmpeg exited with a non-success status.
    Failed {
        /// Human-readable exit status.
        status: String,
        /// Captured standard error, possibly empty.
        stderr: String,
    },
}

impl fmt::Display for MergeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("no file was exported to merge"),
            Self::WorkDir(source) => write!(formatter, "creating the merge workspace: {source}"),
            Self::List { source } => write!(formatter, "writing the concat list: {source}"),
            Self::Spawn(source) => write!(formatter, "starting ffmpeg for the merge: {source}"),
            Self::Failed { status, stderr } => {
                write!(formatter, "ffmpeg failed while merging ({status})")?;
                let detail = stderr.trim();
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for MergeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorkDir(source) | Self::Spawn(source) => Some(source),
            Self::List { source } => Some(source),
            Self::Empty | Self::Failed { .. } => None,
        }
    }
}

/// Merges `inputs` into `output` with the concat demuxer.
pub fn merge_files(inputs: &[PathBuf], output: &Path) -> Result<(), MergeError> {
    if inputs.is_empty() {
        return Err(MergeError::Empty);
    }

    let work_dir = TempDir::create().map_err(MergeError::WorkDir)?;
    let list = work_dir.path().join("inputs.txt");
    let mut text = String::new();
    for input in inputs {
        text.push_str(&concat_line(input));
        text.push('\n');
    }
    fs::write(&list, text).map_err(|source| MergeError::List { source })?;

    let result = Command::new("ffmpeg")
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(&list)
        .arg("-c")
        .arg("copy")
        .arg("-y")
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(MergeError::Spawn)?;

    if !result.status.success() {
        return Err(MergeError::Failed {
            status: result.status.to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_input_list() {
        let error = merge_files(&[], Path::new("/tmp/out.mp4")).expect_err("empty");

        assert!(matches!(error, MergeError::Empty));
    }

    #[test]
    fn reports_a_missing_input_file() {
        let inputs = vec![PathBuf::from("/nonexistent/clip.mp4")];
        let output = std::env::temp_dir().join("rossresscut-merge-missing.mp4");

        let error = merge_files(&inputs, &output).expect_err("missing input");

        assert!(matches!(error, MergeError::Failed { .. }), "{error}");
        fs::remove_file(&output).ok();
    }
}

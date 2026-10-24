//! Shared error type for failures that cross application boundaries.
//!
//! Every error carries the operation that failed; lower-level causes stay
//! reachable through [`std::error::Error::source`] instead of being flattened
//! into a string.

use std::error::Error;
use std::fmt;
use std::io;

/// Result alias used by fallible application code.
pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// Failure raised while performing an application-level operation.
#[derive(Debug)]
pub enum AppError {
    /// A filesystem or other operating-system I/O operation failed.
    Io {
        /// The operation that was being attempted.
        context: String,
        /// The underlying I/O failure.
        source: io::Error,
    },
    /// The application could not start its runtime.
    Startup {
        /// The operation that was being attempted.
        context: String,
        /// The underlying startup failure.
        source: Box<dyn Error + Send + Sync>,
    },
}

impl AppError {
    /// Creates an error for a failed I/O operation.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// Creates an error for a failed application runtime start.
    pub fn startup(
        context: impl Into<String>,
        source: impl Into<Box<dyn Error + Send + Sync>>,
    ) -> Self {
        Self::Startup {
            context: context.into(),
            source: source.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (context, source): (&str, &dyn Error) = match self {
            Self::Io { context, source } => (context, source),
            Self::Startup { context, source } => (context, source.as_ref()),
        };

        write!(formatter, "{context}: {source}")
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Startup { source, .. } => Some(source.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_display_includes_context_and_cause() {
        let error = AppError::io(
            "reading media file",
            io::Error::new(io::ErrorKind::NotFound, "no such file"),
        );

        assert_eq!(error.to_string(), "reading media file: no such file");
        assert!(error.source().is_some());
    }

    #[test]
    fn startup_error_display_includes_context_and_cause() {
        let error = AppError::startup(
            "starting the desktop window",
            io::Error::other("no display"),
        );

        assert_eq!(error.to_string(), "starting the desktop window: no display");
        assert!(error.source().is_some());
    }
}

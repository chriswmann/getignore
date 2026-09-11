//! The crate's error types.
//!
//! [`AppError`] covers failures outside query resolution, such as I/O, network
//! and (de)serialisation errors. [`TemplateError`] covers a query that does not
//! resolve to exactly one template.
//!
//! [`AppError`] variants that wrap another error return it from `source`,
//! keeping the underlying errors reachable to anything walking the chain;
//! message-only variants return `None`.
//!
//! [`AppError::Network`] carries a `&'static str` saying what was being
//! attempted, because `ureq`'s own errors do not identify the failed request.
//! That is also why there is deliberately no `From<ureq::Error>` impl: every
//! network call has to supply its context.
//!
//! `From` impls do exist for `io::Error`, `serde_json::Error` and
//! `SystemTimeError`, which carry enough of their own detail.

use std::error::Error;
use std::fmt::{self, Display};
use std::io;
use std::time::SystemTimeError;

use crate::resolve::TemplatePath;

#[derive(Debug)]
pub enum AppError {
    HomeDir(String),
    DiskWrite(String),
    Io(io::Error),
    Network {
        context: &'static str,
        source: ureq::Error,
    },
    Serialisation(serde_json::Error),
    Time(SystemTimeError),
    TruncatedTree,
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Io(source) => Some(source),
            AppError::Network { context: _, source } => Some(source),
            AppError::Serialisation(source) => Some(source),
            AppError::Time(source) => Some(source),
            _ => None,
        }
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Network { context, source } => {
                write!(f, "Network error while {context}: source: {source}")
            }
            AppError::TruncatedTree => write!(f, "GH tree response was truncated"),
            AppError::Io(err) => write!(f, "IO error: {err}"),
            AppError::HomeDir(err) => write!(f, "Home dir error: {err}"),
            AppError::DiskWrite(err) => write!(f, "Disk write error: {err}"),
            AppError::Time(err) => write!(f, "Time error: {err}"),
            AppError::Serialisation(err) => write!(f, "(De)serialisation error: {err}"),
        }
    }
}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
impl From<SystemTimeError> for AppError {
    fn from(err: SystemTimeError) -> Self {
        Self::Time(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialisation(err)
    }
}

#[derive(Debug)]
pub enum TemplateError {
    DidYouMean {
        query: String,
        suggestions: Vec<TemplatePath>,
    },
    NotFound(String),
    EmptyQuery,
}

impl Error for TemplateError {}

impl Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateError::NotFound(query) => {
                write!(f, "No gitignore entry found for {query}")
            }
            TemplateError::DidYouMean {
                query,
                suggestions: _,
            } => {
                write!(f, "Could not identify single template for {query}.")
            }
            TemplateError::EmptyQuery => write!(f, "No language detected"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn io_error_exposes_its_source() {
        let error = AppError::Io(io::Error::new(io::ErrorKind::NotFound, "test failure"));
        let source = error.source().expect("IO error should have a source");

        assert!(source.downcast_ref::<io::Error>().is_some());
    }

    #[test]
    fn network_error_exposes_its_source() {
        let error = AppError::Network {
            context: "test context",
            source: ureq::Error::HostNotFound,
        };

        let source = error.source().expect("network error should have a source");

        assert!(source.downcast_ref::<ureq::Error>().is_some());
    }

    #[test]
    fn serialisation_error_exposes_its_source() {
        let error = AppError::Serialisation(
            serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err(),
        );
        let source = error
            .source()
            .expect("serialisation error should have a source");

        assert!(source.downcast_ref::<serde_json::Error>().is_some());
    }

    #[test]
    fn time_error_exposes_its_source() {
        let later = UNIX_EPOCH + Duration::from_secs(1);
        let system_time_error = UNIX_EPOCH.duration_since(later).unwrap_err();
        let error = AppError::Time(system_time_error);
        let source = error.source().expect("time error should have a source");

        assert!(source.downcast_ref::<SystemTimeError>().is_some());
    }

    #[test]
    fn message_only_error_has_no_source() {
        let error = AppError::TruncatedTree;
        assert!(error.source().is_none());
    }
}

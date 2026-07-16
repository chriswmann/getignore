use std::error::Error;
use std::fmt::{self, Display};
use std::io;
use std::path::PathBuf;
use std::time::SystemTimeError;

#[derive(Debug)]
pub enum AppError {
    NoLanguage(PathBuf),
    Parse {
        input: String,
        error: String,
    },
    AmbiguousLanguage {
        language: String,
        matches: Vec<String>,
    },
    DidYouMean {
        language: String,
        best: String,
        rest: Vec<String>,
    },
    Disk(String),
    Io(io::Error),
    Network(ureq::Error),
    LanguageNotFound(String),
    Serialisation(serde_json::Error),
    Time(SystemTimeError),
    TruncatedTree,
}

impl Error for AppError {}
impl Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NoLanguage(path) => write!(
                f,
                "No language associated with gitignore entry {}",
                path.display()
            ),
            AppError::LanguageNotFound(language) => {
                write!(f, "No gitignore entry found for {language}")
            }
            AppError::AmbiguousLanguage { language, matches } => write!(
                f,
                "Multiple gitignore entries matched {language}: {}",
                matches.join(", ")
            ),
            AppError::Network(err) => write!(f, "Network error: {err}"),
            AppError::TruncatedTree => write!(f, "GH tree response was truncated"),
            AppError::Io(err) => write!(f, "IO error: {err}"),
            AppError::Disk(err) => write!(f, "Disk error: {err}"),
            AppError::Time(err) => write!(f, "Time error: {err}"),
            AppError::Serialisation(err) => write!(f, "(De)serialisation error: {err}"),
            AppError::Parse { input, error } => write!(f, "Could not parse '{input}': {error}"),
            AppError::DidYouMean {
                language,
                best,
                rest,
            } => write!(
                f,
                "Could not identify single template for {language}. Best match was {best}. Other candidates are {rest:?}",
            ),
        }
    }
}

impl From<ureq::Error> for AppError {
    fn from(err: ureq::Error) -> Self {
        Self::Network(err)
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

use std::fmt::Write;

use tracing::instrument;

use crate::error::{AppError, TemplateError};

#[instrument]
pub fn display_app_error(app_error: &AppError) -> String {
    match app_error {
        AppError::HomeDir(err) => err.clone(),
        AppError::DiskWrite(err) => format!("Could not write to disk: {err}"),
        AppError::Io(err) => err.to_string(),
        AppError::Network { context, source } => format!("Network error {context}, {source}"),
        AppError::Time(err) => format!("There was an error reading the system time: {err}"),
        AppError::Serialisation(err) => format!("There was an error parsing data: {err}"),
        AppError::TruncatedTree => {
            "The github git tree was truncated. Please try again".to_string()
        }
    }
}

#[instrument]
pub fn display_template_error(error: &TemplateError) -> String {
    match error {
        TemplateError::DidYouMean { query, suggestions } => {
            let mut buf =
                format!("'{query}' did not match any templates. Found these suggestions:\n");
            for suggestion in suggestions {
                writeln!(buf, "{suggestion}").unwrap();
            }
            buf
        }
        TemplateError::NotFound(query) => {
            format!("Could not find any templates which matched {query}")
        }
        TemplateError::EmptyQuery => "Template query was empty".to_string(),
    }
}

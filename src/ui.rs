use tracing::{debug, instrument};

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
        TemplateError::DidYouMean { query, best, rest } => {
            let mut buf: String =
                format!("{query} did not match any templates. Did you mean {best}");
            match rest.as_slice() {
                [first] => buf.push_str(format!(" or {first}").as_str()),
                [first, second] => buf.push_str(format!(", {first} or {second}").as_str()),
                _ => debug!(
                    "DidYouMean had {} additional matches, so they were discarded",
                    rest.len()
                ),
            }
            buf.insert(buf.len(), '?');
            buf
        }
        TemplateError::NotFound(query) => {
            format!("Could not find any templates which matched {query}")
        }
        TemplateError::EmptyQuery => "Template query was empty".to_string(),
    }
}

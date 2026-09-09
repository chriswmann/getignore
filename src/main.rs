//! `gi`: fetch a `.gitignore` template from the `github/gitignore` repository.
//!
//! Given a language name, resolves it against a cached index of the
//! repository's templates and writes the matching template to `./.gitignore`,
//! or to the path given by `-d/--destination`.
//!
//! The flow is: parse [`Opts`], locate the cache directory, load or refresh the
//! template index, wrap it in a [`Catalogue`], [`resolve()`] the query to a
//! template path, read that template from the blob cache or fetch it, then
//! write it out.
//!
//! Both the index and the individual templates are cached, so repeat runs are
//! fast and work offline. A stale index that cannot be refreshed is used as a
//! fallback rather than failing the run. Anything unrecoverable surfaces as an
//! [`AppError`].

use std::io::{self, Write};
use std::{fs, path, process::exit, time::Duration};

use clap::Parser;
use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use tracing::debug;
use tracing_subscriber::EnvFilter;
use ureq::Agent;

mod catalogue;
mod error;
mod github;
mod option;
mod resolve;
mod store;

use error::AppError;
use github::fetch_template;
use option::Opts;
use resolve::resolve_template_path;
use store::unix_now;

use crate::error::TemplateError;
use crate::{
    catalogue::Catalogue,
    store::{atomic_write_file, load_blob_from_cache, load_index, save_blob_to_cache},
};

fn main() -> Result<(), AppError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();
    let opts = Opts::parse();
    let app_strategy_args = AppStrategyArgs {
        top_level_domain: "io".to_string(),
        author: "chriswmann".to_string(),
        app_name: "getignore".to_string(),
    };
    let strategy = match choose_app_strategy(app_strategy_args) {
        Ok(strategy) => strategy,
        Err(err) => {
            let app_error = AppError::HomeDir(err.to_string());
            let message = display_app_error(&app_error);
            eprintln!("{message}");
            exit(1);
        }
    };
    let index_path = strategy.cache_dir().join("index.json");
    let blobs_dir = strategy.cache_dir().join("files");
    fs::create_dir_all(&blobs_dir)?;
    let ttl = Duration::from_hours(24 * 7);
    let now = unix_now()?;
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: Agent = config.into();
    let index = load_index(&agent, index_path, ttl, now)?;
    let catalogue = Catalogue::new(index);
    let language = opts.language;
    let template_path = match resolve_template_path(language, &catalogue) {
        Ok(path) => path,
        Err(err) => {
            let message = display_template_error(&err);
            eprintln!("{message}");
            exit(1);
        }
    };
    let entry = catalogue.entry(template_path.as_str()).expect(
        "Catalogue should contain the template path since we resolved it from the catalogue",
    );
    let source_commit = catalogue.source_commit();
    let sha = entry.sha.as_str();
    let blob_path = blobs_dir.join(sha);
    let template = if blob_path.exists() {
        debug!("Blob path {} exists", blob_path.display());
        load_blob_from_cache(&blob_path)?
    } else {
        let template = fetch_template(&agent, source_commit, template_path.as_str())?;
        if let Err(err) = save_blob_to_cache(&template, &blob_path) {
            let message = display_app_error(&err);
            eprintln!("{message}");
            exit(1);
        }
        template
    };

    let path = opts.destination;
    if path.exists() && !should_proceed(path.as_path())? {
        println!("Exiting without saving template.");
        exit(0);
    }
    match atomic_write_file(&template, &path) {
        Ok(()) => debug!("template written to {}", path.display()),
        Err(err) => {
            let message = display_app_error(&err);
            eprintln!("{message}");
            exit(1);
        }
    }
    Ok(())
}

fn should_proceed(path: impl AsRef<path::Path>) -> Result<bool, AppError> {
    let mut input = String::new();

    loop {
        print!(
            "Target file {} already exists. Overwrite [y/N]? ",
            path.as_ref().display()
        );
        io::stdout()
            .flush()
            .expect("Should be able to flush stdout");
        input.clear();
        io::stdin().read_line(&mut input)?;
        input = input.trim().to_lowercase();
        if input.is_empty() {
            return Ok(false);
        }
        match input.as_str() {
            "y" => return Ok(true),
            "n" => return Ok(false),
            _ => {}
        }
    }
}

fn display_app_error(app_error: &AppError) -> String {
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

fn display_template_error(error: &TemplateError) -> String {
    match error {
        TemplateError::DidYouMean { query, best, rest } => {
            let mut buf: String =
                format!("{query} did not match any templates. Did you mean {best}");
            match rest.as_slice() {
                [first] => buf.push_str(format!("or {first}").as_str()),
                [first, second] => buf.push_str(format!(", {first} or {second}").as_str()),
                _ => {}
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

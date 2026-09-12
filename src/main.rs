//! `gi`: fetch a `.gitignore` template from the `github/gitignore` repository.
//!
//! Given a language name, resolves it against a cached index of the
//! repository's templates and writes the matching template to `./.gitignore`,
//! or to the path given by `-d/--destination`.
//!
//! The flow is: parse [`Opts`], locate the cache directory, load or refresh the
//! template index, and wrap it in a [`Catalogue`]. With `--list`, print the
//! catalogue and exit. Otherwise, resolve the query to a template path with
//! [`resolve_template_path`], read that template from the blob cache or fetch
//! it, ask before overwriting an existing destination, then write it out.
//!
//! Both the index and the individual templates are cached, so repeat runs are
//! fast and work offline. A stale index that cannot be refreshed is used as a
//! fallback rather than failing the run. Some errors return from `main` as an
//! [`AppError`]. Others are printed through [`ui`] and end the process with
//! exit code 1.

use std::{fs, process::exit, time::Duration};

use clap::Parser;
use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use tracing::{debug, warn};
use tracing_subscriber::EnvFilter;
use ureq::Agent;

mod catalogue;
mod error;
mod github;
mod option;
mod resolve;
mod store;
mod ui;

use error::AppError;
use github::fetch_template;
use option::Opts;
use resolve::resolve_template_path;
use store::unix_now;
use ui::{display_app_error, display_catalogue, display_template_error, should_proceed};

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
    if opts.list {
        let catalogue_display = display_catalogue(&catalogue);
        println!("{catalogue_display}");
        exit(0);
    }
    let language = opts
        .language
        .expect("Should have a language if --list is not set");
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
            // Logging a warning is consistent with how an index cache write failure is handled
            // in store.rs.
            warn!("{message}");
        }
        template
    };

    let path = opts.destination;
    if path.exists() && !should_proceed(&template_path, path.as_path())? {
        println!("Exiting without saving template.");
        exit(0);
    }
    match atomic_write_file(&template, &path) {
        Ok(()) => println!("Writing {template_path} to {}", path.display()),
        Err(err) => {
            let message = display_app_error(&err);
            eprintln!("{message}");
            exit(1);
        }
    }
    Ok(())
}

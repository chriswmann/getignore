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
use tracing::{debug, warn};
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
    let strategy = choose_app_strategy(app_strategy_args).map_err(|_| {
        AppError::Disk("etcetera app strategy could not be constructed".to_string())
    })?;
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
    let template_path = resolve_template_path(language, &catalogue)?;
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
            warn!(
                "Could not save blob to cache {}: {err}",
                blob_path.display()
            );
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
        Err(err) => warn!("Error writing template to {}: {err}", path.display()),
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

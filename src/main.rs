use std::{fs, time::Duration};

use clap::Parser;
use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use tracing::{debug, warn};
use tracing_subscriber::EnvFilter;
use ureq::Agent;

mod catalogue;
mod errors;
mod github;
mod options;
mod resolve;
mod store;

use errors::AppError;
use github::fetch_template;
use options::Opts;
use resolve::resolve;
use store::{fetch_and_cache_index, is_not_stale, load_index_from_cache, unix_now};

use crate::{
    catalogue::Catalogue,
    resolve::Resolution,
    store::{atomic_write_file, load_blob_from_cache, save_blob_to_cache},
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
    let index = match load_index_from_cache(&index_path) {
        Ok(index) if is_not_stale(&index, ttl, now) => {
            debug!("Serving from cache");
            index
        }
        Ok(index) => {
            debug!("Cache is stale, refetching");
            match fetch_and_cache_index(&agent, &index_path, now) {
                Ok(index) => index,
                Err(err) => {
                    debug!(
                        "Cache is stale but could not reach GitHub, using cached index as fallback: {err}"
                    );
                    index
                }
            }
        }
        Err(err) => {
            warn!("Cache unavailable ({err}), fetching");
            fetch_and_cache_index(&agent, &index_path, now)?
        }
    };
    let catalogue = Catalogue::new(index);
    let language = opts.language;
    let template_path = match resolve(&language, &catalogue) {
        Resolution::Resolved(path) => Ok(path),
        Resolution::Ambiguous { matches } => Err(AppError::AmbiguousLanguage {
            language: language.clone(),
            matches,
        }),
        Resolution::DidYouMean { best, rest } => Err(AppError::DidYouMean {
            language: language.clone(),
            best,
            rest,
        }),
        Resolution::NotFound => Err(AppError::LanguageNotFound(language.clone())),
    }?;
    let entry = catalogue.entry(&template_path).unwrap();
    let source_commit = catalogue.source_commit();
    let sha = entry.sha.as_str();
    let blob_path = blobs_dir.join(sha);
    let template = if blob_path.exists() {
        println!("Blob path {} exists", blob_path.display());
        load_blob_from_cache(&blob_path)?
    } else {
        let template = fetch_template(&agent, source_commit, &template_path)?;
        if let Err(err) = save_blob_to_cache(&template, &blob_path) {
            warn!(
                "Could not save blob to cache {}: {err}",
                blob_path.display()
            );
        }
        template
    };
    match opts.destination {
        Some(path) => match atomic_write_file(&template, &path) {
            Ok(()) => debug!("template written to {}", path.display()),
            Err(err) => warn!("Error writing template to {}: {err}", path.display()),
        },
        None => println!("{template}"),
    }
    Ok(())
}

use std::fs;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use directories::BaseDirs;
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
use store::{fetch_and_cache_index, is_not_stale, load_index_from_cache, unix_now};

use crate::store::{atomic_write_file, load_blob_from_cache, save_blob_to_cache};

const APP_NAME: &str = "getignore";

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();
    let opts = Opts::parse();
    let base = BaseDirs::new().ok_or_else(|| AppError::Disk("Could not open BaseDirs".into()))?;
    let cache_dir = base.cache_dir().join(APP_NAME);
    let blob_cache_dir = cache_dir.join("files/");
    fs::create_dir_all(&blob_cache_dir)?;
    let cache_file = cache_dir.join("getignore.json");
    let cache_file = cache_file.as_path();
    let ttl = Duration::from_hours(24 * 7);
    let now = unix_now()?;
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: Agent = config.into();
    let index = match load_index_from_cache(cache_file) {
        Ok(index) if is_not_stale(&index, ttl, now) => {
            debug!("Serving from cache");
            index
        }
        Ok(index) => {
            debug!("Cache is stale, refetching");
            match fetch_and_cache_index(&agent, &opts, cache_file, now) {
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
            fetch_and_cache_index(&agent, &opts, cache_file, now)?
        }
    };
    let language = opts.language;
    let path = &language;
    let entry = index.entries.get(path).expect("Should have python entry");
    let sha = entry.sha.as_str();
    let blob_path = blob_cache_dir.join(sha);
    let template = if blob_path.exists() {
        println!("Blob path {} exists", blob_path.display());
        load_blob_from_cache(&blob_path)?
    } else {
        let template = fetch_template(&agent, &index.source_commit, path)?;
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

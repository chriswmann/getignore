use std::fs;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use directories::BaseDirs;
use tracing::debug;
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
use store::{fetch_and_cache, is_not_stale, load_cache, unix_now};

const APP_NAME: &str = "getignore";

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let opts = Opts::parse();
    let base = BaseDirs::new().ok_or_else(|| AppError::Disk("Could not open BaseDirs".into()))?;
    let cache_dir = base.cache_dir().join(APP_NAME);
    let template_cache_dir = cache_dir.join("files/");
    fs::create_dir_all(&template_cache_dir)?;
    let cache_file = cache_dir.join("getignore.json");
    let cache_file = cache_file.as_path();
    let ttl = Duration::from_hours(24 * 7);
    let now = unix_now()?;
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: Agent = config.into();
    let index = match load_cache(cache_file) {
        Ok(index) if is_not_stale(&index, ttl, now) => {
            debug!("Serving from cache");
            index
        }
        Ok(_) => {
            debug!("Cache is stale, refetching");
            fetch_and_cache(&agent, &opts, cache_file, now)?
        }
        Err(err) => {
            debug!("Cache unavailable ({err}), fetching");
            fetch_and_cache(&agent, &opts, cache_file, now)?
        }
    };
    let template = fetch_template(&agent, &index.source_commit, "Python.gitignore")?;
    dbg!(template);
    Ok(())
}

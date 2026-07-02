use std::error::Error;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, BufReader};
use std::path::Path;
use std::time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH};
use std::{ffi::OsStr, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use tracing::debug;
use tracing_subscriber::EnvFilter;
use ureq::{Agent, Body, http::Response};

const APP_NAME: &str = "getignore";
const GITIGNORE_LIST_URL: &str =
    "https://api.github.com/repos/github/gitignore/git/trees/main?recursive=1";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Opts {
    #[arg(short, long)]
    gitignore_list_url: Option<String>,
    #[arg(short, long)]
    destination: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedTree {
    fetched_at: u64,
    tree: GitTreeResponse,
}

#[derive(Debug, Serialize, Deserialize)]
struct GitTreeResponse {
    sha: String,
    url: String,
    tree: Vec<GitTreeEntry>,
    truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct GitTreeEntry {
    path: PathBuf,
    mode: String,
    #[serde(rename = "type")]
    kind: GitObjectKind,
    sha: String,
    url: String,

    size: Option<u64>, // Only present for blobs, not for directories or trees.
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum GitObjectKind {
    Blob,
    Tree,
    Commit,
}

#[derive(Debug)]
enum AppError {
    NoLanguage(PathBuf),
    Network(ureq::Error),
    Io(io::Error),
    Disk(String),
    Time(SystemTimeError),
    Serialisation(serde_json::Error),
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
            AppError::Network(err) => write!(f, "Network error: {err}"),
            AppError::Io(err) => write!(f, "IO error: {err}"),
            AppError::Disk(err) => write!(f, "Disk error: {err}"),
            AppError::Time(err) => write!(f, "Time error: {err}"),
            AppError::Serialisation(err) => write!(f, "(De)serialisation error: {err}"),
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

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let opts = Opts::parse();
    let base = BaseDirs::new().ok_or_else(|| AppError::Disk("Could not open BaseDirs".into()))?;
    let cache_dir = base.cache_dir().join(APP_NAME);
    fs::create_dir_all(&cache_dir)?;
    let cache_file = cache_dir.join("getignore.json");
    let cache_file = cache_file.as_path();
    let ttl = Duration::from_hours(24 * 7);
    let now = unix_now()?;
    let tree = match load_cache(cache_file) {
        Ok(cached) if is_not_stale(&cached, ttl, now) => {
            debug!("Serving from cache");
            cached.tree
        }
        Ok(_) => {
            debug!("Cache is stale, refetching");
            fetch_and_cache(&opts, cache_file, now)?
        }
        Err(err) => {
            debug!("Cache unavailable ({err}), fetching");
            fetch_and_cache(&opts, cache_file, now)?
        }
    };
    dbg!(tree);
    Ok(())
}

fn load_from_github(opts: &Opts) -> Result<GitTreeResponse> {
    let tree = load_repo_tree(opts)?;
    let tree: GitTreeResponse = serde_json::from_str(&tree)?;
    for entry in &tree.tree {
        let path = entry.path.clone();
        if entry.kind == GitObjectKind::Blob && path.extension() == Some(OsStr::new("gitignore")) {
            let language = path
                .file_name()
                .ok_or_else(|| AppError::NoLanguage(path.clone()))?;
            let category = path.parent().filter(|&p| !p.as_os_str().is_empty());

            println!("{category:?}");
            println!("{}", language.display());
            println!("{}", path.display());
        }
    }
    Ok(tree)
}

fn get_language_list_response(agent: &Agent, url: &str) -> Result<Response<Body>, ureq::Error> {
    let response = agent
        .get(url)
        .header("User-Agent", "rust-gitignore-client")
        .header("Accept", "application/vnd.github+json")
        .call()?;
    Ok(response)
}

fn load_repo_tree(opts: &Opts) -> Result<String, AppError> {
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: Agent = config.into();
    let url = opts
        .gitignore_list_url
        .as_deref()
        .unwrap_or(GITIGNORE_LIST_URL);
    let mut response = get_language_list_response(&agent, url)?;
    Ok(response.body_mut().read_to_string()?)
}

fn save_cache(cache_file: &Path, cached: &CachedTree) -> Result<()> {
    let json = serde_json::to_string_pretty(cached)?;
    fs::write(cache_file, json)?;
    Ok(())
}

fn load_cache(cache_file: &Path) -> Result<CachedTree, AppError> {
    let file = fs::File::open(cache_file)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

fn unix_now() -> Result<u64, AppError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn is_not_stale(cached: &CachedTree, ttl: Duration, now: u64) -> bool {
    now.saturating_sub(cached.fetched_at) < ttl.as_secs()
}

fn fetch_and_cache(opts: &Opts, cache_file: &Path, now: u64) -> Result<GitTreeResponse> {
    let response = load_from_github(opts)?;
    let cached_tree = CachedTree {
        fetched_at: now,
        tree: response,
    };
    save_cache(cache_file, &cached_tree)?;
    Ok(cached_tree.tree)
}

use std::collections::BTreeMap;
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
    #[arg(short, long)]
    language: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Index {
    version: u32,
    fetched_at: u64,
    source_commit: String,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    name: String,
    sha: String,
}

impl TryFrom<GitTreeEntry> for Entry {
    type Error = AppError;
    fn try_from(git_entry: GitTreeEntry) -> Result<Self, Self::Error> {
        Ok(Self {
            name: git_entry
                .path
                .file_stem()
                .ok_or_else(|| AppError::NoLanguage(git_entry.path.clone()))?
                .to_string_lossy()
                .to_string(),
            sha: git_entry.sha,
        })
    }
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
    AmbiguousLanguage {
        language: String,
        matches: Vec<String>,
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
    let index = match load_cache(cache_file) {
        Ok(index) if is_not_stale(&index, ttl, now) => {
            debug!("Serving from cache");
            index
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
    let lookup = index_lookup(&opts.language, &index)?;
    dbg!(lookup);
    Ok(())
}

fn load_from_github(opts: &Opts) -> Result<GitTreeResponse> {
    let tree = load_repo_tree(opts)?;
    Ok(serde_json::from_str(&tree)?)
}

fn build_index(response: GitTreeResponse, fetched_at: u64) -> Result<Index, AppError> {
    if response.truncated {
        return Err(AppError::TruncatedTree);
    }

    let mut entries = BTreeMap::new();

    for git_entry in response.tree {
        if git_entry.kind == GitObjectKind::Blob
            && git_entry.path.extension().and_then(OsStr::to_str) == Some("gitignore")
        {
            let path = git_entry.path.to_string_lossy().to_string();
            let entry = Entry::try_from(git_entry)?;
            entries.insert(path, entry);
        }
    }

    Ok(Index {
        version: 1,
        fetched_at,
        source_commit: response.sha,
        entries,
    })
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

fn save_cache(cache_file: &Path, index: &Index) -> Result<()> {
    let json = serde_json::to_string_pretty(index)?;
    fs::write(cache_file, json)?;
    Ok(())
}

fn load_cache(cache_file: &Path) -> Result<Index, AppError> {
    let file = fs::File::open(cache_file)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

fn unix_now() -> Result<u64, AppError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn is_not_stale(index: &Index, ttl: Duration, now: u64) -> bool {
    now.saturating_sub(index.fetched_at) < ttl.as_secs()
}

fn fetch_and_cache(opts: &Opts, cache_file: &Path, now: u64) -> Result<Index> {
    let response = load_from_github(opts)?;
    let index = build_index(response, now)?;
    save_cache(cache_file, &index)?;
    Ok(index)
}

fn index_lookup<'a>(language: &str, index: &'a Index) -> Result<(&'a str, &'a Entry), AppError> {
    let needle = normalise_lookup(language);

    let exact_matches: Vec<_> = index
        .entries
        .iter()
        .filter(|(path, entry)| {
            normalise_lookup(path) == needle || normalise_lookup(&entry.name) == needle
        })
        .map(|(path, entry)| (path.as_str(), entry))
        .collect();

    match exact_matches.as_slice() {
        [(path, entry)] => return Ok((*path, *entry)),
        [] => {}
        matches => {
            return Err(AppError::AmbiguousLanguage {
                language: language.to_string(),
                matches: matches
                    .iter()
                    .map(|(path, _)| (*path).to_string())
                    .collect(),
            });
        }
    }

    let loose_matches: Vec<_> = index
        .entries
        .iter()
        .filter(|(path, entry)| {
            normalise_lookup(path).contains(&needle)
                || normalise_lookup(&entry.name).contains(&needle)
        })
        .map(|(path, entry)| (path.as_str(), entry))
        .collect();

    match loose_matches.as_slice() {
        [(path, entry)] => Ok((*path, *entry)),
        [] => Err(AppError::LanguageNotFound(language.to_string())),
        matches => Err(AppError::AmbiguousLanguage {
            language: language.to_string(),
            matches: matches
                .iter()
                .map(|(path, _)| (*path).to_string())
                .collect(),
        }),
    }
}

fn normalise_lookup(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(".gitignore")
        .to_ascii_lowercase()
}

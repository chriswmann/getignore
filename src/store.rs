use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::github::{Entry, GitObjectKind, GitTreeResponse, load_from_github};
use crate::options::Opts;
#[derive(Debug, Serialize, Deserialize)]
pub struct Index {
    version: u32,
    pub fetched_at: u64,
    source_commit: String,
    entries: BTreeMap<String, Entry>,
}
pub fn build_index(response: GitTreeResponse, fetched_at: u64) -> Result<Index, AppError> {
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
fn save_cache(cache_file: &Path, index: &Index) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(index)?;
    fs::write(cache_file, json)?;
    Ok(())
}
pub fn load_cache(cache_file: &Path) -> Result<Index, AppError> {
    let file = fs::File::open(cache_file)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

pub fn unix_now() -> Result<u64, AppError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

pub fn is_not_stale(index: &Index, ttl: Duration, now: u64) -> bool {
    now.saturating_sub(index.fetched_at) < ttl.as_secs()
}

pub fn fetch_and_cache(opts: &Opts, cache_file: &Path, now: u64) -> Result<Index, AppError> {
    let response = load_from_github(opts)?;
    let index = build_index(response, now)?;
    save_cache(cache_file, &index)?;
    Ok(index)
}

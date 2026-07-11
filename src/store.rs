use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, process};

use serde::{Deserialize, Serialize};
use tracing::warn;
use ureq::Agent;

use crate::errors::AppError;
use crate::github::{BlobSha, CommitSha};
use crate::github::{GitObjectKind, GitTreeEntry, GitTreeResponse, load_from_github};
use crate::options::Opts;

#[derive(Debug, Deserialize, Serialize)]
pub struct Index {
    version: u32,
    pub fetched_at: u64,
    pub source_commit: CommitSha,
    pub entries: BTreeMap<String, Entry>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub sha: BlobSha,
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

pub fn save_index_to_cache(index: &Index, cache_file: &Path) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(index)?;
    atomic_write_file(&json, cache_file)
}
pub fn load_index_from_cache(cache_file: &Path) -> Result<Index, AppError> {
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

pub fn fetch_and_cache_index(
    agent: &Agent,
    opts: &Opts,
    cache_file: &Path,
    now: u64,
) -> Result<Index, AppError> {
    let response = load_from_github(agent, opts)?;
    let index = build_index(response, now)?;
    if let Err(err) = save_index_to_cache(&index, cache_file) {
        warn!("Could not cache index to {}: {err}", cache_file.display());
    }
    Ok(index)
}

pub fn save_blob_to_cache(blob: &str, cache_file: &Path) -> Result<(), AppError> {
    atomic_write_file(blob, cache_file)
}

pub fn load_blob_from_cache(cache_file: &Path) -> Result<String, AppError> {
    let content = fs::read_to_string(cache_file)?;
    Ok(content)
}

pub fn atomic_write_file(contents: &str, dest: &Path) -> Result<(), AppError> {
    let dir = dest
        .parent()
        .ok_or_else(|| AppError::Disk(format!("No parent directory for {}", dest.display())))?;
    let file_name = dest
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| AppError::Disk(format!("No file name in {}", dest.display())))?;
    let tmp_path = dir.join(format!("{file_name}.{}.tmp", process::id()));

    fs::write(&tmp_path, contents)?;
    if let Err(err) = fs::rename(&tmp_path, dest) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }
    Ok(())
}

#[expect(clippy::unreadable_literal)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[test]
    fn index_deserialises_cache_fixture() {
        let index =
            serde_json::from_str::<Index>(include_str!("../tests/fixtures/cache-fixture.json"))
                .unwrap();
        assert_eq!(index.version, 1);
        assert_eq!(index.fetched_at, 1750765200);
        assert_eq!(
            index.source_commit,
            CommitSha::new("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678")
        );
        assert_eq!(index.entries["Python.gitignore"].name, "Python");
    }
    #[test]
    fn build_index_carries_metadata_across() {
        let git_tree_response = serde_json::from_str::<GitTreeResponse>(include_str!(
            "../tests/fixtures/trimmed-trees.json"
        ))
        .expect("Should be able to load trimmed trees test fixture as GitTreeResponse");

        let fetched_at = 12345678;
        let index = build_index(git_tree_response, fetched_at).unwrap();
        assert_eq!(index.fetched_at, fetched_at);
        assert_eq!(
            index.source_commit,
            CommitSha::new("dcc0fc7bc2b5ba480cf117ad1be31bafceeaff46")
        );
    }

    #[test]
    fn build_index_filters_to_gitignore_blobs_and_keys_by_path() {
        let git_tree_response = serde_json::from_str::<GitTreeResponse>(include_str!(
            "../tests/fixtures/trimmed-trees.json"
        ))
        .expect("Should be able to load trimmed trees test fixture as GitTreeResponse");

        let fetched_at = 12345678;
        let index = build_index(git_tree_response, fetched_at).unwrap();
        assert_eq!(
            index
                .entries
                .get("Python.gitignore")
                .expect("Python entry should be in test data")
                .sha,
            BlobSha::new("b3ec7d5e13aa02435b3b4372b8cb22b57429924a")
        );
        assert_eq!(index.entries.len(), 6);
        assert_eq!(
            index
                .entries
                .get("community/embedded/AtmelStudio.gitignore")
                .expect("AtmelStudio entry should be in test data")
                .name,
            "AtmelStudio"
        );
        assert_eq!(
            index
                .entries
                .get("ecu.test.gitignore")
                .expect("ecu.test.gitignore entry should be in test data")
                .name,
            "ecu.test"
        );
    }
    #[test]
    fn build_index_returns_truncated_tree_error_when_tree_is_truncated() {
        let git_tree_response = serde_json::from_str::<GitTreeResponse>(include_str!(
            "../tests/fixtures/truncated-trimmed-trees.json"
        ))
        .expect("Should be able to load truncated, trimmed trees test fixture as GitTreeResponse");
        let output = build_index(git_tree_response, 123456);
        assert_matches!(output, Err(AppError::TruncatedTree));
    }
}

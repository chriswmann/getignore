use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;
use ureq::{Agent, Body, http::Response};

use crate::errors::AppError;
use crate::options::Opts;

const GITIGNORE_LIST_URL: &str =
    "https://api.github.com/repos/github/gitignore/git/trees/main?recursive=1";

#[derive(Debug, Deserialize, Serialize)]
pub struct Index {
    version: u32,
    pub fetched_at: u64,
    source_commit: String,
    entries: BTreeMap<String, Entry>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub sha: String,
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
#[derive(Debug, Serialize, Deserialize)]
pub struct GitTreeResponse {
    pub sha: String,
    url: String,
    pub tree: Vec<GitTreeEntry>,
    pub truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GitTreeEntry {
    pub path: PathBuf,
    mode: String,
    #[serde(rename = "type")]
    pub kind: GitObjectKind,
    sha: String,
    url: String,

    size: Option<u64>, // Only present for blobs, not for directories or trees.
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitObjectKind {
    Blob,
    Tree,
    Commit,
}

pub fn load_from_github(opts: &Opts) -> Result<GitTreeResponse, AppError> {
    let tree = load_repo_tree(opts)?;
    Ok(serde_json::from_str(&tree)?)
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
            "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"
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
            "dcc0fc7bc2b5ba480cf117ad1be31bafceeaff46"
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
            "b3ec7d5e13aa02435b3b4372b8cb22b57429924a"
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

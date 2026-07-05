use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use ureq::{Agent, Body, http::Response};

use crate::errors::AppError;
use crate::options::Opts;

const GITIGNORE_LIST_URL: &str =
    "https://api.github.com/repos/github/gitignore/git/trees/main?recursive=1";

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
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

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ureq::{Agent, Body, http::Response};

use crate::errors::AppError;
use crate::options::Opts;

const GITIGNORE_LIST_URL: &str =
    "https://api.github.com/repos/github/gitignore/git/trees/main?recursive=1";

const GITIGNORE_BLOB_URL: &str = "https://raw.githubusercontent.com/github/gitignore";

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
    pub sha: String,
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

pub fn load_from_github(agent: &Agent, opts: &Opts) -> Result<GitTreeResponse, AppError> {
    let tree = load_repo_tree(agent, opts)?;
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

fn load_repo_tree(agent: &Agent, opts: &Opts) -> Result<String, AppError> {
    let url = opts
        .gitignore_list_url
        .as_deref()
        .unwrap_or(GITIGNORE_LIST_URL);
    let mut response = get_language_list_response(agent, url)?;
    Ok(response.body_mut().read_to_string()?)
}

pub fn fetch_template(agent: &Agent, commit: &str, path: &str) -> Result<String, AppError> {
    let url = format!("{GITIGNORE_BLOB_URL}/{commit}/{path}");
    let mut response = agent
        .get(&url)
        .header("User-Agent", "rust-gitignore-client")
        .call()?;

    let body = response.body_mut();
    let body = body.read_to_string()?;
    dbg!(&body);
    Ok(body)
}

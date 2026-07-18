use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use tracing::debug;
use ureq::Agent;

use crate::errors::AppError;

const MAIN_COMMIT_URL: &str = "https://api.github.com/repos/github/gitignore/commits/main";

const RECURSIVE_TREE_URL: &str =
    "https://api.github.com/repos/github/gitignore/git/trees/{}?recursive=1";

const GITIGNORE_BLOB_URL: &str = "https://raw.githubusercontent.com/github/gitignore";

#[derive(Debug, Serialize, Deserialize)]
pub struct GitRecursiveTreeResponse {
    pub source_commit: CommitSha,
    tree_sha: TreeSha,
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
    pub sha: BlobSha,
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct GitCommitResponse {
    sha: CommitSha,
    tree: GitCommitTree,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct GitCommitTree {
    sha: TreeSha,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct CommitSha(String);

impl Display for CommitSha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct TreeSha(String);

impl Display for TreeSha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TreeSha {
    #[cfg(test)]
    pub(crate) fn new(sha: &str) -> Self {
        Self(sha.to_string())
    }
}

impl CommitSha {
    #[cfg(test)]
    pub(crate) fn new(sha: &str) -> Self {
        Self(sha.to_string())
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct BlobSha(String);

impl AsRef<Path> for BlobSha {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl BlobSha {
    pub fn new(sha: &str) -> Self {
        Self(sha.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn get_repo_data(agent: &Agent) -> Result<GitRecursiveTreeResponse, AppError> {
    let main_commit = resolve_main_commit_reference(agent)?;
    let tree_url = git_tree_url(&main_commit.tree.sha);
    load_repo_tree(agent, &tree_url)
}

fn resolve_main_commit_reference(agent: &Agent) -> Result<GitCommitResponse, AppError> {
    let mut response = agent
        .get(MAIN_COMMIT_URL)
        .header("User-Agent", "rust-gitignore-client")
        .call()?;
    let body = response.body_mut().read_to_string()?;
    serde_json::from_str::<GitCommitResponse>(&body)
        .or_else(|err| Err(AppError::Serialisation(err)))
}

fn load_repo_tree(agent: &Agent, tree_url: &str) -> Result<GitRecursiveTreeResponse, AppError> {
    let mut response = agent
        .get(tree_url)
        .header("User-Agent", "rust-gitignore-client")
        .call()?;
    let body = response.body_mut().read_to_string()?;
    serde_json::from_str::<GitRecursiveTreeResponse>(&body)
        .or_else(|err| Err(AppError::Serialisation(err)))
}

pub fn fetch_template(agent: &Agent, commit: &CommitSha, path: &str) -> Result<String, AppError> {
    let url = format!("{GITIGNORE_BLOB_URL}/{commit}/{path}");
    debug!("Fetch template URL: {url}");
    let mut response = agent
        .get(&url)
        .header("User-Agent", "rust-gitignore-client")
        .call()?;

    let body = response.body_mut();
    let body = body.read_to_string()?;
    dbg!(&body);
    Ok(body)
}

fn git_tree_url(tree_sha: &TreeSha) -> String {
    format!("https://api.github.com/repos/github/gitignore/git/trees/{tree_sha}?recursive=1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_response_keeps_commit_and_tree_shas_distinct() {
        let response = serde_json::from_str::<GitCommitResponse>(include_str!(
            "../tests/fixtures/commit-fixture.json"
        ))
        .expect("Should be able to deserialise commit test fixture");

        assert_eq!(
            response.sha,
            CommitSha::new("7638417db6d59f3c431d3e1f261cc637155684cd"),
        );
        assert_eq!(
            response.tree.sha,
            TreeSha::new("691272480426f78a0138979dd3ce63b77f706feb"),
        );
    }

    #[test]
    fn recursive_tree_url_uses_tree_sha() {
        let tree_sha = TreeSha::new("691272480426f78a0138979dd3ce63b77f706feb");

        assert_eq!(
            git_tree_url(&tree_sha),
            "https://api.github.com/repos/github/gitignore/git/trees/691272480426f78a0138979dd3ce63b77f706feb?recursive=1"
        );
    }

    #[test]
    fn ensure_we_use_the_commit_url_for_trees() {
        assert_eq!(
            RECURSIVE_TREE_URL,
            "https://api.github.com/repos/github/gitignore/git/trees/{}?recursive=1"
        );
    }
}

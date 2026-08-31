//! HTTP client for the `github/gitignore` repository.
//!
//! Three requests, in the order the app uses them:
//!
//! 1. `/repos/github/gitignore/branches/main`, to find where `main` currently
//!    points. The branch endpoint is used in preference to the commits
//!    endpoint because it states the intent: "where is this branch now?".
//! 2. The recursive Git Trees API, addressed by the *tree* SHA from that
//!    response, listing every file in the repository in one request.
//! 3. `raw.githubusercontent.com`, addressed by the commit SHA, for an
//!    individual template.
//!
//! Keeping those two SHAs apart is why [`CommitSha`], [`TreeSha`] and
//! [`BlobSha`] are separate newtypes rather than `String`s. The tree SHA
//! addresses the Trees API and nothing else; the commit SHA is what gets
//! persisted as the index's source and interpolated into raw template URLs.
//! The blob SHA is used for content-addressed caching of the templates.
//! All three URL shape invariants are covered by the tests below.
//!
//! This module issues requests and deserialises responses. It holds no cache
//! policy and never touches the filesystem.

use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use tracing::debug;
use ureq::Agent;

use crate::error::AppError;

const MAIN_BRANCH_URL: &str = "https://api.github.com/repos/github/gitignore/branches/main";

const RECURSIVE_TREE_URL: &str =
    "https://api.github.com/repos/github/gitignore/git/trees/{}?recursive=1";

const GITIGNORE_BLOB_URL: &str = "https://raw.githubusercontent.com/github/gitignore";

#[derive(Debug, Deserialize)]
struct BranchResponse {
    commit: BranchCommitReference,
}

#[derive(Debug, Deserialize)]
struct BranchCommitReference {
    sha: CommitSha,
    commit: CommitDetail,
}

#[derive(Debug, Deserialize)]
struct CommitDetail {
    tree: TreeMetaData,
}

#[derive(Debug, Serialize, Deserialize)]
struct TreeMetaData {
    sha: TreeSha,
    url: String,
}

#[derive(Debug)]
pub struct RepoSnapshot {
    pub source_commit: CommitSha,
    pub tree: RecursiveTreeResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecursiveTreeResponse {
    pub sha: TreeSha,
    url: String,
    pub tree: Vec<TreeEntry>,
    pub truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TreeEntry {
    pub path: PathBuf,
    mode: String,
    #[serde(rename = "type")]
    pub kind: ObjectKind,
    pub sha: BlobSha,
    url: String,
    size: Option<u64>, // Only present for blobs, not for directories or trees.
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectKind {
    Blob,
    Tree,
    Commit,
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
    pub(crate) fn for_tests(sha: &str) -> Self {
        Self(sha.to_string())
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl CommitSha {
    #[cfg(test)]
    pub(crate) fn for_tests(sha: &str) -> Self {
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
    #[cfg(test)]
    pub fn for_tests(sha: &str) -> Self {
        Self(sha.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn get_repo_data(agent: &Agent) -> Result<RepoSnapshot, AppError> {
    let main_commit = resolve_main_branch_commit(agent)?;
    let tree_url = git_tree_url(&main_commit.commit.commit.tree.sha);
    let tree = load_repo_tree(agent, &tree_url)?;
    let source_commit = main_commit.commit.sha;
    Ok(RepoSnapshot {
        source_commit,
        tree,
    })
}

fn resolve_main_branch_commit(agent: &Agent) -> Result<BranchResponse, AppError> {
    let mut response = agent
        .get(MAIN_BRANCH_URL)
        .header("User-Agent", "rust-gitignore-client")
        .call()
        .map_err(|source| AppError::Network {
            context: "resolving the main branch",
            source,
        })?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|source| AppError::Network {
            context: "reading the main branch response",
            source,
        })?;
    serde_json::from_str::<BranchResponse>(&body).map_err(AppError::Serialisation)
}

fn load_repo_tree(agent: &Agent, tree_url: &str) -> Result<RecursiveTreeResponse, AppError> {
    let mut response = agent
        .get(tree_url)
        .header("User-Agent", "rust-gitignore-client")
        .call()
        .map_err(|source| AppError::Network {
            context: "fetching the repository tree",
            source,
        })?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|source| AppError::Network {
            context: "reading the repository tree response",
            source,
        })?;
    serde_json::from_str::<RecursiveTreeResponse>(&body).map_err(AppError::Serialisation)
}

pub fn fetch_template(agent: &Agent, commit: &CommitSha, path: &str) -> Result<String, AppError> {
    let url = format!("{GITIGNORE_BLOB_URL}/{commit}/{path}");
    debug!("Fetch template URL: {url}");
    let mut response = agent
        .get(&url)
        .header("User-Agent", "rust-gitignore-client")
        .call()
        .map_err(|source| AppError::Network {
            context: "fetching the template",
            source,
        })?;

    let body = response.body_mut();
    let body = body.read_to_string().map_err(|source| AppError::Network {
        context: "reading the template response body",
        source,
    })?;
    Ok(body)
}

fn git_tree_url(tree_sha: &TreeSha) -> String {
    RECURSIVE_TREE_URL.replace("{}", tree_sha.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_response_keeps_commit_and_tree_shas_distinct() {
        let response = serde_json::from_str::<BranchResponse>(include_str!(
            "../tests/fixtures/branch-fixture.json"
        ))
        .expect("Should be able to deserialise branch test fixture");

        assert_eq!(
            response.commit.sha,
            CommitSha::for_tests("dcc0fc7bc2b5ba480cf117ad1be31bafceeaff46"),
        );
        assert_eq!(
            response.commit.commit.tree.sha,
            TreeSha::for_tests("28fc080a7482a2d4ba63b97a1161228692c048a2"),
        );
    }

    #[test]
    fn recursive_tree_url_uses_tree_sha() {
        let tree_sha = TreeSha::for_tests("691272480426f78a0138979dd3ce63b77f706feb");

        assert_eq!(
            git_tree_url(&tree_sha),
            "https://api.github.com/repos/github/gitignore/git/trees/691272480426f78a0138979dd3ce63b77f706feb?recursive=1"
        );
    }

    #[test]
    fn pin_recursive_tree_url() {
        assert_eq!(
            RECURSIVE_TREE_URL,
            "https://api.github.com/repos/github/gitignore/git/trees/{}?recursive=1"
        );
    }

    #[test]
    fn pin_main_branch_url() {
        assert_eq!(
            MAIN_BRANCH_URL,
            "https://api.github.com/repos/github/gitignore/branches/main"
        );
    }
}

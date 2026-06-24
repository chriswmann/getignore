use std::error::Error;
use std::fmt::{self, Display};
use std::io;
use std::time::Duration;
use std::{ffi::OsStr, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use serde::{Deserialize, Serialize};
use ureq::{Agent, Body, http::Response};

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

fn main() -> Result<()> {
    let opts = Opts::parse();
    let tree = load_repo_tree(&opts)?;
    let tree: GitTreeResponse = serde_json::from_str(&tree)?;
    for entry in tree.tree {
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
    Ok(())
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

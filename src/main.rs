use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use ureq::Agent;

const GITIGNORE_LIST_URL: &str = "https://github.com/github/gitignore";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Opts {
    #[arg(short, long)]
    gitignore_list_url: Option<String>,
    #[arg(short, long)]
    destination: PathBuf,
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: Agent = config.into();
    let url = opts
        .gitignore_list_url
        .as_deref()
        .unwrap_or(GITIGNORE_LIST_URL);
    let body = get_language_list(&agent, url)?;
    println!("body: {body}");
    Ok(())
}

fn get_language_list(agent: &Agent, url: &str) -> Result<String, ureq::Error> {
    let body = agent.get(url).call()?.body_mut().read_to_string()?;
    Ok(body)
}

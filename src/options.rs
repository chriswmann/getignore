use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Opts {
    #[arg(short, long)]
    pub gitignore_list_url: Option<String>,
    #[arg(short, long)]
    destination: PathBuf,
    #[arg(short, long)]
    language: String,
}

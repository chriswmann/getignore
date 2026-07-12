use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Opts {
    pub language: String,
    #[arg(short, long)]
    pub gitignore_list_url: Option<String>,
    #[arg(short, long)]
    pub destination: Option<PathBuf>,
}

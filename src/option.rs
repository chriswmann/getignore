//! Command line interface definition.
//!
//! The language is a plain positional `String` rather than a constrained value.
//! Validating it here would mean reading the cache during argument parsing, and
//! the tiered matching in [`mod@crate::resolve`] gives better errors than clap can:
//! aliases, prefixes and "did you mean" suggestions.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Opts {
    #[arg(required_unless_present = "list")]
    pub language: Option<String>,
    #[arg(default_value_t = false, short, long, conflicts_with = "language")]
    pub list: bool,
    #[arg(default_value = ".gitignore", short, long)]
    pub destination: PathBuf,
}

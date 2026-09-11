//! Command line interface definition.
//!
//! The language is a plain positional `String` rather than a constrained value.
//! Validating it here would mean reading the cache during argument parsing, and
//! the tiered matching in [`mod@crate::resolve`] gives better errors than clap can:
//! aliases, substring matches and "did you mean" suggestions.
//!
//! The language is required unless `--list` is given, and the two conflict, so
//! `main` can rely on having exactly one of them.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// Template to fetch, such as python or rust. Aliases and part of a name also work
    #[arg(required_unless_present = "list")]
    pub language: Option<String>,
    /// Print the path of every available template, then exit
    #[arg(default_value_t = false, short, long, conflicts_with = "language")]
    pub list: bool,
    /// File to write the template to
    #[arg(default_value = ".gitignore", short, long)]
    pub destination: PathBuf,
}

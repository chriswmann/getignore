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
    pub language: String,
    #[arg(short, long)]
    pub destination: Option<PathBuf>,
}

//! Query resolution to identify templates. No IO.
//!
//! Each index path gives one candidate for each trailing part of the path (its
//! tails), so `community/BoxLang/ColdBox.gitignore` gives `coldbox`,
//! `boxlang/coldbox` and `community/boxlang/coldbox`.
//!
//! [`resolve`] tries four tiers in order and stops at the first that answers:
//!
//! 1. exact match of the query against the candidate tails,
//! 2. the hand-maintained alias table in `aliases.txt` (`js` -> `Node`), which
//!    rewrites the query and retries the exact tier,
//! 3. substring match: the templates whose file stem contains the query,
//!    case-insensitively,
//! 4. fuzzy match of the query against the candidate tails by
//!    `strsim::osa_distance`, within a distance of one.
//!
//! A tier with no match passes the query to the next tier, and if no tier
//! matches, the result is [`Resolution::NotFound`]. When the exact or substring
//! tier matches more than one template, it returns [`Resolution::DidYouMean`]
//! with the matches whose file stem is within a distance of two of the first
//! match. The fuzzy tier always returns [`Resolution::DidYouMean`], with the
//! candidates ordered best first, and leaves the decision to the caller, so a
//! typo won't silently fetch the wrong template.
//!
//! Normalising means lowercasing and stripping any `.gitignore` suffix, and is
//! applied to both sides of every comparison. [`TemplatePath`] carries the
//! verbatim index key rather than the normalised form, so the subsequent fetch
//! uses the repository's own casing.

use std::fmt;

use strsim::osa_distance;
use tracing::{debug, instrument};

use crate::catalogue::Catalogue;
use crate::error::TemplateError;

const SUFFIX: &str = ".gitignore";

/// The exact index key for a template, stored verbatim (e.g.
/// `community/BoxLang/ColdBox.gitignore`). Never rebuilt from parts:
/// `main` uses it directly to look up the entry and fetch the blob.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Clone)]
pub struct TemplatePath(String);

impl TemplatePath {
    fn new(path: &str) -> Self {
        Self(path.to_string())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn file_stem(&self) -> &str {
        let stem = self
            .0
            .strip_suffix(SUFFIX)
            .expect("Should be able to strip '.gitignore' from a TemplatePath");
        match stem.rsplit_once('/') {
            Some((_, file_name)) => file_name,
            None => stem,
        }
    }
}

impl fmt::Display for TemplatePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
struct OsaResult<'a> {
    distance: usize,
    path: &'a str,
}

impl<'a> OsaResult<'a> {
    fn new(distance: usize, path: &'a str) -> Self {
        Self { distance, path }
    }
}

#[derive(Debug, PartialEq)]
pub enum Resolution {
    /// Language recognised and the gitignore will be provided.
    Resolved(TemplatePath),
    /// Language not recognised but one or more suggestions found. Suggestions
    /// from the fuzzy tier are ordered best first. Suggestions from the other
    /// tiers keep the candidate order.
    DidYouMean { suggestions: Vec<TemplatePath> },
    /// Language not recognised, no suggestions found.
    NotFound,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Candidate {
    tail: NormalisedSlug,
    path: TemplatePath,
}

impl Candidate {
    #[cfg(test)]
    pub fn for_tests(tail: &str, path: TemplatePath) -> Self {
        Self {
            tail: tail
                .try_into()
                .expect("Should be able to normalise tail for tests"),
            path,
        }
    }
}

#[derive(Debug, Hash, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NormalisedSlug {
    slug: String,
}

impl NormalisedSlug {
    fn as_str(&self) -> &str {
        self.slug.as_str()
    }
}

impl TryFrom<String> for NormalisedSlug {
    type Error = TemplateError;
    fn try_from(mut slug: String) -> Result<Self, Self::Error> {
        slug.make_ascii_lowercase();
        if let Some(stem) = slug.strip_suffix(SUFFIX) {
            slug.truncate(stem.len());
        }
        if slug.is_empty() {
            return Err(TemplateError::EmptyQuery);
        }
        Ok(Self { slug })
    }
}

impl TryFrom<&str> for NormalisedSlug {
    type Error = TemplateError;
    fn try_from(slug: &str) -> Result<Self, Self::Error> {
        Self::try_from(slug.to_string())
    }
}

#[instrument(skip(catalogue))]
pub fn resolve_template_path(
    query: String,
    catalogue: &Catalogue,
) -> Result<TemplatePath, TemplateError> {
    let normalised_query = query.clone().try_into()?;
    let template_path = match resolve(&normalised_query, catalogue) {
        Resolution::Resolved(path) => {
            debug!("Query resolved to {path:?}");
            Ok(path)
        }
        Resolution::DidYouMean { suggestions } => {
            debug!(
                "Could not resolve {normalised_query:?} but found suggestions (AppError::DidYouMean)."
            );
            Err(TemplateError::DidYouMean { query, suggestions })
        }
        Resolution::NotFound => {
            debug!("Cound not resolve query, {query} not found");
            Err(TemplateError::NotFound(query))
        }
    }?;
    Ok(template_path)
}

/// Pure resolution logic, no I/O. Tiers are tried in order: exact
/// (case-insensitive), alias, substring, then fuzzy suggestions.
#[instrument(skip(catalogue))]
fn resolve(query: &NormalisedSlug, catalogue: &Catalogue) -> Resolution {
    let candidates = candidates(catalogue);

    exact_tier(query, &candidates)
        .or_else(|| {
            debug!("Exact tier failed, fell through to alias tier");
            alias_tier(query, &candidates)
        })
        .or_else(|| {
            debug!("Alias tier failed, fell through to contains_tier");
            contains_tier(query, &candidates)
        })
        .or_else(|| {
            debug!("Contains tier failed, fell through to fuzzy tier");
            fuzzy_tier(query, &candidates)
        })
        .unwrap_or(Resolution::NotFound)
}

// Builds candidate matches from the catalogue, sorted by tail and then by
// path, with duplicate (tail, path) pairs removed, so the order is
// deterministic. Each path still gives one candidate per tail. A tier that
// compares something other than the tail, such as `contains_tier`, which
// compares the file stem, can therefore match the same path more than once.
fn candidates(catalogue: &Catalogue) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = catalogue
        .entries()
        .flat_map(|(path, _name)| derive(path))
        .collect();

    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

/// Derives the match candidates (tails) for an index path, paired with the
/// verbatim key the tail resolves to. The tail is what queries are
/// compared against; the `TemplatePath` is what gets fetched. The tail is normalised
/// up-front so that comparison of the segmented tails with the normalised query
/// (user-given language argument) can be performed later.
#[instrument]
fn derive(path: &str) -> Vec<Candidate> {
    let normalised_path: NormalisedSlug = path.try_into().unwrap();

    let path = TemplatePath::new(path);

    tails(normalised_path.as_str())
        .map(|tail| {
            let normalised_tail = tail.try_into().unwrap();
            Candidate {
                tail: normalised_tail,
                path: path.clone(),
            }
        })
        .collect()
}

fn tails(normalised: &str) -> impl Iterator<Item = &str> {
    normalised
        .rmatch_indices('/')
        .map(|(i, _)| &normalised[i + 1..])
        .chain(std::iter::once(normalised))
}

#[instrument(skip(candidates))]
fn exact_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let filtered_paths: Vec<_> = candidates
        .iter()
        .filter(|&candidate| {
            debug!("query: {query:12?} candidate: {:20?}", candidate.tail);
            *query == candidate.tail
        })
        .map(|candidate| candidate.path.clone())
        .collect();
    match_filtered_paths(query.as_str(), &filtered_paths)
}

#[instrument(skip(candidates))]
fn alias_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let target =
        aliases().find_map(|(alias, target)| (alias == query.as_str()).then_some(target))?;
    debug!("alias matched, target={target:?}");
    exact_tier(&target, candidates)
}

#[instrument(skip(candidates))]
fn contains_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let filtered_paths: Vec<TemplatePath> = candidates
        .iter()
        .filter(|candidate| {
            candidate
                .path
                .file_stem()
                .to_lowercase()
                .contains(query.as_str())
        })
        .map(|candidate| candidate.path.clone())
        .collect();
    match_filtered_paths(query.as_str(), &filtered_paths)
}

/// Turns the paths that a tier matched into a resolution. No match passes the
/// query to the next tier, and one match resolves. For more than one match,
/// the suggestions are the paths whose lowercased file stem is within a
/// distance of two of the stem of the first match.
#[instrument]
fn match_filtered_paths(query: &str, filtered_paths: &[TemplatePath]) -> Option<Resolution> {
    match filtered_paths {
        [] => None,
        [only] => Some(Resolution::Resolved(only.clone())),
        suggestions => {
            let first_stem = suggestions.first().expect("Should be able to get first suggestion as we're in a match arm known to have multiple").file_stem();
            let similar_paths = suggestions
                .iter()
                .filter_map(|r| {
                    (osa_distance(&r.file_stem().to_lowercase(), &first_stem.to_lowercase()) < 3)
                        .then_some(r.clone())
                })
                .collect();
            Some(Resolution::DidYouMean {
                suggestions: similar_paths,
            })
        }
    }
}

fn fuzzy_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let query = &query;
    let mut matches: Vec<OsaResult> = candidates
        .iter()
        .filter_map(|candidate| {
            match strsim::osa_distance(query.as_str(), candidate.tail.as_str()) {
                d if d < 2 => {
                    let osa_result = OsaResult::new(d, candidate.path.as_str());
                    debug!("{osa_result:?}");
                    Some(osa_result)
                }
                _ => None,
            }
        })
        .collect();
    if matches.is_empty() {
        None
    } else {
        matches.sort_unstable();
        let suggestions = matches.iter().map(|o| TemplatePath::new(o.path)).collect();

        Some(Resolution::DidYouMean { suggestions })
    }
}

/// Parsed (alias, target) pairs from the embedded aliases.txt file
fn aliases() -> impl Iterator<Item = (&'static str, NormalisedSlug)> {
    include_str!("aliases.txt")
        .lines()
        .filter(|&l| !l.starts_with('#'))
        .filter_map(|l| {
            l.split_once('=').map(|(alias, target)| {
                (
                    alias.trim(),
                    target
                        .trim()
                        .try_into()
                        .expect("Should be able to normalise targets from aliases.txt"),
                )
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_path_handles_root_templates() {
        let rust_template = TemplatePath::new("Rust.gitignore");
        assert_eq!(rust_template.as_str(), "Rust.gitignore");
        let cold_box_template = TemplatePath::new("community/BoxLang/ColdBox.gitignore");
        assert_eq!(
            cold_box_template.as_str(),
            "community/BoxLang/ColdBox.gitignore"
        );
    }

    #[test]
    fn derive_preserves_dotted_entry_names() {
        assert_eq!(
            derive("ecu.test.gitignore"),
            vec![Candidate::for_tests(
                "ecu.test",
                TemplatePath::new("ecu.test.gitignore")
            )],
        );
    }

    #[test]
    fn exact_tier_resolves_to_resolution_resolved_when_there_is_only_one_match() {
        let candidates = vec![
            Candidate {
                tail: "rust"
                    .try_into()
                    .expect("Should be able to normalise 'rust'"),
                path: TemplatePath::new("Rust.gitignore"),
            },
            Candidate {
                tail: "rustici"
                    .try_into()
                    .expect("Should be able to normalise 'rustici'"),
                path: TemplatePath::new("community/DM/Rustici.gitignore"),
            },
            Candidate {
                tail: "xilinx"
                    .try_into()
                    .expect("Should be able to normalise 'xilinx'"),
                path: TemplatePath::new("community/Xilinx.gitignore"),
            },
        ];
        let normalised_query = NormalisedSlug::try_from("rust".to_string())
            .expect("Should be able to normalise 'rust'");
        let answer = exact_tier(&normalised_query, &candidates);
        assert_eq!(
            answer,
            Some(Resolution::Resolved(TemplatePath::new("Rust.gitignore"))),
        );
    }

    #[test]
    fn resolve_resolves_a_case_insensitive_exact_name() {
        let expected = Resolution::Resolved(TemplatePath::new("Python.gitignore"));
        let normalised_query = NormalisedSlug::try_from("python".to_string())
            .expect("Should be able to normalise 'python'");
        assert_eq!(resolve(&normalised_query, &test_catalogue()), expected);
    }

    #[test]
    fn resolve_resolves_to_resolution_resolved_when_one_path_matches() {
        let normalised_query: NormalisedSlug = "coldbox"
            .try_into()
            .expect("Should be able to normalise 'coldbox'");
        let expected =
            Resolution::Resolved(TemplatePath::new("community/BoxLang/ColdBox.gitignore"));
        assert_eq!(resolve(&normalised_query, &test_catalogue()), expected);
    }

    #[test]
    fn contains_tier_resolves_to_none_when_no_paths_match() {
        let normalised_query: NormalisedSlug = "incorrect_path"
            .try_into()
            .expect("Should be able to normalise 'incorrect_path'");
        assert_eq!(contains_tier(&normalised_query, &test_candidates()), None);
    }

    #[test]
    fn contains_tier_resolves_to_resolution_resolved_when_one_path_matches() {
        let normalised_query: NormalisedSlug = "node"
            .try_into()
            .expect("Should be able to normalise 'node'");
        let expected = Some(Resolution::Resolved(TemplatePath::new("Node.gitignore")));
        assert_eq!(
            contains_tier(&normalised_query, &test_candidates()),
            expected
        );
    }

    fn test_catalogue() -> Catalogue {
        let entries = [
            ("Python.gitignore", "Python"),
            ("Node.gitignore", "Node"),
            ("Racket.gitignore", "Racket"),
            ("community/Racket.gitignore", "Racket"),
            ("community/BoxLang/ColdBox.gitignore", "ColdBox"),
        ];
        Catalogue::for_tests(&entries)
    }

    /// The candidates for `test_catalogue()`: one per tail, in `BTreeMap` path
    /// order, shortest tail first within each path. `candidates()` gives the
    /// same set sorted by tail, so a test that uses this list must not depend
    /// on its order.
    fn test_candidates() -> Vec<Candidate> {
        vec![
            Candidate {
                tail: "node"
                    .try_into()
                    .expect("Should be able to normalise 'node'"),
                path: TemplatePath::new("Node.gitignore"),
            },
            Candidate {
                tail: "python"
                    .try_into()
                    .expect("Should be able to normalise 'python'"),
                path: TemplatePath::new("Python.gitignore"),
            },
            Candidate {
                tail: "racket"
                    .try_into()
                    .expect("Should be able to normalise 'racket'"),
                path: TemplatePath::new("Racket.gitignore"),
            },
            Candidate {
                tail: "coldbox"
                    .try_into()
                    .expect("Should be able to normalise 'coldbox'"),
                path: TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            },
            Candidate {
                tail: "boxlang/coldbox"
                    .try_into()
                    .expect("Should be able to normalise 'boxlang/coldbox'"),
                path: TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            },
            Candidate {
                tail: "community/boxlang/coldbox"
                    .try_into()
                    .expect("Should be able to normalise 'community/boxlang/coldbox'"),
                path: TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            },
            Candidate {
                tail: "racket"
                    .try_into()
                    .expect("Should be able to normalise 'racket'"),
                path: TemplatePath::new("community/Racket.gitignore"),
            },
            Candidate {
                tail: "community/racket"
                    .try_into()
                    .expect("Should be able to normalise 'community/racket'"),
                path: TemplatePath::new("community/Racket.gitignore"),
            },
        ]
    }
}

//! Query resolution to identify templates. No IO.
//!
//! [`resolve`] tries four tiers in order and stops at the first that answers:
//!
//! 1. exact match against the normalised path, reporting
//!    [`Resolution::Ambiguous`] when more than one entry matches,
//! 2. the hand-maintained alias table in `aliases.txt` (`js` -> `Node`), which
//!    rewrites the query and retries the exact tier,
//! 3. substring match: the first entry whose path contains the query wins.
//!    Only the query is normalised here, not the path, so this tier is
//!    case-sensitive against the repository's own casing, and it neither
//!    requires the match to be unique nor reports ambiguity,
//! 4. fuzzy match by `strsim::osa_distance`, within a distance of two.
//!
//! The fuzzy tier returns [`Resolution::DidYouMean`] with the candidates
//! ordered best first and leaves the decision to the caller, so a typo
//! won't silently fetch the wrong template.
//!
//! Normalising means lowercasing and stripping any `.gitignore` suffix, and is
//! applied to both sides of every comparison. [`TemplatePath`] carries the
//! verbatim index key rather than the normalised form, so the subsequent fetch
//! uses the repository's own casing.

use std::fmt;

use crate::catalogue::Catalogue;
use crate::error::AppError;

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
    /// There are more than one gitignores for this language.
    Ambiguous { matches: Vec<String> },
    /// Language not recognised but one or more suggestions found. Rest is ordered best first.
    DidYouMean { best: String, rest: Vec<String> },
    /// Language not recognised, no suggestions found.
    NotFound,
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolved(path) => write!(f, "Found exact match: {}", path.as_str()),
            Self::Ambiguous { matches } => write!(f, "Found several matches: {matches:?}"),
            Self::DidYouMean { best, rest } => {
                if rest.is_empty() {
                    write!(f, "Did you mean {best}?")
                } else {
                    write!(f, "Did you mean {best} or one of these: {rest:?}")
                }
            }
            Self::NotFound => write!(f, "No templates matched your query"),
        }
    }
}

#[derive(Debug, PartialEq)]
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

#[derive(Debug, Hash, Clone, PartialEq, Eq)]
struct NormalisedSlug {
    slug: String,
}

impl NormalisedSlug {
    fn as_str(&self) -> &str {
        self.slug.as_str()
    }
}

impl TryFrom<String> for NormalisedSlug {
    type Error = AppError;
    fn try_from(mut slug: String) -> Result<Self, Self::Error> {
        slug.make_ascii_lowercase();
        if let Some(stem) = slug.strip_suffix(SUFFIX) {
            slug.truncate(stem.len());
        }
        if slug.is_empty() {
            return Err(AppError::EmptyQuery);
        }
        Ok(Self { slug })
    }
}

impl TryFrom<&str> for NormalisedSlug {
    type Error = AppError;
    fn try_from(slug: &str) -> Result<Self, Self::Error> {
        Self::try_from(slug.to_string())
    }
}

pub fn resolve_template_path(
    language: String,
    catalogue: &Catalogue,
) -> Result<TemplatePath, AppError> {
    let normalised_query = language.clone().try_into()?;
    let template_path = match resolve(&normalised_query, catalogue) {
        Resolution::Resolved(path) => Ok(path),
        Resolution::Ambiguous { matches } => Err(AppError::AmbiguousLanguage {
            language: language.clone(),
            matches,
        }),
        Resolution::DidYouMean { best, rest } => Err(AppError::DidYouMean {
            language: language.clone(),
            best,
            rest,
        }),
        Resolution::NotFound => Err(AppError::LanguageNotFound(language)),
    }?;
    Ok(template_path)
}

/// Pure resolution logic, no I/O. Tiers are tried in order: exact
/// (case-insensitive), alias, substring, then fuzzy suggestions.
fn resolve(query: &NormalisedSlug, catalogue: &Catalogue) -> Resolution {
    let candidates = candidates(catalogue);

    exact_tier(query, &candidates)
        .or_else(|| alias_tier(query, &candidates))
        .or_else(|| contains_tier(query, &candidates))
        .or_else(|| fuzzy_tier(query, &candidates))
        .unwrap_or(Resolution::NotFound)
}

fn candidates(catalogue: &Catalogue) -> Vec<Candidate> {
    catalogue
        .entries()
        .flat_map(|(path, _name)| derive(path))
        .collect()
}

/// Derives the match candidates (tails) for an index path, paired with the
/// verbatim key the tail resolves to. The tail is what queries are
/// compared against; the `TemplatePath` is what gets fetched. The tail is normalised
/// up-front so that comparison of the segmented tails with the normalised query
/// (user-given language argument) can be performed later.
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

fn exact_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let filtered_paths: Vec<_> = candidates
        .iter()
        .filter(|&candidate| *query == candidate.tail)
        .map(|candidate| candidate.path.as_str().to_string())
        .collect();
    match_filtered_paths(filtered_paths)
}

fn alias_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let target =
        aliases().find_map(|(alias, target)| (alias == query.as_str()).then_some(target))?;
    exact_tier(&target, candidates)
}

// For now `contains_tier` narrows the predicate by filtering out tails containing '/'
// but this needs to be improved. TODO: Split the candidates for exact versus contain tiers
fn contains_tier(query: &NormalisedSlug, candidates: &[Candidate]) -> Option<Resolution> {
    let filtered_paths: Vec<String> = candidates
        .iter()
        .filter(|candidate| {
            !candidate.tail.as_str().contains('/')
                && candidate.tail.as_str().contains(query.as_str())
        })
        .map(|candidate| candidate.path.as_str().to_string())
        .collect();
    match_filtered_paths(filtered_paths)
}

fn match_filtered_paths(mut filtered_paths: Vec<String>) -> Option<Resolution> {
    match filtered_paths.as_slice() {
        [] => None,
        [only] => Some(Resolution::Resolved(TemplatePath::new(only))),
        _ => {
            filtered_paths.dedup();
            Some(Resolution::Ambiguous {
                matches: filtered_paths.into_iter().collect::<Vec<String>>(),
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
                d if d < 3 => Some(OsaResult::new(d, candidate.path.as_str())),
                _ => None,
            }
        })
        .collect();
    if matches.is_empty() {
        None
    } else {
        matches.sort_unstable();
        let best = matches
            .first()
            .expect("Should have a non-empty vector as we've just checked for emptiness above")
            .path;
        let rest = matches.iter().skip(1).map(|o| o.path.to_string()).collect();

        Some(Resolution::DidYouMean {
            best: best.to_string(),
            rest,
        })
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

    #[test]
    fn contains_tier_resolves_to_resolution_ambiguous_when_multiple_paths_match() {
        let normalised_query: NormalisedSlug = "community"
            .try_into()
            .expect("Should be able to normalise 'community'");
        let expected = Some(Resolution::Ambiguous {
            matches: vec![
                "community/BoxLang/ColdBox.gitignore".to_string(),
                "community/Racket.gitignore".to_string(),
            ],
        });
        assert_eq!(
            contains_tier(&normalised_query, &test_candidates()),
            expected
        );
    }

    #[test]
    fn contains_tier_resolves_to_ambiguous_with_unique_matches_when_duplicated_paths_match_different_candidates()
     {
        let normalised_query: NormalisedSlug = "racket"
            .try_into()
            .expect("Should be able to normalise 'racket'");
        let expected = Some(Resolution::Ambiguous {
            matches: vec![
                "Racket.gitignore".to_string(),
                "community/Racket.gitignore".to_string(),
            ],
        });
        assert_eq!(
            contains_tier(&normalised_query, &test_candidates()),
            expected,
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

    /// The candidates `candidates(test_catalogue())` produces: one per tail,
    /// in `BTreeMap` path order, shortest tail first within each path.
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

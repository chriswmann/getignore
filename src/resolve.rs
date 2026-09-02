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
    tail: String,
    path: TemplatePath,
}

impl Candidate {
    #[cfg(test)]
    pub fn for_tests(tail: &str, path: TemplatePath) -> Self {
        Self {
            tail: tail.to_string(),
            path,
        }
    }
}

#[derive(Debug, PartialEq)]
struct NormalisedQuery {
    query: String,
}

impl NormalisedQuery {
    fn new(query: &str) -> Self {
        Self {
            query: normalise(query),
        }
    }
}

pub fn resolve_template_path(
    language: String,
    catalogue: &Catalogue,
) -> Result<TemplatePath, AppError> {
    let template_path = match resolve(&language, catalogue) {
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
fn resolve(query: &str, catalogue: &Catalogue) -> Resolution {
    let query = normalise(query);
    let query = query.as_str();
    let candidates = candidates(catalogue);

    exact_tier(query, catalogue)
        .or_else(|| alias_tier(query, catalogue))
        .or_else(|| prefix_tier(query, catalogue))
        .or_else(|| fuzzy_tier(query, catalogue))
        .unwrap_or(Resolution::NotFound)
}

fn candidates(catalogue: &Catalogue) -> Vec<Candidate> {
    catalogue
        .entries()
        .flat_map(|(path, name)| derive(path, name))
        .collect()
}

/// Derives the match candidates (tails) for an index path, paired with the
/// verbatim key the tail resolves to. The tail is what queries are
/// compared against; the `TemplatePath` is what gets fetched.
fn derive(path: &str, name: &str) -> Vec<Candidate> {
    let name = normalise(name);
    let directories = path
        .rsplit_once('/')
        .map(|(dirs, _)| dirs)
        .unwrap_or_default();
    let segments: Vec<String> = directories
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(normalise)
        .collect();

    let s = segments.iter().rev().fold(vec![name], |mut acc, seg| {
        let next = format!(
            "{}/{}",
            seg,
            acc.last()
                .expect("should have last element because acc was seeded with name"),
        );
        acc.push(next);
        acc
    });
    let path = TemplatePath::new(path);
    s.into_iter()
        .map(|tail| Candidate {
            tail,
            path: path.clone(),
        })
        .collect::<Vec<_>>()
}

fn exact_tier(query: NormalisedQuery, catalogue: &Catalogue) -> Option<Resolution> {
    let matched: Vec<_> = catalogue
        .entries()
        .filter(|(path, _)| query == path)
        .map(|(path, _)| path.to_string())
        .collect();
    match matched.as_slice() {
        [] => None,
        [only] => Some(Resolution::Resolved(TemplatePath::new(only))),
        _ => Some(Resolution::Ambiguous { matches: matched }),
    }
}

fn alias_tier(query: NormalisedQuery, catalogue: &Catalogue) -> Option<Resolution> {
    let target =
        aliases().find_map(|(alias, target)| (alias == query).then_some(target))?;
    exact_tier(&normalise(target), catalogue)
}

fn prefix_tier(query: NormalisedQuery, catalogue: &Catalogue) -> Option<Resolution> {
    catalogue.entries().find_map(|(path, _)| {
        if path.contains(&query) {
            Some(Resolution::Resolved(TemplatePath::new(path)))
        } else {
            None
        }
    })
}

fn fuzzy_tier(query: NormalisedQuery, catalogue: &Catalogue) -> Option<Resolution> {
    let query = &query;
    let mut matches: Vec<OsaResult> = catalogue
        .entries()
        .filter_map(
            |(path, _)| match strsim::osa_distance(query, &normalise(path)) {
                d if d < 3 => Some(OsaResult::new(d, path)),
                _ => None,
            },
        )
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
fn aliases() -> impl Iterator<Item = (&'static str, &'static str)> {
    include_str!("aliases.txt")
        .lines()
        .filter(|&l| !l.starts_with('#'))
        .filter_map(|l| {
            l.split_once('=')
                .map(|(alias, target)| (alias.trim(), target.trim()))
        })
}

fn normalise(query: &str) -> NormalisedQuery {
    let normalised_query = match query.strip_suffix(".gitignore") {
        Some(name) => name.to_lowercase(),
        None => query.to_lowercase(),
    };
    NormalisedQuery::new(normalised_query)
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
    fn test_normalise_normalises_paths_correctly() {
        assert_eq!(normalise("ColdBox.gitignore"), "coldbox".to_string());
        assert_eq!(
            normalise("community/BoxLang/ColdBox.gitignore"),
            "community/boxlang/coldbox"
        );
    }

    #[test]
    fn derive_uses_the_entry_name_and_builds_the_shortest_tails_first() {
        let expected = vec![
            Candidate::for_tests(
                "coldbox",
                TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            ),
            Candidate::for_tests(
                "boxlang/coldbox",
                TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            ),
            Candidate::for_tests(
                "community/boxlang/coldbox",
                TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            ),
        ];
        assert_eq!(
            derive("community/BoxLang/ColdBox.gitignore", "ColdBox"),
            expected
        );
    }

    #[test]
    fn derive_preserves_dotted_entry_names() {
        assert_eq!(
            derive("ecu.test.gitignore", "ecu.test"),
            vec![Candidate::for_tests(
                "ecu.test",
                TemplatePath::new("ecu.test.gitignore")
            )],
        );
    }

    #[test]
    fn exact_tier_matches_once_when_query_is_exact() {
        let entries = &[
            ("Rust.gitignore", "Rust"),
            ("community/DM/Rustici.gitignore", "Rustici"),
            ("community/Xilinx.gitignore", "Xilinx.gitignore"),
        ];
        let catalogue = Catalogue::for_tests(entries);
        let answer = exact_tier("rust", &catalogue);
        assert_eq!(
            answer,
            Some(Resolution::Resolved(TemplatePath::new("Rust.gitignore"))),
        );
    }

    #[test]
    fn resolve_resolves_a_case_insensitive_exact_name() {
        let expected = Resolution::Resolved(TemplatePath::new("Python.gitignore"));
        assert_eq!(resolve("python", &test_catalogue()), expected);
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
}

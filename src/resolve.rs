use std::{fmt, iter::once};

use crate::catalogue::Catalogue;

/// The exact index key for a template, stored verbatim (e.g.
/// `community/BoxLang/ColdBox.gitignore`). Never rebuilt from parts:
/// `main` uses it directly to look up the entry and fetch the blob.
#[derive(Debug, PartialEq)]
struct TemplatePath(String);

impl TemplatePath {
    fn new(path: &str) -> Self {
        Self(path.to_string())
    }

    #[cfg(test)]
    fn as_str(&self) -> &str {
        &self.0
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
    Resolved(String),
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
            Self::Resolved(path) => write!(f, "Found exact match: {path}"),
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

/// Pure resolution logic, no I/O. Tiers are tried in order: exact
/// (case-insensitive), alias, unique prefix, then fuzzy suggestions.
pub fn resolve(query: &str, catalogue: &Catalogue) -> Resolution {
    let query = normalise(query);
    let query = query.as_str();
    let candidates: Vec<(String, TemplatePath)> = catalogue
        .entries()
        .flat_map(|(path, _)| derive(path))
        .collect();

    exact_tier(query, catalogue)
        .or_else(|| alias_tier(query, catalogue))
        .or_else(|| prefix_tier(query, catalogue))
        .or_else(|| fuzzy_tier(query, catalogue))
        .unwrap_or(Resolution::NotFound)
}

/// Derives the match candidates (tails) for an index path, paired with the
/// verbatim key the tail resolves to. The tail is what queries are
/// compared against; the `TemplatePath` is what gets fetched.
fn derive(path: &str) -> Vec<(String, TemplatePath)> {
    let normalised = normalise(path);
    path.rmatch_indices('/')
        .map(|(i, _)| (normalised[i + 1..].to_string(), TemplatePath::new(path)))
        .chain(once((normalised.clone(), TemplatePath::new(path))))
        .collect()
}

fn exact_tier(query: &str, catalogue: &Catalogue) -> Option<Resolution> {
    let matched: Vec<_> = catalogue
        .entries()
        .filter_map(|(path, _)| {
            if query == normalise(path) {
                Some(path.to_string())
            } else {
                None
            }
        })
        .collect();
    match matched.len() {
        0 => None,
        1 => Some(Resolution::Resolved(matched.first().unwrap().clone())),
        _ => Some(Resolution::Ambiguous { matches: matched }),
    }
}

fn alias_tier(query: &str, catalogue: &Catalogue) -> Option<Resolution> {
    let target =
        aliases().find_map(|(alias, target)| (normalise(alias) == query).then_some(target))?;
    exact_tier(&normalise(target), catalogue)
}

fn prefix_tier(query: &str, catalogue: &Catalogue) -> Option<Resolution> {
    catalogue.entries().find_map(|(path, _)| {
        if path.contains(&normalise(query)) {
            Some(Resolution::Resolved(path.to_string()))
        } else {
            None
        }
    })
}

fn fuzzy_tier(query: &str, catalogue: &Catalogue) -> Option<Resolution> {
    let query = normalise(query);
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

fn normalise(query: &str) -> String {
    match query.strip_suffix(".gitignore") {
        Some(name) => name.to_lowercase(),
        None => query.to_lowercase(),
    }
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
    fn derive_matches_multiple_tails() {
        let path = "community/BoxLang/ColdBox.gitignore";
        let expected = vec![
            (
                "coldbox".to_string(),
                TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            ),
            (
                "boxlang/coldbox".to_string(),
                TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            ),
            (
                "community/boxlang/coldbox".to_string(),
                TemplatePath::new("community/BoxLang/ColdBox.gitignore"),
            ),
        ];
        assert_eq!(derive(path), expected);
    }

    #[test]
    fn derive_matches_single_tail() {
        let expected = vec![("rust".to_string(), TemplatePath::new("Rust.gitignore"))];
        assert_eq!(derive("Rust.gitignore"), expected);
    }

    // #[test]
    // fn exact_tier_matches_once_when_query_is_exact() {
    //     let
    // }
}

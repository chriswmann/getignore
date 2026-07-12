use std::iter::once;

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

    fn as_str(&self) -> &str {
        self.0.as_str()
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

/// Pure resolution logic, no I/O. Tiers are tried in order: exact
/// (case-insensitive), alias, unique prefix, then fuzzy suggestions.
fn resolve(query: &str, catalogue: &Catalogue) -> Resolution {
    let query = normalise(query);
    let candidates: Vec<(String, TemplatePath)> = catalogue
        .entries()
        .flat_map(|(path, _)| derive(path))
        .collect();
    todo!();
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

fn exact_match(query: &str, catalogue: &Catalogue) -> Vec<String> {
    catalogue
        .entries()
        .filter_map(|(path, name)| {
            if query == normalise(path) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
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
}

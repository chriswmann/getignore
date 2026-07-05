use crate::catalogue::Catalogue;

#[derive(Debug, PartialEq)]
pub enum Resolution {
    /// Language recognised and the gitignore will be provided.
    Resolved(String),
    /// Language not recognised but one or more suggestions found. Rest is ordered best first.
    DidYouMean { best: String, rest: Vec<String> },
    /// Language not recognised, no suggestions found.
    NotFound,
}

fn resolve(query: &str, catalogue: &Catalogue) -> Resolution {
    todo!("Implement this function");
}

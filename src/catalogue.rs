//! The set of templates available to resolve a query against.
//!
//! A read-only view over a loaded [`Index`]. It exists so that
//! [`mod@crate::resolve`] can iterate `(path, name)` pairs without being passed the
//! store's own types or any means of doing I/O, in order to keep resolution
//! a pure function. Lookup by path and the index's source commit are exposed
//! for the caller that goes on to fetch the template.

use crate::{
    github::CommitSha,
    store::{Entry, Index},
};

#[derive(Debug)]
pub struct Catalogue {
    index: Index,
}

impl Catalogue {
    pub fn new(index: Index) -> Self {
        Self { index }
    }

    #[cfg(test)]
    pub fn for_tests(entries: &[(&str, &str)]) -> Self {
        use std::collections::BTreeMap;

        let source_commit = CommitSha::for_tests("test-commit-sha");
        let entries = entries
            .iter()
            .enumerate()
            .map(|(ind, (path, name))| {
                use crate::github::BlobSha;

                (
                    path.to_string(),
                    Entry {
                        name: name.to_string(),
                        sha: BlobSha::for_tests(&ind.to_string()),
                    },
                )
            })
            .collect::<BTreeMap<String, Entry>>();
        let index = Index::for_tests(entries, source_commit);
        Self { index }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.index
            .entries
            .iter()
            .map(|(p, e)| (p.as_str(), e.name.as_str()))
    }

    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.index.entries.get(path)
    }

    pub fn source_commit(&self) -> &CommitSha {
        &self.index.source_commit
    }
}

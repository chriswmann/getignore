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

        let source_commit = CommitSha::new("test-commit-sha");
        let entries = entries
            .iter()
            .enumerate()
            .map(|(ind, (path, name))| {
                use crate::github::BlobSha;

                (
                    path.to_string(),
                    Entry {
                        name: name.to_string(),
                        sha: BlobSha::new(&ind.to_string()),
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

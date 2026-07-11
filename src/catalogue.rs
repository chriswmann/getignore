use crate::store::Index;

#[derive(Debug)]
pub struct Catalogue {
    index: Index,
}

impl Catalogue {
    pub fn new(index: Index) -> Self {
        Self { index }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.index
            .entries
            .iter()
            .map(|(p, e)| (p.as_str(), e.name.as_str()))
    }
}

use std::collections::HashSet;

use scorarium_engine::contributor::Contributor;
use scorarium_engine::entity::Contributed;
use scorarium_engine::publication::Publication;
use scorarium_engine::search::normalize;
use scorarium_engine::work::Work;

/// Derive a unique label for each exported entity.
#[derive(Default)]
pub(crate) struct Labeler {
    used: HashSet<String>,
}

impl Labeler {
    /// Label a publication `p-<surname>-<title>`.
    pub(crate) fn publication(&mut self, publication: &Publication) -> String {
        let title = publication.title().unwrap_or_default();
        self.unique("p", &[surname(publication.contributors()), title])
    }

    /// Label a work `w-<surname>-<number>`, using its title when it has no catalog number.
    pub(crate) fn work(&mut self, work: &Work) -> String {
        let number = work
            .catalog_numbers()
            .first()
            .map_or(work.title(), |number| number.as_str());
        self.unique("w", &[surname(work.contributors()), number])
    }

    /// Label a person `c-<name>`.
    pub(crate) fn person(&mut self, name: &str) -> String {
        self.unique("c", &[name])
    }

    fn unique(&mut self, prefix: &str, segments: &[&str]) -> String {
        let words = normalize(&segments.join(" ")).replace(' ', "-");
        let base = if words.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}-{words}")
        };
        let mut label = base.clone();
        let mut n = 1;
        while !self.used.insert(label.clone()) {
            n += 1;
            label = format!("{base}-{n}");
        }
        label
    }
}

/// Find the contributor an entity is filed under
pub(crate) fn lead(contributors: &[Contributor]) -> Option<&Contributor> {
    contributors
        .iter()
        .find(|c| matches!(c.role.as_str(), "composer" | "author"))
        .or_else(|| contributors.first())
}

fn surname(contributors: &[Contributor]) -> &str {
    lead(contributors).map_or("", |c| c.sort_name.split(',').next().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_unique() {
        let mut labeler = Labeler::default();
        assert_eq!(
            labeler.unique("w", &["Rachmaninoff", "Op. 21 No. 5"]),
            "w-rachmaninoff-op-21-no-5"
        );
        assert_eq!(
            labeler.unique("w", &["rachmaninoff", "op 21 no 5"]),
            "w-rachmaninoff-op-21-no-5-2"
        );
        assert_eq!(labeler.unique("p", &["", "..."]), "p");
    }
}

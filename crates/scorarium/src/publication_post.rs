use scorarium_archive::{
    ContributorInput, HoldingKind, HoldingRawInput, IdentifierRawInput, PublicationRawInput,
    WorkRawInput,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct PublicationPost {
    title: String,
    publisher: String,
    year: String,
    // `default` covers a submission with none at all
    #[serde(default)]
    holding_id: Vec<String>,
    #[serde(default)]
    holding_kind: Vec<HoldingKind>,
    #[serde(default)]
    holding_location: Vec<String>,
    #[serde(default)]
    holding_file: Vec<String>,
    #[serde(default)]
    identifier_kind: Vec<String>,
    #[serde(default)]
    identifier_value: Vec<String>,
    #[serde(default)]
    contributor_name: Vec<String>,
    #[serde(default)]
    contributor_role: Vec<String>,
    #[serde(default)]
    work_id: Vec<String>,
    #[serde(default)]
    work_title: Vec<String>,
    #[serde(default)]
    work_contributor_name: Vec<String>,
    #[serde(default)]
    work_contributor_role: Vec<String>,
    /// The index of the work whose edit button was clicked; absent on a plain submit
    edit_work: Option<String>,
}

/// A work as the publication form posts it: its title and the one contributor the page shows
#[derive(Debug, PartialEq, Eq)]
pub struct PartialWorkPost {
    pub id: Option<i64>,
    pub title: String,
    pub contributor: ContributorInput,
}

impl PublicationPost {
    /// The index of the work whose edit button was clicked, on the review page.
    pub fn edit_work(&self) -> Option<usize> {
        self.edit_work.as_ref()?.trim().parse().ok()
    }

    /// The whole raw input, given the works the page was showing.
    ///
    /// A posted work naming one of them takes it over, sets its title, and applies the posted
    /// contributor to its lead, leaving the fields and credits the page never showed alone. One
    /// naming nothing becomes a new work. Works no posted work names are dropped.
    pub fn merge(self, mut shown: Vec<WorkRawInput>) -> PublicationRawInput {
        let PublicationPost {
            title,
            publisher,
            year,
            holding_id,
            holding_kind,
            holding_location,
            holding_file,
            identifier_kind,
            identifier_value,
            contributor_name,
            contributor_role,
            work_id,
            work_title,
            work_contributor_name,
            work_contributor_role,
            edit_work: _,
        } = self;

        let mut contents = Vec::new();
        for posted in works(
            work_id,
            work_title,
            work_contributor_name,
            work_contributor_role,
        ) {
            // An id no shown work has names nothing this form may edit
            let existing = posted
                .id
                .and_then(|id| shown.iter().position(|work| work.id == Some(id)))
                .map(|i| shown.remove(i));
            match existing {
                Some(mut work) => {
                    work.title = posted.title;
                    set_lead(&mut work.contributors, &posted.contributor);
                    contents.push(work);
                }
                None => contents.push(WorkRawInput {
                    id: None,
                    title: posted.title,
                    contributors: match posted.contributor {
                        ContributorInput { name, role } if name.is_empty() && role.is_empty() => {
                            Vec::new()
                        }
                        contributor => vec![contributor],
                    },
                    ..WorkRawInput::default()
                }),
            }
        }

        PublicationRawInput {
            title: title.trim().to_string(),
            publisher: publisher.trim().to_string(),
            year: year.trim().to_string(),
            holdings: holdings(holding_id, holding_kind, holding_location, holding_file),
            identifiers: identifiers(identifier_kind, identifier_value),
            contributors: contributors(contributor_name, contributor_role),
            contents,
        }
    }
}

/// The contributor a work shows on the publication pages
pub fn lead_contributor(contributors: &[ContributorInput]) -> Option<usize> {
    // Prioritize composers or authors, if they exist
    contributors
        .iter()
        .position(|contributor| contributor.role == "composer")
        .or_else(|| {
            contributors
                .iter()
                .position(|contributor| contributor.role == "author")
        })
        .or_else(|| (!contributors.is_empty()).then_some(0))
}

/// Apply the posted contributor to a work's lead, leaving the ones the page does not show alone.
///
/// A half-filled contributor is kept rather than dropped: the form holds whatever was typed, and
/// validation is what tells the user about it.
fn set_lead(contributors: &mut Vec<ContributorInput>, posted: &ContributorInput) {
    let empty = posted.name.is_empty() && posted.role.is_empty();
    match lead_contributor(contributors) {
        Some(i) if contributors[i] == *posted => {}
        // The lead is edited rather than replaced, so it keeps its place in the list: that order is
        // what picks the lead for a work with neither a composer nor an author.
        Some(i) if !empty => {
            contributors[i] = posted.clone();
            // Another contributor the lead now duplicates would credit the same person twice
            let mut index = 0;
            contributors.retain(|contributor| {
                let keep = index == i || contributor != posted;
                index += 1;
                keep
            });
        }
        Some(i) => {
            contributors.remove(i);
        }
        None if !empty => contributors.push(posted.clone()),
        None => {}
    }
}

/// Copies from the parallel keys. Every copy submits a kind, a location and a file, and the kind
/// picks which of the two counts.
///
/// The ids are read by position rather than zipped: a missing or short id list leaves the copies it
/// does not reach naming no stored copy, which is what a page with nothing stored yet submits.
pub fn holdings(
    id: Vec<String>,
    kind: Vec<HoldingKind>,
    location: Vec<String>,
    file: Vec<String>,
) -> Vec<HoldingRawInput> {
    kind.into_iter()
        .zip(location)
        .zip(file)
        .enumerate()
        .map(|(i, ((kind, location), file))| HoldingRawInput {
            id: id.get(i).and_then(|id| id.trim().parse().ok()),
            kind,
            location: match kind {
                HoldingKind::Physical => location,
                HoldingKind::Digital => file,
            }
            .trim()
            .to_string(),
        })
        .collect()
}

/// Credits from the parallel keys; the work form decodes the same
pub fn contributors(name: Vec<String>, role: Vec<String>) -> Vec<ContributorInput> {
    name.into_iter()
        .zip(role)
        .map(|(name, role)| ContributorInput {
            name: name.trim().to_string(),
            role: role.trim().to_string(),
        })
        .collect()
}

fn identifiers(kind: Vec<String>, value: Vec<String>) -> Vec<IdentifierRawInput> {
    kind.into_iter()
        .zip(value)
        .map(|(kind, value)| IdentifierRawInput {
            kind: kind.trim().to_string(),
            value: value.trim().to_string(),
        })
        .collect()
}

/// Works from the parallel keys, with the ids read by position for the same reason copies' are.
fn works(
    id: Vec<String>,
    title: Vec<String>,
    name: Vec<String>,
    role: Vec<String>,
) -> Vec<PartialWorkPost> {
    title
        .into_iter()
        .zip(name)
        .zip(role)
        .enumerate()
        .map(|(i, ((title, name), role))| PartialWorkPost {
            id: id.get(i).and_then(|id| id.trim().parse().ok()),
            title: title.trim().to_string(),
            contributor: ContributorInput {
                name: name.trim().to_string(),
                role: role.trim().to_string(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contributor(name: &str, role: &str) -> ContributorInput {
        ContributorInput {
            name: name.into(),
            role: role.into(),
        }
    }

    /// A submission carrying nothing but its works, as (id, title, contributor name, role)
    fn posted(works: &[(Option<i64>, &str, &str, &str)]) -> PublicationPost {
        PublicationPost {
            title: "Album".into(),
            publisher: String::new(),
            year: String::new(),
            holding_id: Vec::new(),
            holding_kind: vec![HoldingKind::Physical],
            holding_location: vec![String::new()],
            holding_file: vec![String::new()],
            identifier_kind: Vec::new(),
            identifier_value: Vec::new(),
            contributor_name: Vec::new(),
            contributor_role: Vec::new(),
            work_id: works
                .iter()
                .map(|(id, ..)| id.map(|id| id.to_string()).unwrap_or_default())
                .collect(),
            work_title: works
                .iter()
                .map(|(_, title, ..)| title.to_string())
                .collect(),
            work_contributor_name: works
                .iter()
                .map(|(_, _, name, _)| name.to_string())
                .collect(),
            work_contributor_role: works.iter().map(|(.., role)| role.to_string()).collect(),
            edit_work: None,
        }
    }

    /// A merged work as (id, title, key, [(contributor name, role)])
    type WorkView<'a> = (Option<i64>, &'a str, &'a str, Vec<(&'a str, &'a str)>);

    fn view(contents: &[WorkRawInput]) -> Vec<WorkView<'_>> {
        contents
            .iter()
            .map(|work| {
                let contributors = work
                    .contributors
                    .iter()
                    .map(|c| (c.name.as_str(), c.role.as_str()))
                    .collect();
                (
                    work.id,
                    work.title.as_str(),
                    work.key.as_str(),
                    contributors,
                )
            })
            .collect()
    }

    /// A publication shows one contributor per work, so merging has to leave everything else alone.
    #[test]
    fn merge_keeps_what_the_page_does_not_show() {
        let shown = vec![WorkRawInput {
            id: Some(1),
            title: "Prelude".into(),
            key: "E minor".into(),
            contributors: vec![
                contributor("Chopin", "composer"),
                contributor("Liszt", "arranger"),
            ],
            ..WorkRawInput::default()
        }];

        let merged =
            posted(&[(Some(1), "Prelude in E minor", "Chopin", "composer")]).merge(shown.clone());
        assert_eq!(
            view(&merged.contents),
            [(
                Some(1),
                "Prelude in E minor",
                "E minor",
                vec![("Chopin", "composer"), ("Liszt", "arranger")]
            )],
            "renaming leaves the key and the arranger the page never showed alone"
        );

        let merged = posted(&[(Some(1), "Prelude", "Liszt", "arranger")]).merge(shown.clone());
        assert_eq!(
            view(&merged.contents),
            [(Some(1), "Prelude", "E minor", vec![("Liszt", "arranger")])],
            "handing the work to a contributor it already credits replaces the lead in place"
        );

        let merged = posted(&[(Some(1), "Prelude", "", "")]).merge(shown.clone());
        assert_eq!(
            view(&merged.contents),
            [(Some(1), "Prelude", "E minor", vec![("Liszt", "arranger")])],
            "clearing the contributor removes the lead and promotes the next"
        );

        let merged = posted(&[
            (Some(1), "Prelude", "Chopin", "composer"),
            (None, "Nocturne", "Field", "composer"),
        ])
        .merge(shown.clone());
        assert_eq!(
            view(&merged.contents)[1],
            (None, "Nocturne", "", vec![("Field", "composer")]),
            "a posted work naming nothing is a new work"
        );

        let merged = posted(&[(None, "Mazurka", "", "")]).merge(shown);
        assert_eq!(
            view(&merged.contents),
            [(None, "Mazurka", "", vec![])],
            "a work the submission no longer lists is dropped, and an empty contributor adds none"
        );
    }

    #[test]
    fn lead_contributor_prefers_composer_then_author() {
        let composer = [contributor("A", "arranger"), contributor("B", "composer")];
        assert_eq!(lead_contributor(&composer), Some(1));
        let author = [contributor("A", "editor"), contributor("B", "author")];
        assert_eq!(lead_contributor(&author), Some(1));
        let neither = [contributor("A", "editor"), contributor("B", "arranger")];
        assert_eq!(lead_contributor(&neither), Some(0));
        assert_eq!(lead_contributor(&[]), None);
    }
}

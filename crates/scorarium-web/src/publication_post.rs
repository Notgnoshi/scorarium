use scorarium_archive::{
    ContributorInput, HoldingKind, HoldingRawInput, IdentifierRawInput, PersonRef, PublicationPost,
    WorkPost, WorkRef,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// A submitted publication form, decoded but not yet merged with the works from the page
pub struct PublicationForm {
    fields: Fields,
    holdings: Vec<HoldingRawInput>,
}

/// A submitted form that does not decode.
#[derive(Debug)]
pub struct BadForm(serde_html_form::de::Error);

impl std::fmt::Display for BadForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not decode form: {}", self.0)
    }
}

impl std::error::Error for BadForm {}

/// Decode a submitted form body.
pub fn decode_form<T: DeserializeOwned>(body: &[u8]) -> Result<T, BadForm> {
    serde_html_form::from_bytes(body).map_err(BadForm)
}

/// Everything posted under a fixed key, which is everything but the copies.
#[derive(Deserialize)]
struct Fields {
    title: String,
    publisher: String,
    year: String,
    #[serde(default)]
    stars: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    identifier_kind: Vec<String>,
    #[serde(default)]
    identifier_value: Vec<String>,
    #[serde(default)]
    contributor_name: Vec<String>,
    #[serde(default)]
    contributor_role: Vec<String>,
    #[serde(default)]
    contributor_person: Vec<String>,
    #[serde(default)]
    contributor_external_person: Vec<String>,
    #[serde(default)]
    link: Vec<String>,
    #[serde(default)]
    work_id: Vec<String>,
    #[serde(default)]
    work_title: Vec<String>,
    #[serde(default)]
    work_catalog_number: Vec<String>,
    #[serde(default)]
    work_contributor_name: Vec<String>,
    #[serde(default)]
    work_contributor_role: Vec<String>,
    #[serde(default)]
    work_contributor_person: Vec<String>,
    #[serde(default)]
    work_contributor_external_person: Vec<String>,
    /// The index of the work whose edit button was clicked; absent on a plain submit
    edit_work: Option<String>,
}

impl PublicationForm {
    /// Decode a submitted form.
    ///
    /// Two passes over the same body: the fields posted under a fixed key come from the derive,
    /// and the copies from the raw pairs, because their keys carry a per-copy suffix the derive
    /// cannot name.
    pub fn decode(body: &[u8]) -> Result<Self, BadForm> {
        let fields: Fields = decode_form(body)?;
        let pairs: Vec<(String, String)> = decode_form(body)?;
        Ok(PublicationForm {
            fields,
            holdings: holdings(&pairs),
        })
    }

    /// The index of the work whose edit button was clicked, on the review page.
    pub fn edit_work(&self) -> Option<usize> {
        self.fields.edit_work.as_ref()?.trim().parse().ok()
    }

    /// What was posted, trimmed, with the parallel keys folded into rows.
    pub fn into_post(self) -> PublicationPost {
        let PublicationForm { fields, holdings } = self;
        let Fields {
            title,
            publisher,
            year,
            stars,
            note,
            tags,
            identifier_kind,
            identifier_value,
            contributor_name,
            contributor_role,
            contributor_person,
            contributor_external_person,
            link,
            work_id,
            work_title,
            work_catalog_number,
            work_contributor_name,
            work_contributor_role,
            work_contributor_person,
            work_contributor_external_person,
            edit_work: _,
        } = fields;
        PublicationPost {
            title: title.trim().to_string(),
            publisher: publisher.trim().to_string(),
            year: year.trim().to_string(),
            stars: stars.trim().to_string(),
            note: note.trim().to_string(),
            tags: tags.trim().to_string(),
            holdings,
            identifiers: identifiers(identifier_kind, identifier_value),
            contributors: contributors(
                contributor_name,
                contributor_role,
                &contributor_person,
                &contributor_external_person,
            ),
            links: link.iter().map(|link| link.trim().to_string()).collect(),
            contents: works(
                work_id,
                work_title,
                work_catalog_number,
                work_contributor_name,
                work_contributor_role,
                &work_contributor_person,
                &work_contributor_external_person,
            ),
        }
    }
}

/// The copies a form posts, in the order the page lists them.
///
/// A copy's kind is a radio group, and radio groups are scoped by name, so every field of a copy
/// carries a suffix unique to that copy: `holding_kind_3`, `holding_location_3`. That suffix is
/// only a grouping token; what it says is never read, and the copies come back in the order they
/// first appear, which is the order the page lists them in.
pub fn holdings(pairs: &[(String, String)]) -> Vec<HoldingRawInput> {
    let mut copies: Vec<(&str, Posted<'_>)> = Vec::new();
    for (key, value) in pairs {
        let Some((field, index)) = key.rsplit_once('_') else {
            continue;
        };
        if !matches!(
            field,
            "holding_id" | "holding_kind" | "holding_location" | "holding_file"
        ) {
            continue;
        }
        let copy = match copies.iter().position(|(seen, _)| *seen == index) {
            Some(i) => &mut copies[i].1,
            None => {
                copies.push((index, Posted::default()));
                &mut copies.last_mut().expect("just pushed").1
            }
        };
        match field {
            "holding_id" => copy.id = value,
            "holding_kind" => copy.kind = value,
            "holding_location" => copy.location = value,
            _ => copy.file = value,
        }
    }
    copies
        .into_iter()
        .map(|(_, copy)| {
            let kind = copy.kind.parse().unwrap_or(HoldingKind::Physical);
            HoldingRawInput {
                id: copy.id.trim().parse().ok(),
                kind,
                // Each kind has its own input, so the copy's kind picks which one counts
                location: match kind {
                    HoldingKind::Physical => copy.location,
                    HoldingKind::Digital => copy.file,
                }
                .trim()
                .to_string(),
            }
        })
        .collect()
}

/// One copy's fields as posted, before its kind says which location counts.
#[derive(Default)]
struct Posted<'a> {
    id: &'a str,
    kind: &'a str,
    location: &'a str,
    file: &'a str,
}

pub fn person_ref(field: &str) -> PersonRef {
    let field = field.trim();
    if field == "new" {
        return PersonRef::New;
    }
    match field.strip_prefix(DRAFT_PREFIX) {
        Some(id) => id.parse().map_or(PersonRef::Unresolved, PersonRef::Draft),
        None => field
            .parse()
            .map_or(PersonRef::Unresolved, PersonRef::Linked),
    }
}

pub fn person_field(person: PersonRef) -> String {
    match person {
        PersonRef::Linked(id) => id.to_string(),
        PersonRef::Draft(id) => format!("{DRAFT_PREFIX}{id}"),
        PersonRef::New => "new".to_string(),
        PersonRef::Unresolved => String::new(),
    }
}

pub fn external_person_ref(field: &str) -> Option<i64> {
    field.trim().parse().ok()
}

pub fn external_person_field(external_person: Option<i64>) -> String {
    external_person.map(|id| id.to_string()).unwrap_or_default()
}

/// The prefix that tells a draft work's or person's id from a stored one in the hidden field
const DRAFT_PREFIX: &str = "draft:";

pub fn work_ref(field: &str) -> Option<WorkRef> {
    let field = field.trim();
    match field.strip_prefix(DRAFT_PREFIX) {
        Some(id) => id.parse().ok().map(WorkRef::Draft),
        None => field.parse().ok().map(WorkRef::Stored),
    }
}

pub fn work_field(id: Option<WorkRef>) -> String {
    match id {
        Some(WorkRef::Stored(id)) => id.to_string(),
        Some(WorkRef::Draft(id)) => format!("{DRAFT_PREFIX}{id}"),
        None => String::new(),
    }
}

/// Credits from the parallel keys; the work form decodes the same
pub fn contributors(
    name: Vec<String>,
    role: Vec<String>,
    person: &[String],
    external_person: &[String],
) -> Vec<ContributorInput> {
    name.into_iter()
        .zip(role)
        .enumerate()
        .map(|(i, (name, role))| ContributorInput {
            name: name.trim().to_string(),
            role: role.trim().to_string(),
            person: person
                .get(i)
                .map(|field| person_ref(field))
                .unwrap_or_default(),
            external_person: external_person
                .get(i)
                .and_then(|field| external_person_ref(field)),
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
    catalog_number: Vec<String>,
    name: Vec<String>,
    role: Vec<String>,
    person: &[String],
    external_person: &[String],
) -> Vec<WorkPost> {
    title
        .into_iter()
        .zip(name)
        .zip(role)
        .enumerate()
        .map(|(i, ((title, name), role))| WorkPost {
            id: id.get(i).and_then(|id| work_ref(id)),
            title: title.trim().to_string(),
            // Read by position, like the id: zipping it in would drop every work when a
            // submission carries no catalog number key at all
            catalog_number: catalog_number
                .get(i)
                .map(|number| number.trim().to_string())
                .unwrap_or_default(),
            contributor: ContributorInput {
                name: name.trim().to_string(),
                role: role.trim().to_string(),
                person: person
                    .get(i)
                    .map(|field| person_ref(field))
                    .unwrap_or_default(),
                external_person: external_person
                    .get(i)
                    .and_then(|field| external_person_ref(field)),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_round_trip_through_the_hidden_field() {
        for id in [None, Some(WorkRef::Stored(5)), Some(WorkRef::Draft(3))] {
            assert_eq!(work_ref(&work_field(id)), id);
        }
        assert_eq!(work_ref(" 5 "), Some(WorkRef::Stored(5)));
        assert_eq!(work_ref("draft:x"), None);
        for person in [
            PersonRef::Unresolved,
            PersonRef::New,
            PersonRef::Linked(5),
            PersonRef::Draft(3),
        ] {
            assert_eq!(person_ref(&person_field(person)), person);
        }
    }

    #[test]
    fn an_external_person_stays_with_the_contributor_it_was_posted_beside() {
        let fields = |values: &[&str]| -> Vec<String> {
            values.iter().map(|value| value.to_string()).collect()
        };
        let posted = contributors(
            fields(&["Glenn Gould", "Johann Sebastian Bach", "Donald Tovey"]),
            fields(&["performer", "composer", "editor"]),
            &fields(&["", "12", "new"]),
            &fields(&["", "7", ""]),
        );

        let external_persons: Vec<Option<i64>> =
            posted.iter().map(|posted| posted.external_person).collect();
        assert_eq!(external_persons, [None, Some(7), None]);
    }

    /// Each copy posts its fields under a suffix of its own, and the copies come back in the
    /// order the page listed them, not in whatever order the suffixes happen to sort.
    #[test]
    fn copies_are_grouped_by_suffix_in_page_order() {
        let pairs: Vec<(String, String)> = [
            ("holding_id_new1", ""),
            ("holding_kind_new1", "digital"),
            ("holding_location_new1", ""),
            ("holding_file_new1", "score.pdf"),
            ("holding_id_0", "5"),
            ("holding_kind_0", "physical"),
            ("holding_location_0", "Shelf"),
            ("holding_file_0", ""),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();

        assert_eq!(
            holdings(&pairs),
            [
                HoldingRawInput {
                    id: None,
                    kind: HoldingKind::Digital,
                    location: "score.pdf".into(),
                },
                HoldingRawInput {
                    id: Some(5),
                    kind: HoldingKind::Physical,
                    location: "Shelf".into(),
                },
            ]
        );
    }

    #[test]
    fn decode_reads_both_the_fields_and_the_copies() {
        let body = b"title=Album&publisher=&year=&holding_id_0=&holding_kind_0=digital&holding_location_0=&holding_file_0=score.pdf&identifier_kind=isbn&identifier_value=x";

        let post = PublicationForm::decode(body).unwrap();

        assert_eq!(post.fields.title, "Album");
        assert_eq!(post.fields.identifier_kind, ["isbn"]);
        assert_eq!(
            post.holdings,
            [HoldingRawInput {
                id: None,
                kind: HoldingKind::Digital,
                location: "score.pdf".into(),
            }]
        );
    }
}

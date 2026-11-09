use scorarium_engine::entity::{AnyEntity, Contributed, Entity, EntityKind};
use scorarium_engine::library::LibraryId;
use scorarium_engine::search::SearchHit;
use scorarium_engine::value::{Classification, Tag};
use scorarium_engine::work::CatalogNumber;

use crate::page::library::entity::group_contributors;

pub(crate) struct Hit {
    pub library: LibraryId,
    pub href: String,
    pub title: String,
    pub detail: Option<String>,
    pub kind: EntityKind,
    pub classification: Option<Classification>,
    pub library_name: String,
    pub tags: Vec<Tag>,
}

impl Hit {
    pub fn kind_rank(&self) -> (u8, u8) {
        let kind = match self.kind {
            EntityKind::Person => 0,
            EntityKind::Publication => 1,
            EntityKind::Work => 2,
        };
        let classification = match self.classification {
            Some(Classification::Music) => 0,
            Some(Classification::Prose) => 1,
            None => 2,
        };
        (kind, classification)
    }

    /// Present `entity`, which belongs to `library`
    pub(crate) fn new(library: LibraryId, library_name: String, entity: &AnyEntity) -> Hit {
        match entity {
            AnyEntity::Publication(publication) => Hit {
                library,
                href: format!("/library/{library}/publication/{}", publication.id()),
                title: publication.title().unwrap_or("Untitled").to_owned(),
                detail: detail(publication.contributors(), &[]),
                kind: EntityKind::Publication,
                classification: publication.classification(),
                library_name,
                tags: publication.tags().to_vec(),
            },
            AnyEntity::Work(work) => Hit {
                library,
                href: format!("/library/{library}/work/{}", work.id()),
                title: work.title().to_owned(),
                detail: detail(work.contributors(), work.catalog_numbers()),
                kind: EntityKind::Work,
                classification: work.classification(),
                library_name,
                tags: work.tags().to_vec(),
            },
            AnyEntity::Person(person) => Hit {
                library,
                href: format!("/library/{library}/person/{}", person.id()),
                title: person.name().to_owned(),
                detail: None,
                kind: EntityKind::Person,
                classification: None,
                library_name,
                tags: Vec::new(),
            },
        }
    }
}

impl From<&SearchHit> for Hit {
    fn from(hit: &SearchHit) -> Hit {
        Hit::new(hit.library, hit.library_name.clone(), &hit.entity)
    }
}

/// Join the primary contributor and the catalog numbers into the secondary text
fn detail(
    contributors: &[scorarium_engine::contributor::Contributor],
    catalog_numbers: &[CatalogNumber],
) -> Option<String> {
    let parts: Vec<String> = primary_contributor(contributors)
        .into_iter()
        .chain(catalog_numbers.iter().map(ToString::to_string))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" • "))
}

/// Pick the "primary" contributor, followed by how many others there are
fn primary_contributor(
    contributors: &[scorarium_engine::contributor::Contributor],
) -> Option<String> {
    let people = group_contributors(contributors);
    let first = people.first()?; // alphabetically first by sort name
    Some(match people.len() - 1 {
        0 => first.name.clone(),
        others => format!("{}, +{others}", first.name),
    })
}

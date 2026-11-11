use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::link::Link;
use scorarium_engine::person::PersonId;
use scorarium_engine::work::{CatalogNumber, Work, WorkId};

use crate::auth::Viewer;
use crate::error::Result;
use crate::icon::Icon;

/// Load an entity of `library` by the id in the URL, hiding drafts from anonymous visitors
pub(crate) async fn load<E: Entity>(
    engine: &Engine,
    library: LibraryId,
    id: &str,
    viewer: &Viewer,
) -> Result<E> {
    let id: E::Id = id.parse().map_err(|_| scorarium_engine::Error::NotFound)?;
    match E::load(engine, id).await? {
        Some(entity)
            if entity.library() == library && (!entity.is_draft() || viewer.is_logged_in()) =>
        {
            Ok(entity)
        }
        _ => Err(scorarium_engine::Error::NotFound.into()),
    }
}

/// Look up a submitted entity id that must belong to `library`
///
/// A malformed id, a deleted entity, and another library's entity are all reported as
/// "That {noun} is not in this library."
pub(crate) async fn in_library<E: Entity>(
    engine: &Engine,
    library: LibraryId,
    id: &str,
    noun: &str,
) -> scorarium_engine::Result<E> {
    let not_here =
        || scorarium_engine::Error::InvalidValue(format!("That {noun} is not in this library."));
    let id: E::Id = id.parse().map_err(|_| not_here())?;
    match E::load(engine, id).await? {
        Some(entity) if entity.library() == library => Ok(entity),
        _ => Err(not_here()),
    }
}

/// A link rendered as the recognized site's name, or the URL's host for a generic link
#[derive(Debug)]
pub(crate) struct ExternalLink {
    pub href: String,
    pub label: String,
    pub icon: Icon,
}

impl From<&Link> for ExternalLink {
    fn from(link: &Link) -> ExternalLink {
        ExternalLink {
            href: link.url.to_string(),
            label: link.site_name().to_owned(),
            icon: Icon::link(link),
        }
    }
}

/// A contributor to a publication or work, with every role they hold on it
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Contributor {
    pub person: PersonId,
    pub name: String,
    pub sort_name: String,
    pub roles: Vec<Role>,
}

/// A work as listed in a publication's contents
#[derive(Debug)]
pub(crate) struct ContainedWork {
    pub id: WorkId,
    pub draft: bool,
    pub title: String,
    /// The work's first catalog number, which is its most confidently recognized one
    pub catalog_number: Option<CatalogNumber>,
    pub contributors: Vec<Contributor>,
}

impl ContainedWork {
    /// Get the lowercase sort name of the first composer or author
    pub fn primary_sort_name(&self) -> Option<String> {
        self.contributors
            .iter()
            .find(|c| c.roles.iter().any(is_primary))
            .map(|c| c.sort_name.to_lowercase())
    }
}

impl From<&Work> for ContainedWork {
    fn from(work: &Work) -> ContainedWork {
        ContainedWork {
            id: work.id(),
            draft: work.is_draft(),
            title: work.title().to_owned(),
            catalog_number: work.catalog_numbers().first().cloned(),
            contributors: group_contributors(work.contributors()),
        }
    }
}

/// Sort works in book order
pub(crate) fn book_order(works: &mut [ContainedWork]) {
    works.sort_by_cached_key(|work| {
        let sort_name = work
            .contributors
            .first()
            .map(|c| c.sort_name.to_lowercase());
        (
            sort_name.is_none(),
            sort_name,
            work.catalog_number.is_none(),
            work.catalog_number.clone(),
            work.title.to_lowercase(),
        )
    });
}

/// Check whether the role is composer or author
pub(crate) fn is_primary(role: &Role) -> bool {
    matches!(role.as_str(), "author" | "composer")
}

/// Group an entity's contributors by person for display
pub(crate) fn group_contributors(
    contributors: &[scorarium_engine::contributor::Contributor],
) -> Vec<Contributor> {
    let mut people: Vec<Contributor> = Vec::new();
    for c in contributors {
        match people.iter_mut().find(|p| p.person == c.person) {
            Some(person) => person.roles.push(c.role.clone()),
            None => people.push(Contributor {
                person: c.person,
                name: c.name.clone(),
                sort_name: c.sort_name.clone(),
                roles: vec![c.role.clone()],
            }),
        }
    }
    for person in &mut people {
        person.roles.sort_by_key(|role| !is_primary(role));
    }
    people.sort_by_key(|p| (!p.roles.iter().any(is_primary), p.sort_name.to_lowercase()));
    people
}

/// Label a role as a listing, so that "composer" reads "Composers"
pub(crate) fn plural_role_label(role: &Role) -> String {
    let mut chars = role.as_str().chars();
    let mut label: String = chars
        .next()
        .into_iter()
        .flat_map(char::to_uppercase)
        .collect();
    label.push_str(chars.as_str());
    if !label.ends_with('s') {
        label.push('s');
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_contributors_puts_primary_roles_first_and_lists_a_person_once() {
        let contributor = |id: &str, name: &str, sort_name: &str, role: &str| {
            scorarium_engine::contributor::Contributor {
                person: id.parse().unwrap(),
                name: name.into(),
                sort_name: sort_name.into(),
                role: role.parse().unwrap(),
            }
        };
        let all = [
            contributor("1", "Erik Satie", "Satie, Erik", "editor"),
            contributor("1", "Erik Satie", "Satie, Erik", "composer"),
            contributor("2", "Claude Debussy", "Debussy, Claude", "author"),
            contributor("3", "Carl Mikuli", "Mikuli, Carl", "editor"),
        ];
        let role = |role: &str| role.parse::<Role>().unwrap();
        assert_eq!(
            group_contributors(&all),
            [
                Contributor {
                    person: "2".parse().unwrap(),
                    name: "Claude Debussy".into(),
                    sort_name: "Debussy, Claude".into(),
                    roles: vec![role("author")],
                },
                Contributor {
                    person: "1".parse().unwrap(),
                    name: "Erik Satie".into(),
                    sort_name: "Satie, Erik".into(),
                    roles: vec![role("composer"), role("editor")],
                },
                Contributor {
                    person: "3".parse().unwrap(),
                    name: "Carl Mikuli".into(),
                    sort_name: "Mikuli, Carl".into(),
                    roles: vec![role("editor")],
                },
            ]
        );
    }
}

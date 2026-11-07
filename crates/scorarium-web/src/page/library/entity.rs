use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::link::Link;
use scorarium_engine::person::PersonId;

use crate::auth::Viewer;
use crate::error::Result;

/// Load an entity of `library` by the id in the URL, hiding drafts from anonymous visitors
pub(super) async fn load<E: Entity>(
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

/// A link rendered as the recognized site's name, or the URL's host for a generic link
pub(super) struct ExternalLink {
    pub href: String,
    pub label: String,
}

impl From<&Link> for ExternalLink {
    fn from(link: &Link) -> ExternalLink {
        let label = match &link.record {
            Some(record) => record.kind().to_string(),
            None => link.url.as_url().host_str().unwrap_or_default().to_owned(),
        };
        ExternalLink {
            href: link.url.to_string(),
            label,
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

/// Group an entity's contributors by person for display
pub(crate) fn group_contributors(
    contributors: &[scorarium_engine::contributor::Contributor],
) -> Vec<Contributor> {
    let is_primary = |role: &Role| matches!(role.as_str(), "author" | "composer");

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

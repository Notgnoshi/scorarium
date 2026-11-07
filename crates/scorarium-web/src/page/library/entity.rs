use scorarium_engine::contributor::Role;
use scorarium_engine::person::PersonId;

/// A contributor to a publication or work, with every role they hold on it
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Contributor {
    pub person: PersonId,
    pub name: String,
    pub sort_name: String,
    pub roles: Vec<Role>,
}

/// Group an entity's contributors by person for display
pub(super) fn group_contributors(
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

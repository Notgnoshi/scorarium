use std::io::Write;

use scorarium_engine::archive::{Library, Person};

use crate::library::write_table;

/// List the persons credited in a library, keeping only those credited with the given role.
pub(crate) async fn list(
    library: &Library,
    role: Option<&str>,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let rows: Vec<[String; 3]> = library
        .person_summaries(role)
        .await?
        .into_iter()
        .map(|person| {
            let title = if person.others > 0 {
                format!("{} +{}", person.title, person.others)
            } else {
                person.title
            };
            [person.id.to_string(), person.name, title]
        })
        .collect();
    write_table(out, &rows)
}

/// Show a person with the publications and works crediting them.
pub(crate) async fn show(person: Person, out: &mut impl Write) -> eyre::Result<()> {
    let row = |label: &str, id: String, value: String| [label.to_string(), id, value];
    let credit = |title: &str, roles: Vec<&str>| {
        if roles.is_empty() {
            title.to_string()
        } else {
            format!("{title} ({})", roles.join(", "))
        }
    };
    let mut rows = vec![
        row("id", person.id.to_string(), String::new()),
        row("name", String::new(), person.name.clone()),
        row("sort name", String::new(), person.sort_name.clone()),
    ];
    for link in &person.links {
        rows.push(row("link", String::new(), link.clone()));
    }
    let mut publications = person.publications().await?;
    publications.sort_by(|a, b| a.title.cmp(&b.title));
    for publication in &publications {
        let value = credit(&publication.title, publication.roles_of(person.id));
        rows.push(row("publication", publication.id.to_string(), value));
    }
    let mut seen = Vec::new();
    for publication in &publications {
        for work in publication.works().await? {
            let roles = work.roles_of(person.id);
            if roles.is_empty() || seen.contains(&work.id) {
                continue;
            }
            seen.push(work.id);
            rows.push(row("work", work.id.to_string(), credit(&work.title, roles)));
        }
    }
    write_table(out, &rows)
}

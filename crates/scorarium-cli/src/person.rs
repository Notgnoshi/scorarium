use std::io::Write;

use scorarium_engine::archive::Library;

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

use std::io::Write;

use scorarium_engine::archive::Work;

use crate::library::write_table;

pub(crate) async fn show(work: Work, out: &mut impl Write) -> eyre::Result<()> {
    let row = |label: &str, id: String, value: String| [label.to_string(), id, value];
    let mut rows = vec![
        row("id", work.id.to_string(), String::new()),
        row("title", String::new(), work.title.clone()),
    ];
    let optional = [
        ("key", work.key.clone()),
        ("time signature", work.time_signature.clone()),
        ("instrumentation", work.instrumentation.clone()),
        ("stars", work.stars.map(|stars| stars.to_string())),
        ("note", work.note.clone()),
    ];
    for (label, value) in optional {
        if let Some(value) = value {
            rows.push(row(label, String::new(), value));
        }
    }
    if !work.tags.is_empty() {
        rows.push(row("tags", String::new(), work.tags.join(", ")));
    }
    for number in &work.catalog_numbers {
        rows.push(row("catalog number", String::new(), number.to_string()));
    }
    for contributor in &work.contributors {
        let value = format!("{} ({})", contributor.name, contributor.role);
        rows.push(row("contributor", String::new(), value));
    }
    for publication in work.publications().await? {
        rows.push(row(
            "publication",
            publication.id.to_string(),
            publication.title,
        ));
    }
    for link in &work.links {
        rows.push(row("link", String::new(), link.clone()));
    }
    write_table(out, &rows)
}

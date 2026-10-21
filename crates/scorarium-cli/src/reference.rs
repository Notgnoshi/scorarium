use scorarium_engine::archive::same_name;

/// Find the one entity a reference names, by id or by name.
///
/// Fail if the reference is ambiguous, or names no candidate.
pub(crate) fn resolve<T>(
    kind: &str,
    reference: &str,
    candidates: Vec<T>,
    id: impl Fn(&T) -> i64,
    name: impl Fn(&T) -> &str,
) -> eyre::Result<T> {
    let parsed = reference.parse::<i64>();
    let mut matches: Vec<T> = candidates
        .into_iter()
        .filter(|c| parsed == Ok(id(c)) || same_name(name(c), reference))
        .collect();
    match matches.len() {
        0 => eyre::bail!("no {kind} '{reference}'"),
        1 => Ok(matches.remove(0)),
        n => {
            let entries: Vec<String> = matches
                .iter()
                .map(|c| format!("  {}  {}", id(c), name(c)))
                .collect();
            let plural = match kind.strip_suffix('y') {
                Some(stem) => format!("{stem}ies"),
                None => format!("{kind}s"),
            };
            eyre::bail!("'{reference}' names {n} {plural}:\n{}", entries.join("\n"));
        }
    }
}

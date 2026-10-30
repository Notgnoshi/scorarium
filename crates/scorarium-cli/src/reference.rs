use std::fmt::Display;
use std::str::FromStr;

use scorarium_engine::search::same_name;

/// Find the one entity a reference names, by id or by name.
///
/// Fail if the reference is ambiguous, or names no candidate.
pub(crate) fn resolve<T, I: FromStr + PartialEq + Display>(
    kind: &str,
    reference: &str,
    candidates: Vec<T>,
    id: impl Fn(&T) -> I,
    name: impl Fn(&T) -> &str,
) -> eyre::Result<T> {
    let parsed = reference.parse::<I>().ok();
    let mut matches: Vec<T> = candidates
        .into_iter()
        .filter(|c| Some(id(c)) == parsed || same_name(name(c), reference))
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

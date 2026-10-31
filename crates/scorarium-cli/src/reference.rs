use std::fmt::Display;
use std::str::FromStr;

use scorarium_engine::entity::Entity;
use scorarium_engine::link::{HttpUrl, Link, LinkId};
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
            eyre::bail!(
                "'{reference}' names {n} {}:\n{}",
                plural(kind, n),
                entries.join("\n")
            );
        }
    }
}

/// Pluralize `kind` for a count of `n`.
fn plural(kind: &str, n: usize) -> String {
    if n == 1 {
        return kind.to_string();
    }
    match kind.strip_suffix('y') {
        Some(stem) => format!("{stem}ies"),
        None => format!("{kind}s"),
    }
}

/// What an `add` reference names: an existing entity to link, or a new one to create.
pub(crate) enum Target<E> {
    Existing(E),
    New,
}

/// Decide whether an `add` reference links an existing entity or creates one.
///
/// A name match is never linked implicitly, because names are not unique. Fail with the candidates
/// so the caller can pick one by id or force creation.
pub(crate) fn target<E: Entity>(
    kind: &str,
    reference: &str,
    new: bool,
    mut candidates: Vec<E>,
    name: impl Fn(&E) -> &str,
    context: impl Fn(&E) -> String,
) -> eyre::Result<Target<E>> {
    if let Ok(id) = reference.parse::<E::Id>()
        && let Some(i) = candidates.iter().position(|c| c.id() == id)
    {
        if new {
            eyre::bail!("--new takes a name, not the id of an existing {kind}");
        }
        return Ok(Target::Existing(candidates.swap_remove(i)));
    }
    if new {
        return Ok(Target::New);
    }
    candidates.retain(|c| same_name(name(c), reference));
    if candidates.is_empty() {
        return Ok(Target::New);
    }
    let entries: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!("  {}  {}  {}", c.id(), name(c), context(c))
                .trim_end()
                .to_string()
        })
        .collect();
    eyre::bail!(
        "'{reference}' names {} {}:\n{}\nre-run with an id to link one, or with --new to create another",
        candidates.len(),
        plural(kind, candidates.len()),
        entries.join("\n")
    )
}

/// Find the link a reference resolves to, by id or by exact URL.
pub(crate) fn link(links: &[Link], reference: &str) -> eyre::Result<LinkId> {
    let id = reference.parse::<LinkId>().ok();
    let url = reference.parse::<HttpUrl>().ok();
    links
        .iter()
        .find(|l| Some(l.id) == id || Some(&l.url) == url.as_ref())
        .map(|l| l.id)
        .ok_or_else(|| eyre::eyre!("no link '{reference}'"))
}

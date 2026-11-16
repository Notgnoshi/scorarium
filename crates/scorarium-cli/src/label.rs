use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use scorarium_engine::entity::{AnyEntityId, EntityKind};
use scorarium_engine::person::PersonId;
use scorarium_engine::publication::PublicationId;
use scorarium_engine::work::WorkId;

/// A session-local name for an entity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Label(String);

impl FromStr for Label {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if valid {
            Ok(Label(s.to_string()))
        } else {
            Err("a label may contain only ASCII letters, digits, '-', and '_'".to_string())
        }
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// The entities bound to labels during a session.
#[derive(Clone, Default)]
pub(crate) struct Labels(BTreeMap<Label, AnyEntityId>);

impl Labels {
    /// Fail if the label is already bound.
    pub(crate) fn check(&self, label: &Label) -> eyre::Result<()> {
        if self.0.contains_key(label) {
            eyre::bail!("label '{label}' is already defined");
        }
        Ok(())
    }

    /// Bind the label to an entity.
    pub(crate) fn bind(&mut self, label: Label, id: AnyEntityId) {
        self.0.insert(label, id);
    }

    /// Look up the publication a `@label` reference names, if the reference is a label.
    pub(crate) fn publication(&self, reference: &str) -> eyre::Result<Option<PublicationId>> {
        Ok(match self.lookup(reference, EntityKind::Publication)? {
            Some(AnyEntityId::Publication(id)) => Some(id),
            _ => None,
        })
    }

    /// Look up the work a `@label` reference names, if the reference is a label.
    pub(crate) fn work(&self, reference: &str) -> eyre::Result<Option<WorkId>> {
        Ok(match self.lookup(reference, EntityKind::Work)? {
            Some(AnyEntityId::Work(id)) => Some(id),
            _ => None,
        })
    }

    /// Look up the person a `@label` reference names, if the reference is a label.
    pub(crate) fn person(&self, reference: &str) -> eyre::Result<Option<PersonId>> {
        Ok(match self.lookup(reference, EntityKind::Person)? {
            Some(AnyEntityId::Person(id)) => Some(id),
            _ => None,
        })
    }

    /// List the labels bound to entities of `kind`, in label order.
    pub(crate) fn of_kind(&self, kind: EntityKind) -> impl Iterator<Item = &Label> {
        self.0
            .iter()
            .filter(move |(_, id)| id.kind() == kind)
            .map(|(label, _)| label)
    }

    fn lookup(&self, reference: &str, expected: EntityKind) -> eyre::Result<Option<AnyEntityId>> {
        let Some(name) = reference.strip_prefix('@') else {
            return Ok(None);
        };
        let label: Label = name.parse().map_err(|e: String| eyre::eyre!(e))?;
        let Some(id) = self.0.get(&label).copied() else {
            eyre::bail!("no label '{label}'");
        };
        if id.kind() != expected {
            eyre::bail!("'{label}' labels a {}, not a {expected}", id.kind());
        }
        Ok(Some(id))
    }
}

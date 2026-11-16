use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use scorarium_engine::entity::AnyEntityId;

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
}

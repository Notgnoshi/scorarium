use std::fmt;
use std::str::FromStr;

use crate::Error;

pub type Timestamp = chrono::DateTime<chrono::Utc>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Publication,
    Work,
    Person,
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EntityKind::Publication => "publication",
            EntityKind::Work => "work",
            EntityKind::Person => "person",
        })
    }
}

impl FromStr for EntityKind {
    type Err = Error;

    fn from_str(text: &str) -> Result<EntityKind, Error> {
        Ok(match text {
            "publication" => EntityKind::Publication,
            "work" => EntityKind::Work,
            "person" => EntityKind::Person,
            other => return Err(eyre::eyre!("unknown entity kind {other:?}").into()),
        })
    }
}

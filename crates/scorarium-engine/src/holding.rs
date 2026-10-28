use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HoldingId(i64);

impl HoldingId {
    pub(crate) fn new(id: i64) -> HoldingId {
        HoldingId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for HoldingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for HoldingId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<HoldingId, ParseIntError> {
        text.parse().map(HoldingId)
    }
}

/// A copy of a publication that the library owns
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holding {
    pub id: HoldingId,
    pub kind: HoldingKind,
    /// Who has borrowed the copy, if anyone
    pub loaned_to: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoldingKind {
    /// A printed copy
    Physical { location: Option<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewHolding<'a> {
    Physical { location: Option<&'a str> },
}

use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LibraryId(pub(crate) i64);

impl fmt::Display for LibraryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for LibraryId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> Result<LibraryId, ParseIntError> {
        text.parse().map(LibraryId)
    }
}

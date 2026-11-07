use std::fmt;
use std::str::FromStr;

use crate::person::PersonId;
use crate::{Error, Result};

/// A person named on a publication or work with a given role
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub person: PersonId,
    pub name: String,
    pub sort_name: String,
    pub role: Role,
}

/// A contributor's role, case insensitive
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Role(String);

impl Role {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create a new [Role] from a string that's known to be valid.
    pub(crate) fn from_valid(role: String) -> Role {
        Role(role)
    }
}

impl FromStr for Role {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let role = s.trim();
        if role.is_empty() {
            return Err(Error::InvalidValue(
                "A contributor's role cannot be blank.".into(),
            ));
        }
        if role.contains(['\n', '\r']) {
            return Err(Error::InvalidValue(
                "A contributor's role cannot span multiple lines.".into(),
            ));
        }
        Ok(Role(role.to_lowercase()))
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_lowercase_and_non_blank() {
        assert_eq!("Composer".parse::<Role>().unwrap().as_str(), "composer");
        assert_eq!(
            " Übersetzer ".parse::<Role>().unwrap().as_str(),
            "übersetzer"
        );
        assert!("  ".parse::<Role>().is_err());
    }
}

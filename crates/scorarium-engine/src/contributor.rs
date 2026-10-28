use crate::person::PersonId;
use crate::{Error, Result};

/// A person named on a publication or work with a given role
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub person: PersonId,
    pub name: String,
    pub role: String,
}

pub(crate) fn clean_role(role: &str) -> Result<&str> {
    let role = role.trim();
    if role.is_empty() {
        return Err(Error::InvalidValue(
            "A contributor's role cannot be blank.".into(),
        ));
    }
    Ok(role)
}

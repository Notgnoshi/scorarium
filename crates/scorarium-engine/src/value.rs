use std::fmt;
use std::str::FromStr;

use crate::{Error, Result};

/// A calendar year from 1 to 9999
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Year(u16);

impl Year {
    pub fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Year {
    type Error = Error;

    fn try_from(year: u16) -> Result<Year> {
        if (1..=9999).contains(&year) {
            Ok(Year(year))
        } else {
            // Hopefully a historian never sees this O.O
            Err(Error::InvalidValue(
                "A year is a number from 1 to 9999.".to_string(),
            ))
        }
    }
}

impl FromStr for Year {
    type Err = Error;

    fn from_str(input: &str) -> Result<Year> {
        let year: u16 = input.trim().parse().unwrap_or(0); // 0 is not a valid year, falls through in try_from
        Year::try_from(year)
    }
}

impl fmt::Display for Year {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A rating from 1 to 5 stars
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stars(u8);

impl Stars {
    pub fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Stars {
    type Error = Error;

    fn try_from(stars: u8) -> Result<Stars> {
        if (1..=5).contains(&stars) {
            Ok(Stars(stars))
        } else {
            Err(Error::InvalidValue("A rating is 1 to 5 stars.".to_string()))
        }
    }
}

impl FromStr for Stars {
    type Err = Error;

    fn from_str(input: &str) -> Result<Stars> {
        // 0 is an invalid value. None is used to represent the lack of a rating.
        let stars: u8 = input.trim().parse().unwrap_or(0);
        Stars::try_from(stars)
    }
}

impl fmt::Display for Stars {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Whether a publication or work is sheet music or prose
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Classification {
    Music,
    Prose,
}

impl fmt::Display for Classification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Classification::Music => "music",
            Classification::Prose => "prose",
        };
        f.write_str(name)
    }
}

impl FromStr for Classification {
    type Err = Error;

    fn from_str(input: &str) -> Result<Classification> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("music") {
            Ok(Classification::Music)
        } else if input.eq_ignore_ascii_case("prose") {
            Ok(Classification::Prose)
        } else {
            Err(Error::InvalidValue(
                "A classification is music or prose.".to_string(),
            ))
        }
    }
}

/// A non-empty lowercase ASCII slug
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(String);

impl Tag {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Tag {
    type Err = Error;

    fn from_str(input: &str) -> Result<Tag> {
        let input = input.trim();
        if input.is_empty() {
            return Err(Error::InvalidValue("A tag cannot be empty.".to_string()));
        }
        let tag = input.to_ascii_lowercase();
        if !tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(Error::InvalidValue(format!(
                "'{input}' contains invalid characters for a tag."
            )));
        }
        Ok(Tag(tag))
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parse a whitespace separated list of tags
pub fn parse_tags(input: &str) -> Result<Vec<Tag>> {
    let mut tags: Vec<Tag> = Vec::new();
    for input in input.split_whitespace() {
        let tag = input.parse::<Tag>()?;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strong_types() {
        assert!(matches!("0".parse::<Year>(), Err(Error::InvalidValue(_))));
        assert!(matches!("1999".parse::<Year>(), Ok(Year(1999))));
        assert!(matches!(Stars::try_from(6), Err(Error::InvalidValue(_))));
        assert!(matches!(Stars::try_from(4), Ok(Stars(4))));
        assert!(matches!(
            "poetry".parse::<Classification>(),
            Err(Error::InvalidValue(_))
        ));
        assert_eq!(
            " MUSIC ".parse::<Classification>().unwrap(),
            Classification::Music
        );
    }

    fn parsed(input: &str) -> Vec<String> {
        parse_tags(input)
            .unwrap()
            .iter()
            .map(|t| t.as_str().to_string())
            .collect()
    }

    #[test]
    fn tags_are_slugs() {
        assert_eq!(
            parsed("Christmas PIANO duet"),
            ["christmas", "piano", "duet"]
        );
        assert_eq!(parsed("christmas christmas"), ["christmas"]);
        assert_eq!(parsed("  \n "), [] as [String; 0]);
        assert_eq!(parsed("want-to_learn2"), ["want-to_learn2"]);
        assert_eq!(" Piano ".parse::<Tag>().unwrap().as_str(), "piano");
        assert!(matches!(
            parse_tags("christmas christmas!"),
            Err(Error::InvalidValue(_))
        ));
        assert!(matches!("Noël".parse::<Tag>(), Err(Error::InvalidValue(_))));
        assert!(matches!("".parse::<Tag>(), Err(Error::InvalidValue(_))));
    }
}

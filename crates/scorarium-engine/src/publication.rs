use std::fmt;
use std::str::FromStr;

use isbn::{Isbn, Isbn13, IsbnError};

use crate::{Error, Result};

/// The kinds of identifier printed on a publication
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IdentifierKind {
    Isbn,
    /// Like an ISBN, but for music
    Ismn,
    /// A generic publisher number
    ///
    /// These are typically printed on the front cover, if present.
    PublisherNumber,
    /// A more specific plate number to identify a particular engraving
    ///
    /// These are typically printed at the bottom of the page, if present.
    PlateNumber,
}

impl fmt::Display for IdentifierKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            IdentifierKind::Isbn => "isbn",
            IdentifierKind::Ismn => "ismn",
            IdentifierKind::PublisherNumber => "publisher_number",
            IdentifierKind::PlateNumber => "plate_number",
        };
        f.write_str(name)
    }
}

impl FromStr for IdentifierKind {
    type Err = Error;

    fn from_str(input: &str) -> Result<IdentifierKind> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("isbn") {
            Ok(IdentifierKind::Isbn)
        } else if input.eq_ignore_ascii_case("ismn") {
            Ok(IdentifierKind::Ismn)
        } else if input.eq_ignore_ascii_case("publisher_number") {
            Ok(IdentifierKind::PublisherNumber)
        } else if input.eq_ignore_ascii_case("plate_number") {
            Ok(IdentifierKind::PlateNumber)
        } else {
            Err(Error::InvalidValue(format!(
                "'{input}' must be one of: isbn, ismn, publisher_number, or plate_number."
            )))
        }
    }
}

/// An identifier printed on a publication, parsed into its canonical form
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identifier {
    kind: IdentifierKind,
    text: String,
}

impl Identifier {
    /// Validate an identifier as typed or printed and put it in canonical form.
    ///
    /// ISBNs become hyphenated ISBN-13 (ISBN-10 is converted), ISMNs the hyphenated `979-0` form,
    /// and publisher and plate numbers are trimmed and uppercased.
    pub fn new(kind: IdentifierKind, text: &str) -> Result<Identifier> {
        let text = match kind {
            IdentifierKind::Isbn => normalize_isbn(strip_label(text, "ISBN"))?,
            IdentifierKind::Ismn => normalize_ismn(strip_label(text, "ISMN"))?,
            IdentifierKind::PublisherNumber | IdentifierKind::PlateNumber => {
                let text = text.trim();
                if text.is_empty() {
                    let message = match kind {
                        IdentifierKind::PublisherNumber => "A publisher number cannot be empty.",
                        _ => "A plate number cannot be empty.",
                    };
                    return Err(Error::InvalidValue(message.to_string()));
                }
                text.to_uppercase()
            }
        };
        Ok(Identifier { kind, text })
    }

    pub fn kind(&self) -> IdentifierKind {
        self.kind
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Whether the text looks like it could be an ISBN or ISMN while it's being typed
pub fn looks_like_identifier(query: &str) -> bool {
    let query = query.trim().as_bytes();
    if query.get(..4).is_some_and(|head| {
        head.eq_ignore_ascii_case(b"isbn") || head.eq_ignore_ascii_case(b"ismn")
    }) {
        return true;
    }
    let compact: Vec<u8> = query
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, b'-' | b' '))
        .collect();
    // The pre-2008 ISMN form starts with M, and an ISBN-10 check digit can be X
    let digits = compact
        .strip_prefix(b"M")
        .or_else(|| compact.strip_prefix(b"m"))
        .unwrap_or(&compact);
    let digits = digits
        .strip_suffix(b"X")
        .or_else(|| digits.strip_suffix(b"x"))
        .unwrap_or(digits);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// Strip off the label prefix an ISBN or ISMN number is often printed with
///
/// "ISBN 978-...", "ISBN-13: 978-...", "ISBN 10: 0-...", and "ISMN M-..."
///
/// This allows users to enter the identifiers as they appear printed on a publication, even though
/// the prefix isn't actually necessary.
fn strip_label<'a>(value: &'a str, label: &str) -> &'a str {
    let value = value.trim();
    let Some(rest) = value
        .get(..label.len())
        .filter(|head| head.eq_ignore_ascii_case(label))
        .map(|_| &value[label.len()..])
    else {
        return value;
    };
    // Treat "ISBN-13", "ISBN 13", and "ISBN13" all the same
    let rest = rest.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
    let rest = match rest.strip_prefix("13").or_else(|| rest.strip_prefix("10")) {
        Some(after) if !after.starts_with(|c: char| c.is_ascii_digit()) => after,
        _ => rest,
    };
    rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace())
}

fn normalize_isbn(text: &str) -> Result<String> {
    let invalid = || Error::InvalidValue(format!("'{text}' is not a valid ISBN."));
    let isbn13 = match text.parse::<Isbn>() {
        Ok(Isbn::_10(isbn10)) => Isbn13::from(isbn10),
        Ok(Isbn::_13(isbn13)) => isbn13,
        Err(_) => return Err(invalid()),
    };
    match isbn13.hyphenate() {
        Ok(hyphenated) => Ok(hyphenated.to_string()),
        Err(IsbnError::UndefinedRange | IsbnError::InvalidGroup) => {
            // There's two yucky choices for parsing the ISBN registration group (because it's
            // constantly changing, which is miserable):
            //
            // 1. Download the ISBN tables from the international authority (and cache it?)
            // 2. Build the ISBN tables into scorarium at build time
            //
            // I picked the second option because it's simple, but it does mean that ISBNs assigned
            // after the scorarium binary is built (and Cargo.lock is updated) have the potential to
            // be unrecognized. Big yikes.
            //
            // I'm betting I don't hit that case because:
            //
            // 1. This is a personal project
            // 2. My personal library doesn't gain many brand new books, and essentially zero new
            //    music books
            // 3. I don't want to have to download the tables
            //
            // If future me runs into this issue, it should "just work" to update the 'isbn' crate
            // to its latest version and release a new scorarium build.
            Err(Error::InvalidValue(format!(
                "The registration group of '{text}' is not in scorarium's ISBN range table, which may be out of date."
            )))
        }
        Err(_) => Err(invalid()),
    }
}

fn normalize_ismn(text: &str) -> Result<String> {
    let invalid = || Error::InvalidValue(format!("'{text}' is not a valid ISMN."));
    let compact: String = text
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .collect::<String>()
        .to_ascii_uppercase();
    // The pre-2008 ten-character form spells the 979-0 prefix as a letter M
    let digits = match compact.strip_prefix('M') {
        Some(rest) => format!("9790{rest}"),
        None => compact,
    };
    if digits.len() != 13
        || !digits.starts_with("9790")
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    let checksum: u32 = digits
        .bytes()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    if !checksum.is_multiple_of(10) {
        return Err(invalid());
    }
    let body = &digits[4..12];
    let registrant_len = match body.as_bytes()[0] {
        b'0' => 3,
        b'1'..=b'3' => 4,
        b'4'..=b'6' => 5,
        b'7' | b'8' => 6,
        _ => 7,
    };
    let (registrant, item) = body.split_at(registrant_len);
    let check = &digits[12..];
    Ok(format!("979-0-{registrant}-{item}-{check}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(kind: IdentifierKind, value: &str) -> Result<String> {
        Identifier::new(kind, value).map(|id| id.as_str().to_string())
    }

    #[test]
    fn isbn() {
        assert_eq!(
            n(IdentifierKind::Isbn, "0-486-23134-8").unwrap(),
            "978-0-486-23134-1"
        );
        for input in [
            "978-1-4950-0871-9",
            "978 1 4950 0871 9",
            "9781495008719",
            "ISBN 978-1-4950-0871-9",
            "ISBN: 978-1-4950-0871-9",
            "ISBN\t978-1-4950-0871-9",
            "isbn-13: 9781495008719",
            "ISBN 13: 9781495008719",
            "ISBN13 9781495008719",
        ] {
            assert_eq!(n(IdentifierKind::Isbn, input).unwrap(), "978-1-4950-0871-9");
        }
        for input in ["ISBN-10 0-7935-7224-X", "ISBN 10: 0-7935-7224-X"] {
            assert_eq!(n(IdentifierKind::Isbn, input).unwrap(), "978-0-7935-7224-3");
        }
        // The leading 10 is part of the number, not the form's name
        assert_eq!(
            n(IdentifierKind::Isbn, "ISBN 108456789X").unwrap(),
            "978-1-0845-6789-4"
        );
        assert!(matches!(
            n(IdentifierKind::Isbn, "978-1-4950-0871-0"),
            Err(Error::InvalidValue(_))
        ));
        // Checksum-valid, but 978-67 is not an assigned registration group
        assert!(matches!(
            n(IdentifierKind::Isbn, "9786700000007"),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn ismn() {
        for input in [
            "M-060-08002-9",
            "ISMN M-060-08002-9",
            "ISMN: 979-0-060-08002-9",
            "ISMN-13 979-0-060-08002-9",
            "979-0-060-08002-9",
        ] {
            assert_eq!(n(IdentifierKind::Ismn, input).unwrap(), "979-0-060-08002-9");
        }
        assert_eq!(
            n(IdentifierKind::Ismn, "9790299102349").unwrap(),
            "979-0-2991-0234-9"
        );
        assert!(matches!(
            n(IdentifierKind::Ismn, "979-0-060-08002-0"),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn free_text_numbers() {
        let plate = Identifier::new(IdentifierKind::PlateNumber, " ut 50061 ").unwrap();
        assert_eq!(plate.as_str(), "UT 50061");
        assert_eq!(plate.to_string(), "UT 50061");
        assert!(matches!(
            n(IdentifierKind::PublisherNumber, "   "),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn identifier_fragments_look_like_identifiers_and_titles_do_not() {
        for input in [
            "ISBN 978",
            "ismn",
            "978",
            "978-0-48",
            "978 0 486 23134 1",
            "M-060-080",
            "0-7935-7224-X",
            // Complete forms the Identifier tests accept
            "ISBN 13: 9781495008719",
            "isbn-13: 9781495008719",
            "ISBN 10: 0-7935-7224-X",
            "9781495008719",
            "M-060-08002-9",
            "979-0-060-08002-9",
        ] {
            assert!(looks_like_identifier(input), "{input:?}");
        }
        for input in [
            "",
            "Three Gymnopedies",
            "Op. 9 No. 2",
            "Symphony 40",
            "1984 Orwell",
            "The ISBN Handbook",
        ] {
            assert!(!looks_like_identifier(input), "{input:?}");
        }
    }
}

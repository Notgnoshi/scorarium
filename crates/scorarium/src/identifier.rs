use std::fmt::{self, Display};
use std::str::FromStr;

use isbn::{Isbn, Isbn13, IsbnError};

/// The kinds of identifier printed on a publication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Isbn,
    Ismn,
    PublisherNumber,
    PlateNumber,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Isbn => "isbn",
            Kind::Ismn => "ismn",
            Kind::PublisherNumber => "publisher_number",
            Kind::PlateNumber => "plate_number",
        }
    }
}

impl FromStr for Kind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "isbn" => Ok(Kind::Isbn),
            "ismn" => Ok(Kind::Ismn),
            "publisher_number" => Ok(Kind::PublisherNumber),
            "plate_number" => Ok(Kind::PlateNumber),
            _ => Err(format!("unknown identifier kind: {s}")),
        }
    }
}

// TODO: Enrich the error type to facilitate better error reporting on form validation.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Bad check digit, wrong length, unexpected characters, or empty.
    Invalid(Kind),
    /// The ISBN validates but its range is unknown to the embedded range table.
    OutOfDate,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(Kind::Isbn) => write!(f, "invalid ISBN"),
            Error::Invalid(Kind::Ismn) => write!(f, "invalid ISMN"),
            Error::Invalid(Kind::PublisherNumber) => write!(f, "invalid publisher number"),
            Error::Invalid(Kind::PlateNumber) => write!(f, "invalid plate number"),
            Error::OutOfDate => write!(f, "scorarium's ISBN ranges are out of date"),
        }
    }
}

impl std::error::Error for Error {}

/// An identifier in canonical form
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized(String);

impl Normalized {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validate an identifier as typed or printed and put it in canonical form.
///
/// ISBNs become hyphenated ISBN-13 (ISBN-10 is converted), ISMNs the hyphenated `979-0` form,
/// and publisher and plate numbers are trimmed and uppercased.
pub fn normalize(kind: Kind, value: &str) -> Result<Normalized, Error> {
    match kind {
        Kind::Isbn => normalize_isbn(strip_label(value, "ISBN")),
        Kind::Ismn => normalize_ismn(strip_label(value, "ISMN")),
        Kind::PublisherNumber | Kind::PlateNumber => {
            let value = value.trim();
            if value.is_empty() {
                return Err(Error::Invalid(kind));
            }
            Ok(Normalized(value.to_uppercase()))
        }
    }
}

/// Drop the label a number is printed with, so "ISBN 978-...", "ISBN-13: 978-...", "ISBN 10: 0-...",
/// and "ISMN M-..." can be typed as they appear on the page.
fn strip_label<'a>(value: &'a str, label: &str) -> &'a str {
    let value = value.trim();
    let Some(rest) = value
        .get(..label.len())
        .filter(|head| head.eq_ignore_ascii_case(label))
        .map(|_| &value[label.len()..])
    else {
        return value;
    };
    // "ISBN-13", "ISBN 13", and "ISBN13" all name the form. A digit right after the 13 or 10
    // means it is the number itself: an unhyphenated ISBN-10 in group 1 can begin with either.
    let rest = rest.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
    let rest = match rest.strip_prefix("13").or_else(|| rest.strip_prefix("10")) {
        Some(after) if !after.starts_with(|c: char| c.is_ascii_digit()) => after,
        _ => rest,
    };
    rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace())
}

fn normalize_isbn(value: &str) -> Result<Normalized, Error> {
    let isbn13 = match value.parse::<Isbn>() {
        Ok(Isbn::_10(isbn10)) => Isbn13::from(isbn10),
        Ok(Isbn::_13(isbn13)) => isbn13,
        Err(_) => return Err(Error::Invalid(Kind::Isbn)),
    };
    match isbn13.hyphenate() {
        Ok(hyphenated) => Ok(Normalized(hyphenated.to_string())),
        Err(IsbnError::UndefinedRange | IsbnError::InvalidGroup) => Err(Error::OutOfDate),
        Err(_) => Err(Error::Invalid(Kind::Isbn)),
    }
}

fn normalize_ismn(value: &str) -> Result<Normalized, Error> {
    let invalid = Error::Invalid(Kind::Ismn);
    let compact: String = value
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
        return Err(invalid);
    }
    let checksum: u32 = digits
        .bytes()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    if !checksum.is_multiple_of(10) {
        return Err(invalid);
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
    Ok(Normalized(format!("979-0-{registrant}-{item}-{check}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(kind: Kind, value: &str) -> Result<String, Error> {
        normalize(kind, value).map(|n| n.0)
    }

    #[test]
    fn isbn10_upconverts() {
        assert_eq!(
            n(Kind::Isbn, "0-486-23134-8"),
            Ok("978-0-486-23134-1".into())
        );
        assert_eq!(
            n(Kind::Isbn, "0-7935-7224-X"),
            Ok("978-0-7935-7224-3".into())
        );
    }

    #[test]
    fn isbn13_hyphenates_regardless_of_input_separators() {
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
            assert_eq!(n(Kind::Isbn, input), Ok("978-1-4950-0871-9".into()));
        }
        for input in ["ISBN-10 0-7935-7224-X", "ISBN 10: 0-7935-7224-X"] {
            assert_eq!(n(Kind::Isbn, input), Ok("978-0-7935-7224-3".into()));
        }
        // The leading 10 is part of the number, not the form's name
        assert_eq!(
            n(Kind::Isbn, "ISBN 108456789X"),
            Ok("978-1-0845-6789-4".into())
        );
    }

    #[test]
    fn isbn_errors() {
        assert_eq!(
            n(Kind::Isbn, "978-1-4950-0871-0"),
            Err(Error::Invalid(Kind::Isbn))
        );
        // Checksum-valid, but 978-67 is not an assigned registration group
        assert_eq!(n(Kind::Isbn, "9786700000007"), Err(Error::OutOfDate));
    }

    #[test]
    fn ismn_hyphenates() {
        assert_eq!(
            n(Kind::Ismn, "M-060-08002-9"),
            Ok("979-0-060-08002-9".into())
        );
        for input in [
            "ISMN M-060-08002-9",
            "ISMN: 979-0-060-08002-9",
            "ISMN-13 979-0-060-08002-9",
            "979-0-060-08002-9",
        ] {
            assert_eq!(n(Kind::Ismn, input), Ok("979-0-060-08002-9".into()));
        }
        assert_eq!(
            n(Kind::Ismn, "9790299102349"),
            Ok("979-0-2991-0234-9".into())
        );
    }

    #[test]
    fn ismn_bad_check_digit() {
        assert_eq!(
            n(Kind::Ismn, "979-0-060-08002-0"),
            Err(Error::Invalid(Kind::Ismn))
        );
    }

    #[test]
    fn free_text_numbers() {
        assert_eq!(n(Kind::PlateNumber, " ut 50061 "), Ok("UT 50061".into()));
        assert_eq!(
            n(Kind::PublisherNumber, "   "),
            Err(Error::Invalid(Kind::PublisherNumber))
        );
    }
}

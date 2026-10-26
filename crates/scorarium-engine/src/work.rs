use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::{Error, Result};

/// A catalog numbering scheme that scorarium knows how to parse
struct Scheme {
    aliases: &'static [&'static str],
}

/// Listed in priority order: earlier schemes are parsed with higher confidence.
static SCHEMES: &[Scheme] = &[
    // Opus number, the "standard" catalog number scheme.
    //
    // https://en.wikipedia.org/wiki/Opus_number
    Scheme {
        aliases: &["op", "opus"],
    },
    // Bach-Werke-Verzeichnis, Wolfgang Schmieder's catalogue of J. S. Bach
    //
    // https://en.wikipedia.org/wiki/Bach-Werke-Verzeichnis
    Scheme { aliases: &["bwv"] },
    // Koechel catalogue of Mozart, written K. or KV (Koechel-Verzeichnis)
    //
    // https://en.wikipedia.org/wiki/K%C3%B6chel_catalogue
    Scheme {
        aliases: &["k", "kv"],
    },
    // Deutsch catalogue of Schubert
    //
    // https://en.wikipedia.org/wiki/Deutsch_catalogue
    Scheme { aliases: &["d"] },
    // Handel-Werke-Verzeichnis, Bernd Baselt's catalogue of Handel
    //
    // https://en.wikipedia.org/wiki/H%C3%A4ndel-Werke-Verzeichnis
    Scheme { aliases: &["hwv"] },
    // Ryom-Verzeichnis, Peter Ryom's catalogue of Vivaldi
    //
    // https://en.wikipedia.org/wiki/Ryom-Verzeichnis
    Scheme { aliases: &["rv"] },
    // Werk ohne Opuszahl, a work its composer published without an opus number, chiefly Beethoven
    //
    // https://en.wikipedia.org/wiki/WoO
    Scheme { aliases: &["woo"] },
    // Humphrey Searle's catalogue of Liszt
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Franz_Liszt
    Scheme { aliases: &["s"] },
    // Andras Szollosy's catalogue of Bartok
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_B%C3%A9la_Bart%C3%B3k
    Scheme { aliases: &["sz"] },
    // Laszlo Somfai's catalogue of Bartok
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_B%C3%A9la_Bart%C3%B3k
    Scheme { aliases: &["bb"] },
    // Jarmil Burghauser's catalogue of Dvorak
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Anton%C3%ADn_Dvo%C5%99%C3%A1k
    Scheme { aliases: &["b"] },
    // Francois Lesure's catalogue of Debussy
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Claude_Debussy
    Scheme { aliases: &["l"] },
    // Franklin Zimmerman's catalogue of Purcell
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Henry_Purcell
    Scheme { aliases: &["z"] },
    // Wydanie Narodowe, Jan Ekier's Polish National Edition of Chopin
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Fr%C3%A9d%C3%A9ric_Chopin
    Scheme { aliases: &["wn"] },
    // Ralph Kirkpatrick's catalogue of Scarlatti's keyboard sonatas, written Kk. to keep it apart
    // from Mozart's K. Chopin's Kobylanska catalogue also uses KK, but numbers such as
    // "KK IVa/16" carry a Roman numeral, so they stay unrecognized rather than parsing as this.
    //
    // https://en.wikipedia.org/wiki/List_of_solo_keyboard_sonatas_by_Domenico_Scarlatti
    Scheme { aliases: &["kk"] },
];

/// A catalog number
///
/// Two values are equal when a musician would read them as the same number, so equal values can
/// print differently: "Op. 27/2" equals "Op. 27 No. 2".
#[derive(Clone, Debug)]
pub struct CatalogNumber {
    /// Raw text entered from the user
    text: String,
    /// Index into [SCHEMES]; None when unrecognized
    scheme: Option<usize>,
    /// The sort tuple: the parsed numbers, or every run of digits when unrecognized
    numbers: Vec<u32>,
}

impl CatalogNumber {
    /// The raw text entered by the user
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the parser knows the scheme this number is written in
    pub fn is_recognized(&self) -> bool {
        self.scheme.is_some()
    }

    /// Get an [Ord]-safe sort key for [CatalogNumber]
    ///
    /// Recognized number schemes are sorted first, in order of decreasing confidence. Unrecognized
    /// numbers sort by the strings of digits in them like a tuple, and then alphabetically to break
    /// ties.
    fn key(&self) -> (usize, bool, &[u32], Option<&str>) {
        (
            self.scheme.unwrap_or(usize::MAX),
            self.numbers.is_empty(),
            &self.numbers,
            self.scheme.is_none().then_some(self.text.as_str()),
        )
    }
}

impl FromStr for CatalogNumber {
    type Err = Error;

    /// Any non-empty catalog number is accepted because there's no universal numbering scheme or
    /// parser that can handle every edge case, of which there are many.
    fn from_str(input: &str) -> Result<CatalogNumber> {
        let text = input.trim();
        if text.is_empty() {
            return Err(Error::InvalidValue(
                "A catalog number cannot be empty.".to_string(),
            ));
        }
        let (scheme, numbers) = match recognize(text) {
            Some((scheme, numbers)) => (Some(scheme), numbers),
            None => (None, digit_runs(text)),
        };
        Ok(CatalogNumber {
            text: text.to_string(),
            scheme,
            numbers,
        })
    }
}

impl fmt::Display for CatalogNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for CatalogNumber {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for CatalogNumber {}

impl Ord for CatalogNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for CatalogNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn recognize(text: &str) -> Option<(usize, Vec<u32>)> {
    let (label, rest) = split_letters(text);
    let scheme = SCHEMES.iter().position(|scheme| {
        scheme
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(label))
    })?;
    let rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
    let (number, rest) = integer(rest)?;
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    if rest.is_empty() {
        return Some((scheme, vec![number]));
    }
    let rest = match rest.strip_prefix('/') {
        Some(rest) => rest,
        None => sub_number_word(rest)?,
    };
    let (sub_number, rest) = integer(rest.trim_start())?;
    rest.is_empty().then(|| (scheme, vec![number, sub_number]))
}

fn split_letters(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    text.split_at(end)
}

fn integer(text: &str) -> Option<(u32, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let number = text[..end].parse().ok()?;
    Some((number, &text[end..]))
}

/// "No. 2", "no 2", "Nr. 2", "N. 2", "Number 2": the word with an optional dot, returning what follows
fn sub_number_word(text: &str) -> Option<&str> {
    let (word, rest) = split_letters(text);
    ["no", "nr", "n", "number"]
        .iter()
        .any(|known| known.eq_ignore_ascii_case(word))
        .then(|| rest.strip_prefix('.').unwrap_or(rest))
}

/// Every run of digits, in order; a run too long for u32 is skipped
fn digit_runs(text: &str) -> Vec<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .filter_map(|run| run.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(text: &str) -> CatalogNumber {
        text.parse().unwrap()
    }

    #[test]
    fn every_spelling_of_an_opus_number_is_equal() {
        let canonical = number("Op. 27 No. 2");
        for spelling in [
            "Op. 27, No. 2",
            "Op. 27/2",
            "Op 27 Nr. 2",
            "op.27 no.2",
            "Opus 27 Number 2",
            "Op 27 No 2",
            "  Op. 27 No. 2  ",
        ] {
            let n = number(spelling);
            assert!(n.is_recognized(), "{spelling:?}");
            assert_eq!(n, canonical, "{spelling:?}");
        }
        assert_ne!(number("Op. 27"), canonical);
        assert_ne!(number("Op. 27 No. 3"), canonical);
        assert_ne!(number("BWV 27"), number("Op. 27"));
        // Text is kept as typed, apart from trimming
        assert_eq!(number("  op.27/2 ").as_str(), "op.27/2");
    }

    #[test]
    fn composer_catalogs_parse_to_their_scheme_and_numbers() {
        let opus = number("Op. 1").scheme;
        for (text, numbers) in [
            ("BWV 988", vec![988]),
            ("K. 331", vec![331]),
            ("KV 331", vec![331]),
            ("D 960", vec![960]),
            ("D. 899 No. 3", vec![899, 3]),
            ("WoO 59", vec![59]),
            ("S. 244/2", vec![244, 2]),
            ("Sz. 107/97", vec![107, 97]),
            ("BB 105", vec![105]),
            ("HWV 56", vec![56]),
            ("RV 269", vec![269]),
            ("Z 626", vec![626]),
            ("L. 75", vec![75]),
            ("B. 178", vec![178]),
            ("WN 37", vec![37]),
            ("Kk. 380", vec![380]),
        ] {
            let n = number(text);
            assert!(n.scheme.is_some(), "{text:?} should be recognized");
            assert_ne!(n.scheme, opus, "{text:?} is not an opus number");
            assert_eq!(n.numbers, numbers, "{text:?}");
        }
        // Same letters, different catalogs: K. is Koechel, Kk. is Kirkpatrick
        assert_ne!(number("K. 380"), number("Kk. 380"));
    }

    #[test]
    fn awkward_numbers_are_unrecognized_but_keep_their_digits() {
        for (text, digits) in [
            ("Op. posth. 66", vec![66]),
            ("Op. 72 No. 1 (Posthumous opus)", vec![72, 1]),
            ("Op. 37a No. 6", vec![37, 6]),
            ("Op. 19b", vec![19]),
            ("BWV Anh. 114", vec![114]),
            ("BWV 846-869", vec![846, 869]),
            ("Hob. XVI:52", vec![52]),
            ("K. 331/300i", vec![331, 300]),
            ("KK IVa/16", vec![16]),
            ("MWV O 14", vec![14]),
            ("No. 1", vec![1]),
            ("none", vec![]),
        ] {
            let n = number(text);
            assert!(!n.is_recognized(), "{text:?}");
            assert_eq!(n.numbers, digits, "{text:?}");
            assert_eq!(n.as_str(), text.trim(), "{text:?}");
        }
        // Unrecognized values are equal only on identical text
        assert_eq!(number("Hob. XVI:52"), number("Hob. XVI:52"));
        assert_ne!(number("Hob. XVI:52"), number("Hob XVI:52"));
        assert!(matches!(
            "  ".parse::<CatalogNumber>(),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn sorts_by_scheme_then_tuple_with_unrecognized_last() {
        let mut numbers: Vec<CatalogNumber> = [
            "Op. posth. 66",
            "D 960",
            "Op. 100",
            "BWV 988",
            "Op. 27 No. 2",
            "Hob. XVI:52",
            "Op. 16",
            "Op. 27",
            "Op. 87 No. 24",
            "BWV 846",
            "none",
        ]
        .into_iter()
        .map(number)
        .collect();
        numbers.sort();
        let sorted: Vec<&str> = numbers.iter().map(CatalogNumber::as_str).collect();
        assert_eq!(
            sorted,
            [
                "Op. 16",
                "Op. 27",
                "Op. 27 No. 2",
                "Op. 87 No. 24",
                "Op. 100",
                "BWV 846",
                "BWV 988",
                "D 960",
                // Unrecognized: by digit runs, then text; no digits at all sorts last
                "Hob. XVI:52",
                "Op. posth. 66",
                "none",
            ]
        );
    }
}

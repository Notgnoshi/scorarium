use std::cmp::Ordering;
use std::fmt::{self, Display};

/// A catalog scheme the parser knows
struct Scheme {
    /// The label as customarily written
    #[expect(dead_code)]
    label: &'static str,
    /// Spellings accepted before the number, compared case-insensitively; a trailing dot is
    /// optional in the input and not part of the alias
    aliases: &'static [&'static str],
}

/// Listed in priority order: earlier schemes are parsed with higher confidence, and thus sort
/// first, and when a work carries several numbers the earliest scheme's number is the one shown.
static SCHEMES: &[Scheme] = &[
    // Opus number, the "standard" catalog number scheme.
    //
    // https://en.wikipedia.org/wiki/Opus_number
    Scheme {
        label: "Op.",
        aliases: &["op", "opus"],
    },
    // Bach-Werke-Verzeichnis, Wolfgang Schmieder's catalogue of J. S. Bach
    //
    // https://en.wikipedia.org/wiki/Bach-Werke-Verzeichnis
    Scheme {
        label: "BWV",
        aliases: &["bwv"],
    },
    // Koechel catalogue of Mozart, written K. or KV (Koechel-Verzeichnis)
    //
    // https://en.wikipedia.org/wiki/K%C3%B6chel_catalogue
    Scheme {
        label: "K.",
        aliases: &["k", "kv"],
    },
    // Deutsch catalogue of Schubert
    //
    // https://en.wikipedia.org/wiki/Deutsch_catalogue
    Scheme {
        label: "D",
        aliases: &["d"],
    },
    // Handel-Werke-Verzeichnis, Bernd Baselt's catalogue of Handel
    //
    // https://en.wikipedia.org/wiki/H%C3%A4ndel-Werke-Verzeichnis
    Scheme {
        label: "HWV",
        aliases: &["hwv"],
    },
    // Ryom-Verzeichnis, Peter Ryom's catalogue of Vivaldi
    //
    // https://en.wikipedia.org/wiki/Ryom-Verzeichnis
    Scheme {
        label: "RV",
        aliases: &["rv"],
    },
    // Werk ohne Opuszahl, a work its composer published without an opus number, chiefly Beethoven
    //
    // https://en.wikipedia.org/wiki/WoO
    Scheme {
        label: "WoO",
        aliases: &["woo"],
    },
    // Humphrey Searle's catalogue of Liszt
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Franz_Liszt
    Scheme {
        label: "S.",
        aliases: &["s"],
    },
    // Andras Szollosy's catalogue of Bartok
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_B%C3%A9la_Bart%C3%B3k
    Scheme {
        label: "Sz.",
        aliases: &["sz"],
    },
    // Laszlo Somfai's catalogue of Bartok
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_B%C3%A9la_Bart%C3%B3k
    Scheme {
        label: "BB",
        aliases: &["bb"],
    },
    // Jarmil Burghauser's catalogue of Dvorak
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Anton%C3%ADn_Dvo%C5%99%C3%A1k
    Scheme {
        label: "B.",
        aliases: &["b"],
    },
    // Francois Lesure's catalogue of Debussy
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Claude_Debussy
    Scheme {
        label: "L.",
        aliases: &["l"],
    },
    // Franklin Zimmerman's catalogue of Purcell
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Henry_Purcell
    Scheme {
        label: "Z",
        aliases: &["z"],
    },
    // Wydanie Narodowe, Jan Ekier's Polish National Edition of Chopin
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Fr%C3%A9d%C3%A9ric_Chopin
    Scheme {
        label: "WN",
        aliases: &["wn"],
    },
    // Ralph Kirkpatrick's catalogue of Scarlatti's keyboard sonatas, written Kk. to keep it apart
    // from Mozart's K. Chopin's Kobylanska catalogue also uses KK, but numbers such as
    // "KK IVa/16" carry a Roman numeral, so they stay unrecognized rather than parsing as this.
    //
    // https://en.wikipedia.org/wiki/List_of_solo_keyboard_sonatas_by_Domenico_Scarlatti
    Scheme {
        label: "Kk.",
        aliases: &["kk"],
    },
];

/// A catalog number as stored: the text as typed, plus what the parser made of it
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogNumber {
    text: String,
    /// Index into [SCHEMES]; None when unrecognized
    scheme: Option<usize>,
    /// The sort tuple: the parsed numbers, or every run of digits when unrecognized
    numbers: Vec<u32>,
}

impl CatalogNumber {
    /// Recognize a catalog number, keeping an unrecognized one as typed
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        match recognize(text) {
            Some((scheme, numbers)) => Self {
                text: text.to_string(),
                scheme: Some(scheme),
                numbers,
            },
            None => Self {
                text: text.to_string(),
                scheme: None,
                numbers: digit_runs(text),
            },
        }
    }

    /// The number as it was typed
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the parser knows the scheme this number is written in
    pub fn is_recognized(&self) -> bool {
        self.scheme.is_some()
    }

    /// Where this number's scheme sorts; None when unrecognized. Lower is higher priority.
    pub fn scheme_priority(&self) -> Option<usize> {
        self.scheme
    }

    /// Whether two numbers are the same number, however each was spelled
    pub fn matches(&self, other: &Self) -> bool {
        match (self.scheme, other.scheme) {
            (Some(a), Some(b)) => a == b && self.numbers == other.numbers,
            (None, None) => self.text == other.text,
            _ => false,
        }
    }

    /// How well this stored number fits what was typed; None when it should not be suggested
    pub fn similarity(&self, typed: &Self) -> Option<Similarity> {
        if typed.text.is_empty() {
            return None;
        }
        if self.matches(typed) {
            return Some(Similarity::Exact);
        }
        if let (Some(mine), Some(theirs)) = (self.scheme, typed.scheme)
            && mine == theirs
            && self.numbers.starts_with(&typed.numbers)
        {
            return Some(Similarity::Prefix);
        }
        // A half-typed label matches nothing structurally, so the text is the last resort
        self.text
            .to_lowercase()
            .contains(&typed.text.to_lowercase())
            .then_some(Similarity::Text)
    }
}

/// How well a stored number fits what was typed, best first
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Similarity {
    /// The same number, however either was spelled
    Exact,
    /// The same scheme, and every number typed so far agrees
    Prefix,
    /// The typed text appears somewhere in it
    Text,
}

impl Display for CatalogNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Ord for CatalogNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        // None sorts after every Some: unrecognized numbers come last
        let rank = |n: &Self| n.scheme.unwrap_or(usize::MAX);
        rank(self)
            .cmp(&rank(other))
            // An empty tuple would otherwise sort first; a value with no digits belongs last
            .then_with(|| self.numbers.is_empty().cmp(&other.numbers.is_empty()))
            .then_with(|| self.numbers.cmp(&other.numbers))
            .then_with(|| self.text.cmp(&other.text))
    }
}

impl PartialOrd for CatalogNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Label, integer, and optionally a separator and a second integer, with nothing left over
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

/// The leading ASCII letters and what follows them
fn split_letters(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    text.split_at(end)
}

/// A leading integer and what follows it; None when the text does not start with digits
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

    fn parsed(text: &str) -> (Option<usize>, Vec<u32>) {
        let n = CatalogNumber::parse(text);
        (n.scheme, n.numbers.clone())
    }

    #[test]
    fn every_spelling_of_an_opus_number_matches() {
        let canonical = CatalogNumber::parse("Op. 27 No. 2");
        for spelling in [
            "Op. 27, No. 2",
            "Op. 27/2",
            "Op 27 Nr. 2",
            "op.27 no.2",
            "Opus 27 Number 2",
            "Op 27 No 2",
            "  Op. 27 No. 2  ",
        ] {
            let n = CatalogNumber::parse(spelling);
            assert!(n.is_recognized(), "{spelling:?}");
            assert!(n.matches(&canonical), "{spelling:?}");
        }
        assert!(!CatalogNumber::parse("Op. 27").matches(&canonical));
        assert!(!CatalogNumber::parse("Op. 27 No. 3").matches(&canonical));
        assert!(!CatalogNumber::parse("BWV 27").matches(&CatalogNumber::parse("Op. 27")));
        // Text is kept as typed, apart from trimming
        assert_eq!(CatalogNumber::parse("  op.27/2 ").as_str(), "op.27/2");
    }

    #[test]
    fn composer_catalogs_parse_to_their_scheme_and_numbers() {
        let opus = CatalogNumber::parse("Op. 1").scheme_priority();
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
            let (scheme, parsed) = parsed(text);
            assert!(scheme.is_some(), "{text:?} should be recognized");
            assert_ne!(scheme, opus, "{text:?} is not an opus number");
            assert_eq!(parsed, numbers, "{text:?}");
        }
        // Same letters, different catalogs: K. is Koechel, Kk. is Kirkpatrick
        assert!(!CatalogNumber::parse("K. 380").matches(&CatalogNumber::parse("Kk. 380")));
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
            ("", vec![]),
        ] {
            let n = CatalogNumber::parse(text);
            assert!(!n.is_recognized(), "{text:?}");
            assert_eq!(n.scheme_priority(), None, "{text:?}");
            assert_eq!(n.numbers, digits, "{text:?}");
            assert_eq!(n.as_str(), text.trim(), "{text:?}");
        }
        // Unrecognized values match only on identical text
        assert!(CatalogNumber::parse("Hob. XVI:52").matches(&CatalogNumber::parse("Hob. XVI:52")));
        assert!(!CatalogNumber::parse("Hob. XVI:52").matches(&CatalogNumber::parse("Hob XVI:52")));
    }

    #[test]
    fn similarity_tiers() {
        let sim = |stored: &str, typed: &str| {
            CatalogNumber::parse(stored).similarity(&CatalogNumber::parse(typed))
        };
        assert_eq!(sim("Op. 27 No. 2", "op.27/2"), Some(Similarity::Exact));
        assert_eq!(sim("Op. 27 No. 2", "Op. 27"), Some(Similarity::Prefix));
        assert_eq!(
            sim("Op. 27", "Op. 27 No. 2"),
            None,
            "a longer tuple is not a prefix"
        );
        assert_eq!(
            sim("BWV 27", "Op. 27"),
            None,
            "prefixes do not cross schemes"
        );
        assert_eq!(
            sim("Op. 27 No. 2", "op"),
            Some(Similarity::Text),
            "a bare label finds every number of that scheme"
        );
        assert_eq!(sim("Hob. XVI:52", "hob"), Some(Similarity::Text));
        assert_eq!(sim("Hob. XVI:52", "Hob. XVI:52"), Some(Similarity::Exact));
        assert_eq!(sim("Hob. XVI:52", "BWV"), None);
        assert_eq!(
            sim("Op. 27 No. 2", ""),
            None,
            "nothing typed suggests nothing"
        );
        assert!(Similarity::Exact < Similarity::Prefix && Similarity::Prefix < Similarity::Text);
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
        .map(CatalogNumber::parse)
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

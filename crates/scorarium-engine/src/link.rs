use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::entity::EntityKind;
use crate::{Error, Result};

/// A HTTP/S web address
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HttpUrl(Url);

impl HttpUrl {
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl TryFrom<Url> for HttpUrl {
    type Error = Error;

    fn try_from(url: Url) -> Result<HttpUrl> {
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            Ok(HttpUrl(url))
        } else {
            Err(Error::InvalidValue(format!(
                "Failed to parse '{url}' as a web address."
            )))
        }
    }
}

impl FromStr for HttpUrl {
    type Err = Error;

    fn from_str(input: &str) -> Result<HttpUrl> {
        let input = input.trim();
        let url = Url::parse(input).map_err(|_| {
            Error::InvalidValue(format!("Failed to parse '{input}' as a web address."))
        })?;
        HttpUrl::try_from(url)
    }
}

impl fmt::Display for HttpUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkId(pub(crate) i64);

impl fmt::Display for LinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for LinkId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<LinkId, ParseIntError> {
        text.parse().map(LinkId)
    }
}

/// A site whose record URLs are recognized
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkKind {
    OpenLibrary,
    Wikidata,
    MusicBrainz,
    Imslp,
    Viaf,
    Gnd,
    Isni,
    Loc,
    K10plus,
    Dnb,
    Harvard,
    Goodreads,
    LibraryThing,
    InternetArchive,
    Gutenberg,
    Librivox,
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LinkKind::OpenLibrary => "openlibrary",
            LinkKind::Wikidata => "wikidata",
            LinkKind::MusicBrainz => "musicbrainz",
            LinkKind::Imslp => "imslp",
            LinkKind::Viaf => "viaf",
            LinkKind::Gnd => "gnd",
            LinkKind::Isni => "isni",
            LinkKind::Loc => "loc",
            LinkKind::K10plus => "k10plus",
            LinkKind::Dnb => "dnb",
            LinkKind::Harvard => "harvard",
            LinkKind::Goodreads => "goodreads",
            LinkKind::LibraryThing => "librarything",
            LinkKind::InternetArchive => "internetarchive",
            LinkKind::Gutenberg => "gutenberg",
            LinkKind::Librivox => "librivox",
        })
    }
}

impl FromStr for LinkKind {
    type Err = Error;

    fn from_str(input: &str) -> Result<LinkKind> {
        Ok(match input {
            "openlibrary" => LinkKind::OpenLibrary,
            "wikidata" => LinkKind::Wikidata,
            "musicbrainz" => LinkKind::MusicBrainz,
            "imslp" => LinkKind::Imslp,
            "viaf" => LinkKind::Viaf,
            "gnd" => LinkKind::Gnd,
            "isni" => LinkKind::Isni,
            "loc" => LinkKind::Loc,
            "k10plus" => LinkKind::K10plus,
            "dnb" => LinkKind::Dnb,
            "harvard" => LinkKind::Harvard,
            "goodreads" => LinkKind::Goodreads,
            "librarything" => LinkKind::LibraryThing,
            "internetarchive" => LinkKind::InternetArchive,
            "gutenberg" => LinkKind::Gutenberg,
            "librivox" => LinkKind::Librivox,
            other => return Err(eyre::eyre!("unknown link kind {other:?}").into()),
        })
    }
}

/// A record at a known site, with the ID in the site's canonical form
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExternalRecord {
    kind: LinkKind,
    id: String,
}

impl ExternalRecord {
    /// Try to parse the given URL into an external record
    pub fn recognize(entity: EntityKind, url: &HttpUrl) -> Option<ExternalRecord> {
        use EntityKind::*;
        let url = url.as_url();
        let host = url.host_str()?;
        let segments: Vec<&str> = url.path_segments()?.collect();
        let (kind, id) = match host {
            "openlibrary.org" => (LinkKind::OpenLibrary, openlibrary(entity, &segments)?),
            "www.wikidata.org" | "m.wikidata.org" => (LinkKind::Wikidata, wikidata(&segments)?),
            "musicbrainz.org" | "www.musicbrainz.org" | "beta.musicbrainz.org" => {
                (LinkKind::MusicBrainz, musicbrainz(entity, &segments)?)
            }
            "imslp.org" => (LinkKind::Imslp, imslp(entity, url)?),
            "viaf.org" | "www.viaf.org" if entity == Person => (LinkKind::Viaf, viaf(&segments)?),
            "d-nb.info" if entity == Publication => (LinkKind::Dnb, dnb(&segments)?),
            "d-nb.info" | "lobid.org" | "explore.gnd.network" if entity != Publication => {
                (LinkKind::Gnd, gnd("gnd", &segments)?)
            }
            "www.deutsche-digitale-bibliothek.de" if entity != Publication => {
                (LinkKind::Gnd, gnd("entity", &segments)?)
            }
            "isni.org" | "www.isni.org" if entity == Person => (LinkKind::Isni, isni(&segments)?),
            "lccn.loc.gov" | "www.loc.gov" | "id.loc.gov" => {
                (LinkKind::Loc, loc(entity, host, &segments)?)
            }
            "opac.k10plus.de" | "kxp.k10plus.de" if entity == Publication => (
                LinkKind::K10plus,
                ppn(&url.query_pairs().find(|(key, _)| key == "PPN")?.1)?,
            ),
            "uri.gbv.de" if entity == Publication => (LinkKind::K10plus, k10plus_uri(&segments)?),
            "id.lib.harvard.edu" if entity == Publication => {
                (LinkKind::Harvard, harvard(&segments)?)
            }
            "goodreads.com" | "www.goodreads.com" => {
                (LinkKind::Goodreads, goodreads(entity, &segments)?)
            }
            "librarything.com" | "www.librarything.com" => {
                (LinkKind::LibraryThing, librarything(entity, &segments)?)
            }
            "archive.org" if entity == Publication => {
                (LinkKind::InternetArchive, internetarchive(&segments)?)
            }
            "gutenberg.org" | "www.gutenberg.org" => {
                (LinkKind::Gutenberg, gutenberg(entity, &segments)?)
            }
            "librivox.org" if entity == Person => (LinkKind::Librivox, librivox(&segments)?),
            _ => return None,
        };
        Some(ExternalRecord { kind, id })
    }

    pub fn kind(&self) -> LinkKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The canonical URL of this record
    pub fn url(&self, entity: EntityKind) -> Option<HttpUrl> {
        use EntityKind::*;
        let id = &self.id;
        let url = match (self.kind, entity) {
            (LinkKind::OpenLibrary, Publication) => format!("https://openlibrary.org/books/{id}"),
            (LinkKind::OpenLibrary, Work) => format!("https://openlibrary.org/works/{id}"),
            (LinkKind::OpenLibrary, Person) => format!("https://openlibrary.org/authors/{id}"),
            (LinkKind::Wikidata, _) => format!("https://www.wikidata.org/wiki/{id}"),
            (LinkKind::MusicBrainz, Work) => format!("https://musicbrainz.org/work/{id}"),
            (LinkKind::MusicBrainz, Person) => format!("https://musicbrainz.org/artist/{id}"),
            (LinkKind::Imslp, Work | Person) => {
                let title = match entity {
                    Person => format!("Category:{id}"),
                    _ => id.to_string(),
                };
                let mut url = Url::parse("https://imslp.org/wiki").ok()?;
                url.path_segments_mut().ok()?.push(&title);
                return HttpUrl::try_from(url).ok();
            }
            (LinkKind::Viaf, Person) => format!("https://viaf.org/viaf/{id}"),
            (LinkKind::Gnd, Work | Person) => format!("https://d-nb.info/gnd/{id}"),
            (LinkKind::Isni, Person) => format!("https://isni.org/isni/{id}"),
            (LinkKind::Loc, Publication) => format!("https://lccn.loc.gov/{id}"),
            (LinkKind::Loc, Person) => format!("https://id.loc.gov/authorities/names/{id}"),
            (LinkKind::K10plus, Publication) => {
                format!("https://opac.k10plus.de/DB=2.299/PPNSET?PPN={id}")
            }
            (LinkKind::Dnb, Publication) => format!("https://d-nb.info/{id}"),
            (LinkKind::Harvard, Publication) => {
                format!("https://id.lib.harvard.edu/alma/{id}/catalog")
            }
            (LinkKind::Goodreads, Publication) => {
                format!("https://www.goodreads.com/book/show/{id}")
            }
            (LinkKind::Goodreads, Work) => format!("https://www.goodreads.com/work/editions/{id}"),
            (LinkKind::Goodreads, Person) => format!("https://www.goodreads.com/author/show/{id}"),
            (LinkKind::LibraryThing, Work) => format!("https://www.librarything.com/work/{id}"),
            (LinkKind::LibraryThing, Person) => {
                format!("https://www.librarything.com/author/{id}")
            }
            (LinkKind::InternetArchive, Publication) => {
                format!("https://archive.org/details/{id}")
            }
            (LinkKind::Gutenberg, Work) => format!("https://www.gutenberg.org/ebooks/{id}"),
            (LinkKind::Gutenberg, Person) => {
                format!("https://www.gutenberg.org/ebooks/author/{id}")
            }
            (LinkKind::Librivox, Person) => format!("https://librivox.org/author/{id}"),
            _ => return None,
        };
        Url::parse(&url).ok()?.try_into().ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: LinkId,
    pub url: HttpUrl,
    pub record: Option<ExternalRecord>,
}

/// https://www.wikidata.org/wiki/Property:P648
fn openlibrary(entity: EntityKind, segments: &[&str]) -> Option<String> {
    let (prefix, letter) = match entity {
        EntityKind::Publication => ("books", 'M'),
        EntityKind::Work => ("works", 'W'),
        EntityKind::Person => ("authors", 'A'),
    };
    let [first, id, ..] = segments else {
        return None;
    };
    if *first != prefix {
        return None;
    }
    let id = id.strip_suffix(".json").unwrap_or(id);
    let digits = id.strip_prefix("OL")?.strip_suffix(letter)?;
    number(digits).then(|| id.to_string())
}

/// Parse a Wikidata URL into its ID
fn wikidata(segments: &[&str]) -> Option<String> {
    let id = match segments {
        ["wiki", id] | ["entity", id] | ["wiki", "Special:EntityPage", id] => *id,
        ["wiki", "Special:EntityData", id] => id.split_once('.').map_or(*id, |(id, _)| id),
        _ => return None,
    };
    number(id.strip_prefix('Q')?).then(|| id.to_string())
}

/// https://www.wikidata.org/wiki/Property:P434 and https://www.wikidata.org/wiki/Property:P435
fn musicbrainz(entity: EntityKind, segments: &[&str]) -> Option<String> {
    let record = match entity {
        EntityKind::Work => "work",
        EntityKind::Person => "artist",
        EntityKind::Publication => return None,
    };
    let [first, uuid, ..] = segments else {
        return None;
    };
    if *first != record {
        return None;
    }
    let uuid = uuid.to_ascii_lowercase();
    let groups: Vec<&str> = uuid.split('-').collect();
    let well_formed = groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()));
    well_formed.then_some(uuid)
}

/// https://www.wikidata.org/wiki/Property:P839
fn imslp(entity: EntityKind, url: &Url) -> Option<String> {
    let title = match url.path() {
        "/index.php" => url
            .query_pairs()
            .find(|(key, _)| key == "title")?
            .1
            .into_owned(),
        path => percent_decode_str(path.strip_prefix("/wiki/")?)
            .decode_utf8()
            .ok()?
            .into_owned(),
    };
    let title = title.replace(' ', "_");
    let (namespace, title) = match title.split_once(':') {
        Some(("Category", title)) => (EntityKind::Person, title),
        Some(_) => return None,
        None => (EntityKind::Work, title.as_str()),
    };
    (namespace == entity && !title.is_empty()).then(|| title.to_string())
}

/// https://www.wikidata.org/wiki/Property:P214
fn viaf(segments: &[&str]) -> Option<String> {
    let id = match segments {
        ["viaf", id] | ["viaf", id, ""] => id,
        [language, "viaf", id] | [language, "viaf", id, ""] if language.len() == 2 => id,
        _ => return None,
    };
    number(id).then(|| id.to_string())
}

/// https://www.wikidata.org/wiki/Property:P227
fn gnd(prefix: &str, segments: &[&str]) -> Option<String> {
    let id = match segments {
        [first, id] | [first, id, "about", ..] if *first == prefix => *id,
        _ => return None,
    };
    let (digits, check) = id.split_at_checked(id.len().checked_sub(1)?)?;
    let digits = digits.strip_suffix('-').unwrap_or(digits);
    let well_formed = number(digits) && (check == "X" || all_digits(check));
    well_formed.then(|| id.to_string())
}

/// https://www.wikidata.org/wiki/Property:P213
fn isni(segments: &[&str]) -> Option<String> {
    let id = match segments {
        ["isni", id] | ["isni", id, "about", ..] => id.replace("%20", ""),
        _ => return None,
    };
    let (digits, check) = id.split_at_checked(15)?;
    let well_formed =
        all_digits(digits) && (check == "X" || (check.len() == 1 && all_digits(check)));
    well_formed.then_some(id)
}

/// https://www.wikidata.org/wiki/Property:P244 and https://www.wikidata.org/wiki/Property:P1144
fn loc(entity: EntityKind, host: &str, segments: &[&str]) -> Option<String> {
    let lccn = match (entity, host, segments) {
        (EntityKind::Publication, "lccn.loc.gov", [lccn] | [lccn, ""]) => *lccn,
        (EntityKind::Publication, "www.loc.gov", ["item", lccn] | ["item", lccn, ""]) => *lccn,
        (EntityKind::Person, "id.loc.gov", ["authorities", "names", lccn]) => {
            lccn.strip_suffix(".html").unwrap_or(lccn)
        }
        _ => return None,
    };
    normalize_lccn(lccn)
}

/// https://www.loc.gov/marc/lccn-namespace.html
fn normalize_lccn(lccn: &str) -> Option<String> {
    let lccn = lccn.replace("%20", "");
    let lccn = match lccn.split_once('-') {
        Some((prefix, serial)) if serial.len() <= 6 && all_digits(serial) => {
            format!("{prefix}{serial:0>6}")
        }
        Some(_) => return None,
        None => lccn.to_string(),
    };
    let digits = lccn.trim_start_matches(|c: char| c.is_ascii_lowercase());
    let well_formed =
        lccn.len() - digits.len() <= 3 && matches!(digits.len(), 8 | 10) && all_digits(digits);
    well_formed.then_some(lccn)
}

fn k10plus_uri(segments: &[&str]) -> Option<String> {
    let ["document", document] = segments else {
        return None;
    };
    ppn(document.strip_prefix("opac-de-627:ppn:")?)
}

/// https://www.wikidata.org/wiki/Property:P6721
fn ppn(ppn: &str) -> Option<String> {
    let ppn = ppn.to_ascii_uppercase();
    let (digits, check) = ppn.split_at_checked(ppn.len().checked_sub(1)?)?;
    let well_formed = (7..=9).contains(&digits.len())
        && all_digits(digits)
        && (check == "X" || all_digits(check));
    well_formed.then_some(ppn)
}

/// https://www.wikidata.org/wiki/Property:P1292
fn dnb(segments: &[&str]) -> Option<String> {
    let id = match segments {
        [id] | [id, "about", ..] => *id,
        _ => return None,
    };
    let digits = id.strip_suffix('X').unwrap_or(id);
    (all_digits(digits) && digits.len() >= 8 && (8..=10).contains(&id.len()))
        .then(|| id.to_string())
}

fn harvard(segments: &[&str]) -> Option<String> {
    let ["alma", mmsid, "catalog"] = segments else {
        return None;
    };
    all_digits(mmsid).then(|| mmsid.to_string())
}

/// https://www.wikidata.org/wiki/Property:P2969, https://www.wikidata.org/wiki/Property:P8383,
/// and https://www.wikidata.org/wiki/Property:P2963
fn goodreads(entity: EntityKind, segments: &[&str]) -> Option<String> {
    let segments = match segments {
        [language, rest @ ..] if language.len() == 2 => rest,
        _ => segments,
    };
    let id = match (entity, segments) {
        (EntityKind::Publication, ["book", "show", id, ..]) => id,
        (EntityKind::Work, ["work", "editions", id, ..]) => id,
        (EntityKind::Person, ["author", "show" | "list", id, ..]) => id,
        _ => return None,
    };
    let id = id.split(['.', '-']).next()?;
    number(id).then(|| id.to_string())
}

/// https://www.wikidata.org/wiki/Property:P1085 and https://www.wikidata.org/wiki/Property:P7400
fn librarything(entity: EntityKind, segments: &[&str]) -> Option<String> {
    match (entity, segments) {
        (EntityKind::Work, ["work", id, ..]) => all_digits(id).then(|| id.to_string()),
        (EntityKind::Person, ["author", slug, ..]) => {
            let well_formed = !slug.is_empty()
                && slug
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            well_formed.then(|| slug.to_string())
        }
        _ => None,
    }
}

/// https://www.wikidata.org/wiki/Property:P724
fn internetarchive(segments: &[&str]) -> Option<String> {
    let ["details", id, ..] = segments else {
        return None;
    };
    let mut bytes = id.bytes();
    let well_formed = bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'@')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    well_formed.then(|| id.to_string())
}

/// https://www.wikidata.org/wiki/Property:P2034 and https://www.wikidata.org/wiki/Property:P1938
fn gutenberg(entity: EntityKind, segments: &[&str]) -> Option<String> {
    let id = match (entity, segments) {
        (EntityKind::Person, ["ebooks", "author", id, ..]) => id,
        (EntityKind::Work, ["ebooks", id, ..]) => id,
        _ => return None,
    };
    number(id).then(|| id.to_string())
}

/// https://www.wikidata.org/wiki/Property:P1899
fn librivox(segments: &[&str]) -> Option<String> {
    let ["author", id, ..] = segments else {
        return None;
    };
    all_digits(id).then(|| id.to_string())
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn number(text: &str) -> bool {
    all_digits(text) && !text.starts_with('0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url_accepts_only_web_addresses() {
        for (input, canonical) in [
            ("  https://IMSLP.org  ", "https://imslp.org/"),
            ("https://imslp.org:443/", "https://imslp.org/"),
            ("HTTPS://Example.com", "https://example.com/"),
        ] {
            assert_eq!(input.parse::<HttpUrl>().unwrap().to_string(), canonical);
        }
        for input in [
            "imslp.org",
            "javascript:alert(1)",
            "file://server/share/score.pdf",
            "http://",
            "",
        ] {
            assert!(
                matches!(input.parse::<HttpUrl>(), Err(Error::InvalidValue(_))),
                "{input:?}"
            );
        }
    }

    #[test]
    fn urls_resolve_to_records() {
        use EntityKind::*;
        let openlibrary_id = |id: &'static str| Some((LinkKind::OpenLibrary, id));
        let wikidata_id = |id: &'static str| Some((LinkKind::Wikidata, id));
        let musicbrainz_id = |id: &'static str| Some((LinkKind::MusicBrainz, id));
        let imslp_id = |id: &'static str| Some((LinkKind::Imslp, id));
        let viaf_id = |id: &'static str| Some((LinkKind::Viaf, id));
        let gnd_id = |id: &'static str| Some((LinkKind::Gnd, id));
        let isni_id = |id: &'static str| Some((LinkKind::Isni, id));
        let loc_id = |id: &'static str| Some((LinkKind::Loc, id));
        let k10plus_id = |id: &'static str| Some((LinkKind::K10plus, id));
        let dnb_id = |id: &'static str| Some((LinkKind::Dnb, id));
        let harvard_id = |id: &'static str| Some((LinkKind::Harvard, id));
        let goodreads_id = |id: &'static str| Some((LinkKind::Goodreads, id));
        let librarything_id = |id: &'static str| Some((LinkKind::LibraryThing, id));
        let internetarchive_id = |id: &'static str| Some((LinkKind::InternetArchive, id));
        let gutenberg_id = |id: &'static str| Some((LinkKind::Gutenberg, id));
        let librivox_id = |id: &'static str| Some((LinkKind::Librivox, id));
        let cases = &[
            (
                Publication,
                "https://openlibrary.org/books/OL7353617M",
                openlibrary_id("OL7353617M"),
            ),
            (
                Publication,
                "https://openlibrary.org/books/OL7353617M/Fantastic_Mr._Fox",
                openlibrary_id("OL7353617M"),
            ),
            (
                Work,
                "https://openlibrary.org/works/OL45804W.json",
                openlibrary_id("OL45804W"),
            ),
            (
                Work,
                "http://openlibrary.org/works/OL45804W/editions?limit=10",
                openlibrary_id("OL45804W"),
            ),
            (
                Person,
                "https://openlibrary.org/authors/OL34184A/Roald_Dahl",
                openlibrary_id("OL34184A"),
            ),
            (
                Publication,
                "https://openlibrary.org/authors/OL34184A",
                None,
            ),
            (Work, "https://openlibrary.org/works/OL45804M", None),
            (Work, "https://openlibrary.org/works/OLW", None),
            (Work, "https://www.openlibrary.org/works/OL45804W", None),
            (
                Person,
                "https://www.wikidata.org/wiki/Q42",
                wikidata_id("Q42"),
            ),
            (
                Work,
                "https://m.wikidata.org/wiki/Q255#sitelinks-wikipedia",
                wikidata_id("Q255"),
            ),
            (
                Publication,
                "http://www.wikidata.org/entity/Q255",
                wikidata_id("Q255"),
            ),
            (
                Person,
                "https://www.wikidata.org/wiki/Special:EntityPage/Q42",
                wikidata_id("Q42"),
            ),
            (
                Person,
                "https://www.wikidata.org/wiki/Special:EntityData/Q42.json",
                wikidata_id("Q42"),
            ),
            (Person, "https://www.wikidata.org/wiki/q42", None),
            (Person, "https://www.wikidata.org/wiki/Q42/", None),
            (Person, "https://www.wikidata.org/wiki/Property:P31", None),
            (Person, "https://wikidata.org/wiki/Q42", None),
            (Person, "https://en.wikipedia.org/wiki/Douglas_Adams", None),
            (
                Person,
                "https://musicbrainz.org/artist/1f9df192-a621-4f98-8bb4-38c0e9b3f9d4",
                musicbrainz_id("1f9df192-a621-4f98-8bb4-38c0e9b3f9d4"),
            ),
            (
                Person,
                "https://beta.musicbrainz.org/artist/1F9DF192-A621-4F98-8BB4-38C0E9B3F9D4/releases",
                musicbrainz_id("1f9df192-a621-4f98-8bb4-38c0e9b3f9d4"),
            ),
            (
                Work,
                "https://www.musicbrainz.org/work/4b3e1a4f-1b9e-3d3a-a0a5-6f1f8f2d2d30",
                musicbrainz_id("4b3e1a4f-1b9e-3d3a-a0a5-6f1f8f2d2d30"),
            ),
            (
                Publication,
                "https://musicbrainz.org/release/4b3e1a4f-1b9e-3d3a-a0a5-6f1f8f2d2d30",
                None,
            ),
            (
                Work,
                "https://musicbrainz.org/artist/1f9df192-a621-4f98-8bb4-38c0e9b3f9d4",
                None,
            ),
            (Person, "https://musicbrainz.org/artist/1f9df192", None),
            (
                Person,
                "https://imslp.org/wiki/Category:Bach,_Johann_Sebastian",
                imslp_id("Bach,_Johann_Sebastian"),
            ),
            (
                Person,
                "https://imslp.org/index.php?title=Category:Bach,+Johann+Sebastian&oldid=1",
                imslp_id("Bach,_Johann_Sebastian"),
            ),
            (
                Work,
                "https://imslp.org/wiki/Die_Walk%C3%BCre,_WWV_86B_(Wagner,_Richard)#Sheet_Music",
                imslp_id("Die_Walküre,_WWV_86B_(Wagner,_Richard)"),
            ),
            (
                Work,
                "https://imslp.org/wiki/3_Gymnop%C3%A9dies_(Satie,_Erik)",
                imslp_id("3_Gymnopédies_(Satie,_Erik)"),
            ),
            (
                Work,
                "https://imslp.org/wiki/Foo%2FBar_%3F%23",
                imslp_id("Foo/Bar_?#"),
            ),
            (
                Work,
                "https://imslp.org/wiki/Category:Bach,_Johann_Sebastian",
                None,
            ),
            (
                Person,
                "https://imslp.org/wiki/Bach,_Johann_Sebastian",
                None,
            ),
            (
                Work,
                "https://imslp.org/wiki/File:PMLP01234-score.pdf",
                None,
            ),
            (Work, "https://imslp.org/wiki/", None),
            (
                Work,
                "https://www.imslp.org/wiki/3_Gymnop%C3%A9dies_(Satie,_Erik)",
                None,
            ),
            (
                Person,
                "https://viaf.org/viaf/24590349",
                viaf_id("24590349"),
            ),
            (
                Person,
                "http://www.viaf.org/viaf/24590349/",
                viaf_id("24590349"),
            ),
            (
                Person,
                "https://viaf.org/en/viaf/24590349",
                viaf_id("24590349"),
            ),
            (
                Person,
                "https://viaf.org/viaf/sourceID/LC%7Cn78890351",
                None,
            ),
            (Person, "https://viaf.org/processed/LC%7Cn78890351", None),
            (Work, "https://viaf.org/viaf/24590349", None),
            (
                Person,
                "https://d-nb.info/gnd/118505955",
                gnd_id("118505955"),
            ),
            (
                Person,
                "https://d-nb.info/gnd/118505955/about/html",
                gnd_id("118505955"),
            ),
            (Work, "https://lobid.org/gnd/300008937", gnd_id("300008937")),
            (
                Work,
                "https://explore.gnd.network/gnd/4005728-8",
                gnd_id("4005728-8"),
            ),
            (
                Person,
                "https://www.deutsche-digitale-bibliothek.de/entity/118505955",
                gnd_id("118505955"),
            ),
            (Person, "https://d-nb.info/gnd/1-X", gnd_id("1-X")),
            (Person, "https://d-nb.info/gnd/0123", None),
            (Publication, "https://d-nb.info/gnd/118505955", None),
            (Publication, "https://lobid.org/gnd/118505955", None),
            (
                Person,
                "https://isni.org/isni/0000000121974701",
                isni_id("0000000121974701"),
            ),
            (
                Person,
                "https://www.isni.org/isni/0000%200001%202197%204701/about",
                isni_id("0000000121974701"),
            ),
            (
                Person,
                "https://isni.org/isni/000000012197470X",
                isni_id("000000012197470X"),
            ),
            (Person, "https://isni.org/isni/00000001219747", None),
            (Work, "https://isni.org/isni/0000000121974701", None),
            (
                Publication,
                "https://lccn.loc.gov/2001000002",
                loc_id("2001000002"),
            ),
            (Publication, "https://lccn.loc.gov/85-2", loc_id("85000002")),
            (
                Publication,
                "https://www.loc.gov/item/n78-890351/",
                loc_id("n78890351"),
            ),
            (
                Person,
                "https://id.loc.gov/authorities/names/n78890351.html",
                loc_id("n78890351"),
            ),
            (
                Person,
                "http://id.loc.gov/authorities/names/n78890351",
                loc_id("n78890351"),
            ),
            (
                Publication,
                "https://id.loc.gov/authorities/names/n78890351",
                None,
            ),
            (Person, "https://lccn.loc.gov/n78890351", None),
            (
                Person,
                "https://id.loc.gov/authorities/subjects/sh85026371",
                None,
            ),
            (Publication, "https://lccn.loc.gov/n78-8903512", None),
            (
                Publication,
                "https://opac.k10plus.de/DB=2.299/PPNSET?PPN=1663713685",
                k10plus_id("1663713685"),
            ),
            (
                Publication,
                "https://kxp.k10plus.de/DB=2.1/PPNSET?PPN=16637136x&HILN=1",
                k10plus_id("16637136X"),
            ),
            (
                Publication,
                "http://uri.gbv.de/document/opac-de-627:ppn:1663713685",
                k10plus_id("1663713685"),
            ),
            (
                Publication,
                "https://opac.k10plus.de/DB=2.299/PPNSET?PPN=12",
                None,
            ),
            (
                Work,
                "https://opac.k10plus.de/DB=2.299/PPNSET?PPN=1663713685",
                None,
            ),
            (
                Publication,
                "https://d-nb.info/1234567890",
                dnb_id("1234567890"),
            ),
            (
                Publication,
                "https://d-nb.info/12345678X/about",
                dnb_id("12345678X"),
            ),
            (Publication, "https://d-nb.info/1234567", None),
            (Work, "https://d-nb.info/1234567890", None),
            (
                Publication,
                "https://id.lib.harvard.edu/alma/990000000000203941/catalog",
                harvard_id("990000000000203941"),
            ),
            (
                Publication,
                "https://id.lib.harvard.edu/alma/990000000000203941",
                None,
            ),
            (
                Work,
                "https://id.lib.harvard.edu/alma/990000000000203941/catalog",
                None,
            ),
            (
                Publication,
                "https://www.goodreads.com/book/show/6327.Fantastic_Mr_Fox",
                goodreads_id("6327"),
            ),
            (
                Publication,
                "https://goodreads.com/de/book/show/6327-fantastic-mr-fox",
                goodreads_id("6327"),
            ),
            (
                Work,
                "https://www.goodreads.com/work/editions/1067394",
                goodreads_id("1067394"),
            ),
            (
                Person,
                "https://www.goodreads.com/author/show/4273.Roald_Dahl",
                goodreads_id("4273"),
            ),
            (
                Person,
                "https://www.goodreads.com/author/list/4273",
                goodreads_id("4273"),
            ),
            (
                Publication,
                "https://www.goodreads.com/author/show/4273",
                None,
            ),
            (Publication, "https://www.goodreads.com/book/show/0", None),
            (
                Work,
                "https://www.librarything.com/work/2127",
                librarything_id("2127"),
            ),
            (
                Person,
                "https://librarything.com/author/dickenscharles-1",
                librarything_id("dickenscharles-1"),
            ),
            (Person, "https://www.librarything.com/author/Dickens", None),
            (
                Person,
                "https://www.librarything.nl/author/dickenscharles-1",
                None,
            ),
            (Publication, "https://www.librarything.com/work/2127", None),
            (
                Publication,
                "https://archive.org/details/ghostseer01schiuoft",
                internetarchive_id("ghostseer01schiuoft"),
            ),
            (
                Publication,
                "https://archive.org/details/ghostseer01schiuoft/page/n5/mode/2up",
                internetarchive_id("ghostseer01schiuoft"),
            ),
            (
                Publication,
                "https://archive.org/download/ghostseer01schiuoft",
                None,
            ),
            (
                Work,
                "https://archive.org/details/ghostseer01schiuoft",
                None,
            ),
            (
                Work,
                "https://www.gutenberg.org/ebooks/84",
                gutenberg_id("84"),
            ),
            (
                Person,
                "https://gutenberg.org/ebooks/author/61",
                gutenberg_id("61"),
            ),
            (Work, "https://www.gutenberg.org/ebooks/author/61", None),
            (Person, "https://www.gutenberg.org/ebooks/84", None),
            (Person, "https://librivox.org/author/96", librivox_id("96")),
            (Work, "https://librivox.org/author/96", None),
            (Person, "https://www.librivox.org/author/96", None),
        ];
        for (entity, url, expected) in cases {
            let url: HttpUrl = url.parse().unwrap();
            let expected = expected.map(|(kind, id)| ExternalRecord {
                kind,
                id: id.to_string(),
            });
            assert_eq!(ExternalRecord::recognize(*entity, &url), expected, "{url}");
            if let Some(expected) = expected {
                let built = expected.url(*entity).unwrap();
                assert_eq!(
                    ExternalRecord::recognize(*entity, &built),
                    Some(expected),
                    "{built}"
                );
            }
        }
    }
}

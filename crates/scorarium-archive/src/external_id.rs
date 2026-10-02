use url::Url;

/// The entity kind a link belongs to
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Publication,
    Work,
    Person,
}

/// A site whose record URLs are recognized
#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
pub enum Kind {
    /// A link that does not name a known external record or site
    Generic,
    OpenLibrary,
    Wikidata,
}

/// A record from a known external site, with the ID in the site's canonical form
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalId {
    pub kind: Kind,
    pub id: String,
}

/// Which record at a known site this URL points to, if any
pub fn recognize(entity: EntityKind, url: &Url) -> Option<ExternalId> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.collect();
    let (kind, id) = match url.host_str()? {
        "openlibrary.org" => (Kind::OpenLibrary, openlibrary(entity, &segments)?),
        "www.wikidata.org" | "m.wikidata.org" => (Kind::Wikidata, wikidata(&segments)?),
        _ => return None,
    };
    Some(ExternalId { kind, id })
}

/// Construct a canonical URL for an external ID
pub fn build(entity: EntityKind, kind: Kind, id: &str) -> Option<Url> {
    let url = match (kind, entity) {
        (Kind::OpenLibrary, EntityKind::Publication) => {
            format!("https://openlibrary.org/books/{id}")
        }
        (Kind::OpenLibrary, EntityKind::Work) => format!("https://openlibrary.org/works/{id}"),
        (Kind::OpenLibrary, EntityKind::Person) => format!("https://openlibrary.org/authors/{id}"),
        (Kind::Wikidata, _) => format!("https://www.wikidata.org/wiki/{id}"),
        (Kind::Generic, _) => return None,
    };
    Url::parse(&url).ok()
}

/// Parse an Open Library URL into its ID
///
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
    fn urls_resolve_to_records() {
        use EntityKind::*;
        let openlibrary_id = |id: &'static str| Some((Kind::OpenLibrary, id));
        let wikidata_id = |id: &'static str| Some((Kind::Wikidata, id));
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
        ];
        for (entity, url, expected) in cases {
            let url = Url::parse(url).unwrap();
            let expected = expected.map(|(kind, id)| ExternalId {
                kind,
                id: id.to_string(),
            });
            assert_eq!(recognize(*entity, &url), expected, "{url}");
            if let Some(expected) = expected {
                let built = build(*entity, expected.kind, &expected.id).unwrap();
                assert_eq!(recognize(*entity, &built), Some(expected), "{built}");
            }
        }
    }
}

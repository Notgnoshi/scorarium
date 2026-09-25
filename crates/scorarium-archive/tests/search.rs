use scorarium_archive::{
    Archive, ContributorInput, Entity, HoldingKind, HoldingRawInput, PersonRef,
    PublicationRawInput, SearchHit, WorkRawInput,
};

fn holding(kind: HoldingKind, location: &str) -> HoldingRawInput {
    HoldingRawInput {
        id: None,
        kind,
        location: location.into(),
    }
}

/// Each hit as (library, kind, title or name), which is what the results page shows
fn shown(hits: &[SearchHit]) -> Vec<(&str, &str, &str)> {
    hits.iter()
        .map(|hit| {
            let (kind, label) = match &hit.entity {
                Entity::Publication(p) => ("publication", p.title.as_str()),
                Entity::Work(w) => ("work", w.title.as_str()),
                Entity::Person(p) => ("person", p.name.as_str()),
            };
            (hit.library_name.as_str(), kind, label)
        })
        .collect()
}

#[tokio::test]
async fn search_sees_what_the_viewer_sees() {
    let archive = Archive::in_memory().await.unwrap();
    let music = archive.create_library("Sheet music", false).await.unwrap();
    let books = archive.create_library("Books", true).await.unwrap();
    let nocturnes = PublicationRawInput {
        title: "Nocturnes".into(),
        holdings: vec![holding(HoldingKind::Physical, "Piano bench")],
        contents: vec![WorkRawInput {
            title: "Nocturne".into(),
            contributors: vec![ContributorInput {
                name: "Frederic Chopin".into(),
                role: "composer".into(),
                person: PersonRef::New,
            }],
            catalog_numbers: vec!["Op. 9 No. 2".into()],
            ..WorkRawInput::default()
        }],
        ..PublicationRawInput::default()
    };
    music
        .create_publication(&nocturnes.parse().unwrap())
        .await
        .unwrap();
    let hymnal = PublicationRawInput {
        title: "The Hymnal".into(),
        note: "Carols for the season".into(),
        tags: "christmas".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        ..PublicationRawInput::default()
    };
    books
        .create_publication(&hymnal.parse().unwrap())
        .await
        .unwrap();

    // A private library answers nobody but a logged-in viewer
    assert_eq!(shown(&archive.search("hymnal", true).await.unwrap()), []);
    assert_eq!(
        shown(&archive.search("hymnal", false).await.unwrap()),
        [("Books", "publication", "The Hymnal")]
    );

    // A note and a tag are not searched, however visible they are
    assert_eq!(shown(&archive.search("carols", false).await.unwrap()), []);
    assert_eq!(
        shown(&archive.search("christmas", false).await.unwrap()),
        []
    );

    // A catalog number finds its work, spaced out or run together
    for typed in ["Op. 9 No. 2", "op9no2", "op9"] {
        assert_eq!(
            shown(&archive.search(typed, false).await.unwrap()),
            [("Sheet music", "work", "Nocturne")],
            "{typed:?}"
        );
    }

    // A composer is both the work crediting them and a person of their own
    assert_eq!(
        shown(&archive.search("chopin", false).await.unwrap()),
        [
            ("Sheet music", "person", "Frederic Chopin"),
            ("Sheet music", "work", "Nocturne")
        ]
    );
    // The typeahead reads titles alone, so a composer's name finds nothing there
    assert_eq!(
        shown(&archive.suggest_titles("chopin", false).await.unwrap()),
        []
    );
    assert_eq!(
        shown(&archive.suggest_titles("nocturne", false).await.unwrap()),
        // The work's title is the typed word, so it outranks the publication's near miss
        [
            ("Sheet music", "work", "Nocturne"),
            ("Sheet music", "publication", "Nocturnes")
        ]
    );
}

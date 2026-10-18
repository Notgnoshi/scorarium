use scorarium_archive::{Archive, CatalogNumber, Entity, HoldingKind, SearchHit, Tag};

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
    let mut edit = archive.edit().await.unwrap();
    let music = edit.create_library("Sheet music", false).await.unwrap();
    let mut library = edit.library(music).await.unwrap();
    let chopin = library
        .create_person("Frederic Chopin", "Chopin, Frederic")
        .await
        .unwrap();
    let nocturne = library.create_work("Nocturne").await.unwrap();
    let mut work = library.work(nocturne).await.unwrap();
    work.add_contributor(chopin, "composer").await.unwrap();
    work.add_catalog_number(&CatalogNumber::parse("Op. 9 No. 2"))
        .await
        .unwrap();
    let (nocturnes, _) = library
        .create_publication("Nocturnes", HoldingKind::Physical, Some("Piano bench"))
        .await
        .unwrap();
    library
        .publication(nocturnes)
        .await
        .unwrap()
        .add_work(nocturne)
        .await
        .unwrap();

    let books = edit.create_library("Books", true).await.unwrap();
    let mut library = edit.library(books).await.unwrap();
    let (hymnal, _) = library
        .create_publication("The Hymnal", HoldingKind::Physical, None)
        .await
        .unwrap();
    let mut hymnal = library.publication(hymnal).await.unwrap();
    hymnal
        .set_note(Some("Carols for the season"))
        .await
        .unwrap();
    hymnal
        .add_tag(&"christmas".parse::<Tag>().unwrap())
        .await
        .unwrap();
    edit.commit().await.unwrap();

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

    // A composer is both a person of their own and the work they are a contributor to
    assert_eq!(
        shown(&archive.search("chopin", false).await.unwrap()),
        [
            ("Sheet music", "person", "Frederic Chopin"),
            ("Sheet music", "work", "Nocturne")
        ]
    );
    assert_eq!(
        shown(&archive.search("nocturne", false).await.unwrap()),
        // The work's title is the typed word, so it outranks the publication's near miss
        [
            ("Sheet music", "work", "Nocturne"),
            ("Sheet music", "publication", "Nocturnes")
        ]
    );
}

use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, IdentifierRawInput,
    PublicationRawInput, WorkRawInput, identifier,
};

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
    }
}

fn holding(kind: HoldingKind, location: &str) -> HoldingRawInput {
    HoldingRawInput {
        id: None,
        kind,
        location: location.into(),
    }
}

/// A publication carrying one of everything, for the tests that read it back.
fn gymnopedies() -> PublicationRawInput {
    PublicationRawInput {
        title: "Three gymnopedies".into(),
        publisher: "Dover".into(),
        year: "1987".into(),
        holdings: vec![
            holding(HoldingKind::Physical, "Piano bench"),
            holding(HoldingKind::Digital, "gymnopedies.pdf"),
        ],
        identifiers: vec![
            IdentifierRawInput {
                kind: "isbn".into(),
                value: "0-486-23134-8".into(),
            },
            IdentifierRawInput {
                kind: "plate_number".into(),
                value: "ut 50061".into(),
            },
        ],
        contributors: vec![
            contributor("Erik Satie", "composer"),
            contributor("Bob", "arranger"),
        ],
        contents: vec![WorkRawInput {
            title: "Gymnopedie No. 1".into(),
            key: "D major".into(),
            contributors: vec![
                contributor("Erik Satie", "composer"),
                contributor("Sue", "editor"),
            ],
            ..WorkRawInput::default()
        }],
    }
}

#[tokio::test]
async fn a_created_publication_reads_back_whole() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music").await.unwrap();

    let created = library
        .create_publication(&gymnopedies().parse().unwrap())
        .await
        .unwrap();

    assert_eq!(created.title, "Three gymnopedies");
    assert_eq!(created.publisher.as_deref(), Some("Dover"));
    assert_eq!(created.year, Some(1987));
    // What was typed as an ISBN-10 comes back as the hyphenated ISBN-13 it normalizes to
    let identifiers: Vec<(identifier::Kind, &str)> = created
        .identifiers
        .iter()
        .map(|identifier| (identifier.kind, identifier.value.as_str()))
        .collect();
    assert_eq!(
        identifiers,
        [
            (identifier::Kind::Isbn, "978-0-486-23134-1"),
            (identifier::Kind::PlateNumber, "UT 50061"),
        ]
    );
    let contributors: Vec<(&str, &str)> = created
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        contributors,
        [("Erik Satie", "composer"), ("Bob", "arranger")]
    );
    let holdings: Vec<(HoldingKind, Option<&str>)> = created
        .holdings
        .iter()
        .map(|holding| (holding.kind, holding.location.as_deref()))
        .collect();
    assert_eq!(
        holdings,
        [
            (HoldingKind::Physical, Some("Piano bench")),
            (HoldingKind::Digital, Some("gymnopedies.pdf")),
        ]
    );

    // Both lookups see what create returned
    let found = library.publication(created.id).await.unwrap().unwrap();
    assert_eq!(found.title, created.title);
    assert_eq!(found.identifiers, created.identifiers);
    assert_eq!(found.contributors, created.contributors);
    assert_eq!(found.holdings, created.holdings);
    let listed = library.publications().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    // A publication belongs to its own library alone
    let books = archive.create_library("Books").await.unwrap();
    assert!(books.publication(created.id).await.unwrap().is_none());
    assert!(books.publications().await.unwrap().is_empty());
}

#[tokio::test]
async fn works_come_back_whole_and_scoped_to_their_library() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music").await.unwrap();
    let publication = library
        .create_publication(&gymnopedies().parse().unwrap())
        .await
        .unwrap();

    let contents = publication.works().await.unwrap();

    assert_eq!(contents.len(), 1);
    let gymnopedie = &contents[0];
    assert_eq!(gymnopedie.title, "Gymnopedie No. 1");
    assert_eq!(gymnopedie.key.as_deref(), Some("D major"));
    assert_eq!(gymnopedie.time_signature, None);
    let credits: Vec<(&str, &str)> = gymnopedie
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(credits, [("Erik Satie", "composer"), ("Sue", "editor")]);

    // The work is reachable on its own, and only through the library holding it
    let found = library.work(gymnopedie.id).await.unwrap().unwrap();
    assert_eq!(found.title, gymnopedie.title);
    let books = archive.create_library("Books").await.unwrap();
    assert!(books.work(gymnopedie.id).await.unwrap().is_none());

    let containing = gymnopedie.publications().await.unwrap();
    assert_eq!(containing.len(), 1);
    assert_eq!(containing[0].id, publication.id);
}

/// The edit pages open on what is stored, so a stored publication must describe itself in exactly
/// the raw input that would produce it again.
#[tokio::test]
async fn raw_input_shows_what_was_stored() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music").await.unwrap();
    let publication = library
        .create_publication(&gymnopedies().parse().unwrap())
        .await
        .unwrap();
    let contents = publication.works().await.unwrap();

    let mut expected = gymnopedies();
    // What the archive assigned or rewrote along the way
    expected.holdings[0].id = Some(publication.holdings[0].id);
    expected.holdings[1].id = Some(publication.holdings[1].id);
    expected.identifiers[0].value = "978-0-486-23134-1".into();
    expected.identifiers[1].value = "UT 50061".into();
    expected.contents[0].id = Some(contents[0].id);

    assert_eq!(publication.raw_input(&contents), expected);
    assert_eq!(contents[0].raw_input(), expected.contents[0]);
}

#[tokio::test]
async fn suggestions_span_publications_and_their_contents() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music").await.unwrap();
    library
        .create_publication(&gymnopedies().parse().unwrap())
        .await
        .unwrap();

    // Only the work credits an editor, so a role of a work is a role of the library
    assert_eq!(
        library.roles().await.unwrap(),
        ["arranger", "composer", "editor"]
    );
    // By sort name, which is the surname heuristic at work
    assert_eq!(
        library.person_names().await.unwrap(),
        ["Bob", "Erik Satie", "Sue"]
    );

    let gnossiennes = PublicationRawInput {
        title: "Gnossiennes".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contributors: vec![contributor("Erik Satie", "composer")],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&gnossiennes.parse().unwrap())
        .await
        .unwrap();

    // A name the library already has is the same person, credited twice
    assert_eq!(
        library.person_names().await.unwrap(),
        ["Bob", "Erik Satie", "Sue"]
    );
}

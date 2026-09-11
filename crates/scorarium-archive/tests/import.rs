use scorarium_archive::{
    Archive, Draft, HoldingKind, HoldingRawInput, Library, NotFound, PublicationRawInput,
    WorkRawInput, parse_holdings,
};

fn work_ids(draft: &Draft) -> Vec<Option<i64>> {
    draft.input.contents.iter().map(|work| work.id).collect()
}

fn holdings(kind: HoldingKind, location: &str) -> Vec<HoldingRawInput> {
    vec![HoldingRawInput {
        id: None,
        kind,
        location: location.into(),
    }]
}

async fn library() -> (Archive, Library) {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    (archive, library)
}

#[tokio::test]
async fn a_started_import_shows_up_everywhere_it_should() {
    let (archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "Piano bench")).unwrap();

    let import = library
        .start_import("0-486-23134-8", &copies)
        .await
        .unwrap();

    assert_eq!(import.library_name, "Sheet music");
    assert_eq!(import.holdings.len(), 1);
    assert_eq!(import.holdings[0].location.as_deref(), Some("Piano bench"));

    let listed = library.pending_imports().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, import.id);
    assert_eq!(archive.pending_imports().await.unwrap().len(), 1);
    assert_eq!(archive.pending_import_count().await.unwrap(), 1);
    assert!(library.pending_import(import.id).await.unwrap().is_some());

    // An import belongs to the library it was started in
    let books = archive.create_library("Books", false).await.unwrap();
    assert!(books.pending_import(import.id).await.unwrap().is_none());
}

/// The review page opens on something rather than nothing: the copies as entered, and the query
/// read as an identifier when it is one.
#[tokio::test]
async fn a_fresh_draft_is_seeded_from_the_entry_page() {
    let (_archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Digital, "satie.pdf")).unwrap();

    let identified = library
        .start_import("0-486-23134-8", &copies)
        .await
        .unwrap()
        .draft();

    assert!(!identified.saved);
    assert_eq!(identified.input.title, "");
    assert_eq!(identified.input.identifiers.len(), 1);
    assert_eq!(identified.input.identifiers[0].kind, "isbn");
    // Normalized on the way in, so the review page shows the ISBN-13 it will store
    assert_eq!(identified.input.identifiers[0].value, "978-0-486-23134-1");
    assert_eq!(identified.input.holdings.len(), 1);
    assert_eq!(identified.input.holdings[0].location, "satie.pdf");

    let titled = library
        .start_import("Three gymnopedies", &copies)
        .await
        .unwrap()
        .draft();

    assert_eq!(titled.input.title, "Three gymnopedies");
    assert!(titled.input.identifiers.is_empty());
}

#[tokio::test]
async fn saving_names_every_work_the_page_did_not() {
    let (_archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();

    let mut input = import.draft().input;
    input.title = "Three gymnopedies".into();
    input.contents = vec![
        WorkRawInput {
            title: "Gymnopedie No. 1".into(),
            ..WorkRawInput::default()
        },
        WorkRawInput {
            title: "Gymnopedie No. 2".into(),
            ..WorkRawInput::default()
        },
    ];

    let saved = import.save_draft(input);

    assert_eq!(work_ids(&saved), [Some(1), Some(2)]);
    assert!(saved.saved);
    // What comes back next is what was stored, not a fresh seed
    let reopened = import.draft();
    assert!(reopened.saved);
    assert_eq!(reopened.input.title, "Three gymnopedies");
    assert_eq!(work_ids(&reopened), [Some(1), Some(2)]);

    // Dropping a work does not hand its id to the next one added
    let mut input = reopened.input;
    input.contents.remove(0);
    input.contents.push(WorkRawInput {
        title: "Gymnopedie No. 3".into(),
        ..WorkRawInput::default()
    });
    let saved = import.save_draft(input);
    assert_eq!(work_ids(&saved), [Some(2), Some(3)]);
}

#[tokio::test]
async fn accepting_creates_the_publication_once() {
    let (archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "Piano bench")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();
    let input = PublicationRawInput {
        title: "Three gymnopedies".into(),
        contents: vec![WorkRawInput {
            // A draft's work ids mean nothing to the database and are ignored
            id: Some(7),
            title: "Gymnopedie No. 1".into(),
            ..WorkRawInput::default()
        }],
        ..import.draft().input
    }
    .parse()
    .unwrap();
    // A second tab, holding the same import
    let stale = library.pending_import(import.id).await.unwrap().unwrap();

    let publication = import.accept_into_publication(&input).await.unwrap();

    assert_eq!(publication.title, "Three gymnopedies");
    assert_eq!(publication.holdings.len(), 1);
    let works = publication.works().await.unwrap();
    assert_eq!(works.len(), 1);
    assert_ne!(works[0].id, 7);
    assert!(library.pending_imports().await.unwrap().is_empty());
    assert_eq!(archive.pending_import_count().await.unwrap(), 0);

    // The second tab must not create a second publication
    let err = stale.accept_into_publication(&input).await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
    assert_eq!(library.publications().await.unwrap().len(), 1);
}

#[tokio::test]
async fn discarding_removes_the_import() {
    let (_archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();
    let stale = library.pending_import(import.id).await.unwrap().unwrap();

    import.discard().await.unwrap();

    assert!(library.pending_imports().await.unwrap().is_empty());
    let err = stale.discard().await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
}

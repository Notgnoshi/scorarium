use scorarium_archive::{
    Accepted, Archive, ContributorInput, DraftPublication, HoldingKind, HoldingRawInput, Library,
    NotFound, PersonRef, PublicationPost, PublicationRawInput, ValidationError, WorkPost,
    WorkRawInput, WorkRef, parse_holdings,
};

fn work_ids(draft: &DraftPublication) -> Vec<Option<WorkRef>> {
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
        .draft()
        .await
        .unwrap();

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
        .draft()
        .await
        .unwrap();

    assert_eq!(titled.input.title, "Three gymnopedies");
    assert!(titled.input.identifiers.is_empty());
}

#[tokio::test]
async fn saving_names_every_work_the_page_did_not() {
    let (_archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();

    let mut input = import.draft().await.unwrap().input;
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

    let saved = import.save_draft(input).await.unwrap();

    assert_eq!(
        work_ids(&saved),
        [Some(WorkRef::Draft(1)), Some(WorkRef::Draft(2))]
    );
    assert!(saved.saved);
    // What comes back next is what was stored, not a fresh seed
    let reopened = import.draft().await.unwrap();
    assert!(reopened.saved);
    assert_eq!(reopened.input.title, "Three gymnopedies");
    assert_eq!(
        work_ids(&reopened),
        [Some(WorkRef::Draft(1)), Some(WorkRef::Draft(2))]
    );

    // Dropping a work does not hand its id to the next one added
    let mut input = reopened.input;
    input.contents.remove(0);
    input.contents.push(WorkRawInput {
        title: "Gymnopedie No. 3".into(),
        ..WorkRawInput::default()
    });
    let saved = import.save_draft(input).await.unwrap();
    assert_eq!(
        work_ids(&saved),
        [Some(WorkRef::Draft(2)), Some(WorkRef::Draft(3))]
    );
}

#[tokio::test]
async fn accepting_a_picked_stored_work_links_it_whole() {
    let (_archive, library) = library().await;
    let preludes = library
        .create_publication(
            &PublicationRawInput {
                title: "Preludes".into(),
                holdings: holdings(HoldingKind::Physical, ""),
                contents: vec![WorkRawInput {
                    title: "Raindrop".into(),
                    key: "D-flat major".into(),
                    ..WorkRawInput::default()
                }],
                ..PublicationRawInput::default()
            }
            .parse()
            .unwrap(),
        )
        .await
        .unwrap();
    let raindrop = preludes.works().await.unwrap().remove(0).id;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();

    // The review form carries no key, so a key that survives means the work was not rewritten
    let Accepted::Published(publication) = import
        .accept(PublicationPost {
            title: "Nocturnes".into(),
            holdings: holdings(HoldingKind::Physical, ""),
            contents: vec![WorkPost {
                id: Some(WorkRef::Stored(raindrop)),
                title: "Raindrop".into(),
                ..WorkPost::default()
            }],
            ..PublicationPost::default()
        })
        .await
        .unwrap()
    else {
        panic!("a valid draft was refused");
    };
    let works = publication.works().await.unwrap();
    assert_eq!(works.len(), 1);
    assert_eq!(works[0].id, raindrop);
    assert_eq!(works[0].key.as_deref(), Some("D-flat major"));
}

#[tokio::test]
async fn accepting_creates_the_publication_once() {
    let (archive, library) = library().await;
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "Piano bench")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();
    let post = PublicationPost {
        title: "Three gymnopedies".into(),
        holdings: import.draft().await.unwrap().input.holdings,
        contributors: vec![ContributorInput {
            name: "Erik Satie".into(),
            role: "composer".into(),
            person: PersonRef::Unresolved,
        }],
        contents: vec![WorkPost {
            // A draft's work ids mean nothing to the database and are ignored
            id: Some(WorkRef::Draft(7)),
            title: "Gymnopedie No. 1".into(),
            ..WorkPost::default()
        }],
        ..PublicationPost::default()
    };
    // A second tab, holding the same import
    let stale = library.pending_import(import.id).await.unwrap().unwrap();

    // A draft that is not ready is kept, with the edits, for the review page to explain
    let untitled = PublicationPost {
        title: String::new(),
        ..post.clone()
    };
    let refused = library
        .pending_import(import.id)
        .await
        .unwrap()
        .unwrap()
        .accept(untitled)
        .await
        .unwrap();
    let Accepted::Refused(errors) = refused else {
        panic!("an untitled draft was accepted");
    };
    assert_eq!(errors.title, Some(ValidationError::TitleRequired));
    let kept = import.draft().await.unwrap();
    assert!(kept.saved);
    assert_eq!(kept.input.contents[0].title, "Gymnopedie No. 1");
    assert!(library.publications().await.unwrap().is_empty());

    let Accepted::Published(publication) = import.accept(post.clone()).await.unwrap() else {
        panic!("a valid draft was refused");
    };
    assert_eq!(publication.title, "Three gymnopedies");
    assert_eq!(publication.holdings.len(), 1);
    assert_eq!(publication.contributors.len(), 1);
    assert_eq!(publication.contributors[0].name, "Erik Satie");
    let works = publication.works().await.unwrap();
    assert_eq!(works.len(), 1);
    assert_ne!(works[0].id, 7);
    assert!(library.pending_imports().await.unwrap().is_empty());
    assert_eq!(archive.pending_import_count().await.unwrap(), 0);

    // The second tab must not create a second publication
    let err = stale.accept(post).await.unwrap_err();
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

#[tokio::test]
async fn merging_works_moves_drafts_to_the_survivor() {
    let (_archive, library) = library().await;
    let publication = library
        .create_publication(
            &PublicationRawInput {
                title: "Preludes".into(),
                holdings: holdings(HoldingKind::Physical, ""),
                contents: vec![
                    WorkRawInput {
                        title: "Raindrop".into(),
                        ..WorkRawInput::default()
                    },
                    WorkRawInput {
                        title: "Prelude in D-flat".into(),
                        ..WorkRawInput::default()
                    },
                ],
                ..PublicationRawInput::default()
            }
            .parse()
            .unwrap(),
        )
        .await
        .unwrap();
    let works = publication.works().await.unwrap();
    let (survivor, absorbed) = (works[0].id, works[1].id);
    let copies = parse_holdings(&holdings(HoldingKind::Physical, "")).unwrap();
    let import = library.start_import("", &copies).await.unwrap();
    let mut input = import.draft().await.unwrap().input;
    input.contents = vec![works[1].raw_input()];
    import.save_draft(input).await.unwrap();

    library.merge_works(absorbed, survivor).await.unwrap();

    assert_eq!(
        work_ids(&import.draft().await.unwrap()),
        [Some(WorkRef::Stored(survivor))]
    );
}

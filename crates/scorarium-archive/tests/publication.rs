use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, IdentifierRawInput, NotFound,
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
    let library = archive.create_library("Sheet music", false).await.unwrap();

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
    let books = archive.create_library("Books", false).await.unwrap();
    assert!(books.publication(created.id).await.unwrap().is_none());
    assert!(books.publications().await.unwrap().is_empty());
}

#[tokio::test]
async fn works_come_back_whole_and_scoped_to_their_library() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
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
    let books = archive.create_library("Books", false).await.unwrap();
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
    let library = archive.create_library("Sheet music", false).await.unwrap();
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
    let library = archive.create_library("Sheet music", false).await.unwrap();
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

/// The edit page posts the whole publication back, so update has to reconcile every child: keep
/// one, change one, add one, drop one, and collect whatever that leaves credited nowhere.
#[tokio::test]
async fn update_reconciles_every_child() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let mut start = gymnopedies();
    start.contributors.push(contributor("Cid", "editor"));
    start.contents.push(WorkRawInput {
        title: "Gymnopedie No. 2".into(),
        contributors: vec![contributor("Tom", "editor")],
        ..WorkRawInput::default()
    });
    let mut publication = library
        .create_publication(&start.parse().unwrap())
        .await
        .unwrap();
    // Bob is credited elsewhere too, so dropping his credit here must not collect him
    let gnossiennes = PublicationRawInput {
        title: "Gnossiennes".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contributors: vec![contributor("Bob", "arranger")],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&gnossiennes.parse().unwrap())
        .await
        .unwrap();

    let works = publication.works().await.unwrap();
    let kept_holding = publication.holdings[0].id;
    let kept_work = works[0].id;
    let dropped_work = works[1].id;

    let mut edit = publication.raw_input(&works);
    edit.title = "Trois gymnopedies".into();
    // Keep the shelved copy at a new location, drop the file, add a copy
    edit.holdings[0].location = "Shelf".into();
    edit.holdings.remove(1);
    edit.holdings
        .push(holding(HoldingKind::Physical, "Music room"));
    edit.identifiers = vec![IdentifierRawInput {
        kind: "ismn".into(),
        value: "M-060-08002-9".into(),
    }];
    // Bob survives elsewhere; Cid is credited nowhere else
    edit.contributors = vec![contributor("Erik Satie", "composer")];
    // The kept work takes a new field and a new credit; the other is no longer listed
    edit.contents[0].time_signature = "3/4".into();
    edit.contents[0]
        .contributors
        .push(contributor("Ann", "arranger"));
    edit.contents.remove(1);
    edit.contents.push(WorkRawInput {
        title: "Gymnopedie No. 3".into(),
        ..WorkRawInput::default()
    });

    publication.update(&edit.parse().unwrap()).await.unwrap();

    assert_eq!(publication.title, "Trois gymnopedies");
    // The kept copy keeps its identity and takes its new location; the file is gone
    assert_eq!(publication.holdings.len(), 2);
    assert_eq!(publication.holdings[0].id, kept_holding);
    assert_eq!(publication.holdings[0].location.as_deref(), Some("Shelf"));
    assert_eq!(publication.holdings[1].kind, HoldingKind::Physical);
    // Identifiers are replaced outright
    let identifiers: Vec<&str> = publication
        .identifiers
        .iter()
        .map(|identifier| identifier.value.as_str())
        .collect();
    assert_eq!(identifiers, ["979-0-060-08002-9"]);
    let credits: Vec<&str> = publication
        .contributors
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(credits, ["Erik Satie"]);

    let works = publication.works().await.unwrap();
    assert_eq!(works.len(), 2);
    assert_eq!(works[0].id, kept_work);
    assert_eq!(works[0].time_signature.as_deref(), Some("3/4"));
    let credits: Vec<&str> = works[0]
        .contributors
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(credits, ["Erik Satie", "Sue", "Ann"]);
    assert_eq!(works[1].title, "Gymnopedie No. 3");

    // The work the edit stopped listing is gone, and with it the person only it credited
    assert!(library.work(dropped_work).await.unwrap().is_none());
    assert_eq!(
        library.person_names().await.unwrap(),
        ["Ann", "Bob", "Erik Satie", "Sue"]
    );

    // The handle shows what was committed, not what was submitted
    let fresh = library.publication(publication.id).await.unwrap().unwrap();
    assert_eq!(fresh.raw_input(&works), publication.raw_input(&works));
}

#[tokio::test]
async fn updating_a_deleted_publication_is_not_found() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let publication = library
        .create_publication(&gymnopedies().parse().unwrap())
        .await
        .unwrap();
    let mut stale = library.publication(publication.id).await.unwrap().unwrap();
    let input = gymnopedies().parse().unwrap();

    publication.delete().await.unwrap();

    let err = stale.update(&input).await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
    let err = stale.delete().await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
}

#[tokio::test]
async fn delete_collects_only_what_nothing_else_reaches() {
    let archive = Archive::in_memory().await.unwrap();
    archive.populate_demo().await.unwrap();
    let library = archive
        .libraries()
        .await
        .unwrap()
        .into_iter()
        .find(|library| library.name == "Sheet music")
        .unwrap();
    let album = library
        .publications()
        .await
        .unwrap()
        .into_iter()
        .find(|publication| publication.title == "Russian piano album")
        .unwrap();
    let works = album.works().await.unwrap();
    let prelude = works
        .iter()
        .find(|work| work.title == "Prelude in C-sharp minor")
        .unwrap()
        .id;
    let etude = works
        .iter()
        .find(|work| work.title == "Etude-Tableau")
        .unwrap()
        .id;

    album.delete().await.unwrap();

    // The prelude is in the Rachmaninoff collection too; the etude was in the album alone
    assert!(library.work(prelude).await.unwrap().is_some());
    assert!(library.work(etude).await.unwrap().is_none());
    // Rachmaninoff is credited elsewhere; the anthology's other composers were credited here alone
    let names = library.person_names().await.unwrap();
    assert!(names.iter().any(|name| name == "Sergei Rachmaninoff"));
    assert!(!names.iter().any(|name| name == "Dmitri Kabalevsky"));
}

#[tokio::test]
async fn a_work_rebuilds_its_credits_in_input_order() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let publication = library
        .create_publication(&gymnopedies().parse().unwrap())
        .await
        .unwrap();
    let mut work = publication.works().await.unwrap().pop().unwrap();
    let satie = work.contributors[0].person_id;

    let mut edit = work.raw_input();
    edit.title = "Gymnopedie No. 1 (revised)".into();
    edit.time_signature = "3/4".into();
    // Sue goes, Satie stays but is no longer first
    edit.contributors = vec![
        contributor("Ann", "arranger"),
        contributor("Erik Satie", "composer"),
    ];
    work.update(&edit.parse().unwrap()).await.unwrap();

    assert_eq!(work.title, "Gymnopedie No. 1 (revised)");
    assert_eq!(work.time_signature.as_deref(), Some("3/4"));
    let credits: Vec<(&str, &str)> = work
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(credits, [("Ann", "arranger"), ("Erik Satie", "composer")]);
    // A name the library already has is the same person, not a second one
    assert_eq!(work.contributors[1].person_id, satie);
    // Sue was credited only here
    assert_eq!(
        library.person_names().await.unwrap(),
        ["Ann", "Bob", "Erik Satie"]
    );

    // A work collected along with its last publication cannot be edited
    let mut stale = library.work(work.id).await.unwrap().unwrap();
    publication.delete().await.unwrap();
    let err = stale.update(&edit.parse().unwrap()).await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
}

use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, PublicationRawInput, WorkRawInput,
};

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
    }
}

fn work(title: &str, contributors: Vec<ContributorInput>) -> WorkRawInput {
    WorkRawInput {
        title: title.into(),
        contributors,
        ..WorkRawInput::default()
    }
}

fn publication(
    title: &str,
    contributors: Vec<ContributorInput>,
    contents: Vec<WorkRawInput>,
) -> PublicationRawInput {
    PublicationRawInput {
        title: title.into(),
        holdings: vec![HoldingRawInput {
            id: None,
            kind: HoldingKind::Physical,
            location: String::new(),
        }],
        contributors,
        contents,
        ..PublicationRawInput::default()
    }
}

/// Satie is credited directly on one publication and through a work on both; Sue and Ann only ever
/// through works, Bob only ever directly.
async fn library() -> (Archive, scorarium_archive::Library) {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music").await.unwrap();
    for input in [
        publication(
            "Three gymnopedies",
            vec![contributor("Erik Satie", "composer")],
            vec![work(
                "Gymnopedie No. 1",
                vec![
                    contributor("Erik Satie", "composer"),
                    contributor("Sue", "editor"),
                ],
            )],
        ),
        publication(
            "Gnossiennes",
            vec![contributor("Bob", "arranger")],
            vec![work(
                "Gnossienne No. 1",
                vec![
                    contributor("Erik Satie", "composer"),
                    contributor("Ann", "editor"),
                ],
            )],
        ),
    ] {
        library
            .create_publication(&input.parse().unwrap())
            .await
            .unwrap();
    }
    (archive, library)
}

#[tokio::test]
async fn publications_union_direct_and_work_credits() {
    let (archive, library) = library().await;
    let satie = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .remove(0);

    assert_eq!(satie.name, "Erik Satie");
    assert_eq!(satie.sort_name, "Satie, Erik");

    // The first credits him twice over, directly and through its work, and counts once
    let publications = satie.publications().await.unwrap();
    let mut titles: Vec<&str> = publications
        .iter()
        .map(|publication| publication.title.as_str())
        .collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Gnossiennes", "Three gymnopedies"]);

    // Someone credited only through a work is still reachable
    let sue = library.persons_with_role("editor").await.unwrap().remove(1);
    assert_eq!(sue.name, "Sue");
    let credits = sue.publications().await.unwrap();
    assert_eq!(credits.len(), 1);
    assert_eq!(credits[0].title, "Three gymnopedies");

    // A person belongs to their own library alone
    let books = archive.create_library("Books").await.unwrap();
    assert!(library.person(satie.id).await.unwrap().is_some());
    assert!(books.person(satie.id).await.unwrap().is_none());
}

#[tokio::test]
async fn a_role_spans_publications_and_works() {
    let (archive, library) = library().await;
    let other = archive.create_library("Books").await.unwrap();
    other
        .create_publication(
            &publication(
                "Practical Vim",
                vec![contributor("Drew Neil", "editor")],
                Vec::new(),
            )
            .parse()
            .unwrap(),
        )
        .await
        .unwrap();

    let names = |persons: Vec<scorarium_archive::Person>| {
        persons
            .into_iter()
            .map(|person| person.name)
            .collect::<Vec<_>>()
    };

    // Both editors are credited on works alone, and come back by sort name, not by id
    assert_eq!(
        names(library.persons_with_role("editor").await.unwrap()),
        ["Ann", "Sue"]
    );
    // Credited on a publication alone
    assert_eq!(
        names(library.persons_with_role("arranger").await.unwrap()),
        ["Bob"]
    );
    // The other library's editor is not this library's
    assert_eq!(
        names(other.persons_with_role("editor").await.unwrap()),
        ["Drew Neil"]
    );
}

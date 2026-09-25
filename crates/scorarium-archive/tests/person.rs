use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, PersonRawInput, PersonRef,
    PublicationRawInput, WorkRawInput,
};

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
        person: PersonRef::Unresolved,
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
    let library = archive.create_library("Sheet music", false).await.unwrap();
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
    let books = archive.create_library("Books", false).await.unwrap();
    assert!(library.person(satie.id).await.unwrap().is_some());
    assert!(books.person(satie.id).await.unwrap().is_none());
}

#[tokio::test]
async fn a_summary_describes_the_person_asked_for() {
    let (_archive, library) = library().await;
    let satie = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .remove(0);

    // He is credited on both publications and on a work of each, and the direct credit leads
    let summary = library.person_summary(satie.id).await.unwrap().unwrap();
    assert_eq!(
        (summary.id, summary.name, summary.title, summary.others),
        (
            satie.id,
            "Erik Satie".to_string(),
            "Three gymnopedies".to_string(),
            2
        )
    );

    assert!(library.person_summary(9999).await.unwrap().is_none());
}

#[tokio::test]
async fn an_edit_renames_and_relinks() {
    let (_archive, library) = library().await;
    let mut satie = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .remove(0);

    let input = PersonRawInput {
        name: "  Ralph Vaughan Williams  ".into(),
        links: vec![
            "https://imslp.org/wiki/Category:Vaughan_Williams,_Ralph".into(),
            "  https://EN.wikipedia.org/wiki/Ralph_Vaughan_Williams  ".into(),
        ],
    };
    satie.update(&input.parse().unwrap()).await.unwrap();

    let reloaded = library.person(satie.id).await.unwrap().unwrap();
    assert_eq!(reloaded.name, "Ralph Vaughan Williams");
    // The heuristic takes the last word as the surname, so a compound one sorts under its tail
    assert_eq!(reloaded.sort_name, "Williams, Ralph Vaughan");
    assert_eq!(
        reloaded.links,
        [
            "https://imslp.org/wiki/Category:Vaughan_Williams,_Ralph",
            "https://en.wikipedia.org/wiki/Ralph_Vaughan_Williams",
        ]
    );

    // Renaming onto a name someone else already has merges nothing: two persons may share a name
    let sue = library.persons_with_role("editor").await.unwrap().remove(1);
    let mut bob = library
        .persons_with_role("arranger")
        .await
        .unwrap()
        .remove(0);
    bob.update(
        &PersonRawInput {
            name: sue.name.clone(),
            links: Vec::new(),
        }
        .parse()
        .unwrap(),
    )
    .await
    .unwrap();
    assert_ne!(bob.id, sue.id);
    assert_eq!(library.person(bob.id).await.unwrap().unwrap().name, "Sue");
    assert_eq!(library.person(sue.id).await.unwrap().unwrap().name, "Sue");
}

#[tokio::test]
async fn a_role_spans_publications_and_works() {
    let (archive, library) = library().await;
    let other = archive.create_library("Books", false).await.unwrap();
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

#[tokio::test]
async fn a_linked_contributor_credits_that_person_whatever_the_name_says() {
    let (_archive, library) = library().await;
    let satie = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .remove(0);
    let mut bob = library
        .persons_with_role("arranger")
        .await
        .unwrap()
        .remove(0);
    bob.update(
        &PersonRawInput {
            name: "Erik Satie".into(),
            links: Vec::new(),
        }
        .parse()
        .unwrap(),
    )
    .await
    .unwrap();

    let mut credit = contributor("E. Satie", "composer");
    credit.person = PersonRef::Linked(bob.id);
    let stored = library
        .create_publication(
            &publication("Sports et divertissements", vec![credit], Vec::new())
                .parse()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stored.contributors[0].person_id, bob.id);
    // The credit shows the stored name: a contributor never renames anyone
    assert_eq!(stored.contributors[0].name, "Erik Satie");
    assert_eq!(
        library.person(satie.id).await.unwrap().unwrap().name,
        "Erik Satie"
    );
    let saties = library
        .person_names()
        .await
        .unwrap()
        .into_iter()
        .filter(|name| name == "Erik Satie")
        .count();
    assert_eq!(saties, 2, "no third Satie was created");
}

#[tokio::test]
async fn new_contributors_sharing_a_name_become_one_person_per_submission() {
    let (_archive, library) = library().await;
    let satie = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .remove(0)
        .id;
    let new = |name: &str, role: &str| {
        let mut credit = contributor(name, role);
        credit.person = PersonRef::New;
        credit
    };
    let stored = library
        .create_publication(
            &publication(
                "Sports et divertissements",
                vec![new("Erik Satie", "composer")],
                vec![
                    work("Choral inappetissant", vec![new("erik satie", "composer")]),
                    work("La balancoire", vec![new("erik satie", "composer")]),
                ],
            )
            .parse()
            .unwrap(),
        )
        .await
        .unwrap();

    // One person for the publication and both works, deliberately a namesake of the fixture's
    let created = stored.contributors[0].person_id;
    assert_ne!(created, satie);
    let works = stored.works().await.unwrap();
    assert_eq!(works[0].contributors[0].person_id, created);
    assert_eq!(works[1].contributors[0].person_id, created);
    // The first spelling seen names the person
    assert_eq!(works[1].contributors[0].name, "Erik Satie");
    let saties = library
        .person_names()
        .await
        .unwrap()
        .into_iter()
        .filter(|name| name == "Erik Satie")
        .count();
    assert_eq!(saties, 2);
}

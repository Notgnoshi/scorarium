use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, PublicationRawInput, WorkRawInput,
};

fn composer(name: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: "composer".into(),
    }
}

fn shelf() -> HoldingRawInput {
    HoldingRawInput {
        id: None,
        kind: HoldingKind::Physical,
        location: "Shelf".into(),
    }
}

/// A publication holding one work with the given catalog numbers
fn album(title: &str, work: &str, numbers: &[&str]) -> PublicationRawInput {
    PublicationRawInput {
        title: title.into(),
        holdings: vec![shelf()],
        contents: vec![WorkRawInput {
            title: work.into(),
            contributors: vec![composer("Frederic Chopin")],
            catalog_numbers: numbers.iter().map(|n| n.to_string()).collect(),
            ..WorkRawInput::default()
        }],
        ..PublicationRawInput::default()
    }
}

fn values(work: &scorarium_archive::Work) -> Vec<&str> {
    work.catalog_numbers.iter().map(|n| n.as_str()).collect()
}

#[tokio::test]
async fn catalog_numbers_round_trip_and_survive_publication_edits() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music").await.unwrap();
    let mut publication = library
        .create_publication(
            &album("Preludes", "Raindrop", &["Op. 28 No. 15", "B. 107"])
                .parse()
                .unwrap(),
        )
        .await
        .unwrap();
    let mut work = publication.works().await.unwrap().remove(0);
    assert_eq!(values(&work), ["Op. 28 No. 15", "B. 107"]);

    // Editing through the edit page's input replaces the list in input order
    let mut input = work.raw_input();
    input.catalog_numbers = vec!["Op. 28/15".into(), "KK IVb/1".into()];
    work.update(&input.parse().unwrap()).await.unwrap();
    assert_eq!(values(&work), ["Op. 28/15", "KK IVb/1"]);
    let reloaded = library.work(work.id).await.unwrap().unwrap();
    assert_eq!(values(&reloaded), ["Op. 28/15", "KK IVb/1"]);

    // The publication form cannot show every number, so saving it through raw_input must keep them
    let contents = publication.works().await.unwrap();
    let mut input = publication.raw_input(&contents);
    input.title = "24 Preludes".into();
    publication.update(&input.parse().unwrap()).await.unwrap();
    let reloaded = library.work(work.id).await.unwrap().unwrap();
    assert_eq!(values(&reloaded), ["Op. 28/15", "KK IVb/1"]);
}

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
    let library = archive.create_library("Sheet music", false).await.unwrap();
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

#[tokio::test]
async fn a_new_work_with_a_known_number_joins_the_existing_work() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let mut preludes = album("Preludes", "Raindrop Prelude", &["Op. 28 No. 15"]);
    preludes.contents[0].key = "D-flat major".into();
    let preludes = library
        .create_publication(&preludes.parse().unwrap())
        .await
        .unwrap();
    let raindrop = preludes.works().await.unwrap().remove(0);

    // Same composer, same number in a different spelling, a blank key, and an extra credit
    let mut anthology = album("Anthology", "Prelude in D-flat", &["Op. 28/15", "B. 107"]);
    anthology.contents[0].contributors.push(ContributorInput {
        name: "Sue".into(),
        role: "editor".into(),
    });
    let anthology = library
        .create_publication(&anthology.parse().unwrap())
        .await
        .unwrap();

    let works = anthology.works().await.unwrap();
    assert_eq!(works.len(), 1);
    let merged = &works[0];
    assert_eq!(
        merged.id, raindrop.id,
        "the existing work was linked, not a new one"
    );
    assert_eq!(
        merged.title, "Raindrop Prelude",
        "the survivor's title wins"
    );
    assert_eq!(merged.key.as_deref(), Some("D-flat major"));
    assert_eq!(
        values(merged),
        ["Op. 28 No. 15", "B. 107"],
        "a matching spelling is not added, a new number is"
    );
    let credits: Vec<(&str, &str)> = merged
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        credits,
        [("Frederic Chopin", "composer"), ("Sue", "editor")]
    );
    assert_eq!(merged.publications().await.unwrap().len(), 2);

    // The same number under another composer is another work
    let mut other = album("Sonatas", "Sonata", &["Op. 28 No. 15"]);
    other.contents[0].contributors = vec![composer("Ludwig van Beethoven")];
    let other = library
        .create_publication(&other.parse().unwrap())
        .await
        .unwrap();
    assert_ne!(other.works().await.unwrap()[0].id, raindrop.id);
}

#[tokio::test]
async fn editing_a_number_into_a_collision_merges_into_the_older_work() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let preludes = library
        .create_publication(
            &album("Preludes", "Raindrop Prelude", &["Op. 28 No. 15"])
                .parse()
                .unwrap(),
        )
        .await
        .unwrap();
    let older = preludes.works().await.unwrap().remove(0);
    let anthology = library
        .create_publication(
            &album("Anthology", "Prelude in D-flat", &["B. 107"])
                .parse()
                .unwrap(),
        )
        .await
        .unwrap();
    let mut newer = anthology.works().await.unwrap().remove(0);
    let newer_id = newer.id;

    let mut input = newer.raw_input();
    input.catalog_numbers.push("op. 28 no. 15".into());
    input.title = "Retitled".into();
    newer.update(&input.parse().unwrap()).await.unwrap();

    assert_eq!(newer.id, older.id, "self is now the survivor");
    assert_eq!(
        newer.title, "Raindrop Prelude",
        "the survivor's title wins over the edit"
    );
    assert_eq!(values(&newer), ["Op. 28 No. 15", "B. 107"]);
    assert!(
        library.work(newer_id).await.unwrap().is_none(),
        "the edited work is gone"
    );
    assert_eq!(
        anthology.works().await.unwrap()[0].id,
        older.id,
        "the anthology now contains the survivor"
    );
}

#[tokio::test]
async fn merge_works_is_available_directly() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let a = library
        .create_publication(&album("A", "Nocturne", &["Op. 9 No. 2"]).parse().unwrap())
        .await
        .unwrap();
    let b = library
        .create_publication(
            &album("B", "Nocturne in E-flat", &["B. 54"])
                .parse()
                .unwrap(),
        )
        .await
        .unwrap();
    let into = a.works().await.unwrap()[0].id;
    let from = b.works().await.unwrap()[0].id;

    let merged = library.merge_works(from, into).await.unwrap();
    assert_eq!(merged.id, into);
    assert_eq!(values(&merged), ["Op. 9 No. 2", "B. 54"]);
    assert_eq!(merged.publications().await.unwrap().len(), 2);
    let err = library.merge_works(from, into).await.unwrap_err();
    assert!(err.downcast_ref::<scorarium_archive::NotFound>().is_some());
}

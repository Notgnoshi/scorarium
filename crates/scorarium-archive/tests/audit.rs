use scorarium_archive::{
    Action, Archive, ContributorInput, EntityKind, Field, HoldingKind, HoldingRawInput,
    IdentifierRawInput, PublicationInput, PublicationRawInput, Source, WorkRawInput,
};

/// A publication with a copy and an identifier, containing one work whose only composer is "Bach"
fn goldberg() -> PublicationInput {
    PublicationRawInput {
        title: "Goldberg Variations".into(),
        holdings: vec![HoldingRawInput {
            id: None,
            kind: HoldingKind::Physical,
            location: String::new(),
        }],
        identifiers: vec![IdentifierRawInput {
            kind: "plate_number".into(),
            value: "BA 5162".into(),
        }],
        contents: vec![WorkRawInput {
            title: "Goldberg Variations".into(),
            contributors: vec![ContributorInput {
                name: "Bach".into(),
                role: "composer".into(),
            }],
            ..WorkRawInput::default()
        }],
        ..PublicationRawInput::default()
    }
    .parse()
    .unwrap()
}

#[tokio::test]
async fn consequences_group_under_their_headline() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();

    let entries = archive.audit_log().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].group_id, None);
    assert_eq!(entries[0].source, Source::User);
    assert_eq!(entries[0].event.action, Action::Created);
    let entity = entries[0].event.entity.as_ref().unwrap();
    assert_eq!(entity.kind, EntityKind::Library);
    assert_eq!(entity.id, library.id);
    assert_eq!(entity.label, "Test");
}

#[tokio::test]
async fn a_rolled_back_mutation_records_nothing() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let before = archive.audit_log().await.unwrap().len();

    // Updating a library that was already deleted fails after the headline is written
    let mut doomed = archive.library(library.id).await.unwrap().unwrap();
    archive
        .library(library.id)
        .await
        .unwrap()
        .unwrap()
        .delete()
        .await
        .unwrap();
    assert!(doomed.update("Gone", false).await.is_err());

    // The delete added one entry; the failed update added none
    assert_eq!(archive.audit_log().await.unwrap().len(), before + 1);
}

/// Each public mutation leaves exactly one headline
#[tokio::test]
async fn every_mutation_records_a_headline() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let input = goldberg();
    let mut publication = library.create_publication(&input).await.unwrap();
    publication.update(&input).await.unwrap();
    publication.delete().await.unwrap();
    archive.claim_password("hunter2").await.unwrap();
    archive.change_password("hunter3").await.unwrap();

    let headlines: Vec<_> = archive
        .audit_log()
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| entry.group_id.is_none())
        .map(|entry| entry.event.action)
        .collect();
    // Newest first: password change and claim, delete, update, create publication, create library
    assert_eq!(
        headlines,
        vec![
            Action::PasswordChanged,
            Action::PasswordClaimed,
            Action::Deleted,
            Action::Updated,
            Action::Created,
            Action::Created,
        ]
    );
}

#[tokio::test]
async fn an_update_names_only_the_fields_that_changed() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let mut publication = library.create_publication(&goldberg()).await.unwrap();
    library.create_publication(&goldberg()).await.unwrap();

    // Edited the way the edit page does it, so the contained work keeps its id
    let works = publication.works().await.unwrap();
    let mut retitled = publication.raw_input(&works);
    retitled.title = "Aria mit verschiedenen Veraenderungen".into();
    publication
        .update(&retitled.parse().unwrap())
        .await
        .unwrap();

    let latest = archive.audit_log().await.unwrap().remove(0);
    assert_eq!(latest.event.action, Action::Updated);
    assert_eq!(latest.event.fields, vec![Field::Title]);
}

/// Saving a publication without touching anything records an update with no fields
#[tokio::test]
async fn an_update_that_changes_nothing_names_no_fields() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let mut publication = library.create_publication(&goldberg()).await.unwrap();
    library.create_publication(&goldberg()).await.unwrap();

    let works = publication.works().await.unwrap();
    let unchanged = publication.raw_input(&works).parse().unwrap();
    publication.update(&unchanged).await.unwrap();

    let latest = archive.audit_log().await.unwrap().remove(0);
    assert_eq!(latest.event.action, Action::Updated);
    assert_eq!(latest.event.fields, Vec::new());
}

/// Removing a work from a publication orphans the work, and orphaning the work orphans the person
/// credited only on it. All three facts belong to the one edit that caused them, and the entries
/// are the only place the deleted names still exist.
#[tokio::test]
async fn an_edit_records_the_deletions_it_caused() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let mut publication = library.create_publication(&goldberg()).await.unwrap();

    let works = publication.works().await.unwrap();
    let mut emptied = publication.raw_input(&works);
    emptied.contents.clear();
    publication.update(&emptied.parse().unwrap()).await.unwrap();

    let entries = archive.audit_log().await.unwrap();
    let headline = &entries[0];
    assert_eq!(headline.group_id, None);
    assert_eq!(headline.event.action, Action::Updated);
    assert_eq!(headline.event.fields, vec![Field::Contents]);

    let consequences: Vec<_> = entries
        .iter()
        .filter(|entry| entry.group_id == Some(headline.id))
        .map(|entry| {
            let entity = entry.event.entity.as_ref().unwrap();
            (entry.source, entity.kind, entity.label.as_str())
        })
        .collect();
    assert_eq!(
        consequences,
        vec![
            (
                Source::OrphanCleanup,
                EntityKind::Work,
                "Goldberg Variations"
            ),
            (Source::OrphanCleanup, EntityKind::Person, "Bach"),
        ]
    );
}

use scorarium_archive::{
    Action, Archive, AuditSubject, ContributorInput, Field, HoldingKind, HoldingRawInput,
    IdentifierRawInput, PersonRef, PublicationInput, PublicationRawInput, Source, WorkRawInput,
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
                person: PersonRef::New,
                external_person: None,
            }],
            ..WorkRawInput::default()
        }],
        ..PublicationRawInput::default()
    }
    .parse()
    .unwrap()
}

#[tokio::test]
async fn creating_a_library_records_one_entry() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();

    let entries = archive.audit_log(None, None).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source, Source::User);
    assert_eq!(entries[0].action, Action::Created);
    let entity = entries[0].entity.as_ref().unwrap();
    assert_eq!(entity.kind, AuditSubject::Library);
    assert_eq!(entity.id, library.id);
    assert_eq!(entity.label, "Test");
}

#[tokio::test]
async fn a_rolled_back_mutation_records_nothing() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let before = archive.audit_log(None, None).await.unwrap().len();

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
    assert_eq!(
        archive.audit_log(None, None).await.unwrap().len(),
        before + 1
    );
}

/// Each public mutation leaves exactly one entry of its own, beside whatever its consequences add
#[tokio::test]
async fn every_mutation_records_an_entry() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let input = goldberg();
    let mut publication = library.create_publication(&input).await.unwrap().0;
    let _ = publication.update(&input).await.unwrap();
    publication.delete().await.unwrap();
    archive.claim_password("hunter2").await.unwrap();
    archive.change_password("hunter3").await.unwrap();

    let actions: Vec<_> = archive
        .audit_log(None, None)
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| entry.source == Source::User)
        .map(|entry| entry.action)
        .collect();
    // Newest first: password change and claim, delete, update, create publication, create library
    assert_eq!(
        actions,
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
    let mut publication = library.create_publication(&goldberg()).await.unwrap().0;
    let _ = library.create_publication(&goldberg()).await.unwrap();

    // Edited the way the edit page does it, so the contained work keeps its id
    let works = publication.works().await.unwrap();
    let mut retitled = publication.raw_input(&works);
    retitled.title = "Aria mit verschiedenen Veraenderungen".into();
    let _ = publication
        .update(&retitled.parse().unwrap())
        .await
        .unwrap();

    let latest = archive.audit_log(None, Some(1)).await.unwrap().remove(0);
    assert_eq!(latest.action, Action::Updated);
    assert_eq!(latest.fields, vec![Field::Title]);
    // The entry names the publication as it reads after the edit
    assert_eq!(
        latest.entity.unwrap().label,
        "Aria mit verschiedenen Veraenderungen"
    );
}

/// Saving a publication without touching anything records an update with no fields
#[tokio::test]
async fn an_update_that_changes_nothing_names_no_fields() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let mut publication = library.create_publication(&goldberg()).await.unwrap().0;
    let _ = library.create_publication(&goldberg()).await.unwrap();

    let works = publication.works().await.unwrap();
    let unchanged = publication.raw_input(&works).parse().unwrap();
    let _ = publication.update(&unchanged).await.unwrap();

    let latest = archive.audit_log(None, Some(1)).await.unwrap().remove(0);
    assert_eq!(latest.action, Action::Updated);
    assert_eq!(latest.fields, Vec::new());
}

/// Removing a work from a publication orphans the work, and orphaning the work orphans the person
/// credited only on it. Each deletion is its own entry, below the edit that caused it, and the
/// entries are the only place the deleted names still exist.
#[tokio::test]
async fn an_edit_records_the_deletions_it_caused() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test", false).await.unwrap();
    let mut publication = library.create_publication(&goldberg()).await.unwrap().0;

    let works = publication.works().await.unwrap();
    let mut emptied = publication.raw_input(&works);
    emptied.contents.clear();
    let _ = publication.update(&emptied.parse().unwrap()).await.unwrap();

    let entries = archive.audit_log(None, Some(3)).await.unwrap();
    let shown: Vec<_> = entries
        .iter()
        .map(|entry| {
            let entity = entry.entity.as_ref().unwrap();
            (
                entry.source,
                entry.action,
                entity.kind,
                entity.label.as_str(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                Source::User,
                Action::Updated,
                AuditSubject::Publication,
                "Goldberg Variations"
            ),
            (
                Source::OrphanCleanup,
                Action::Deleted,
                AuditSubject::Person,
                "Bach"
            ),
            (
                Source::OrphanCleanup,
                Action::Deleted,
                AuditSubject::Work,
                "Goldberg Variations"
            ),
        ]
    );
    assert_eq!(entries[0].fields, vec![Field::Contents]);
}

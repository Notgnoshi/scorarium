use scorarium_archive::{Action, Archive, EntityKind, Source};

#[tokio::test]
#[ignore]
async fn consequences_group_under_their_headline() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test").await.unwrap();

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
#[ignore]
async fn a_rolled_back_mutation_records_nothing() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Test").await.unwrap();
    let before = archive.audit_log().await.unwrap().len();

    // Renaming a library that was already deleted fails after the headline is written
    let mut doomed = archive.library(library.id).await.unwrap().unwrap();
    archive
        .library(library.id)
        .await
        .unwrap()
        .unwrap()
        .delete()
        .await
        .unwrap();
    assert!(doomed.rename("Gone").await.is_err());

    // The delete added one entry; the failed rename added none
    assert_eq!(archive.audit_log().await.unwrap().len(), before + 1);
}

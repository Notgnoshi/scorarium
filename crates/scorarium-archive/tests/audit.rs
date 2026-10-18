use scorarium_archive::{Action, Archive, AuditSubject, Field, HoldingKind, Source};

#[tokio::test]
async fn every_mutation_records_an_entry() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Test", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let (id, _) = library
        .create_publication("Goldberg Variations", HoldingKind::Physical, None)
        .await
        .unwrap();
    library
        .publication(id)
        .await
        .unwrap()
        .delete()
        .await
        .unwrap();
    edit.commit().await.unwrap();
    archive.claim_password("hunter2").await.unwrap();
    archive.change_password("hunter3").await.unwrap();

    let entries: Vec<_> = archive
        .audit_log(None, None)
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| entry.source == Source::User)
        .collect();
    let actions: Vec<_> = entries.iter().map(|entry| entry.action).collect();
    // Newest first: password change and claim, delete, create publication, create library
    assert_eq!(
        actions,
        [
            Action::PasswordChanged,
            Action::PasswordClaimed,
            Action::Deleted,
            Action::Created,
            Action::Created,
        ]
    );
    let library = entries.last().unwrap().entity.as_ref().unwrap();
    assert_eq!(library.kind, AuditSubject::Library);
    assert_eq!(library.id, library_id);
    assert_eq!(library.label, "Test");
}

/// Removing a work from a publication orphans the work, and orphaning the work orphans the person
/// who is a contributor only to it.
#[tokio::test]
async fn an_edit_records_the_deletions_it_caused() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Test", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let (id, _) = library
        .create_publication("Goldberg Variations", HoldingKind::Physical, None)
        .await
        .unwrap();
    let bach = library.create_person("Bach", "Bach").await.unwrap();
    let work = library.create_work("Goldberg Variations").await.unwrap();
    library
        .work(work)
        .await
        .unwrap()
        .add_contributor(bach, "composer")
        .await
        .unwrap();
    library
        .publication(id)
        .await
        .unwrap()
        .add_work(work)
        .await
        .unwrap();
    edit.commit().await.unwrap();

    let mut edit = archive.edit().await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    library
        .publication(id)
        .await
        .unwrap()
        .remove_work(work)
        .await
        .unwrap();
    edit.commit().await.unwrap();

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
        [
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
            (
                Source::User,
                Action::Updated,
                AuditSubject::Publication,
                "Goldberg Variations"
            ),
        ]
    );
    assert_eq!(entries[2].field, Some(Field::Contents));
}

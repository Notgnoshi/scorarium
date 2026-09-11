use scorarium_archive::{Action, Archive, Field, NotFound};

#[tokio::test]
async fn library_retrieval() {
    let archive = Archive::in_memory().await.unwrap();
    archive.create_library("Sheet music", false).await.unwrap();
    let books = archive.create_library("Books", true).await.unwrap();

    let listed: Vec<(String, bool)> = archive
        .libraries()
        .await
        .unwrap()
        .into_iter()
        .map(|library| (library.name, library.private))
        .collect();
    assert_eq!(
        listed,
        [
            ("Books".to_string(), true),
            ("Sheet music".to_string(), false)
        ]
    );

    let found = archive.library(books.id).await.unwrap().unwrap();
    assert_eq!(found.name, "Books");
    assert!(found.private);
    assert!(archive.library(books.id + 1000).await.unwrap().is_none());
}

#[tokio::test]
async fn update_changes_the_handle_and_the_stored_values() {
    let archive = Archive::in_memory().await.unwrap();
    let mut library = archive.create_library("Books", false).await.unwrap();

    library.update("Novels", true).await.unwrap();

    assert_eq!(library.name, "Novels");
    assert!(library.private);
    let stored = archive.library(library.id).await.unwrap().unwrap();
    assert_eq!(stored.name, "Novels");
    assert!(stored.private);

    let newest = archive.audit_log().await.unwrap().remove(0);
    assert_eq!(newest.event.action, Action::Updated);
    assert_eq!(newest.event.fields, [Field::Name, Field::Visibility]);
    assert_eq!(newest.event.entity.unwrap().label, "Novels");
}

/// Two browser tabs, one of which deleted the library the other is still attempting to use
#[tokio::test]
async fn writing_a_deleted_library_is_not_found() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Books", false).await.unwrap();
    let mut stale = archive.library(library.id).await.unwrap().unwrap();

    library.delete().await.unwrap();
    assert!(archive.library(stale.id).await.unwrap().is_none());

    let err = stale.update("Novels", false).await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
    let err = stale.delete().await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
}

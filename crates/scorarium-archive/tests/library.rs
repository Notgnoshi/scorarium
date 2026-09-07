use scorarium_archive::{Archive, NotFound};

#[tokio::test]
async fn library_retrieval() {
    let archive = Archive::in_memory().await.unwrap();
    archive.create_library("Sheet music").await.unwrap();
    let books = archive.create_library("Books").await.unwrap();

    let names: Vec<String> = archive
        .libraries()
        .await
        .unwrap()
        .into_iter()
        .map(|library| library.name)
        .collect();
    assert_eq!(names, ["Books", "Sheet music"]);

    let found = archive.library(books.id).await.unwrap().unwrap();
    assert_eq!(found.name, "Books");
    assert!(archive.library(books.id + 1000).await.unwrap().is_none());
}

#[tokio::test]
async fn rename_changes_the_handle_and_the_stored_name() {
    let archive = Archive::in_memory().await.unwrap();
    let mut library = archive.create_library("Books").await.unwrap();

    library.rename("Novels").await.unwrap();

    assert_eq!(library.name, "Novels");
    let stored = archive.library(library.id).await.unwrap().unwrap();
    assert_eq!(stored.name, "Novels");
}

/// Two browser tabs, one of which deleted the library the other is still attempting to use
#[tokio::test]
async fn writing_a_deleted_library_is_not_found() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Books").await.unwrap();
    let mut stale = archive.library(library.id).await.unwrap().unwrap();

    library.delete().await.unwrap();
    assert!(archive.library(stale.id).await.unwrap().is_none());

    let err = stale.rename("Novels").await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
    let err = stale.delete().await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
}

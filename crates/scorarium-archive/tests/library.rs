use scorarium_archive::{Archive, Field, NotFound};

#[tokio::test]
async fn library_retrieval() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    edit.create_library("Sheet music", false).await.unwrap();
    let books = edit.create_library("Books", true).await.unwrap();
    edit.commit().await.unwrap();

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

    let found = archive.library(books).await.unwrap().unwrap();
    assert_eq!(found.name, "Books");
    assert!(found.private);
    assert!(archive.library(books + 1000).await.unwrap().is_none());
}

#[tokio::test]
async fn rename_records_only_what_changed() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let id = edit.create_library("Books", false).await.unwrap();
    let mut library = edit.library(id).await.unwrap();
    library.rename("Novels").await.unwrap();
    library.set_private(false).await.unwrap();
    edit.commit().await.unwrap();

    assert_eq!(archive.library(id).await.unwrap().unwrap().name, "Novels");
    assert_eq!(archive.recent_fields(5).await.unwrap(), [Field::Name]);
    let newest = archive.audit_log(None, Some(1)).await.unwrap().remove(0);
    assert_eq!(newest.entity.unwrap().label, "Novels");
}

#[tokio::test]
async fn a_handle_for_a_deleted_library_is_not_found() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let id = edit.create_library("Books", false).await.unwrap();
    edit.commit().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    edit.library(id).await.unwrap().delete().await.unwrap();
    edit.commit().await.unwrap();

    let mut edit = archive.edit().await.unwrap();
    let error = edit.library(id).await.unwrap_err();
    assert!(error.downcast_ref::<NotFound>().is_some(), "{error}");
}

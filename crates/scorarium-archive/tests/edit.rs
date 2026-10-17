use std::time::Duration;

use scorarium_archive::Archive;

#[tokio::test]
async fn dropping_a_handle_rolls_back() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    edit.create_library("Books", false).await.unwrap();
    drop(edit);

    assert!(archive.libraries().await.unwrap().is_empty());
    assert!(archive.audit_log(None, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_second_handle_waits_for_the_first_to_commit() {
    let data_dir = tempfile::tempdir().unwrap();
    let first = Archive::open(data_dir.path(), true).await.unwrap();
    let second = Archive::open(data_dir.path(), true).await.unwrap();

    let mut edit = first.edit().await.unwrap();
    edit.create_library("Books", false).await.unwrap();
    let mut waiting = tokio::spawn(async move {
        let edit = second.edit().await.unwrap();
        let names: Vec<String> = second
            .libraries()
            .await
            .unwrap()
            .into_iter()
            .map(|library| library.name)
            .collect();
        drop(edit);
        names
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut waiting)
            .await
            .is_err(),
        "the second handle opened while the first held the write lock"
    );

    edit.commit().await.unwrap();
    assert_eq!(waiting.await.unwrap(), ["Books"]);
}

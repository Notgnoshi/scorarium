use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::{db, router};
use scorarium_tests::TestDb;

#[tokio::test]
async fn work_page() {
    let state = TestDb::new().demo().build().await;
    let libraries = db::list_libraries(&state.pool).await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = db::publication::list(&state.pool, sheet_music.id)
        .await
        .unwrap();
    let album = publications
        .iter()
        .find(|p| p.title == "Russian piano album")
        .unwrap();
    let masterpieces = publications
        .iter()
        .find(|p| p.title.starts_with("Rachmaninoff masterpieces"))
        .unwrap();
    let works = db::work::list_in_publication(&state.pool, sheet_music.id, album.id)
        .await
        .unwrap();
    let prelude = works
        .iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let server = TestServer::new(router(Arc::new(state)));

    let response = server
        .get(&format!("/library/{}/work/{}", sheet_music.id, prelude.id))
        .await;
    response.assert_status_ok();
    let album_href = format!(
        "href=\"/library/{}/publication/{}\"",
        sheet_music.id, album.id
    );
    let masterpieces_href = format!(
        "href=\"/library/{}/publication/{}\"",
        sheet_music.id, masterpieces.id
    );
    for expected in [
        "Prelude in C-sharp minor",
        "Op. 3 No. 2",
        "C-sharp minor",
        "piano",
        "Sergei Rachmaninoff",
        "composer",
        // The work is in two publications, both of which are listed
        "Russian piano album",
        album_href.as_str(),
        "Rachmaninoff masterpieces for solo piano",
        masterpieces_href.as_str(),
        "physical",
        // The breadcrumb back to the library
        "Sheet music",
    ] {
        response.assert_text_contains(expected);
    }

    // A work is only reachable through its own library
    let response = server
        .get(&format!("/library/{}/work/{}", books.id, prelude.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let missing = works.iter().map(|w| w.id).max().unwrap() + 100;
    let response = server
        .get(&format!("/library/{}/work/{missing}", sheet_music.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

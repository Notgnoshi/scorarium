use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::{db, router};
use scorarium_tests::TestDb;

#[tokio::test]
async fn person_page() {
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
    let album_works = db::work::list_in_publication(&state.pool, sheet_music.id, album.id)
        .await
        .unwrap();
    let prelude = album_works
        .iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let masterpieces_works =
        db::work::list_in_publication(&state.pool, sheet_music.id, masterpieces.id)
            .await
            .unwrap();
    let polichinelle = masterpieces_works
        .iter()
        .find(|w| w.title == "Polichinelle")
        .unwrap();
    let person_id = |name: &str| {
        album
            .contributors
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .person_id
    };
    let rachmaninoff = person_id("Sergei Rachmaninoff");
    let kabalevsky = person_id("Dmitri Kabalevsky");
    let server = TestServer::new(router(Arc::new(state)));

    let response = server
        .get(&format!(
            "/library/{}/person/{rachmaninoff}",
            sheet_music.id
        ))
        .await;
    response.assert_status_ok();
    let publication_href =
        |id: i64| format!("href=\"/library/{}/publication/{id}\"", sheet_music.id);
    let work_href = |id: i64| format!("href=\"/library/{}/work/{id}\"", sheet_music.id);
    for expected in [
        "Sergei Rachmaninoff",
        "Russian piano album",
        publication_href(album.id).as_str(),
        "Rachmaninoff masterpieces for solo piano",
        publication_href(masterpieces.id).as_str(),
        "Prelude in C-sharp minor",
        work_href(prelude.id).as_str(),
        "Etude-Tableau",
        "Polichinelle",
        work_href(polichinelle.id).as_str(),
        "composer",
        // The breadcrumb back to the library
        "Sheet music",
    ] {
        response.assert_text_contains(expected);
    }

    // Credited on the anthology, but none of his pieces are entered as works
    let response = server
        .get(&format!("/library/{}/person/{kabalevsky}", sheet_music.id))
        .await;
    response.assert_status_ok();
    response.assert_text_contains("Russian piano album");
    response.assert_text_contains("composer");
    assert!(!response.text().contains("Prelude in C-sharp minor"));

    // A person is only reachable through their own library
    let response = server
        .get(&format!("/library/{}/person/{rachmaninoff}", books.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let missing = album
        .contributors
        .iter()
        .map(|c| c.person_id)
        .max()
        .unwrap()
        + 100;
    let response = server
        .get(&format!("/library/{}/person/{missing}", sheet_music.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

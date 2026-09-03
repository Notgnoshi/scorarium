use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::{db, router};
use scorarium_tests::{TestDb, browser};

#[tokio::test]
async fn library_page() {
    let state = TestDb::new().library("Sheet music").build().await;
    // Ask the database for the id rather than assuming SQLite hands out 1 for the first row
    let id = db::list_libraries(&state.pool).await.unwrap()[0].id;
    let server = TestServer::new(router(Arc::new(state)));

    let response = server.get("/").await;
    response.assert_text_contains(format!("href=\"/library/{id}\""));

    let response = server.get(&format!("/library/{id}")).await;
    response.assert_status_ok();
    response.assert_text_contains("Sheet music");
    response.assert_text_contains("Home");

    let response = server.get(&format!("/library/{}", id + 1)).await;
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn library_page_lists_publications() {
    let state = TestDb::new().demo().build().await;
    let libraries = db::list_libraries(&state.pool).await.unwrap();
    let library = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = db::publication::list(&state.pool, library.id)
        .await
        .unwrap();
    let gymnopedies = publications
        .iter()
        .find(|p| p.title.starts_with("Three gymnopedies"))
        .unwrap();
    let link = format!(
        "href=\"/library/{}/publication/{}\"",
        library.id, gymnopedies.id
    );
    let satie_link = format!(
        "href=\"/library/{}/person/{}\"",
        library.id, gymnopedies.contributors[0].person_id
    );
    let server = TestServer::new(router(Arc::new(state)));

    let response = server.get(&format!("/library/{}", library.id)).await;
    response.assert_status_ok();
    response.assert_text_contains("Russian piano album");
    response.assert_text_contains("Three gymnopedies for the piano");
    response.assert_text_contains("Erik Satie");
    response.assert_text_contains(link);
    response.assert_text_contains(satie_link);
}

/// A library links only to the listings that would have something in them.
#[tokio::test]
async fn library_page_links_to_listings() {
    let state = TestDb::new().demo().build().await;
    let libraries = db::list_libraries(&state.pool).await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap().id;
    let sheet_music = libraries
        .iter()
        .find(|l| l.name == "Sheet music")
        .unwrap()
        .id;
    let server = TestServer::new(router(Arc::new(state)));

    let response = server.get(&format!("/library/{sheet_music}")).await;
    response.assert_text_contains(format!("href=\"/library/{sheet_music}/composers\""));
    assert!(
        !response
            .text()
            .contains(&format!("href=\"/library/{sheet_music}/authors\""))
    );

    let response = server.get(&format!("/library/{books}")).await;
    response.assert_text_contains(format!("href=\"/library/{books}/authors\""));
    assert!(
        !response
            .text()
            .contains(&format!("href=\"/library/{books}/composers\""))
    );
}

#[tokio::test]
async fn library_crud_flow() {
    let state = TestDb::new().password("hunter2").build().await;
    // Keep a handle on the database to look up ids the UI only exposes as links
    let pool = state.pool.clone();
    let server = browser(state);

    // Managing libraries requires login
    let response = server.post("/library").form(&[("name", "Books")]).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");

    server.post("/login").form(&[("password", "hunter2")]).await;

    // Whitespace passes the form's `required` attribute, so the server must reject it too
    let response = server.post("/library").form(&[("name", "   ")]).await;
    response.assert_status_ok();
    response.assert_text_contains("must not be empty");

    let response = server.post("/library").form(&[("name", "Books")]).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/");
    server.get("/").await.assert_text_contains("Books");

    let id = db::list_libraries(&pool).await.unwrap()[0].id;
    let response = server
        .post(&format!("/library/{id}/rename"))
        .form(&[("name", "Novels")])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    let location = format!("/library/{id}");
    response.assert_header("location", location.as_str());
    let home = server.get("/").await;
    home.assert_text_contains("Novels");
    assert!(!home.text().contains("Books"));

    let response = server.post(&format!("/library/{id}/delete")).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/");
    assert!(!server.get("/").await.text().contains("Novels"));

    // The deleted library's id no longer exists
    let response = server
        .post(&format!("/library/{id}/rename"))
        .form(&[("name", "Novels")])
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
    let response = server.post(&format!("/library/{id}/delete")).await;
    response.assert_status(StatusCode::NOT_FOUND);
}

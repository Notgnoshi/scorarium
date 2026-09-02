use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::{db, router};
use scorarium_tests::TestDb;

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

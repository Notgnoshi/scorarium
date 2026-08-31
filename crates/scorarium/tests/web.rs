use std::sync::Arc;

use axum_test::TestServer;
use scorarium::router;
use scorarium_tests::TestDb;

#[tokio::test]
async fn index_lists_libraries() {
    let state = TestDb::new()
        .library("lib2-ASDF")
        .library("lib1-QWERT")
        .build()
        .await;
    let server = TestServer::new(router(Arc::new(state)));

    let response = server.get("/").await;
    response.assert_status_ok();
    response.assert_text_contains("lib1-QWERT");
    response.assert_text_contains("lib2-ASDF");
}

use std::sync::Arc;

use axum_test::TestServer;
use scorarium::{AppState, db, router};

#[tokio::test]
async fn hello_world() {
    let tmp = tempfile::tempdir().unwrap();
    let pool = db::connect(tmp.path()).await.unwrap();
    let server = TestServer::new(router(Arc::new(AppState { pool })));
    let response = server.get("/").await;
    response.assert_status_ok();
    response.assert_text("scorarium");
}

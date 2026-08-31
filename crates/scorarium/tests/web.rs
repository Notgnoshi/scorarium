use std::sync::Arc;

use axum_test::TestServer;
use scorarium::{AppState, router};

#[tokio::test]
async fn hello_world() {
    let server = TestServer::new(router(Arc::new(AppState::default())));
    let response = server.get("/").await;
    response.assert_status_ok();
    response.assert_text("scorarium");
}

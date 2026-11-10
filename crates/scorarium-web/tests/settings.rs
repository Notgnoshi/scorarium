use axum::http::StatusCode;
use scorarium_test::TestApp;

#[tokio::test]
async fn login_is_required() {
    let app = TestApp::logged_out(true).await;
    let response = app.server.get("/settings").await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login?next=/settings");

    let app = TestApp::demo().await;
    app.server.get("/settings").await.assert_status_ok();
}

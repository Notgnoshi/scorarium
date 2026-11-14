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

#[tokio::test]
async fn reset_restores_the_demo_libraries() {
    let app = TestApp::demo().await;
    for library in app.engine.libraries().await.unwrap() {
        library.delete().await.unwrap();
    }

    let response = app.server.post("/settings/demo/reset").await;

    response.assert_status(StatusCode::SEE_OTHER);
    let libraries = app.engine.libraries().await.unwrap();
    let ids: Vec<String> = libraries
        .iter()
        .map(|library| library.id().to_string())
        .collect();
    assert_eq!(ids, ["1", "2"]);
}

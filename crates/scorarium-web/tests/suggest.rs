use axum::http::StatusCode;
use scorarium_test::TestApp;

#[tokio::test]
async fn logged_out_suggestion_request_redirects_htmx_to_login() {
    let app = TestApp::logged_out(true).await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let page = format!("/library/{}/work/1/edit", library.id());
    let response = app
        .server
        .get(&format!("/api/library/{}/suggest/role", library.id()))
        .add_header("HX-Current-URL", format!("http://localhost{page}"))
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
    response.assert_header("HX-Redirect", format!("/login?next={page}"));
}

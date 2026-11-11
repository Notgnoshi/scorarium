use axum::http::StatusCode;
use scorarium_test::TestApp;

#[tokio::test]
async fn tag_pages_require_login_and_validate_the_tag() {
    let app = TestApp::logged_out(false).await;
    let library = app
        .engine
        .create_library("Public".parse().unwrap(), false)
        .await
        .unwrap();
    for path in [
        format!("/library/{}/tags", library.id()),
        format!("/library/{}/tag/piano", library.id()),
    ] {
        let response = app.server.get(&path).await;
        response.assert_status(StatusCode::SEE_OTHER);
        response.assert_header("location", format!("/login?next={path}"));
    }

    let app = TestApp::demo().await;
    let library = app
        .engine
        .create_library("Mine".parse().unwrap(), false)
        .await
        .unwrap();
    app.server
        .get(&format!("/library/{}/tags", library.id()))
        .await
        .assert_status_ok();
    app.server
        .get(&format!("/library/{}/tag/piano", library.id()))
        .await
        .assert_status_ok();
    app.server
        .get(&format!("/library/{}/tag/piano%21", library.id()))
        .await
        .assert_status_not_found();
}

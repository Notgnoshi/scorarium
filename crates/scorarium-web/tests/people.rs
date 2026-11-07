use axum::http::StatusCode;
use scorarium_test::TestApp;

#[tokio::test]
async fn people_pages_follow_library_visibility_and_validate_the_role() {
    let app = TestApp::logged_out(false).await;
    let mut library = app.engine.create_library("Secret", true).await.unwrap();
    for path in [
        format!("/library/{}/people", library.id()),
        format!("/library/{}/people/composer", library.id()),
    ] {
        let response = app.server.get(&path).await;
        response.assert_status(StatusCode::SEE_OTHER);
        response.assert_header("location", format!("/login?next={path}"));
    }

    library.set_private(false).await.unwrap();
    app.server
        .get(&format!("/library/{}/people", library.id()))
        .await
        .assert_status_ok();
    app.server
        .get(&format!("/library/{}/people/composer", library.id()))
        .await
        .assert_status_ok();
    app.server
        .get(&format!("/library/{}/people/%20", library.id()))
        .await
        .assert_status_not_found();
}

use axum::http::StatusCode;
use scorarium_test::TestApp;

#[tokio::test]
async fn create_requires_login_and_makes_a_private_library() {
    let app = TestApp::logged_out(true).await;
    let before = app.engine.libraries().await.unwrap().len();
    app.server
        .post("/library")
        .form(&[("name", "Books")])
        .await
        .assert_status(StatusCode::SEE_OTHER);
    assert_eq!(app.engine.libraries().await.unwrap().len(), before);

    let app = TestApp::demo().await;
    let response = app.server.post("/library").form(&[("name", "Books")]).await;
    response.assert_status(StatusCode::SEE_OTHER);
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    assert_eq!(library.name(), "Books");
    assert!(library.is_private());
    response.assert_header("location", format!("/library/{}", library.id()));
}

#[tokio::test]
async fn private_library_redirects_anonymous_to_login() {
    let app = TestApp::logged_out(false).await;
    let library = app.engine.create_library("Secret", true).await.unwrap();
    let path = format!("/library/{}", library.id());
    let response = app.server.get(&path).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", format!("/login?next={path}"));
    app.server
        .get("/library/999999")
        .await
        .assert_status_not_found();
    app.server
        .get("/library/abc")
        .await
        .assert_status_not_found();
}

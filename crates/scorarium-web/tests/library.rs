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

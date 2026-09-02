use axum::http::StatusCode;
use scorarium_tests::{TestDb, browser};

#[tokio::test]
async fn password_change_flow() {
    let server = browser(TestDb::new().password("hunter2").build().await);

    // The page requires login
    let response = server.get("/password").await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");

    let response = server.post("/login").form(&[("password", "hunter2")]).await;
    response.assert_status(StatusCode::SEE_OTHER);

    let response = server.get("/password").await;
    response.assert_status_ok();
    response.assert_text_contains("Change password");

    // The wrong current password doesn't change anything
    let response = server
        .post("/password")
        .form(&[
            ("current", "wrong"),
            ("new", "hunter3"),
            ("confirm", "hunter3"),
        ])
        .await;
    response.assert_status_ok();
    response.assert_text_contains("Wrong current password");

    let response = server
        .post("/password")
        .form(&[
            ("current", "hunter2"),
            ("new", "hunter3"),
            ("confirm", "hunter3"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/");

    // Only the new password logs in now
    server.post("/logout").await;
    let response = server.post("/login").form(&[("password", "hunter2")]).await;
    response.assert_status_ok();
    response.assert_text_contains("Login failed");
    let response = server.post("/login").form(&[("password", "hunter3")]).await;
    response.assert_status(StatusCode::SEE_OTHER);
}

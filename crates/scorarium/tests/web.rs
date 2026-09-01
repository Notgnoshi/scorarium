use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::router;
use scorarium_tests::TestDb;

/// A test server that, like a browser, saves cookies across requests.
fn browser(state: scorarium::AppState) -> TestServer {
    let mut server = TestServer::new(router(Arc::new(state)));
    server.save_cookies();
    server
}

#[tokio::test]
async fn index_lists_libraries() {
    let state = TestDb::new()
        .library("lib2-ASDF")
        .library("lib1-QWERT")
        .build()
        .await;
    let server = TestServer::new(router(Arc::new(state)));

    let response = server.get("/").await;
    response.assert_status_ok();
    response.assert_text_contains("lib1-QWERT");
    response.assert_text_contains("lib2-ASDF");
}

#[tokio::test]
async fn claim_flow() {
    let server = browser(TestDb::new().build().await);

    // With no password stored, the login page offers to set one
    let response = server.get("/login").await;
    response.assert_status_ok();
    response.assert_text_contains("Set password");

    // A typo'd confirmation must not claim the password
    let response = server
        .post("/login")
        .form(&[("password", "hunter2"), ("confirm", "hunter3")])
        .await;
    response.assert_status_ok();
    response.assert_text_contains("did not match");

    let response = server
        .post("/login")
        .form(&[("password", "hunter2"), ("confirm", "hunter2")])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/");

    // The claim also logged us in: the login page now redirects home
    let response = server.get("/login").await;
    response.assert_status(StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn login_logout_flow() {
    let server = browser(TestDb::new().password("hunter2").build().await);

    let response = server.post("/login").form(&[("password", "wrong")]).await;
    response.assert_status_ok();
    response.assert_text_contains("Login failed");

    let response = server.post("/login").form(&[("password", "hunter2")]).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/");

    // Logged in: the login page redirects home
    let response = server.get("/login").await;
    response.assert_status(StatusCode::SEE_OTHER);

    let response = server.post("/logout").await;
    response.assert_status(StatusCode::SEE_OTHER);

    // Logged out: the login page shows the form again
    let response = server.get("/login").await;
    response.assert_status_ok();
    response.assert_text_contains("Log in");
}

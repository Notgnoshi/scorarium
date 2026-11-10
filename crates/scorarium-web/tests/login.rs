use axum::http::StatusCode;
use scorarium_engine::auth::{PasswordState, Token};
use scorarium_test::TestApp;
use scorarium_web::SESSION_COOKIE;

#[tokio::test]
async fn claim_logs_in_and_logout_revokes() {
    let app = TestApp::logged_out(false).await;
    let response = app
        .server
        .post("/login")
        .form(&[("password", "hunter2"), ("confirm", "hunter2")])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        app.engine.password_state().await.unwrap(),
        PasswordState::Claimed
    );
    let token = Token::from(response.cookie(SESSION_COOKIE).value());
    assert!(app.engine.is_authenticated(&token));
    app.server
        .post("/logout")
        .await
        .assert_status(StatusCode::SEE_OTHER);
    assert!(!app.engine.is_authenticated(&token));
}

#[tokio::test]
async fn login_next_redirects_only_within_the_site() {
    let app = TestApp::logged_out(false).await;
    app.engine.replace_password("secret").await.unwrap();
    for (next, location) in [
        ("//evil.com", "/"),
        ("/\\evil.com", "/"),
        ("/\t/evil.com", "/"),
        ("/settings", "/settings"),
    ] {
        let response = app
            .server
            .post("/login")
            .form(&[("password", "secret"), ("next", next)])
            .await;
        response.assert_status(StatusCode::SEE_OTHER);
        response.assert_header("location", location);
        let token = Token::from(response.cookie(SESSION_COOKIE).value());
        assert!(app.engine.is_authenticated(&token));
    }
}

#[tokio::test]
async fn changing_the_password_keeps_only_the_new_session() {
    let app = TestApp::logged_out(false).await;
    app.engine.replace_password("old").await.unwrap();
    let response = app.server.post("/login").form(&[("password", "old")]).await;
    let old = Token::from(response.cookie(SESSION_COOKIE).value());

    let response = app
        .server
        .post("/settings/password")
        .form(&[("new", "new"), ("confirm", "typo")])
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    assert!(app.engine.is_authenticated(&old));
    app.engine.login("old").await.unwrap();

    let response = app
        .server
        .post("/settings/password")
        .form(&[("new", "new"), ("confirm", "new")])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/settings?changed=true");
    assert!(!app.engine.is_authenticated(&old));
    let new = Token::from(response.cookie(SESSION_COOKIE).value());
    assert!(app.engine.is_authenticated(&new));
    app.engine.login("new").await.unwrap();
}

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

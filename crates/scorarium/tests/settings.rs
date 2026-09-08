use axum::http::StatusCode;
use scorarium_archive::{HoldingKind, HoldingRawInput, PublicationRawInput, WorkRawInput};
use scorarium_tests::{TestDb, browser};

#[tokio::test]
async fn password_change_flow() {
    let server = browser(TestDb::new().password("hunter2").build().await);

    // The page requires login
    let response = server.get("/settings").await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");

    let response = server.post("/login").form(&[("password", "hunter2")]).await;
    response.assert_status(StatusCode::SEE_OTHER);

    let response = server.get("/settings").await;
    response.assert_status_ok();
    response.assert_text_contains("Change password");

    // The wrong current password doesn't change anything
    let response = server
        .post("/settings/password")
        .form(&[
            ("current", "wrong"),
            ("new", "hunter3"),
            ("confirm", "hunter3"),
        ])
        .await;
    response.assert_status_ok();
    response.assert_text_contains("Wrong current password");

    let response = server
        .post("/settings/password")
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

#[tokio::test]
async fn settings_lists_unrecognized_catalog_numbers() {
    let state = TestDb::new()
        .library("Sheet music")
        .password("hunter2")
        .build()
        .await;
    let library = state.archive.libraries().await.unwrap().remove(0);
    let publication = library
        .create_publication(
            &PublicationRawInput {
                title: "Haydn and Chopin".into(),
                holdings: vec![HoldingRawInput {
                    id: None,
                    kind: HoldingKind::Physical,
                    location: "Shelf".into(),
                }],
                contents: vec![
                    WorkRawInput {
                        title: "Sonata in E-flat".into(),
                        catalog_numbers: vec!["Hob. XVI:52".into()],
                        ..WorkRawInput::default()
                    },
                    WorkRawInput {
                        title: "Raindrop".into(),
                        catalog_numbers: vec!["Op. 28 No. 15".into()],
                        ..WorkRawInput::default()
                    },
                ],
                ..PublicationRawInput::default()
            }
            .parse()
            .unwrap(),
        )
        .await
        .unwrap();
    let sonata = publication
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.title == "Sonata in E-flat")
        .unwrap();
    let server = browser(state.clone());

    let response = server.get("/settings/catalog-numbers").await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");
    server.post("/login").form(&[("password", "hunter2")]).await;

    let response = server.get("/settings").await;
    response.assert_status_ok();
    response.assert_text_contains("Change password");
    response.assert_text_contains("1 unrecognized catalog number");

    let response = server.get("/settings/catalog-numbers").await;
    response.assert_status_ok();
    response.assert_text_contains("Hob. XVI:52");
    response.assert_text_contains("Sonata in E-flat");
    // askama's urlencode filter leaves "/" alone, as the existing work rows' back links show
    response.assert_text_contains(format!(
        "href=\"/library/{}/work/{}/edit?back=/settings/catalog-numbers\"",
        library.id, sonata.id
    ));
    assert!(!response.text().contains("Op. 28 No. 15"));
}

/// The demo logs everyone in, so it offers the settings page but not the password on it.
#[tokio::test]
async fn the_demo_has_settings_without_a_password() {
    let server = browser(TestDb::new().demo().build().await);

    // The page it leads to hides its own link, so the link is checked from somewhere else
    let response = server.get("/").await;
    response.assert_text_contains("href=\"/settings\"");

    let response = server.get("/settings").await;
    response.assert_status_ok();
    response.assert_text_contains("Catalog number schemes");
    assert!(!response.text().contains("Change password"));

    let response = server
        .post("/settings/password")
        .form(&[("current", ""), ("new", "hunter2"), ("confirm", "hunter2")])
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

use axum::http::StatusCode;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::NewHolding;
use scorarium_engine::publication::ImportSeed;
use scorarium_test::TestApp;

#[tokio::test]
async fn publication_page_follows_library_visibility_and_hides_drafts() {
    let app = TestApp::logged_out(true).await;
    let mut library = app.engine.create_library("Secret", true).await.unwrap();
    let draft = library
        .import(
            ImportSeed::Title {
                title: "Draft".into(),
                record: None,
            },
            NewHolding::Physical { location: None },
        )
        .await
        .unwrap();
    let path = format!("/library/{}/publication/{}", library.id(), draft.id());

    let response = app.server.get(&path).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", format!("/login?next={path}"));

    library.set_private(false).await.unwrap();
    app.server.get(&path).await.assert_status_not_found();

    let app = TestApp::demo().await;
    let library = app.engine.create_library("Mine", false).await.unwrap();
    let other = app.engine.create_library("Other", false).await.unwrap();
    let draft = library
        .import(
            ImportSeed::Title {
                title: "Draft".into(),
                record: None,
            },
            NewHolding::Physical { location: None },
        )
        .await
        .unwrap();
    app.server
        .get(&format!(
            "/library/{}/publication/{}",
            library.id(),
            draft.id()
        ))
        .await
        .assert_status_ok();
    app.server
        .get(&format!(
            "/library/{}/publication/{}",
            other.id(),
            draft.id()
        ))
        .await
        .assert_status_not_found();
}

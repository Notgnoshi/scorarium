use axum::http::StatusCode;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::NewHolding;
use scorarium_engine::publication::ImportSeed;
use scorarium_test::TestApp;

#[tokio::test]
async fn a_private_librarys_cover_needs_login() {
    let app = TestApp::demo().await;
    let library = app.engine.create_library("Secret", true).await.unwrap();
    let mut publication = library
        .import(
            ImportSeed::Title {
                title: "Gymnopedies".into(),
                record: None,
            },
            NewHolding::Physical { location: None },
        )
        .await
        .unwrap();
    let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0";
    publication
        .set_cover(AssetSource::Upload {
            data: Box::new(png),
            filename: "cover.png".into(),
        })
        .await
        .unwrap();
    let path = format!(
        "/library/{}/publication/{}/cover",
        library.id(),
        publication.id()
    );

    let response = app.server.get(&path).clear_cookies().await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", format!("/login?next={path}"));

    let response = app.server.get(&path).await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), png);
}

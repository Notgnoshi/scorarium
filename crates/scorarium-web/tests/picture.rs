use axum_test::multipart::{MultipartForm, Part};
use scorarium_engine::entity::Entity;
use scorarium_engine::publication::Publication;
use scorarium_test::TestApp;

#[tokio::test]
async fn a_cover_upload_is_stored_and_a_locked_demo_refuses_it() {
    let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0";
    for (app, expected) in [
        (TestApp::demo().await, true),
        (TestApp::locked_demo().await, false),
    ] {
        let library = app.engine.libraries().await.unwrap().pop().unwrap();
        let publication = library.publications().await.unwrap().pop().unwrap();
        let form = MultipartForm::new().add_part("file", Part::bytes(png).file_name("cover.png"));
        app.server
            .post(&format!(
                "/api/library/{}/publication/{}/cover/upload",
                library.id(),
                publication.id()
            ))
            .multipart(form)
            .await;
        let after = Publication::load(&app.engine, publication.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.cover().is_some(), expected);
    }
}

use axum_test::multipart::{MultipartForm, Part};
use scorarium_engine::asset::StagedId;
use scorarium_test::TestApp;
use serde::Deserialize;

#[derive(Deserialize)]
struct Staged {
    id: String,
}

#[tokio::test]
async fn a_staged_upload_stays_until_it_is_discarded() {
    let form = || {
        let file = Part::bytes(&b"%PDF-1.4 not really"[..]).file_name("score.pdf");
        MultipartForm::new().add_part("file", file)
    };

    let app = TestApp::demo().await;
    let response = app.server.post("/api/stage").multipart(form()).await;
    let Ok(id) = response.json::<Staged>().id.parse::<StagedId>();
    assert_eq!(app.engine.staged(&id).unwrap().filename, "score.pdf");

    app.server.post(&format!("/api/stage/{id}/discard")).await;
    assert!(app.engine.staged(&id).is_none());

    let locked = TestApp::locked_demo().await;
    let response = locked.server.post("/api/stage").multipart(form()).await;
    response.assert_status_failure();
}

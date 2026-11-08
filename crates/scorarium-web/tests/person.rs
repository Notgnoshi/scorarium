use axum::http::StatusCode;
use scorarium_engine::entity::Entity;
use scorarium_engine::person::Person;
use scorarium_test::TestApp;

#[tokio::test]
async fn edit_derives_the_sort_name() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let response = app
        .server
        .post(&format!(
            "/library/{}/person/{}/edit",
            library.id(),
            person.id()
        ))
        .form(&[("name", "Erik Satie"), ("sort_name", "")])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header(
        "location",
        format!("/library/{}/person/{}", library.id(), person.id()),
    );
    let person = Person::load(&app.engine, person.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(person.name(), "Erik Satie");
    assert_eq!(person.sort_name(), "Satie, Erik");
}

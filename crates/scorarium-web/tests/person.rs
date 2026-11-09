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

#[tokio::test]
async fn back_is_honored_only_for_same_site_paths() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let edit = format!("/library/{}/person/{}/edit", library.id(), person.id());
    let form = [("name", person.name()), ("sort_name", "")];
    let response = app
        .server
        .post(&format!("{edit}?back=https://example.com"))
        .form(&form)
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header(
        "location",
        format!("/library/{}/person/{}", library.id(), person.id()),
    );
    let back = format!("/library/{}", library.id());
    let response = app
        .server
        .post(&format!("{edit}?back={back}"))
        .form(&form)
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", back);
}

use axum::http::StatusCode;
use scorarium_engine::entity::Entity;
use scorarium_engine::publication::{Identifier, IdentifierKind, Publication};
use scorarium_engine::value::Year;
use scorarium_test::TestApp;

#[tokio::test]
async fn edit_applies_scalars_and_staged_identifiers_in_one_save() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let publication = library.publications().await.unwrap().pop().unwrap();
    app.server
        .post(&format!(
            "/library/{}/publication/{}/edit",
            library.id(),
            publication.id()
        ))
        .form(&[
            ("title", "Renamed"),
            ("publisher", ""),
            ("year", "1999"),
            ("note", ""),
            ("add_identifier", "isbn:9780793525041"),
        ])
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.title(), Some("Renamed"));
    assert_eq!(after.year().map(Year::get), Some(1999));
    let isbn = Identifier::new(IdentifierKind::Isbn, "978-0-7935-2504-1").unwrap();
    assert!(after.identifiers().contains(&isbn));
}

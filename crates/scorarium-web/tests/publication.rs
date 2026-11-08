use axum::http::StatusCode;
use scorarium_engine::entity::{Contributed, Entity};
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

#[tokio::test]
async fn adding_a_title_with_a_known_catalog_number_links_the_existing_work() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let works = library.works().await.unwrap();
    let etude = works.iter().find(|w| w.title() == "Etude-Tableau").unwrap();
    let rachmaninoff = etude.contributors()[0].person;
    let masterpieces = library
        .publications()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.title() == Some("Rachmaninoff masterpieces for solo piano"))
        .unwrap();
    app.server
        .post(&format!(
            "/api/library/{}/publication/{}/contents/add",
            library.id(),
            masterpieces.id()
        ))
        .form(&[
            ("name", "Sergei Rachmaninoff"),
            ("person", &rachmaninoff.to_string()),
            ("title", "Etude in A minor"),
            ("work", ""),
            ("catalog_number", "Op.39/2"),
            ("role", "composer"),
        ])
        .await
        .assert_status_ok();
    let contents = masterpieces.works().await.unwrap();
    assert!(contents.iter().any(|w| w.id() == etude.id()));
    assert_eq!(library.works().await.unwrap().len(), works.len());
}

#[tokio::test]
async fn range_add_creates_links_and_is_idempotent() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let works = library.works().await.unwrap();
    let polichinelle = works.iter().find(|w| w.title() == "Polichinelle").unwrap();
    let rachmaninoff = polichinelle.contributors()[0].person.to_string();
    let album = library
        .publications()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.title() == Some("Russian piano album"))
        .unwrap();
    let before = album.works().await.unwrap().len();
    let url = format!(
        "/api/library/{}/publication/{}/contents/add-range",
        library.id(),
        album.id()
    );
    let form = [
        ("name", "Sergei Rachmaninoff"),
        ("person", rachmaninoff.as_str()),
        ("title", "Prelude"),
        ("prefix", "Op. 3 No."),
        ("from", "1"),
        ("to", "5"),
        ("role", "composer"),
    ];
    app.server.post(&url).form(&form).await.assert_status_ok();
    let contents = album.works().await.unwrap();
    assert_eq!(contents.len(), before + 4);
    assert!(contents.iter().any(|w| w.id() == polichinelle.id()));
    assert_eq!(library.works().await.unwrap().len(), works.len() + 3);

    app.server.post(&url).form(&form).await.assert_status_ok();
    assert_eq!(album.works().await.unwrap().len(), before + 4);
    assert_eq!(library.works().await.unwrap().len(), works.len() + 3);
}

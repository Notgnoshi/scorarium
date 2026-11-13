use axum::http::StatusCode;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::holding::{HoldingKind, NewHolding};
use scorarium_engine::publication::{Identifier, IdentifierKind, ImportSeed, Publication};
use scorarium_engine::value::Year;
use scorarium_test::TestApp;

#[tokio::test]
async fn edit_applies_scalars_and_lists_in_one_save() {
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
            ("identifier[new1][kind]", "isbn"),
            ("identifier[new1][value]", "9780793525041"),
            ("contributor[new1][person]", "new"),
            ("contributor[new1][name]", "Created"),
            ("contributor[new1][role]", "editor"),
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
    assert!(
        after
            .contributors()
            .iter()
            .any(|c| c.name == "Created" && c.role.as_str() == "editor")
    );
}

#[tokio::test]
async fn edit_changes_the_kind_of_an_identifier_and_removes_another() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut publication = library.publications().await.unwrap().pop().unwrap();
    for identifier in publication.identifiers().to_vec() {
        publication.remove_identifier(&identifier).await.unwrap();
    }
    let isbn = Identifier::new(IdentifierKind::Isbn, "978-0-7935-2504-1").unwrap();
    let publisher = Identifier::new(IdentifierKind::PublisherNumber, "HL 123").unwrap();
    publication.add_identifier(isbn).await.unwrap();
    publication.add_identifier(publisher).await.unwrap();

    app.server
        .post(&format!(
            "/library/{}/publication/{}/edit",
            library.id(),
            publication.id()
        ))
        .form(&[
            ("title", "Renamed"),
            ("publisher", ""),
            ("year", ""),
            ("note", ""),
            ("identifier[0][saved][kind]", "isbn"),
            ("identifier[0][saved][value]", "978-0-7935-2504-1"),
            ("identifier[0][removed]", "true"),
            ("identifier[0][kind]", "isbn"),
            ("identifier[0][value]", "978-0-7935-2504-1"),
            ("identifier[1][saved][kind]", "publisher_number"),
            ("identifier[1][saved][value]", "HL 123"),
            ("identifier[1][kind]", "plate_number"),
            ("identifier[1][value]", "HL 123"),
        ])
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    let plate = Identifier::new(IdentifierKind::PlateNumber, "HL 123").unwrap();
    assert_eq!(after.identifiers(), [plate]);
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

#[tokio::test]
async fn edit_adds_holdings_before_removing_the_only_one() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let publication = library.publications().await.unwrap().pop().unwrap();
    let only = publication.holdings()[0].id.to_string();
    let pdf = AssetSource::Upload {
        data: Box::new(&b"%PDF-1.4 not really"[..]),
        filename: "score.pdf".into(),
    };
    let staged = app.engine.stage(pdf).await.unwrap().id.to_string();

    let edit = format!(
        "/library/{}/publication/{}/edit",
        library.id(),
        publication.id()
    );
    let form = [
        ("title", publication.title().unwrap()),
        ("publisher", ""),
        ("year", ""),
        ("note", ""),
        ("holding[0][saved][id]", &only),
        ("holding[0][removed]", "true"),
        ("holding[new1][location]", "Piano bench"),
        ("holding[new2][kind]", "physical"),
        ("holding[new2][location]", "Music room"),
        ("holding[new3][kind]", "digital"),
        ("holding[new3][staged]", &staged),
        ("holding[new4][kind]", "digital"),
    ];
    app.server.post(&edit).form(&form).await;

    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    let physical = |location: &str| HoldingKind::Physical {
        location: Some(location.into()),
    };
    let kinds: Vec<&HoldingKind> = after.holdings().iter().map(|h| &h.kind).collect();
    assert_eq!(kinds.len(), 3);
    assert_eq!(*kinds[0], physical("Piano bench"));
    assert_eq!(*kinds[1], physical("Music room"));
    assert!(matches!(kinds[2], HoldingKind::Digital { file } if file.ends_with("score.pdf")));
}

#[tokio::test]
async fn edit_refuses_to_remove_every_holding() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let publication = library.publications().await.unwrap().pop().unwrap();
    let only = publication.holdings()[0].id.to_string();

    let edit = format!(
        "/library/{}/publication/{}/edit",
        library.id(),
        publication.id()
    );
    let form = [
        ("title", "Renamed"),
        ("publisher", ""),
        ("year", ""),
        ("note", ""),
        ("holding[0][saved][id]", &only),
        ("holding[0][removed]", "true"),
    ];
    app.server.post(&edit).form(&form).await;

    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.holdings().len(), 1);
    assert_eq!(after.title(), publication.title());
}

#[tokio::test]
async fn commit_saves_the_form_and_then_commits() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let seed = ImportSeed::Title {
        title: "Gymnopedies".into(),
        record: None,
    };
    let draft = library
        .import(seed, NewHolding::Physical { location: None })
        .await
        .unwrap();
    let url = format!(
        "/library/{}/publication/{}/commit",
        library.id(),
        draft.id()
    );
    let load = async || {
        Publication::load(&app.engine, draft.id())
            .await
            .unwrap()
            .unwrap()
    };
    let form = |title| {
        [
            ("title", title),
            ("publisher", ""),
            ("year", ""),
            ("note", ""),
        ]
    };

    app.server.post(&url).form(&form(" ")).await;
    assert!(load().await.is_draft());

    app.server
        .post(&url)
        .form(&form("Renamed"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = load().await;
    assert!(!after.is_draft());
    assert_eq!(after.title(), Some("Renamed"));
}

#[tokio::test]
async fn edit_clears_the_location_of_the_only_holding() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut publication = library.publications().await.unwrap().pop().unwrap();
    let only = publication.holdings()[0].id;
    publication
        .set_physical_holding_location(only, Some("Piano bench"))
        .await
        .unwrap();

    let edit = format!(
        "/library/{}/publication/{}/edit",
        library.id(),
        publication.id()
    );
    let form = [
        ("title", publication.title().unwrap()),
        ("publisher", ""),
        ("year", ""),
        ("note", ""),
        ("holding[0][saved][id]", &only.to_string()),
        ("holding[0][location]", ""),
    ];
    app.server
        .post(&edit)
        .form(&form)
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.holdings()[0].kind,
        HoldingKind::Physical { location: None }
    );
}

#[tokio::test]
async fn edit_refuses_a_file_field_that_holds_text() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let publication = library.publications().await.unwrap().pop().unwrap();
    let edit = format!(
        "/library/{}/publication/{}/edit",
        library.id(),
        publication.id()
    );
    let form = [
        ("title", "Renamed"),
        ("publisher", ""),
        ("year", ""),
        ("note", ""),
        ("holding[new1][kind]", "digital"),
        ("holding[new1][url]", "https://example.com/score.pdf"),
    ];
    app.server
        .post(&edit)
        .form(&form)
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.title(), publication.title());
    assert_eq!(after.holdings(), publication.holdings());
}

#[tokio::test]
async fn edit_ignores_a_holding_that_the_library_no_longer_has() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let publication = library.publications().await.unwrap().pop().unwrap();
    let edit = format!(
        "/library/{}/publication/{}/edit",
        library.id(),
        publication.id()
    );
    let only = publication.holdings()[0].id;
    let gone = (only.get() + 1000).to_string();
    let form = [
        ("title", publication.title().unwrap()),
        ("publisher", ""),
        ("year", ""),
        ("note", ""),
        ("holding[0][saved][id]", &only.to_string()),
        ("holding[0][location]", "Piano bench"),
        ("holding[1][saved][id]", &gone),
        ("holding[1][removed]", "true"),
    ];
    app.server
        .post(&edit)
        .form(&form)
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = Publication::load(&app.engine, publication.id())
        .await
        .unwrap()
        .unwrap();
    let kinds: Vec<&HoldingKind> = after.holdings().iter().map(|h| &h.kind).collect();
    assert_eq!(
        kinds,
        [&HoldingKind::Physical {
            location: Some("Piano bench".into())
        }]
    );
}

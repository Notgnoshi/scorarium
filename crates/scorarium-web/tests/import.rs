use axum::http::StatusCode;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::HoldingKind;
use scorarium_engine::library::Library;
use scorarium_engine::publication::{Identifier, IdentifierKind, Publication};
use scorarium_test::TestApp;

async fn newest(library: &Library) -> Publication {
    library.publications().await.unwrap().pop().unwrap()
}

async fn holdings(library: &Library) -> Vec<HoldingKind> {
    let publication = newest(library).await;
    publication
        .holdings()
        .iter()
        .map(|h| h.kind.clone())
        .collect()
}

#[tokio::test]
async fn import_follows_the_holdings_rules() {
    let app = TestApp::demo().await;
    let library = app
        .engine
        .create_library("Sheet music", false)
        .await
        .unwrap();
    let url = format!("/library/{}/import", library.id());
    let stage = async |name: &str| {
        let source = AssetSource::Upload {
            data: Box::new(&b"%PDF-1.4 not really"[..]),
            filename: name.into(),
        };
        app.engine.stage(source).await.unwrap().id.to_string()
    };

    app.server.post(&url).form(&[("text", "Gymnopedies")]).await;
    assert_eq!(
        holdings(&library).await,
        [HoldingKind::Physical { location: None }]
    );

    let typed = [
        ("text", "Gnossiennes"),
        ("kind", "physical"),
        ("location", "Shelf 3"),
    ];
    app.server.post(&url).form(&typed).await;
    assert_eq!(
        holdings(&library).await,
        [HoldingKind::Physical {
            location: Some("Shelf 3".into())
        }]
    );

    let (score, parts) = (stage("score.pdf").await, stage("parts.pdf").await);
    let files = [
        ("text", "Sarabandes"),
        ("kind", "digital"),
        ("staged", score.as_str()),
        ("staged", parts.as_str()),
    ];
    app.server.post(&url).form(&files).await;
    assert!(matches!(
        holdings(&library).await.as_slice(),
        [HoldingKind::Digital { .. }, HoldingKind::Digital { .. }]
    ));

    // Digital with no file is refused, so no fourth draft is created.
    let nothing = [("text", "Nocturnes"), ("kind", "digital")];
    app.server.post(&url).form(&nothing).await;
    assert_eq!(library.publications().await.unwrap().len(), 3);
}

#[tokio::test]
async fn import_takes_an_identifier_or_a_title() {
    let app = TestApp::demo().await;
    let library = app
        .engine
        .create_library("Sheet music", false)
        .await
        .unwrap();
    let url = format!("/library/{}/import", library.id());

    app.server
        .post(&url)
        .form(&[("text", "9780486253923")])
        .await;
    let draft = newest(&library).await;
    let isbn = Identifier::new(IdentifierKind::Isbn, "978-0-486-25392-3").unwrap();
    assert!(draft.is_draft());
    assert_eq!(draft.title(), None);
    assert_eq!(draft.identifiers(), [isbn]);

    app.server.post(&url).form(&[("text", "Gymnopedies")]).await;
    assert_eq!(newest(&library).await.title(), Some("Gymnopedies"));
}

#[tokio::test]
async fn import_more_keeps_locations_and_drops_files() {
    let app = TestApp::demo().await;
    let library = app
        .engine
        .create_library("Sheet music", false)
        .await
        .unwrap();
    let url = format!("/library/{}/import", library.id());
    let source = AssetSource::Upload {
        data: Box::new(&b"%PDF-1.4 not really"[..]),
        filename: "score.pdf".into(),
    };
    let staged = app.engine.stage(source).await.unwrap().id.to_string();

    let form = [
        ("text", "Gymnopedies"),
        ("more", "on"),
        ("kind", "physical"),
        ("add_location", "Attic"),
        ("staged", staged.as_str()),
    ];
    let response = app.server.post(&url).form(&form).await;

    response.assert_status(StatusCode::SEE_OTHER);
    let location = response.header("location");
    let location = location.to_str().unwrap();
    assert!(location.starts_with(&format!("{url}?")), "{location}");
    assert!(location.contains("add_location=Attic"), "{location}");
    assert!(!location.contains(&staged), "{location}");
}

#[tokio::test]
async fn import_attaches_a_picked_candidates_record() {
    let app = TestApp::demo().await;
    let library = app.engine.create_library("Books", false).await.unwrap();
    let url = format!("/library/{}/import", library.id());
    let record = "https://openlibrary.org/books/OL7636066M";

    app.server
        .post(&url)
        .form(&[("text", "Pro Git"), ("record", record)])
        .await;
    let draft = newest(&library).await;
    let links: Vec<String> = draft
        .links()
        .iter()
        .map(|link| link.url.to_string())
        .collect();
    assert_eq!(links, [record]);
}

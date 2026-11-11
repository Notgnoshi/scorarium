use axum::http::StatusCode;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::NewHolding;
use scorarium_engine::publication::ImportSeed;
use scorarium_test::TestApp;

#[tokio::test]
async fn a_private_librarys_cover_needs_login() {
    let app = TestApp::demo().await;
    let library = app
        .engine
        .create_library("Secret".parse().unwrap(), true)
        .await
        .unwrap();
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

#[tokio::test]
async fn an_html_holding_downloads_and_a_pdf_or_text_displays() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut publication = library.publications().await.unwrap().pop().unwrap();
    for (name, disposition) in [
        ("page.html", "attachment; filename*=UTF-8''page.html"),
        ("score.pdf", "inline; filename*=UTF-8''score.pdf"),
        ("notes.md", "inline; filename*=UTF-8''notes.md"),
    ] {
        let holding = publication
            .add_holding(NewHolding::Digital(AssetSource::Upload {
                data: Box::new(&b"<script>alert(1)</script>"[..]),
                filename: name.into(),
            }))
            .await
            .unwrap();
        let path = format!(
            "/library/{}/publication/{}/holding/{holding}",
            library.id(),
            publication.id()
        );
        let response = app.server.get(&path).await;
        response.assert_status_ok();
        response.assert_header("content-disposition", disposition);

        // A browser replaces its cached copy's headers with a 304's, so a disposition here would
        // overwrite the one chosen above.
        let revalidated = app
            .server
            .get(&path)
            .add_header("if-none-match", response.header("etag"))
            .await;
        revalidated.assert_status(StatusCode::NOT_MODIFIED);
        assert_eq!(revalidated.maybe_header("content-disposition"), None);
    }
}

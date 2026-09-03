use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::{db, router};
use scorarium_tests::TestDb;

#[tokio::test]
async fn publication_page() {
    let state = TestDb::new().demo().build().await;
    let libraries = db::list_libraries(&state.pool).await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = db::publication::list(&state.pool, sheet_music.id)
        .await
        .unwrap();
    let gymnopedies = publications
        .iter()
        .find(|p| p.title.starts_with("Three gymnopedies"))
        .unwrap();
    let works = db::work::list_in_publication(&state.pool, sheet_music.id, gymnopedies.id)
        .await
        .unwrap();
    let gymnopedie = works
        .iter()
        .find(|w| w.title == "Gymnopedie No. 1")
        .unwrap();
    let book_publications = db::publication::list(&state.pool, books.id).await.unwrap();
    let practical_vim = book_publications
        .iter()
        .find(|p| p.title == "Practical Vim")
        .unwrap();
    let bierce_writings = book_publications
        .iter()
        .find(|p| p.title.ends_with("Ambrose Bierce"))
        .unwrap();
    let server = TestServer::new(router(Arc::new(state)));

    let response = server
        .get(&format!(
            "/library/{}/publication/{}",
            sheet_music.id, gymnopedies.id
        ))
        .await;
    response.assert_status_ok();
    let work_href = format!(
        "href=\"/library/{}/work/{}\"",
        sheet_music.id, gymnopedie.id
    );
    for expected in [
        "Three gymnopedies for the piano",
        "Schirmer",
        "978-0-7935-2590-4",
        "VOL 1869",
        "Erik Satie",
        "composer",
        "physical",
        "Piano bench",
        // The breadcrumb back to the library
        "Sheet music",
        "Gymnopedie No. 1",
        work_href.as_str(),
    ] {
        response.assert_text_contains(expected);
    }
    // Satie is credited on the publication and on every work, so his page is linked from both the
    // contributors list and the works table
    let satie_href = format!(
        "href=\"/library/{}/person/{}\"",
        sheet_music.id, gymnopedies.contributors[0].person_id
    );
    assert!(response.text().matches(&satie_href).count() >= 2);
    // None of the gymnopedies has a catalog number, so the column is left out
    assert!(!response.text().contains("Catalog numbers"));
    assert!(!response.text().contains("Author"));

    // A book's works have authors, not composers, and never catalog numbers
    let response = server
        .get(&format!(
            "/library/{}/publication/{}",
            books.id, bierce_writings.id
        ))
        .await;
    response.assert_status_ok();
    response.assert_text_contains("The Parenticide Club");
    response.assert_text_contains("Author");
    assert!(!response.text().contains("Composer"));
    assert!(!response.text().contains("Catalog numbers"));

    // A publication without works hides the section rather than showing an empty table
    let response = server
        .get(&format!(
            "/library/{}/publication/{}",
            books.id, practical_vim.id
        ))
        .await;
    response.assert_status_ok();
    assert!(!response.text().contains("Works"));

    // A publication is only reachable through its own library
    let response = server
        .get(&format!(
            "/library/{}/publication/{}",
            books.id, gymnopedies.id
        ))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let missing = publications.iter().map(|p| p.id).max().unwrap() + 1;
    let response = server
        .get(&format!(
            "/library/{}/publication/{missing}",
            sheet_music.id
        ))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

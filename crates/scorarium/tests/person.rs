use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::{db, router};
use scorarium_tests::TestDb;

#[tokio::test]
async fn person_page() {
    let state = TestDb::new().demo().build().await;
    let libraries = db::list_libraries(&state.pool).await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = db::publication::list(&state.pool, sheet_music.id)
        .await
        .unwrap();
    let album = publications
        .iter()
        .find(|p| p.title == "Russian piano album")
        .unwrap();
    let masterpieces = publications
        .iter()
        .find(|p| p.title.starts_with("Rachmaninoff masterpieces"))
        .unwrap();
    let album_works = db::work::list_in_publication(&state.pool, sheet_music.id, album.id)
        .await
        .unwrap();
    let prelude = album_works
        .iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let masterpieces_works =
        db::work::list_in_publication(&state.pool, sheet_music.id, masterpieces.id)
            .await
            .unwrap();
    let polichinelle = masterpieces_works
        .iter()
        .find(|w| w.title == "Polichinelle")
        .unwrap();
    let person_id = |name: &str| {
        album
            .contributors
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .person_id
    };
    let rachmaninoff = person_id("Sergei Rachmaninoff");
    let kabalevsky = person_id("Dmitri Kabalevsky");
    let server = TestServer::new(router(Arc::new(state)));

    let response = server
        .get(&format!(
            "/library/{}/person/{rachmaninoff}",
            sheet_music.id
        ))
        .await;
    response.assert_status_ok();
    let publication_href =
        |id: i64| format!("href=\"/library/{}/publication/{id}\"", sheet_music.id);
    let work_href = |id: i64| format!("href=\"/library/{}/work/{id}\"", sheet_music.id);
    for expected in [
        "Sergei Rachmaninoff",
        "Russian piano album",
        publication_href(album.id).as_str(),
        "Rachmaninoff masterpieces for solo piano",
        publication_href(masterpieces.id).as_str(),
        "Prelude in C-sharp minor",
        work_href(prelude.id).as_str(),
        "Etude-Tableau",
        "Polichinelle",
        work_href(polichinelle.id).as_str(),
        "composer",
        // The breadcrumb back to the library
        "Sheet music",
    ] {
        response.assert_text_contains(expected);
    }

    // Credited on the anthology, but none of his pieces are entered as works
    let response = server
        .get(&format!("/library/{}/person/{kabalevsky}", sheet_music.id))
        .await;
    response.assert_status_ok();
    response.assert_text_contains("Russian piano album");
    response.assert_text_contains("composer");
    assert!(!response.text().contains("Prelude in C-sharp minor"));

    // A person is only reachable through their own library
    let response = server
        .get(&format!("/library/{}/person/{rachmaninoff}", books.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let missing = album
        .contributors
        .iter()
        .map(|c| c.person_id)
        .max()
        .unwrap()
        + 100;
    let response = server
        .get(&format!("/library/{}/person/{missing}", sheet_music.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn composers_and_authors_pages() {
    let state = TestDb::new().demo().build().await;
    let libraries = db::list_libraries(&state.pool).await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap().id;
    let sheet_music = libraries
        .iter()
        .find(|l| l.name == "Sheet music")
        .unwrap()
        .id;
    let composers = db::person::list_with_role(&state.pool, sheet_music, "composer")
        .await
        .unwrap();
    let server = TestServer::new(router(Arc::new(state)));

    let response = server
        .get(&format!("/library/{sheet_music}/composers"))
        .await;
    response.assert_status_ok();
    // Kabalevsky is credited only on a publication, the others on works too
    for name in ["Sergei Rachmaninoff", "Erik Satie", "Dmitri Kabalevsky"] {
        let person = composers.iter().find(|p| p.name == name).unwrap();
        response.assert_text_contains(name);
        response.assert_text_contains(format!(
            "href=\"/library/{sheet_music}/person/{}\"",
            person.id
        ));
    }
    assert!(!response.text().contains("Ambrose Bierce"));

    let response = server.get(&format!("/library/{books}/authors")).await;
    response.assert_status_ok();
    for name in ["Ambrose Bierce", "Drew Neil", "Scott Chacon", "Ben Straub"] {
        response.assert_text_contains(name);
    }

    // No authors in the sheet music library: an empty page, not an error
    let response = server.get(&format!("/library/{sheet_music}/authors")).await;
    response.assert_status_ok();
    assert!(!response.text().contains("/person/"));

    let missing = libraries.iter().map(|l| l.id).max().unwrap() + 1;
    for listing in ["composers", "authors"] {
        let response = server.get(&format!("/library/{missing}/{listing}")).await;
        response.assert_status(StatusCode::NOT_FOUND);
    }
}

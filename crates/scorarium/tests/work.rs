use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::router;
use scorarium_tests::{TestDb, browser};

#[tokio::test]
async fn work_page() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = sheet_music.publications().await.unwrap();
    let album = publications
        .iter()
        .find(|p| p.title == "Russian piano album")
        .unwrap();
    let masterpieces = publications
        .iter()
        .find(|p| p.title.starts_with("Rachmaninoff masterpieces"))
        .unwrap();
    let works = album.works().await.unwrap();
    let prelude = works
        .iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let server = TestServer::new(router(state));

    let response = server
        .get(&format!("/library/{}/work/{}", sheet_music.id, prelude.id))
        .await;
    response.assert_status_ok();
    let album_href = format!(
        "href=\"/library/{}/publication/{}\"",
        sheet_music.id, album.id
    );
    let masterpieces_href = format!(
        "href=\"/library/{}/publication/{}\"",
        sheet_music.id, masterpieces.id
    );
    let composer_href = format!(
        "href=\"/library/{}/person/{}\"",
        sheet_music.id, prelude.contributors[0].person_id
    );
    for expected in [
        "Prelude in C-sharp minor",
        "Op. 3 No. 2",
        "C-sharp minor",
        "piano",
        "Sergei Rachmaninoff",
        composer_href.as_str(),
        "composer",
        // The work is in two publications, both of which are listed
        "Russian piano album",
        album_href.as_str(),
        "Rachmaninoff masterpieces for solo piano",
        masterpieces_href.as_str(),
        "physical",
        // The breadcrumb back to the library
        "Sheet music",
    ] {
        response.assert_text_contains(expected);
    }

    // A work is only reachable through its own library
    let response = server
        .get(&format!("/library/{}/work/{}", books.id, prelude.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let missing = works.iter().map(|w| w.id).max().unwrap() + 100;
    let response = server
        .get(&format!("/library/{}/work/{missing}", sheet_music.id))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn work_edit_saves_catalog_numbers() {
    let state = TestDb::new().demo().password("hunter2").build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = sheet_music.publications().await.unwrap();
    let album = publications
        .iter()
        .find(|p| p.title == "Russian piano album")
        .unwrap();
    let prelude = album
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let server = browser(state.clone());
    server.post("/login").form(&[("password", "hunter2")]).await;
    let view = format!("/library/{}/work/{}", sheet_music.id, prelude.id);
    let edit = format!("{view}/edit");

    // The stored number opens in the form, marked recognized
    let response = server.get(&edit).await;
    response.assert_status_ok();
    response.assert_text_contains("value=\"Op. 3 No. 2\"");
    response.assert_text_contains("Recognized catalog number scheme");

    let response = server
        .post(&edit)
        .form(&[
            ("title", "Prelude in C-sharp minor"),
            ("key", "C-sharp minor"),
            ("time_signature", ""),
            ("instrumentation", "piano"),
            ("catalog_number", "Op. 3 No. 2"),
            ("catalog_number", "Morceaux de fantaisie II"),
            ("contributor_name", "Sergei Rachmaninoff"),
            ("contributor_role", "composer"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", &view);

    let response = server.get(&view).await;
    response.assert_text_contains("Op. 3 No. 2");
    response.assert_text_contains("Morceaux de fantaisie II");
    response.assert_text_contains("Unrecognized catalog number scheme");
}

/// Two works of the same composer cannot hold the same catalog number, so editing one onto the
/// other's number folds it in and the edit lands on the survivor.
#[tokio::test]
async fn editing_a_work_onto_another_number_merges_them() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = sheet_music.publications().await.unwrap();
    let album = publications
        .iter()
        .find(|p| p.title == "Russian piano album")
        .unwrap();
    let masterpieces = publications
        .iter()
        .find(|p| p.title.starts_with("Rachmaninoff masterpieces"))
        .unwrap();
    let prelude = album
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let polichinelle = masterpieces
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.title == "Polichinelle")
        .unwrap();
    let server = browser(state.clone());

    let response = server
        .post(&format!(
            "/library/{}/work/{}/edit",
            sheet_music.id, polichinelle.id
        ))
        .form(&[
            ("title", "Polichinelle"),
            ("key", "F-sharp minor"),
            ("time_signature", ""),
            ("instrumentation", "piano"),
            // The prelude's number, under the same composer
            ("catalog_number", "Op. 3 No. 2"),
            ("contributor_name", "Sergei Rachmaninoff"),
            ("contributor_role", "composer"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header(
        "location",
        &format!("/library/{}/work/{}", sheet_music.id, prelude.id),
    );

    let response = server
        .get(&format!(
            "/library/{}/work/{}",
            sheet_music.id, polichinelle.id
        ))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

/// Save and Cancel return to the page that opened the edit page, but only when it is a page here.
#[tokio::test]
async fn the_edit_page_returns_where_it_came_from() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = sheet_music.publications().await.unwrap();
    let album = publications
        .iter()
        .find(|p| p.title == "Russian piano album")
        .unwrap();
    let prelude = album.works().await.unwrap().remove(0);
    let server = browser(state.clone());
    let view = format!("/library/{}/work/{}", sheet_music.id, prelude.id);

    let response = server
        .get(&format!("{view}/edit"))
        .add_query_param("back", "/settings/catalog-numbers")
        .await;
    response.assert_text_contains("href=\"/settings/catalog-numbers\"");

    // A protocol-relative URL leads off this site, and browsers read a backslash as a slash
    for back in ["//evil.example", "/\\evil.example"] {
        let response = server
            .get(&format!("{view}/edit"))
            .add_query_param("back", back)
            .await;
        response.assert_text_contains(format!("href=\"{view}\""));
        assert!(!response.text().contains("evil.example"));
    }
}

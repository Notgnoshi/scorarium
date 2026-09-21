use axum::http::StatusCode;
use scorarium_tests::{TestDb, browser, demo_login};

#[tokio::test]
async fn person_page() {
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
    let album_works = album.works().await.unwrap();
    let prelude = album_works
        .iter()
        .find(|w| w.title == "Prelude in C-sharp minor")
        .unwrap();
    let masterpieces_works = masterpieces.works().await.unwrap();
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
    // The Books library is private
    let server = browser(state);
    demo_login(&server).await;

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
async fn person_edit_renames_and_links() {
    let state = TestDb::new().demo().build().await;
    let library = state
        .archive
        .libraries()
        .await
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Sheet music")
        .unwrap();
    let kabalevsky = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Dmitri Kabalevsky")
        .unwrap();
    let view = format!("/library/{}/person/{}", library.id, kabalevsky.id);
    let edit = format!("{view}/edit");
    let imslp = "https://imslp.org/wiki/Category:Vaughan_Williams,_Ralph";
    let henle = "https://www.henle.de/en/";
    let server = browser(state);

    // Editing requires login, and the button that leads there is hidden until then
    let response = server.get(&edit).await;
    response.assert_status(StatusCode::SEE_OTHER);
    let response = server.get(&view).await;
    assert!(!response.text().contains(&format!("href=\"{edit}\"")));
    demo_login(&server).await;
    let response = server.get(&view).await;
    response.assert_text_contains(format!("href=\"{edit}\""));

    // The form opens on the stored name
    let response = server.get(&edit).await;
    response.assert_status_ok();
    response.assert_text_contains("value=\"Dmitri Kabalevsky\"");

    // A rejected submission comes back with its message, having changed nothing
    let response = server.post(&edit).form(&[("name", "  ")]).await;
    response.assert_status_ok();
    response.assert_text_contains(scorarium_archive::ValidationError::NameRequired.to_string());
    assert_eq!(
        library.person(kabalevsky.id).await.unwrap().unwrap().name,
        "Dmitri Kabalevsky"
    );

    let response = server
        .post(&edit)
        .form(&[
            ("name", "Ralph Vaughan Williams"),
            ("link", imslp),
            ("link", henle),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", &view);

    let response = server.get(&view).await;
    response.assert_status_ok();
    for expected in [
        "Ralph Vaughan Williams",
        &format!("href=\"{imslp}\""),
        &format!("href=\"{henle}\""),
        "IMSLP",
        "henle.de",
    ] {
        response.assert_text_contains(expected);
    }

    // The sort name is derived from the name, so the listing is where a rename shows up. The
    // heuristic takes the last word as the surname, so this one now sorts under W, at the end.
    let response = server
        .get(&format!("/library/{}/composers", library.id))
        .await;
    response.assert_status_ok();
    let body = response.text();
    let renamed = body.find("Ralph Vaughan Williams").unwrap();
    let last_other = body.find("Pyotr Ilyich Tchaikovsky").unwrap();
    assert!(renamed > last_other, "the renamed composer sorts under W");
}

#[tokio::test]
async fn composers_and_authors_pages() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap().id;
    let library = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let sheet_music = library.id;
    let composers = library.persons_with_role("composer").await.unwrap();
    // The Books library is private
    let server = browser(state);
    demo_login(&server).await;

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
    // Satie is credited on the publication and each of its three works
    response.assert_text_contains("Three gymnopedies for the piano +3");
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

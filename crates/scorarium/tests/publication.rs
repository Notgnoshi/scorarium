use axum::http::StatusCode;
use axum_test::TestServer;
use scorarium::router;
use scorarium_archive::{
    ContributorInput, HoldingKind, HoldingRawInput, IdentifierRawInput, PublicationRawInput,
    WorkRawInput,
};
use scorarium_tests::{TestDb, browser};

#[tokio::test]
async fn publication_page() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap();
    let sheet_music = libraries.iter().find(|l| l.name == "Sheet music").unwrap();
    let publications = sheet_music.publications().await.unwrap();
    let gymnopedies = publications
        .iter()
        .find(|p| p.title.starts_with("Three gymnopedies"))
        .unwrap();
    let works = gymnopedies.works().await.unwrap();
    let gymnopedie = works
        .iter()
        .find(|w| w.title == "Gymnopedie No. 1")
        .unwrap();
    let book_publications = books.publications().await.unwrap();
    let practical_vim = book_publications
        .iter()
        .find(|p| p.title == "Practical Vim")
        .unwrap();
    let bierce_writings = book_publications
        .iter()
        .find(|p| p.title.ends_with("Ambrose Bierce"))
        .unwrap();
    let server = TestServer::new(router(state));

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

#[tokio::test]
async fn publication_edit_flow() {
    let state = TestDb::new()
        .library("Books")
        .password("hunter2")
        .build()
        .await;
    let library = state.archive.libraries().await.unwrap().remove(0);
    let author = ContributorInput {
        name: "Drew Neil".into(),
        role: "author".into(),
    };
    let input = PublicationRawInput {
        title: "Practial Vim".into(),
        holdings: vec![HoldingRawInput {
            id: None,
            kind: HoldingKind::Physical,
            location: "Desk".into(),
        }],
        identifiers: vec![IdentifierRawInput {
            kind: "isbn".into(),
            value: "978-1-68050-127-8".into(),
        }],
        contributors: vec![author.clone()],
        contents: vec![
            WorkRawInput {
                title: "Chapter One".into(),
                // A second credit the publication form cannot show, so it has to say there is one
                contributors: vec![
                    author.clone(),
                    ContributorInput {
                        name: "Marion Wenz".into(),
                        role: "translator".into(),
                    },
                ],
                ..WorkRawInput::default()
            },
            WorkRawInput {
                title: "Chapter Two".into(),
                contributors: vec![author],
                ..WorkRawInput::default()
            },
        ],
        ..PublicationRawInput::default()
    };
    let stored = library
        .create_publication(&input.parse().unwrap())
        .await
        .unwrap();
    let publication = stored.id;
    let shelf = stored.holdings[0].id;
    let solo = stored.contributors[0].person_id;
    let chapters = stored.works().await.unwrap();
    let (one, two) = (chapters[0].id, chapters[1].id);
    let library_id = library.id;
    let server = browser(state.clone());
    let view = format!("/library/{library_id}/publication/{publication}");
    let edit = format!("{view}/edit");

    // Editing and deleting require login, and the button that leads there is hidden until then
    let response = server.get(&edit).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");
    let response = server.post(&format!("{view}/delete")).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");
    let response = server.get(&view).await;
    assert!(!response.text().contains(&format!("href=\"{edit}\"")));
    server.post("/login").form(&[("password", "hunter2")]).await;
    let response = server.get(&view).await;
    response.assert_text_contains(format!("href=\"{edit}\""));

    // The form opens on the stored values, with the empty ones shown rather than hidden
    let response = server.get(&edit).await;
    response.assert_status_ok();
    response.assert_text_contains("value=\"Practial Vim\"");
    response.assert_text_contains("978-1-68050-127-8");
    response.assert_text_contains("Drew Neil");
    response.assert_text_contains("id=\"publisher\"");
    response.assert_text_contains("value=\"Chapter One\"");
    response.assert_text_contains(format!("name=\"work_id\" value=\"{one}\""));
    response.assert_text_contains("and 1 more");

    // A rejected submission comes back with its message, having changed nothing
    let response = server
        .post(&edit)
        .form(&[
            ("title", "Practical Vim"),
            ("publisher", ""),
            ("year", "recently"),
            ("holding_id_0", &shelf.to_string()),
            ("holding_kind_0", "physical"),
            ("holding_location_0", "Desk"),
            ("holding_file_0", ""),
        ])
        .await;
    response.assert_status_ok();
    response.assert_text_contains("The year must be a number.");
    let stored = library.publication(publication).await.unwrap().unwrap();
    assert_eq!(stored.title, "Practial Vim");

    // A copy the form still names keeps its identity; the contributor dropped here is credited
    // nowhere else, so the person goes with the edit
    let response = server
        .post(&edit)
        .form(&[
            ("title", "Practical Vim"),
            ("publisher", "Pragmatic Bookshelf"),
            ("year", "2015"),
            ("holding_id_0", &shelf.to_string()),
            ("holding_id_1", ""),
            ("holding_kind_0", "physical"),
            ("holding_kind_1", "digital"),
            ("holding_location_0", "Piano bench"),
            ("holding_location_1", ""),
            ("holding_file_0", ""),
            ("holding_file_1", "practical-vim.pdf"),
            ("identifier_kind", "isbn"),
            ("identifier_value", "978-1-68050-127-8"),
            ("contributor_name", "Tim Pope"),
            ("contributor_role", "editor"),
            // Retitle one chapter, drop the other, and add a work
            ("work_id", &one.to_string()),
            ("work_id", ""),
            ("work_title", "Chapter 1"),
            ("work_title", "Appendix"),
            ("work_contributor_name", "Drew Neil"),
            ("work_contributor_name", "Tim Pope"),
            ("work_contributor_role", "author"),
            ("work_contributor_role", "author"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", &view);

    let stored = library.publication(publication).await.unwrap().unwrap();
    assert_eq!(stored.title, "Practical Vim");
    assert_eq!(stored.publisher.as_deref(), Some("Pragmatic Bookshelf"));
    assert_eq!(stored.year, Some(2015));
    assert_eq!(
        stored
            .holdings
            .iter()
            .map(|h| (h.id == shelf, h.location.as_deref()))
            .collect::<Vec<_>>(),
        [
            (true, Some("Piano bench")),
            (false, Some("practical-vim.pdf"))
        ]
    );
    assert_eq!(
        stored
            .contributors
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Tim Pope"]
    );
    let works = stored.works().await.unwrap();
    assert_eq!(
        works
            .iter()
            .map(|w| (w.id == one, w.title.as_str()))
            .collect::<Vec<_>>(),
        [(true, "Chapter 1"), (false, "Appendix")]
    );
    assert!(library.work(two).await.unwrap().is_none());
    assert_eq!(
        works[0]
            .contributors
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Drew Neil", "Marion Wenz"],
        "the contributor the row does not show is left alone"
    );
    // Dropped from the publication's contributors but still the author of Chapter 1, so a work
    // credit is enough to keep a person
    assert!(library.person(solo).await.unwrap().is_some());

    let response = server.get(&view).await;
    response.assert_text_contains("Practical Vim");
    response.assert_text_contains("Appendix");
    response.assert_text_contains("Pragmatic Bookshelf");
    response.assert_text_contains("Piano bench");
    response.assert_text_contains("practical-vim.pdf");

    // Removing the last copy is what the delete dialog warns about, so the form says so up front
    let response = server.get(&edit).await;
    response.assert_text_contains("will delete this publication");
    response.assert_text_contains("Delete this publication and its contents?");

    // Deleting takes the publication and the person left credited nowhere
    let response = server
        .post(&format!(
            "/library/{library_id}/publication/{publication}/delete"
        ))
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", &format!("/library/{library_id}"));
    assert!(library.publication(publication).await.unwrap().is_none());
    assert_eq!(library.person_names().await.unwrap(), [] as [String; 0]);

    // Deleting again is a miss, not a second delete
    let response = server
        .post(&format!(
            "/library/{library_id}/publication/{publication}/delete"
        ))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

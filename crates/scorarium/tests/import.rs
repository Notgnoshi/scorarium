use axum::http::StatusCode;
use scorarium::db;
use scorarium_tests::{TestDb, browser};

#[tokio::test]
async fn manual_import_flow() {
    let state = TestDb::new()
        .library("Scores")
        .password("hunter2")
        .build()
        .await;
    // Keep a handle on the database to look up ids the UI only exposes as links
    let pool = state.pool.clone();
    let server = browser(state);
    let library = db::list_libraries(&pool).await.unwrap()[0].id;
    let entry = format!("/library/{library}/import");

    // Importing requires login
    let response = server.get(&entry).await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", "/login");
    server.post("/login").form(&[("password", "hunter2")]).await;

    // A digital copy needs a file; the rejected form comes back as typed
    let response = server
        .post(&entry)
        .form(&[
            ("query", "Gnossiennes"),
            ("holding_kind", "physical"),
            ("holding_kind", "digital"),
            ("holding_location", ""),
            ("holding_file", ""),
        ])
        .await;
    response.assert_status_ok();
    response.assert_text_contains("Choose a file for a digital copy.");
    response.assert_text_contains("value=\"Gnossiennes\"");

    // Start needs at least one copy
    let response = server.post(&entry).form(&[("query", "")]).await;
    response.assert_status_ok();
    response.assert_text_contains("A publication needs at least one copy.");

    // Several copies go through to the review page
    let response = server
        .post(&entry)
        .form(&[
            ("holding_kind", "physical"),
            ("holding_kind", "digital"),
            ("holding_location", ""),
            ("holding_file", "satie.pdf"),
            ("holding_kind", "physical"),
            ("holding_location", "Piano bench"),
            ("holding_file", ""),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    let review = response.header("location").to_str().unwrap().to_string();
    assert!(review.starts_with(&format!("{entry}/")), "{review}");

    let response = server.get(&review).await;
    response.assert_status_ok();
    response.assert_text_contains("value=\"satie.pdf\"");
    response.assert_text_contains("value=\"Piano bench\"");
    response.assert_text_contains("Untitled import");

    // The entry page lists what was just started
    let response = server.get(&entry).await;
    response.assert_text_contains(format!("href=\"{review}\""));

    // The cross-library queue names the library, and the header counts the import
    let response = server.get("/review").await;
    response.assert_status_ok();
    response.assert_text_contains("Scores");
    response.assert_text_contains(format!("href=\"{review}\""));
    let home = server.get("/").await;
    home.assert_text_contains("href=\"/review\"");
    home.assert_text_contains("<span class=\"badge text-bg-primary rounded-pill\">1</span>");

    // A bad draft still saves, and the review page flags every field
    server
        .post(&format!("{review}/save"))
        .form(&[
            ("title", ""),
            ("publisher", ""),
            ("year", "abc"),
            ("holding_kind", "physical"),
            ("holding_kind", "digital"),
            ("holding_location", ""),
            ("holding_file", ""),
            ("identifier_kind", "isbn"),
            ("identifier_value", "not-an-isbn"),
            ("identifier_kind", "isbn"),
            ("identifier_value", ""),
            ("contributor_name", "Erik Satie"),
            ("contributor_role", ""),
        ])
        .await;
    let response = server.get(&review).await;
    response.assert_text_contains("A title is required.");
    response.assert_text_contains("The year must be a number.");
    response.assert_text_contains("value=\"abc\"");
    response.assert_text_contains("Choose a file for a digital copy.");
    response.assert_text_contains("invalid ISBN");
    response.assert_text_contains("Fill this in or remove it.");
    response.assert_text_contains("A role is required.");

    // Save a draft; the review page and the lists pick up its title
    let response = server
        .post(&format!("{review}/save"))
        .form(&[
            ("title", "Three gymnopedies"),
            ("publisher", "Schirmer"),
            ("year", "1888"),
            ("holding_kind", "physical"),
            ("holding_kind", "digital"),
            ("holding_location", ""),
            ("holding_file", "satie.pdf"),
            ("identifier_kind", "isbn"),
            ("identifier_value", "0-486-23134-8"),
            ("contributor_name", "Erik Satie"),
            ("contributor_role", "composer"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", review.as_str());
    let response = server.get(&review).await;
    response.assert_text_contains("value=\"Three gymnopedies\"");
    response.assert_text_contains("value=\"1888\"");
    response.assert_text_contains("value=\"satie.pdf\"");
    response.assert_text_contains("value=\"0-486-23134-8\"");
    response.assert_text_contains("value=\"Erik Satie\"");
    server
        .get(&entry)
        .await
        .assert_text_contains("Three gymnopedies");

    // A typed identifier seeds the draft's identifier row, in normalized form, and the holding
    // typed on the entry page seeds the review page's holding controls
    let response = server
        .post(&entry)
        .form(&[
            ("query", "0486231348"),
            ("holding_kind", "physical"),
            ("holding_location", "Piano bench"),
            ("holding_file", ""),
        ])
        .await;
    let seeded = response.header("location").to_str().unwrap().to_string();
    let response = server.get(&seeded).await;
    response.assert_text_contains("value=\"978-0-486-23134-1\"");
    response.assert_text_contains("value=\"Piano bench\"");
    server
        .post(&format!("{seeded}/delete"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    server
        .get(&seeded)
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Anything else seeds the title, which the lists show before any save
    let response = server
        .post(&entry)
        .form(&[
            ("query", "Gnossiennes"),
            ("holding_kind", "physical"),
            ("holding_location", ""),
            ("holding_file", ""),
        ])
        .await;
    let seeded = response.header("location").to_str().unwrap().to_string();
    server
        .get(&seeded)
        .await
        .assert_text_contains("value=\"Gnossiennes\"");
    server.get(&entry).await.assert_text_contains("Gnossiennes");
    server.post(&format!("{seeded}/delete")).await;

    // Submit: the publication exists, the import is gone, the header badge is gone
    let response = server
        .post(&format!("{review}/submit"))
        .form(&[
            ("title", "Three gymnopedies"),
            ("publisher", "Schirmer"),
            ("year", "1888"),
            ("holding_kind", "physical"),
            ("holding_kind", "digital"),
            ("holding_location", ""),
            ("holding_file", "satie.pdf"),
            ("identifier_kind", "isbn"),
            ("identifier_value", "0-486-23134-8"),
            ("contributor_name", "Erik Satie"),
            ("contributor_role", "composer"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    let publication = response.header("location").to_str().unwrap().to_string();
    assert!(
        publication.starts_with(&format!("/library/{library}/publication/")),
        "{publication}"
    );
    let response = server.get(&publication).await;
    response.assert_status_ok();
    response.assert_text_contains("978-0-486-23134-1");
    response.assert_text_contains("Erik Satie");
    response.assert_text_contains("satie.pdf");
    server
        .get(&review)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    assert!(!server.get("/").await.text().contains("rounded-pill"));
}

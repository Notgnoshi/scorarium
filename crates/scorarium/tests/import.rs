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

    let response = server
        .post(&entry)
        .form(&[("kind", "digital"), ("file", "satie.pdf")])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    let review = response.header("location").to_str().unwrap().to_string();
    assert!(review.starts_with(&format!("{entry}/")), "{review}");

    let response = server.get(&review).await;
    response.assert_status_ok();
    response.assert_text_contains("satie.pdf");
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
        .form(&[("title", ""), ("publisher", ""), ("year", "abc")])
        .await;
    let response = server.get(&review).await;
    response.assert_text_contains("A title is required.");
    response.assert_text_contains("The year must be a number.");
    response.assert_text_contains("value=\"abc\"");

    // Save a draft; the review page and the lists pick up its title
    let response = server
        .post(&format!("{review}/save"))
        .form(&[
            ("title", "Three gymnopedies"),
            ("publisher", "Schirmer"),
            ("year", "1888"),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", review.as_str());
    let response = server.get(&review).await;
    response.assert_text_contains("value=\"Three gymnopedies\"");
    response.assert_text_contains("value=\"1888\"");
    server
        .get(&entry)
        .await
        .assert_text_contains("Three gymnopedies");
}

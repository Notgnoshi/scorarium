use scorarium_tests::{TestDb, browser, demo_login};
use serde_json::Value;

#[tokio::test]
async fn search_shows_what_the_viewer_may_see() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let sheet_music = libraries
        .iter()
        .find(|l| l.name == "Sheet music")
        .unwrap()
        .id;
    let books = libraries.iter().find(|l| l.name == "Books").unwrap().id;
    let server = browser(state);

    // The box is on every page but the login form, and always searches every library
    let response = server.get(&format!("/library/{sheet_music}")).await;
    response.assert_text_contains("action=\"/search\"");
    assert!(!response.text().contains("name=\"library\""));

    let response = server.get("/search?q=rachmaninoff").await;
    response.assert_status_ok();
    response.assert_text_contains("Sergei Rachmaninoff");
    response.assert_text_contains("Rachmaninoff masterpieces for solo piano");
    // The Books library is private, so an anonymous search never names it
    assert!(!response.text().contains("Books"));

    let response = server.get("/search?q=bierce").await;
    response.assert_text_contains("Nothing matched.");

    // An empty query is the page without results, not an empty result set
    let response = server.get("/search?q=").await;
    response.assert_status_ok();
    assert!(!response.text().contains("<table"));

    // The login page has nowhere to return to, so it carries no search box
    assert!(
        !server
            .get("/login")
            .await
            .text()
            .contains("action=\"/search\"")
    );

    // The typeahead serves anonymous viewers, with public results only
    let body: Value = server.get("/suggest/title?q=rachmaninoff").await.json();
    let first = &body["matches"][0];
    assert_eq!(first["kind"], "person");
    assert!(
        first["href"]
            .as_str()
            .unwrap()
            .starts_with(&format!("/library/{sheet_music}/person/"))
    );
    assert!(
        first["secondary"]
            .as_str()
            .unwrap()
            .ends_with(" in Sheet music")
    );
    let body: Value = server.get("/suggest/title?q=vim").await.json();
    assert_eq!(body["matches"].as_array().unwrap().len(), 0);

    demo_login(&server).await;
    let response = server.get("/search?q=bierce").await;
    response.assert_text_contains("The Collected Writings of Ambrose Bierce");
    response.assert_text_contains("Books");

    // A hit names the library it is in, since results span all of them
    let response = server.get("/search?q=rachmaninoff").await;
    response.assert_text_contains("Sheet music");

    // Logging in opens the private library to the typeahead as well
    let body: Value = server.get("/suggest/title?q=vim").await.json();
    assert_eq!(body["matches"][0]["value"], "Practical Vim");
    assert!(
        body["matches"][0]["href"]
            .as_str()
            .unwrap()
            .starts_with(&format!("/library/{books}/publication/"))
    );
}

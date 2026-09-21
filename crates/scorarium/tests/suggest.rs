use axum::http::StatusCode;
use scorarium_archive::{
    ContributorInput, HoldingKind, HoldingRawInput, Library, PublicationRawInput, WorkRawInput,
};
use scorarium_tests::{TestDb, browser, demo_login};
use serde_json::Value;

/// A publication whose works are all credited to one composer, as (title, catalog number)
async fn publish(library: &Library, title: &str, composer: &str, works: &[(&str, &str)]) {
    let input = PublicationRawInput {
        title: title.into(),
        holdings: vec![HoldingRawInput {
            id: None,
            kind: HoldingKind::Physical,
            location: "Shelf".into(),
        }],
        contents: works
            .iter()
            .map(|(title, number)| WorkRawInput {
                title: (*title).into(),
                contributors: vec![ContributorInput {
                    name: composer.into(),
                    role: "composer".into(),
                }],
                catalog_numbers: vec![(*number).into()],
                ..WorkRawInput::default()
            })
            .collect(),
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&input.parse().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn field_suggestions_are_formatted_and_capped() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let books = libraries.iter().find(|l| l.name == "Books").unwrap().id;
    let sheet_music = libraries
        .iter()
        .find(|l| l.name == "Sheet music")
        .unwrap()
        .id;
    let server = browser(state);

    // Suggestions name what a library holds, so they are for a logged-in viewer alone
    let response = server
        .get(&format!("/library/{books}/suggest/tag?q="))
        .await;
    response.assert_status(StatusCode::SEE_OTHER);

    demo_login(&server).await;
    let body: Value = server
        .get(&format!("/library/{sheet_music}/suggest/person?q=rach"))
        .await
        .json();
    let first = &body["matches"][0];
    assert_eq!(first["kind"], "person");
    assert_eq!(first["value"], "Sergei Rachmaninoff");
    assert_eq!(first["primary"], "Sergei Rachmaninoff");
    assert!(
        first["secondary"]
            .as_str()
            .unwrap()
            .starts_with("Rachmaninoff masterpieces for solo piano +")
    );
    assert!(first["reference"]["id"].is_number());
    // Only a work number input asks about the scheme
    assert!(body.get("recognized").is_none());

    // A tag already chosen is not offered again
    let body: Value = server
        .get(&format!("/library/{books}/suggest/tag?q=&exclude=editor"))
        .await
        .json();
    let tags: Vec<&str> = body["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["value"].as_str().unwrap())
        .collect();
    assert_eq!(tags, ["reference"]);
    assert_eq!(body["matches"][0]["count"], 1);
    assert_eq!(body["matches"][0]["secondary"], "1 tagged");

    // A library that has credited nobody still offers the conventional roles
    let body: Value = server
        .get(&format!("/library/{books}/suggest/role?q="))
        .await
        .json();
    let roles: Vec<&str> = body["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["value"].as_str().unwrap())
        .collect();
    assert_eq!(
        roles,
        ["arranger", "author", "composer", "editor", "translator"]
    );
    assert_eq!(body["matches"][0]["secondary"], "");

    server
        .get(&format!("/library/{books}/suggest/colour?q=x"))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn external_suggestions_are_gated_and_marked() {
    let state = TestDb::new().demo().build().await;
    let libraries = state.archive.libraries().await.unwrap();
    let sheet_music = libraries
        .iter()
        .find(|l| l.name == "Sheet music")
        .unwrap()
        .id;
    let server = browser(state.clone());
    demo_login(&server).await;
    let suggest = format!("/library/{sheet_music}/suggest/publication");

    let body: Value = server
        .get(&format!("{suggest}?q=bagatelles%20rondos&source=external"))
        .await
        .json();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    let hit = matches
        .iter()
        .find(|m| m["reference"]["source_id"] == "OL1258206W")
        .expect("the Dover printing's work");
    assert_eq!(hit["kind"], "publication");
    assert_eq!(hit["reference"]["source"], "Open Library");
    // The ISBN is the value so that submitting the pick seeds the review page
    assert_eq!(hit["value"], "978-0-486-25392-3");
    assert_eq!(
        hit["primary"],
        "Bagatelles, Rondos and Other Shorter Works for Piano"
    );
    assert_eq!(hit["secondary"], "Ludwig van Beethoven");
    assert_eq!(hit["exact"], false);
    assert!(hit["reference"]["id"].is_null());

    // An identifier being typed and a short title both stay off the wire. Empty matches alone
    // would also come from a swallowed error, so the call history is the assertion that matters.
    for q in ["978-0-48", "bagat"] {
        let body: Value = server
            .get(&format!("{suggest}?q={q}&source=external"))
            .await
            .json();
        assert_eq!(body["matches"].as_array().unwrap().len(), 0, "{q:?}");
    }
    assert_eq!(state.sources.call_history().calls.len(), 1);

    // Only the publication input has an external source
    server
        .get(&format!(
            "/library/{sheet_music}/suggest/person?q=rach&source=external"
        ))
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // A library match's reference is a local id, not an external source
    let body: Value = server
        .get(&format!("{suggest}?q=isle&source=local"))
        .await
        .json();
    assert!(body["matches"][0]["reference"]["id"].is_number());
}

#[tokio::test]
async fn work_number_suggestions_carry_the_indicator_and_contributor() {
    let state = TestDb::new()
        .library("Sheet music")
        .password("hunter2")
        .build()
        .await;
    let library = state.archive.libraries().await.unwrap().remove(0);
    publish(
        &library,
        "Nocturnes",
        "Frederic Chopin",
        &[("Nocturne in E-flat major", "Op. 9 No. 2")],
    )
    .await;
    publish(
        &library,
        "Sonatas",
        "Ludwig van Beethoven",
        &[("Sonata No. 14", "Op. 27 No. 2")],
    )
    .await;
    let server = browser(state);
    server.post("/login").form(&[("password", "hunter2")]).await;
    let route = |q: &str| format!("/library/{}/suggest/work-number?{q}", library.id);

    let body: Value = server.get(&route("q=op%209%20no%202")).await.json();
    assert_eq!(body["recognized"], true);
    let first = &body["matches"][0];
    assert_eq!(first["kind"], "work");
    assert_eq!(first["value"], "Op. 9 No. 2");
    assert_eq!(first["primary"], "Op. 9 No. 2");
    assert_eq!(
        first["secondary"],
        "Nocturne in E-flat major by Frederic Chopin"
    );
    assert_eq!(first["exact"], true);
    assert_eq!(first["recognized"], true);
    assert_eq!(first["title"], "Nocturne in E-flat major");
    assert_eq!(first["contributor"], "Frederic Chopin");
    assert_eq!(first["role"], "composer");

    // A title is not a number, but it still finds the numbers of the works carrying it
    let body: Value = server.get(&route("q=nocturne")).await.json();
    assert_eq!(body["recognized"], false);
    assert_eq!(body["matches"][0]["recognized"], true);

    // The neighbouring composer input narrows the numbers to that composer's works
    let body: Value = server
        .get(&route("q=no%202&composer=Ludwig%20van%20Beethoven"))
        .await
        .json();
    let values: Vec<&str> = body["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["value"].as_str().unwrap())
        .collect();
    assert_eq!(values, ["Op. 27 No. 2"]);

    server
        .get(&format!(
            "/library/{}/suggest/catalog-numbers?q=op9",
            library.id
        ))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

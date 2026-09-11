use axum::http::StatusCode;
use scorarium_archive::{
    ContributorInput, HoldingKind, HoldingRawInput, Library, PublicationRawInput, WorkRawInput,
};
use scorarium_tests::{TestDb, browser};
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

/// The (value, title, composer, exact) of each match, in the order the route ranked them
fn matches(body: &Value) -> Vec<(&str, &str, &str, bool)> {
    body["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["value"].as_str().unwrap(),
                m["title"].as_str().unwrap(),
                m["composer"].as_str().unwrap_or_default(),
                m["exact"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn catalog_number_suggestions_rank_by_how_well_they_fit() {
    let state = TestDb::new()
        .library("Sheet music")
        .password("hunter2")
        .build()
        .await;
    let library = state.archive.libraries().await.unwrap().remove(0);
    publish(
        &library,
        "Beethoven sonatas",
        "Ludwig van Beethoven",
        &[
            ("Sonata No. 13", "Op. 27 No. 1"),
            ("Sonata No. 14", "Op. 27 No. 2"),
        ],
    )
    .await;
    publish(
        &library,
        "Chopin",
        "Frederic Chopin",
        &[
            ("Nocturne", "Op. 27 No. 2"),
            ("Etude", "Op. 10 No. 3"),
            ("Nocturne in C-sharp minor", "KK IVa/16"),
        ],
    )
    .await;
    let server = browser(state.clone());
    server.post("/login").form(&[("password", "hunter2")]).await;
    let suggest = format!("/library/{}/suggest/catalog-numbers", library.id);

    let body = server
        .get(&suggest)
        .add_query_params([("q", "Op. 27"), ("composer", "Frederic Chopin")])
        .await
        .json::<Value>();
    assert_eq!(body["recognized"], true);
    assert_eq!(
        matches(&body),
        [("Op. 27 No. 2", "Nocturne", "Frederic Chopin", false)],
        "a composer narrows the suggestions to their own numbers"
    );

    let body = server
        .get(&suggest)
        .add_query_params([("q", "op.27/2"), ("composer", "Frederic Chopin")])
        .await
        .json::<Value>();
    assert_eq!(
        matches(&body),
        [("Op. 27 No. 2", "Nocturne", "Frederic Chopin", true)],
        "another spelling of the same number is an exact match"
    );

    let body = server
        .get(&suggest)
        .add_query_params([("q", "Op. 27")])
        .await
        .json::<Value>();
    assert_eq!(
        matches(&body),
        [
            (
                "Op. 27 No. 1",
                "Sonata No. 13",
                "Ludwig van Beethoven",
                false
            ),
            ("Op. 27 No. 2", "Nocturne", "Frederic Chopin", false),
            (
                "Op. 27 No. 2",
                "Sonata No. 14",
                "Ludwig van Beethoven",
                false
            ),
        ],
        "with no composer the whole library is searched, by number then title"
    );

    let body = server
        .get(&suggest)
        .add_query_params([("q", "kk")])
        .await
        .json::<Value>();
    assert_eq!(body["recognized"], false);
    assert_eq!(
        matches(&body),
        [(
            "KK IVa/16",
            "Nocturne in C-sharp minor",
            "Frederic Chopin",
            false
        )],
        "a number the parser does not recognize is still found by its text"
    );

    let body = server
        .get(&suggest)
        .add_query_params([("q", "")])
        .await
        .json::<Value>();
    assert_eq!(body["recognized"], false);
    assert_eq!(matches(&body), [], "nothing typed suggests nothing");

    let response = server
        .get(&format!(
            "/library/{}/suggest/catalog-numbers",
            library.id + 100
        ))
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

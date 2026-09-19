use std::time::Duration;

use scorarium::enrich::open_library::search_titles;
use scorarium_tests::TestDb;
use tokio::time::Instant;

#[tokio::test]
async fn title_candidates_arrive_within_the_deadline() {
    let state = TestDb::new().build().await;
    let deadline = Instant::now() + Duration::from_secs(3);

    let hits = search_titles(&state.sources.open_library(), "bagatelles rondos", deadline).await;

    assert_eq!(hits.len(), 2);
    let hit = hits
        .iter()
        .find(|hit| hit.olid == "OL1258206W")
        .expect("the Dover printing's work");
    let edition = hit.edition.as_ref().expect("a matched edition");
    assert_eq!(edition.isbn_13, ["9780486253923"]);
}

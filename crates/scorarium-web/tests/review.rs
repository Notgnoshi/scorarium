use scorarium_engine::entity::Entity;
use scorarium_engine::holding::NewHolding;
use scorarium_engine::publication::{ImportSeed, Publication};
use scorarium_test::TestApp;

#[tokio::test]
async fn discard_asks_first_and_acts_only_on_what_was_confirmed() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let import = async |title: &str| {
        let seed = ImportSeed::Title {
            title: title.into(),
            record: None,
        };
        let holding = NewHolding::Physical { location: None };
        library.import(seed, holding).await.unwrap()
    };
    let draft = import("Gymnopedies").await;
    let mut committed = import("Piano Classics").await;
    let discard = |p: &Publication| format!("/api/review/publication/{}/discard", p.id());
    let exists = async |p: &Publication| {
        Publication::load(&app.engine, p.id())
            .await
            .unwrap()
            .is_some()
    };
    let unconfirmed: [(&str, &str); 0] = [];
    let confirmed = [("confirmed", "draft")];

    app.server.post(&discard(&draft)).form(&unconfirmed).await;
    assert!(exists(&draft).await);

    app.server.post(&discard(&draft)).form(&confirmed).await;
    assert!(!exists(&draft).await);

    // The page was drawn while this was a draft, and it was committed before the confirmation.
    committed.commit().await.unwrap();
    app.server.post(&discard(&committed)).form(&confirmed).await;
    assert!(exists(&committed).await);
}

use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{HoldingId, NewHolding};
use scorarium_engine::publication::{ImportSeed, Publication};
use scorarium_test::TestApp;

#[tokio::test]
async fn a_stale_confirmation_does_not_delete_the_publication() {
    let app = TestApp::demo().await;
    let library = app
        .engine
        .create_library("Sheet music", false)
        .await
        .unwrap();
    let mut publication = library
        .import(
            ImportSeed::Title {
                title: "Gymnopedies".into(),
                record: None,
            },
            NewHolding::Physical { location: None },
        )
        .await
        .unwrap();
    let first = publication.holdings()[0].id;
    let second = publication
        .add_holding(NewHolding::Physical { location: None })
        .await
        .unwrap();
    let url = format!(
        "/api/library/{}/publication/{}/holding/remove",
        library.id(),
        publication.id()
    );
    let remove = async |holding: HoldingId, confirmed: &str| {
        app.server
            .post(&url)
            .form(&[
                ("holding", holding.to_string().as_str()),
                ("confirmed", confirmed),
            ])
            .await;
    };
    let load = async || {
        Publication::load(&app.engine, publication.id())
            .await
            .unwrap()
    };

    remove(first, "holding").await;
    assert_eq!(load().await.unwrap().holdings().len(), 1);

    // The second holding is now the last, so a confirmation to delete one copy is stale.
    remove(second, "holding").await;
    assert_eq!(load().await.unwrap().holdings().len(), 1);

    remove(second, "publication").await;
    assert!(load().await.is_none());
}

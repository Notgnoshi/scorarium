use axum::http::StatusCode;
use scorarium_engine::entity::Entity;
use scorarium_engine::work::Work;
use scorarium_test::TestApp;

/// Build a Save of `work` that leaves its scalar fields blank, followed by `extra`
fn save<'a>(work: &'a Work, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut form = vec![
        ("title", work.title()),
        ("key", ""),
        ("time_signature", ""),
        ("instrumentation", ""),
        ("note", ""),
    ];
    form.extend_from_slice(extra);
    form
}

#[tokio::test]
async fn rejected_edit_applies_nothing() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let work = library.works().await.unwrap().pop().unwrap();
    let mut form = save(&work, &[("add_link", "not a url")]);
    form[0].1 = "Renamed";
    app.server
        .post(&format!(
            "/library/{}/work/{}/edit",
            library.id(),
            work.id()
        ))
        .form(&form)
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    assert_eq!(after.title(), work.title());
}

#[tokio::test]
async fn removing_and_re_adding_a_link_keeps_it() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut work = library.works().await.unwrap().pop().unwrap();
    let url = "https://example.com/again";
    work.add_link(url.parse().unwrap()).await.unwrap();
    let link = work
        .links()
        .iter()
        .find(|l| l.url.to_string() == url)
        .unwrap()
        .clone();
    let id = link.id.to_string();
    app.server
        .post(&format!(
            "/library/{}/work/{}/edit",
            library.id(),
            work.id()
        ))
        .form(&save(&work, &[("remove_link", &id), ("add_link", url)]))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    assert!(after.links().iter().any(|l| l.url == link.url));
}

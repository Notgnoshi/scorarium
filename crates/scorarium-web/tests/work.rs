use axum::http::StatusCode;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::person::Person;
use scorarium_engine::work::{CatalogNumber, Work};
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

#[tokio::test]
async fn edit_applies_additions_and_removals_in_one_save() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut work = library
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|work| work.links().is_empty())
        .unwrap();
    work.add_link("https://example.com/removed".parse().unwrap())
        .await
        .unwrap();
    work.add_link("https://example.com/kept".parse().unwrap())
        .await
        .unwrap();
    let [removed, kept] = work.links() else {
        panic!("expected exactly two links");
    };
    let removed = removed.id.to_string();
    let mut form = save(
        &work,
        &[
            ("catalog_number[new1][value]", "Op. 9 No. 2"),
            ("remove_link", &removed),
        ],
    );
    form[0].1 = "Renamed";
    app.server
        .post(&format!(
            "/library/{}/work/{}/edit",
            library.id(),
            work.id()
        ))
        .form(&form)
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    assert_eq!(after.title(), "Renamed");
    assert!(
        after
            .catalog_numbers()
            .contains(&"Op. 9 No. 2".parse().unwrap())
    );
    assert_eq!(after.links(), std::slice::from_ref(kept));
}

#[tokio::test]
async fn edit_saves_the_catalog_numbers_that_the_page_shows() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut work = library
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|work| work.catalog_numbers().is_empty())
        .unwrap();
    for number in ["Op. 9 No. 1", "Op. 9 No. 2"] {
        work.add_catalog_number(number.parse().unwrap())
            .await
            .unwrap();
    }
    let form = save(
        &work,
        &[
            ("catalog_number[0][saved][value]", "Op. 9 No. 1"),
            ("catalog_number[0][removed]", "true"),
            ("catalog_number[0][value]", "Op. 9 No. 1"),
            ("catalog_number[1][saved][value]", "Op. 9 No. 2"),
            ("catalog_number[1][value]", "Op. 9 No. 3"),
            ("catalog_number[new1][value]", ""),
            ("catalog_number[new2][value]", "Op. 10"),
        ],
    );
    app.server
        .post(&format!(
            "/library/{}/work/{}/edit",
            library.id(),
            work.id()
        ))
        .form(&form)
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    let expected: Vec<CatalogNumber> =
        vec!["Op. 9 No. 3".parse().unwrap(), "Op. 10".parse().unwrap()];
    assert_eq!(after.catalog_numbers(), expected);
}

#[tokio::test]
async fn adding_a_new_contributor_creates_a_committed_person() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let work = library
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|work| !work.is_draft())
        .unwrap();
    app.server
        .post(&format!(
            "/api/library/{}/work/{}/contributor/add",
            library.id(),
            work.id()
        ))
        .form(&[
            ("name", "New Person"),
            ("person", "new"),
            ("role", "editor"),
        ])
        .await
        .assert_status_ok();
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    let added = after
        .contributors()
        .iter()
        .find(|c| c.name == "New Person" && c.role.as_str() == "editor")
        .expect("the new contributor is listed");
    let person = Person::load(&app.engine, added.person)
        .await
        .unwrap()
        .unwrap();
    assert!(!person.is_draft());
}

#[tokio::test]
async fn adding_an_existing_contributor() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let work = library
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|work| !work.is_draft())
        .unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let id = person.id().to_string();
    app.server
        .post(&format!(
            "/api/library/{}/work/{}/contributor/add",
            library.id(),
            work.id()
        ))
        .form(&[
            ("name", person.name()),
            ("person", id.as_str()),
            ("role", "arranger"),
        ])
        .await
        .assert_status_ok();
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    assert!(
        after
            .contributors()
            .iter()
            .any(|c| c.person == person.id() && c.role.as_str() == "arranger")
    );
}

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
    let mut form = save(&work, &[("link[new1][url]", "not a url")]);
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
        .form(&save(
            &work,
            &[
                ("link[0][saved][id]", &id),
                ("link[0][saved][url]", url),
                ("link[0][removed]", "true"),
                ("link[0][url]", url),
                ("link[new1][url]", url),
            ],
        ))
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
    let (removed_id, kept_id) = (removed.id.to_string(), kept.id.to_string());
    let mut form = save(
        &work,
        &[
            ("catalog_number[new1][value]", "Op. 9 No. 2"),
            ("link[0][saved][id]", &removed_id),
            ("link[0][saved][url]", "https://example.com/removed"),
            ("link[0][removed]", "true"),
            ("link[0][url]", "https://example.com/removed"),
            ("link[1][saved][id]", &kept_id),
            ("link[1][saved][url]", "https://example.com/kept"),
            ("link[1][url]", "https://example.com/edited"),
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
    let urls: Vec<String> = after
        .links()
        .iter()
        .map(|link| link.url.to_string())
        .collect();
    assert_eq!(urls, ["https://example.com/edited"]);
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
async fn edit_applies_contributor_edits_in_one_save() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let mut work = library
        .works()
        .await
        .unwrap()
        .into_iter()
        .find(|work| !work.is_draft())
        .unwrap();
    let mut ids = vec![];
    for (name, role) in [
        ("Kept", "composer"),
        ("Moved", "editor"),
        ("Removed", "editor"),
    ] {
        let person = work
            .create_contributor(name.parse().unwrap(), role.parse().unwrap())
            .await
            .unwrap();
        ids.push(person.id().to_string());
    }
    let form = save(
        &work,
        &[
            ("contributor[0][saved][person]", &ids[0]),
            ("contributor[0][saved][role]", "composer"),
            ("contributor[0][removed]", "true"),
            ("contributor[0][role]", "composer"),
            ("contributor[1][saved][person]", &ids[1]),
            ("contributor[1][saved][role]", "editor"),
            ("contributor[1][role]", "translator"),
            ("contributor[2][saved][person]", &ids[2]),
            ("contributor[2][saved][role]", "editor"),
            ("contributor[2][removed]", "true"),
            ("contributor[2][role]", "editor"),
            ("contributor[new1][person]", &ids[0]),
            ("contributor[new1][name]", "Kept"),
            ("contributor[new1][role]", "composer"),
            ("contributor[new2][person]", "new"),
            ("contributor[new2][name]", "Created"),
            ("contributor[new2][role]", "editor"),
            ("contributor[new3][person]", "new"),
            ("contributor[new3][name]", "created"),
            ("contributor[new3][role]", "arranger"),
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
    let mut roles: Vec<(&str, &str)> = after
        .contributors()
        .iter()
        .filter(|c| ["Kept", "Moved", "Removed", "Created"].contains(&c.name.as_str()))
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    roles.sort();
    assert_eq!(
        roles,
        [
            ("Created", "arranger"),
            ("Created", "editor"),
            ("Kept", "composer"),
            ("Moved", "translator")
        ]
    );
    let created = after
        .contributors()
        .iter()
        .find(|c| c.name == "Created")
        .unwrap();
    let person = Person::load(&app.engine, created.person)
        .await
        .unwrap()
        .unwrap();
    assert!(!person.is_draft());
}

#[tokio::test]
async fn save_refuses_a_contributor_with_no_selected_person() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let work = library.works().await.unwrap().pop().unwrap();
    let form = save(
        &work,
        &[
            ("contributor[new1][name]", "Chop"),
            ("contributor[new1][role]", "composer"),
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
        .assert_status(StatusCode::BAD_REQUEST);
    let after = Work::load(&app.engine, work.id()).await.unwrap().unwrap();
    assert_eq!(after.contributors(), work.contributors());
}

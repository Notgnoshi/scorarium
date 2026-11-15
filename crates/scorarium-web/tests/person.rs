use axum::http::StatusCode;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::{Entity, ProposalId, Source};
use scorarium_engine::person::{Person, Proposal};
use scorarium_test::TestApp;

#[tokio::test]
async fn edit_derives_the_sort_name() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let response = app
        .server
        .post(&format!(
            "/library/{}/person/{}/edit",
            library.id(),
            person.id()
        ))
        .form(&[
            ("name", "Erik Satie"),
            ("sort_name", ""),
            ("birth_year", ""),
            ("death_year", ""),
        ])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header(
        "location",
        format!("/library/{}/person/{}", library.id(), person.id()),
    );
    let person = Person::load(&app.engine, person.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(person.name(), "Erik Satie");
    assert_eq!(person.sort_name(), "Satie, Erik");
}

#[tokio::test]
async fn back_is_honored_only_for_same_site_paths() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let edit = format!("/library/{}/person/{}/edit", library.id(), person.id());
    let form = [
        ("name", person.name()),
        ("sort_name", ""),
        ("birth_year", ""),
        ("death_year", ""),
    ];
    let response = app
        .server
        .post(&format!("{edit}?back=https://example.com"))
        .form(&form)
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header(
        "location",
        format!("/library/{}/person/{}", library.id(), person.id()),
    );
    let back = format!("/library/{}", library.id());
    let response = app
        .server
        .post(&format!("{edit}?back={back}"))
        .form(&form)
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", back.clone());
    let response = app
        .server
        .post(&format!("{edit}?back={back}&next=https://example.com"))
        .form(&form)
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", back);
}

#[tokio::test]
async fn a_staged_profile_picture_is_saved_with_the_form() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let edit = format!("/library/{}/person/{}/edit", library.id(), person.id());
    let load = async || {
        Person::load(&app.engine, person.id())
            .await
            .unwrap()
            .unwrap()
    };
    let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0";
    let staged = app
        .engine
        .stage(AssetSource::Upload {
            data: Box::new(png),
            filename: "satie.png".into(),
        })
        .await
        .unwrap();
    let id = staged.id.to_string();
    assert!(person.profile().is_none());

    let form = [
        ("name", person.name()),
        ("sort_name", ""),
        ("birth_year", ""),
        ("death_year", ""),
        ("picture", &id),
    ];
    app.server.post(&edit).form(&form).await;
    assert!(load().await.profile().is_some());

    let form = [
        ("name", person.name()),
        ("sort_name", ""),
        ("birth_year", ""),
        ("death_year", ""),
        ("remove_picture", "1"),
    ];
    app.server.post(&edit).form(&form).await;
    assert!(load().await.profile().is_none());
}

#[tokio::test]
async fn unfetched_picture_text_refuses_the_form() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let response = app
        .server
        .post(&format!(
            "/library/{}/person/{}/edit",
            library.id(),
            person.id()
        ))
        .form(&[
            ("name", "Renamed"),
            ("sort_name", ""),
            ("birth_year", ""),
            ("death_year", ""),
            ("picture_url", "https://example.com/satie.png"),
        ])
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    let saved = Person::load(&app.engine, person.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.name(), person.name());
}

#[tokio::test]
async fn save_deletes_the_proposals_that_were_accepted_or_rejected() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let ids = async || -> Vec<ProposalId> {
        let proposals = person.proposals().await.unwrap();
        proposals.iter().map(|proposal| proposal.id).collect()
    };
    let propose = async |name: &str| {
        let proposal = Proposal::Name(name.into());
        let id = person.propose(Source::OpenLibrary, proposal).await;
        id.unwrap().unwrap()
    };
    let mut expected = ids().await;
    let accepted = propose("A. Satie").await;
    let rejected = propose("B. Satie").await;
    let pending = propose("C. Satie").await;
    expected.push(pending);

    let form = [
        ("name".to_owned(), "A. Satie"),
        ("sort_name".to_owned(), ""),
        ("birth_year".to_owned(), ""),
        ("death_year".to_owned(), ""),
        (format!("proposal[{accepted}]"), "accepted"),
        (format!("proposal[{rejected}]"), "rejected"),
        (format!("proposal[{pending}]"), "pending"),
        ("proposal[999999]".to_owned(), "rejected"),
    ];
    let edit = format!("/library/{}/person/{}/edit", library.id(), person.id());
    let response = app.server.post(&edit).form(&form).await;

    response.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(ids().await, expected);
}

#[tokio::test]
async fn reject_remaining_rejects_only_the_posted_proposals() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let ids = async || -> Vec<ProposalId> {
        let proposals = person.proposals().await.unwrap();
        proposals.iter().map(|proposal| proposal.id).collect()
    };
    let propose = async |name: &str| {
        let proposal = Proposal::Name(name.into());
        let id = person.propose(Source::OpenLibrary, proposal).await;
        id.unwrap().unwrap()
    };
    let mut expected = ids().await;
    let first = propose("A. Satie").await;
    let second = propose("B. Satie").await;
    let unseen = propose("C. Satie").await;
    expected.push(unseen);

    let form = [
        ("name".to_owned(), person.name().to_owned()),
        ("sort_name".to_owned(), String::new()),
        ("birth_year".to_owned(), String::new()),
        ("death_year".to_owned(), String::new()),
        (format!("proposal[{first}]"), "pending".to_owned()),
        (format!("proposal[{second}]"), "pending".to_owned()),
        ("reject_remaining".to_owned(), "1".to_owned()),
    ];
    let edit = format!("/library/{}/person/{}/edit", library.id(), person.id());
    let response = app.server.post(&edit).form(&form).await;

    response.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(ids().await, expected);
}

#[tokio::test]
async fn enrich_saves_the_form_and_then_starts_enrichment() {
    let app = TestApp::demo().await;
    let library = app.engine.libraries().await.unwrap().pop().unwrap();
    let person = library.people(None).await.unwrap().pop().unwrap();
    let edit = format!("/library/{}/person/{}/edit", library.id(), person.id());
    let enrich = format!("/library/{}/person/{}/enrich", library.id(), person.id());

    app.server
        .post(&enrich)
        .form(&[
            ("name", " "),
            ("sort_name", ""),
            ("birth_year", ""),
            ("death_year", ""),
        ])
        .await;
    assert!(person.enrichment().is_none());

    let response = app
        .server
        .post(&enrich)
        .form(&[
            ("name", "Erik Satie"),
            ("sort_name", ""),
            ("birth_year", ""),
            ("death_year", ""),
        ])
        .await;

    response.assert_status(StatusCode::SEE_OTHER);
    response.assert_header("location", edit);
    assert!(person.enrichment().is_some());
    let person = Person::load(&app.engine, person.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(person.name(), "Erik Satie");
}

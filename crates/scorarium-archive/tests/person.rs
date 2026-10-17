use scorarium_archive::{Archive, Field, HoldingKind, NotFound, Person};

fn names(persons: Vec<Person>) -> Vec<String> {
    persons.into_iter().map(|person| person.name).collect()
}

async fn library() -> (Archive, i64) {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let satie = library
        .create_person("Erik Satie", "Satie, Erik")
        .await
        .unwrap();
    let sue = library.create_person("Sue", "Sue").await.unwrap();
    let bob = library.create_person("Bob", "Bob").await.unwrap();
    let ann = library.create_person("Ann", "Ann").await.unwrap();
    let gymnopedie = library.create_work("Gymnopedie No. 1").await.unwrap();
    let gnossienne = library.create_work("Gnossienne No. 1").await.unwrap();
    let (gymnopedies, _) = library
        .create_publication("Three gymnopedies", HoldingKind::Physical, None)
        .await
        .unwrap();
    let (gnossiennes, _) = library
        .create_publication("Gnossiennes", HoldingKind::Physical, None)
        .await
        .unwrap();

    let mut work = library.work(gymnopedie).await.unwrap();
    work.add_contributor(satie, "composer").await.unwrap();
    work.add_contributor(sue, "editor").await.unwrap();
    let mut work = library.work(gnossienne).await.unwrap();
    work.add_contributor(satie, "composer").await.unwrap();
    work.add_contributor(ann, "editor").await.unwrap();
    let mut publication = library.publication(gymnopedies).await.unwrap();
    publication
        .add_contributor(satie, "composer")
        .await
        .unwrap();
    publication.add_work(gymnopedie).await.unwrap();
    let mut publication = library.publication(gnossiennes).await.unwrap();
    publication.add_contributor(bob, "arranger").await.unwrap();
    publication.add_work(gnossienne).await.unwrap();
    edit.commit().await.unwrap();
    (archive, library_id)
}

/// Create a second library holding one editor, and return its id.
async fn other_library(archive: &Archive) -> i64 {
    let mut edit = archive.edit().await.unwrap();
    let id = edit.create_library("Books", false).await.unwrap();
    let mut library = edit.library(id).await.unwrap();
    let drew = library
        .create_person("Drew Neil", "Neil, Drew")
        .await
        .unwrap();
    let (vim, _) = library
        .create_publication("Practical Vim", HoldingKind::Physical, None)
        .await
        .unwrap();
    let mut publication = library.publication(vim).await.unwrap();
    publication.add_contributor(drew, "editor").await.unwrap();
    edit.commit().await.unwrap();
    id
}

#[tokio::test]
async fn a_person_is_reached_through_every_contributor_link() {
    let (archive, library_id) = library().await;
    let other_id = other_library(&archive).await;
    let library = archive.library(library_id).await.unwrap().unwrap();
    let satie = library
        .persons_with_role("composer")
        .await
        .unwrap()
        .remove(0);
    let sue = library.persons_with_role("editor").await.unwrap().remove(1);

    // Satie is a contributor on the first publication twice over, and it counts once
    let mut titles: Vec<String> = satie
        .publications()
        .await
        .unwrap()
        .into_iter()
        .map(|publication| publication.title)
        .collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Gnossiennes", "Three gymnopedies"]);

    // Sue is a contributor on a work alone, and is still reachable
    let titles: Vec<String> = sue
        .publications()
        .await
        .unwrap()
        .into_iter()
        .map(|publication| publication.title)
        .collect();
    assert_eq!(titles, ["Three gymnopedies"]);

    // The direct contributor leads, and the other publication counts among the others
    let summary = library.person_summary(satie.id).await.unwrap().unwrap();
    assert_eq!(
        (summary.name, summary.title, summary.others),
        ("Erik Satie".to_string(), "Three gymnopedies".to_string(), 2)
    );
    assert!(library.person_summary(9999).await.unwrap().is_none());

    // A person belongs to their own library alone
    let other = archive.library(other_id).await.unwrap().unwrap();
    assert!(other.person(satie.id).await.unwrap().is_none());
}

#[tokio::test]
async fn a_role_spans_publications_and_works() {
    let (archive, library_id) = library().await;
    let other_id = other_library(&archive).await;
    let library = archive.library(library_id).await.unwrap().unwrap();
    let other = archive.library(other_id).await.unwrap().unwrap();

    // Both editors are contributors on works alone, and come back by sort name, not by id
    assert_eq!(
        names(library.persons_with_role("editor").await.unwrap()),
        ["Ann", "Sue"]
    );
    // A contributor on a publication alone
    assert_eq!(
        names(library.persons_with_role("arranger").await.unwrap()),
        ["Bob"]
    );
    // The other library's editor is not this library's
    assert_eq!(
        names(other.persons_with_role("editor").await.unwrap()),
        ["Drew Neil"]
    );
}

/// Orphans are collected on commit, so test what happens when a person is created and *then* a
/// contributor linking to that person is made from a single transaction.
#[tokio::test]
async fn a_person_credited_again_before_commit_survives() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Scores", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let satie = library
        .create_person("Erik Satie", "Satie, Erik")
        .await
        .unwrap();
    let (id, _) = library
        .create_publication("Gymnopedies", HoldingKind::Physical, None)
        .await
        .unwrap();
    let mut publication = library.publication(id).await.unwrap();
    let error = publication
        .add_contributor(999, "composer")
        .await
        .unwrap_err();
    assert!(error.downcast_ref::<NotFound>().is_some(), "{error}");
    publication
        .add_contributor(satie, "composer")
        .await
        .unwrap();
    publication
        .remove_contributor(satie, "composer")
        .await
        .unwrap();
    publication
        .add_contributor(satie, "composer")
        .await
        .unwrap();
    edit.commit().await.unwrap();

    let library = archive.library(library_id).await.unwrap().unwrap();
    assert!(library.person(satie).await.unwrap().is_some());
    assert_eq!(
        archive.recent_fields(5).await.unwrap(),
        [
            Field::Contributors,
            Field::Contributors,
            Field::Contributors
        ]
    );
}

use scorarium_archive::{Archive, CatalogNumber, Field, HoldingKind, LibraryEdit, NotFound};
use url::Url;

fn values(work: &scorarium_archive::Work) -> Vec<&str> {
    work.catalog_numbers.iter().map(|n| n.as_str()).collect()
}

/// Create a work with one composer in a new publication
async fn work_in_new_publication(
    library: &mut LibraryEdit<'_>,
    publication: &str,
    title: &str,
    composer: i64,
) -> i64 {
    let (publication, _) = library
        .create_publication(publication, HoldingKind::Physical, None)
        .await
        .unwrap();
    let work = library.create_work(title).await.unwrap();
    library
        .publication(publication)
        .await
        .unwrap()
        .add_work(work)
        .await
        .unwrap();
    library
        .work(work)
        .await
        .unwrap()
        .add_contributor(composer, "composer")
        .await
        .unwrap();
    work
}

#[tokio::test]
async fn merge_works_folds_the_newer_work_into_the_older() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let chopin = library
        .create_person("Frederic Chopin", "Chopin, Frederic")
        .await
        .unwrap();
    let sue = library.create_person("Sue", "Sue").await.unwrap();
    let beethoven = library
        .create_person("Ludwig van Beethoven", "Beethoven, Ludwig van")
        .await
        .unwrap();

    let older = work_in_new_publication(&mut library, "Preludes", "Raindrop Prelude", chopin).await;
    let mut work = library.work(older).await.unwrap();
    work.set_key(Some("D-flat major")).await.unwrap();
    work.add_catalog_number(&CatalogNumber::parse("Op. 28 No. 15"))
        .await
        .unwrap();
    work.add_link(&Url::parse("https://www.wikidata.org/wiki/Q255").unwrap())
        .await
        .unwrap();

    let newer =
        work_in_new_publication(&mut library, "Anthology", "Prelude in D-flat", chopin).await;
    let mut work = library.work(newer).await.unwrap();
    work.add_contributor(sue, "editor").await.unwrap();
    for number in ["Op. 28/15", "B. 107"] {
        work.add_catalog_number(&CatalogNumber::parse(number))
            .await
            .unwrap();
    }
    for link in [
        "https://www.wikidata.org/entity/Q255",
        "https://imslp.org/wiki/Preludes,_Op.28_(Chopin,_Frederic)",
    ] {
        work.add_link(&Url::parse(link).unwrap()).await.unwrap();
    }

    let other = work_in_new_publication(&mut library, "Sonatas", "Sonata", beethoven).await;
    library
        .work(other)
        .await
        .unwrap()
        .add_catalog_number(&CatalogNumber::parse("Op. 28 No. 15"))
        .await
        .unwrap();

    assert_eq!(library.find_duplicate(newer).await.unwrap(), Some(older));
    assert_eq!(library.find_duplicate(other).await.unwrap(), None);

    library.merge_works(newer, older).await.unwrap();
    let err = library.merge_works(newer, older).await.unwrap_err();
    assert!(err.downcast_ref::<NotFound>().is_some());
    edit.commit().await.unwrap();

    let library = archive.library(library_id).await.unwrap().unwrap();
    let merged = library.work(older).await.unwrap().unwrap();
    assert_eq!(merged.title, "Raindrop Prelude");
    assert_eq!(merged.key.as_deref(), Some("D-flat major"));
    assert_eq!(values(&merged), ["Op. 28 No. 15", "B. 107"]);
    let contributors: Vec<(&str, &str)> = merged
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        contributors,
        [("Frederic Chopin", "composer"), ("Sue", "editor")]
    );
    assert_eq!(
        merged.links,
        [
            "https://www.wikidata.org/wiki/Q255",
            "https://imslp.org/wiki/Preludes,_Op.28_(Chopin,_Frederic)",
        ]
    );
    assert_eq!(merged.publications().await.unwrap().len(), 2);
    assert!(library.work(newer).await.unwrap().is_none());
}

#[tokio::test]
async fn a_catalog_number_is_matched_however_it_is_spelled() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Scores", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let (publication, _) = library
        .create_publication("Sonatas", HoldingKind::Physical, None)
        .await
        .unwrap();
    let id = library.create_work("Moonlight sonata").await.unwrap();
    library
        .publication(publication)
        .await
        .unwrap()
        .add_work(id)
        .await
        .unwrap();
    let mut work = library.work(id).await.unwrap();
    work.add_catalog_number(&CatalogNumber::parse("Op. 27 No. 2"))
        .await
        .unwrap();
    work.add_catalog_number(&CatalogNumber::parse("op.27 no.2"))
        .await
        .unwrap();
    work.remove_catalog_number(&CatalogNumber::parse("Opus 27 Number 2"))
        .await
        .unwrap();
    edit.commit().await.unwrap();

    let library = archive.library(library_id).await.unwrap().unwrap();
    assert!(
        library
            .work(id)
            .await
            .unwrap()
            .unwrap()
            .catalog_numbers
            .is_empty()
    );
    assert_eq!(
        archive.recent_fields(5).await.unwrap(),
        [
            Field::CatalogNumbers,
            Field::CatalogNumbers,
            Field::Contents
        ]
    );
}

use scorarium_archive::{Archive, Field, HoldingKind, Stars, Tag, identifier};
use url::Url;

#[tokio::test]
async fn a_publication_reads_back_whole_and_scoped_to_its_library() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let books_id = edit.create_library("Books", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let satie = library
        .create_person("Erik Satie", "Satie, Erik")
        .await
        .unwrap();
    let bob = library.create_person("Bob", "Bob").await.unwrap();
    let sue = library.create_person("Sue", "Sue").await.unwrap();
    let work_id = library.create_work("Gymnopedie No. 1").await.unwrap();
    let (id, _) = library
        .create_publication(
            "Three gymnopedies",
            HoldingKind::Physical,
            Some("Piano bench"),
        )
        .await
        .unwrap();

    let plate = identifier::normalize(identifier::Kind::PlateNumber, "ut 50061").unwrap();
    let link = Url::parse("https://imslp.org/wiki/3_Gymnopedies_(Satie,_Erik)").unwrap();
    let mut publication = library.publication(id).await.unwrap();
    publication.set_publisher(Some("Dover")).await.unwrap();
    publication.set_year(Some(1987)).await.unwrap();
    publication
        .set_stars(Some(Stars::try_from(4).unwrap()))
        .await
        .unwrap();
    publication
        .set_note(Some("Bought secondhand."))
        .await
        .unwrap();
    publication
        .add_identifier(identifier::Kind::PlateNumber, &plate)
        .await
        .unwrap();
    publication
        .add_contributor(satie, "composer")
        .await
        .unwrap();
    publication.add_contributor(bob, "arranger").await.unwrap();
    publication.add_link(&link).await.unwrap();
    publication
        .add_tag(&"reference".parse::<Tag>().unwrap())
        .await
        .unwrap();
    publication
        .add_holding(HoldingKind::Digital, Some("gymnopedies.pdf"))
        .await
        .unwrap();
    publication.add_work(work_id).await.unwrap();

    let mut work = library.work(work_id).await.unwrap();
    work.set_key(Some("D major")).await.unwrap();
    work.add_contributor(sue, "editor").await.unwrap();
    edit.commit().await.unwrap();

    let library = archive.library(library_id).await.unwrap().unwrap();
    let publication = library.publication(id).await.unwrap().unwrap();
    assert_eq!(publication.title, "Three gymnopedies");
    assert_eq!(publication.publisher.as_deref(), Some("Dover"));
    assert_eq!(publication.year, Some(1987));
    assert_eq!(publication.stars, Some(4));
    assert_eq!(publication.note.as_deref(), Some("Bought secondhand."));
    assert_eq!(publication.tags, ["reference"]);
    assert_eq!(
        publication.links,
        ["https://imslp.org/wiki/3_Gymnopedies_(Satie,_Erik)"]
    );
    let identifiers: Vec<_> = publication
        .identifiers
        .iter()
        .map(|identifier| (identifier.kind, identifier.value.as_str()))
        .collect();
    assert_eq!(identifiers, [(identifier::Kind::PlateNumber, "UT 50061")]);
    let contributors: Vec<_> = publication
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        contributors,
        [("Erik Satie", "composer"), ("Bob", "arranger")]
    );
    let holdings: Vec<_> = publication
        .holdings
        .iter()
        .map(|holding| (holding.kind, holding.location.as_deref()))
        .collect();
    assert_eq!(
        holdings,
        [
            (HoldingKind::Physical, Some("Piano bench")),
            (HoldingKind::Digital, Some("gymnopedies.pdf")),
        ]
    );
    let listed = library.publications().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);

    let works = publication.works().await.unwrap();
    assert_eq!(works.len(), 1);
    assert_eq!(works[0].id, work_id);
    assert_eq!(works[0].title, "Gymnopedie No. 1");
    assert_eq!(works[0].key.as_deref(), Some("D major"));
    let work_contributors: Vec<_> = works[0]
        .contributors
        .iter()
        .map(|c| (c.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(work_contributors, [("Sue", "editor")]);
    let found = library.work(work_id).await.unwrap().unwrap();
    assert_eq!(found.title, "Gymnopedie No. 1");
    assert_eq!(found.key.as_deref(), Some("D major"));
    let containing = found.publications().await.unwrap();
    assert_eq!(containing.len(), 1);
    assert_eq!(containing[0].id, id);
    assert_eq!(
        library.roles().await.unwrap(),
        ["arranger", "composer", "editor"]
    );

    let books = archive.library(books_id).await.unwrap().unwrap();
    assert!(books.publication(id).await.unwrap().is_none());
    assert!(books.work(work_id).await.unwrap().is_none());
}

#[tokio::test]
async fn delete_collects_only_what_nothing_else_reaches() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let shared_person = library.create_person("Shared", "Shared").await.unwrap();
    let sole_person = library.create_person("Sole", "Sole").await.unwrap();
    let shared_work = library.create_work("Shared work").await.unwrap();
    let sole_work = library.create_work("Sole work").await.unwrap();
    let (first, _) = library
        .create_publication("First", HoldingKind::Physical, None)
        .await
        .unwrap();
    let (second, _) = library
        .create_publication("Second", HoldingKind::Physical, None)
        .await
        .unwrap();
    let mut work = library.work(shared_work).await.unwrap();
    work.add_contributor(shared_person, "composer")
        .await
        .unwrap();
    let mut work = library.work(sole_work).await.unwrap();
    work.add_contributor(sole_person, "composer").await.unwrap();
    let mut publication = library.publication(first).await.unwrap();
    publication.add_work(shared_work).await.unwrap();
    publication.add_work(sole_work).await.unwrap();
    let mut publication = library.publication(second).await.unwrap();
    publication.add_work(shared_work).await.unwrap();
    edit.commit().await.unwrap();

    let mut edit = archive.edit().await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    library
        .publication(first)
        .await
        .unwrap()
        .delete()
        .await
        .unwrap();
    edit.commit().await.unwrap();

    let library = archive.library(library_id).await.unwrap().unwrap();
    assert!(library.work(shared_work).await.unwrap().is_some());
    assert!(library.person(shared_person).await.unwrap().is_some());
    assert!(library.work(sole_work).await.unwrap().is_none());
    assert!(library.person(sole_person).await.unwrap().is_none());
}

#[tokio::test]
async fn a_publication_left_without_holdings_is_collected() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Books", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let (id, first) = library
        .create_publication("Gymnopedies", HoldingKind::Physical, Some("Shelf"))
        .await
        .unwrap();
    let mut publication = library.publication(id).await.unwrap();
    let second = publication
        .add_holding(HoldingKind::Digital, None)
        .await
        .unwrap();
    publication.remove_holding(first).await.unwrap();
    publication.remove_holding(second).await.unwrap();
    publication.remove_holding(second).await.unwrap();
    edit.commit().await.unwrap();

    let library = archive.library(library_id).await.unwrap().unwrap();
    assert!(library.publication(id).await.unwrap().is_none());
    assert_eq!(
        archive.recent_fields(5).await.unwrap(),
        [Field::Holdings, Field::Holdings, Field::Holdings]
    );
}

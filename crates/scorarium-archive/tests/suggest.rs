use scorarium_archive::{
    Archive, CatalogNumber, HoldingKind, LibraryEdit, PersonRef, SuggestField, Suggested,
    Suggestion, WorkSummary,
};

/// The work a suggestion offers, which every work suggestion here is expected to be
fn work_of(suggestion: &Suggestion) -> &WorkSummary {
    match &suggestion.item {
        Suggested::Work { work, .. } => work,
        other => panic!("Not a work: {other:?}"),
    }
}

/// Who a work summary credits, as (name, role)
fn credited(work: &WorkSummary) -> Option<(&str, &str)> {
    work.contributor
        .as_ref()
        .map(|person| (person.name.as_str(), person.role.as_str()))
}

/// Create a work by `composer` carrying `numbers`, and put it in `publication`
async fn piece(
    library: &mut LibraryEdit<'_>,
    publication: i64,
    title: &str,
    composer: i64,
    numbers: &[&str],
) -> i64 {
    let work = library.create_work(title).await.unwrap();
    let mut edit = library.work(work).await.unwrap();
    edit.add_contributor(composer, "composer").await.unwrap();
    for number in numbers {
        edit.add_catalog_number(&CatalogNumber::parse(number))
            .await
            .unwrap();
    }
    library
        .publication(publication)
        .await
        .unwrap()
        .add_work(work)
        .await
        .unwrap();
    work
}

#[tokio::test]
async fn distinct_values_rank_by_match_and_list_whole_when_nothing_is_typed() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let private_id = edit.create_library("Books", true).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let bob = library.create_person("Bob", "Bob").await.unwrap();
    let (publication, _) = library
        .create_publication(
            "Three gymnopedies",
            HoldingKind::Physical,
            Some("Piano bench"),
        )
        .await
        .unwrap();
    let work = library.create_work("Gymnopedie No. 1").await.unwrap();
    {
        let mut edit = library.publication(publication).await.unwrap();
        edit.set_publisher(Some("Dover")).await.unwrap();
        edit.add_contributor(bob, "editor").await.unwrap();
        edit.add_work(work).await.unwrap();
    }
    {
        let mut edit = library.work(work).await.unwrap();
        edit.set_key(Some("G major")).await.unwrap();
        edit.add_tag(&"christmas".parse().unwrap()).await.unwrap();
    }
    edit.commit().await.unwrap();
    let library = archive.library(library_id).await.unwrap().unwrap();

    let plain = |exact: bool, item: Suggested| Suggestion { exact, item };
    let roles = library
        .suggest(SuggestField::Role, "", false, false)
        .await
        .unwrap();
    assert_eq!(
        roles,
        ["arranger", "author", "composer", "editor", "translator"]
            .map(|role| plain(false, Suggested::Role(role.into())))
    );
    assert_eq!(
        library
            .suggest(SuggestField::Publisher, "dovr", false, false)
            .await
            .unwrap(),
        [plain(false, Suggested::Publisher("Dover".into()))]
    );
    assert_eq!(
        library
            .suggest(SuggestField::Publisher, "dover", false, false)
            .await
            .unwrap(),
        [plain(true, Suggested::Publisher("Dover".into()))]
    );
    assert_eq!(
        library
            .suggest(SuggestField::Tag, "chr", false, false)
            .await
            .unwrap(),
        [plain(
            false,
            Suggested::Tag {
                name: "christmas".into(),
                count: 1
            }
        )]
    );
    assert_eq!(
        library
            .suggest(SuggestField::Location, "", false, false)
            .await
            .unwrap(),
        [plain(false, Suggested::Location("Piano bench".into()))]
    );
    // One letter has no typo budget
    assert!(
        library
            .suggest(SuggestField::Key, "F", false, false)
            .await
            .unwrap()
            .is_empty()
    );

    let private = archive.library(private_id).await.unwrap().unwrap();
    assert!(
        private
            .suggest(SuggestField::Publisher, "", true, false)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn entities_are_one_suggestion_each() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let ann = library.create_person("Ann", "Ann").await.unwrap();
    let sue = library.create_person("Sue", "Sue").await.unwrap();
    let chopin = library
        .create_person("Frederic Chopin", "Chopin, Frederic")
        .await
        .unwrap();
    let faure = library
        .create_person("Gabriel Faure", "Faure, Gabriel")
        .await
        .unwrap();
    let bierce = library
        .create_person("Ambrose Bierce", "Bierce, Ambrose")
        .await
        .unwrap();
    let emerson = library
        .create_person("Ralph Waldo Emerson", "Emerson, Ralph Waldo")
        .await
        .unwrap();

    let (nocturnes, _) = library
        .create_publication("Nocturnes", HoldingKind::Physical, None)
        .await
        .unwrap();
    library
        .publication(nocturnes)
        .await
        .unwrap()
        .add_contributor(ann, "editor")
        .await
        .unwrap();
    // The editor is a contributor first, so only the lead rule puts the composer forward
    for (composer, numbers) in [
        (chopin, &["B. 49", "Op. 9 No. 2"][..]),
        (faure, &["Op. 33 No. 1"]),
    ] {
        let work = library.create_work("Nocturne").await.unwrap();
        let mut edit = library.work(work).await.unwrap();
        edit.add_contributor(sue, "editor").await.unwrap();
        edit.add_contributor(composer, "composer").await.unwrap();
        for number in numbers {
            edit.add_catalog_number(&CatalogNumber::parse(number))
                .await
                .unwrap();
        }
        library
            .publication(nocturnes)
            .await
            .unwrap()
            .add_work(work)
            .await
            .unwrap();
    }

    let (dictionary, _) = library
        .create_publication("The Devil's Dictionary", HoldingKind::Physical, None)
        .await
        .unwrap();
    library
        .publication(dictionary)
        .await
        .unwrap()
        .add_contributor(bierce, "author")
        .await
        .unwrap();

    let (essays, _) = library
        .create_publication("Essays: First Series", HoldingKind::Physical, None)
        .await
        .unwrap();
    let reliance = library.create_work("Self-Reliance").await.unwrap();
    library
        .work(reliance)
        .await
        .unwrap()
        .add_contributor(emerson, "author")
        .await
        .unwrap();
    library
        .publication(essays)
        .await
        .unwrap()
        .add_work(reliance)
        .await
        .unwrap();
    edit.commit().await.unwrap();
    let library = archive.library(library_id).await.unwrap().unwrap();

    let works = library
        .suggest(SuggestField::Work, "nocturne", false, false)
        .await
        .unwrap();
    // Both nocturnes match exactly, in load order; Op. outranks B. so it leads the numbers
    assert_eq!(works.len(), 2);
    assert!(works[0].exact);
    assert_eq!(work_of(&works[0]).title, "Nocturne");
    assert_eq!(
        credited(work_of(&works[0])),
        Some(("Frederic Chopin", "composer"))
    );
    assert_eq!(work_of(&works[0]).numbers, ["Op. 9 No. 2", "B. 49"]);
    assert_eq!(
        credited(work_of(&works[1])),
        Some(("Gabriel Faure", "composer"))
    );

    // A work in a book is credited to its author, not to a composer
    let essay = library
        .suggest(SuggestField::Work, "self reliance", false, false)
        .await
        .unwrap();
    assert_eq!(
        credited(work_of(&essay[0])),
        Some(("Ralph Waldo Emerson", "author"))
    );

    let faure = library
        .suggest(SuggestField::Work, "nocturne faure", false, false)
        .await
        .unwrap();
    assert_eq!(faure.len(), 1);
    assert!(!faure[0].exact);

    let persons = library
        .suggest(SuggestField::Person, "bierce", false, false)
        .await
        .unwrap();
    assert!(matches!(
        &persons[0].item,
        Suggested::Person(p)
            if p.name == "Ambrose Bierce" && p.title == "The Devil's Dictionary" && p.others == 0
    ));

    assert!(
        library
            .suggest(SuggestField::Publication, "", false, false)
            .await
            .unwrap()
            .is_empty()
    );
    let dictionary = library
        .suggest(SuggestField::Publication, "devil", false, false)
        .await
        .unwrap();
    assert!(
        matches!(&dictionary[0].item, Suggested::Publication(p) if p.people == ["Ambrose Bierce"])
    );
}

#[tokio::test]
async fn work_numbers_rank_exact_then_prefix_then_fuzzy() {
    let archive = Archive::in_memory().await.unwrap();
    let mut edit = archive.edit().await.unwrap();
    let library_id = edit.create_library("Sheet music", false).await.unwrap();
    let mut library = edit.library(library_id).await.unwrap();
    let chopin = library
        .create_person("Frederic Chopin", "Chopin, Frederic")
        .await
        .unwrap();
    let rachmaninoff = library
        .create_person("Sergei Rachmaninoff", "Rachmaninoff, Sergei")
        .await
        .unwrap();
    let (nocturnes, _) = library
        .create_publication("Nocturnes", HoldingKind::Physical, None)
        .await
        .unwrap();
    piece(
        &mut library,
        nocturnes,
        "Nocturne in E-flat major",
        chopin,
        &["Op. 9 No. 2"],
    )
    .await;
    piece(
        &mut library,
        nocturnes,
        "Nocturne in C-sharp minor",
        chopin,
        &["Op. 27 No. 2"],
    )
    .await;
    let (morceaux, _) = library
        .create_publication("Morceaux de fantaisie", HoldingKind::Physical, None)
        .await
        .unwrap();
    piece(
        &mut library,
        morceaux,
        "Prelude in C-sharp minor",
        rachmaninoff,
        &["Op. 3 No. 2"],
    )
    .await;
    piece(&mut library, morceaux, "Nocturne", rachmaninoff, &[]).await;
    edit.commit().await.unwrap();
    let library = archive.library(library_id).await.unwrap().unwrap();

    let numbers = |suggestions: &[Suggestion]| -> Vec<(bool, String, String)> {
        suggestions
            .iter()
            .map(|suggestion| match &suggestion.item {
                Suggested::Work {
                    work,
                    number: Some(number),
                } => (suggestion.exact, number.clone(), work.title.clone()),
                other => panic!("Not a numbered work: {other:?}"),
            })
            .collect()
    };
    let suggest = async |typed: &str, composer: Option<i64>| {
        library
            .suggest(
                SuggestField::WorkNumber {
                    composer: composer.map(PersonRef::Linked),
                },
                typed,
                false,
                false,
            )
            .await
            .unwrap()
    };

    // The same number, however either was spelled
    assert_eq!(
        numbers(&suggest("op 9 no 2", None).await),
        [(
            true,
            "Op. 9 No. 2".to_string(),
            "Nocturne in E-flat major".to_string()
        )]
    );
    // A number the typed text has only begun is offered, but is not yet the number itself
    assert_eq!(
        numbers(&suggest("op9", None).await),
        [(
            false,
            "Op. 9 No. 2".to_string(),
            "Nocturne in E-flat major".to_string()
        )]
    );
    assert_eq!(
        numbers(&suggest("Op. 27", None).await),
        [(
            false,
            "Op. 27 No. 2".to_string(),
            "Nocturne in C-sharp minor".to_string()
        )]
    );
    // A fragment that parses as no number at all still reaches every number spelling it, through
    // the fuzzy tier, where the order is how well each matched rather than catalog order
    let mut seconds: Vec<String> = numbers(&suggest("no 2", None).await)
        .into_iter()
        .map(|(_, number, _)| number)
        .collect();
    seconds.sort();
    assert_eq!(seconds, ["Op. 27 No. 2", "Op. 3 No. 2", "Op. 9 No. 2"]);
    // A composer nobody answers to has no numbers to offer
    assert!(numbers(&suggest("no 2", Some(-1)).await).is_empty());
    let narrowed: Vec<String> = numbers(&suggest("no 2", Some(rachmaninoff)).await)
        .into_iter()
        .map(|(_, number, _)| number)
        .collect();
    assert_eq!(narrowed, ["Op. 3 No. 2"]);
    // A title reaches the numbers of the works carrying it, through the fuzzy tier. The
    // Rachmaninoff work of the same name has no number, so it has nothing to offer a number input.
    let by_title = numbers(&suggest("nocturne", None).await);
    assert!(by_title.iter().all(|(exact, ..)| !exact));
    let mut titled: Vec<String> = by_title.into_iter().map(|(_, number, _)| number).collect();
    titled.sort();
    assert_eq!(titled, ["Op. 27 No. 2", "Op. 9 No. 2"]);
}

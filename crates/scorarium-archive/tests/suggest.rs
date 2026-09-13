use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, PublicationRawInput, SuggestField,
    Suggested, Suggestion, WorkRawInput, WorkSummary,
};

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
    }
}

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

fn holding(kind: HoldingKind, location: &str) -> HoldingRawInput {
    HoldingRawInput {
        id: None,
        kind,
        location: location.into(),
    }
}

#[tokio::test]
async fn distinct_values_rank_by_match_and_list_whole_when_nothing_is_typed() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let input = PublicationRawInput {
        title: "Three gymnopedies".into(),
        publisher: "Dover".into(),
        holdings: vec![holding(HoldingKind::Physical, "Piano bench")],
        contributors: vec![contributor("Bob", "editor")],
        contents: vec![WorkRawInput {
            title: "Gymnopedie No. 1".into(),
            key: "G major".into(),
            tags: "christmas".into(),
            ..WorkRawInput::default()
        }],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&input.parse().unwrap())
        .await
        .unwrap();

    let plain = |exact: bool, item: Suggested| Suggestion { exact, item };
    let roles = library
        .suggest(SuggestField::Role, "", false)
        .await
        .unwrap();
    assert_eq!(
        roles,
        ["arranger", "author", "composer", "editor", "translator"]
            .map(|role| plain(false, Suggested::Role(role.into())))
    );
    assert_eq!(
        library
            .suggest(SuggestField::Publisher, "dovr", false)
            .await
            .unwrap(),
        [plain(false, Suggested::Publisher("Dover".into()))]
    );
    assert_eq!(
        library
            .suggest(SuggestField::Publisher, "dover", false)
            .await
            .unwrap(),
        [plain(true, Suggested::Publisher("Dover".into()))]
    );
    assert_eq!(
        library
            .suggest(SuggestField::Tag, "chr", false)
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
            .suggest(SuggestField::Location, "", false)
            .await
            .unwrap(),
        [plain(false, Suggested::Location("Piano bench".into()))]
    );
    // One letter has no typo budget
    assert!(
        library
            .suggest(SuggestField::Key, "F", false)
            .await
            .unwrap()
            .is_empty()
    );

    let private = archive.create_library("Books", true).await.unwrap();
    assert!(
        private
            .suggest(SuggestField::Publisher, "", true)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn entities_are_one_suggestion_each() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    // The editor is credited first, so only the lead rule puts the composer forward
    let nocturne = |composer: &str, numbers: &[&str]| WorkRawInput {
        title: "Nocturne".into(),
        contributors: vec![
            contributor("Sue", "editor"),
            contributor(composer, "composer"),
        ],
        catalog_numbers: numbers.iter().map(|n| (*n).into()).collect(),
        ..WorkRawInput::default()
    };
    let input = PublicationRawInput {
        title: "Nocturnes".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contributors: vec![contributor("Ann", "editor")],
        contents: vec![
            nocturne("Frederic Chopin", &["B. 49", "Op. 9 No. 2"]),
            nocturne("Gabriel Faure", &["Op. 33 No. 1"]),
        ],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&input.parse().unwrap())
        .await
        .unwrap();
    let book = PublicationRawInput {
        title: "The Devil's Dictionary".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contributors: vec![contributor("Ambrose Bierce", "author")],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&book.parse().unwrap())
        .await
        .unwrap();
    let essays = PublicationRawInput {
        title: "Essays: First Series".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contents: vec![WorkRawInput {
            title: "Self-Reliance".into(),
            contributors: vec![contributor("Ralph Waldo Emerson", "author")],
            ..WorkRawInput::default()
        }],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&essays.parse().unwrap())
        .await
        .unwrap();

    let works = library
        .suggest(SuggestField::Work, "nocturne", false)
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
        .suggest(SuggestField::Work, "self reliance", false)
        .await
        .unwrap();
    assert_eq!(
        credited(work_of(&essay[0])),
        Some(("Ralph Waldo Emerson", "author"))
    );

    let faure = library
        .suggest(SuggestField::Work, "nocturne faure", false)
        .await
        .unwrap();
    assert_eq!(faure.len(), 1);
    assert!(!faure[0].exact);

    let persons = library
        .suggest(SuggestField::Person, "bierce", false)
        .await
        .unwrap();
    // A book with no works counts as one work
    assert!(
        matches!(&persons[0].item, Suggested::Person(p) if p.name == "Ambrose Bierce" && p.works == 1)
    );

    assert!(
        library
            .suggest(SuggestField::Publication, "", false)
            .await
            .unwrap()
            .is_empty()
    );
    let dictionary = library
        .suggest(SuggestField::Publication, "devil", false)
        .await
        .unwrap();
    assert!(
        matches!(&dictionary[0].item, Suggested::Publication(p) if p.people == ["Ambrose Bierce"])
    );
}

#[tokio::test]
async fn work_numbers_rank_exact_then_prefix_then_fuzzy() {
    let archive = Archive::in_memory().await.unwrap();
    let library = archive.create_library("Sheet music", false).await.unwrap();
    let piece = |title: &str, composer: &str, number: &str| WorkRawInput {
        title: title.into(),
        contributors: vec![contributor(composer, "composer")],
        catalog_numbers: vec![number.into()],
        ..WorkRawInput::default()
    };
    let chopin = PublicationRawInput {
        title: "Nocturnes".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contents: vec![
            piece("Nocturne in E-flat major", "Frederic Chopin", "Op. 9 No. 2"),
            piece(
                "Nocturne in C-sharp minor",
                "Frederic Chopin",
                "Op. 27 No. 2",
            ),
        ],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&chopin.parse().unwrap())
        .await
        .unwrap();
    let rachmaninoff = PublicationRawInput {
        title: "Morceaux de fantaisie".into(),
        holdings: vec![holding(HoldingKind::Physical, "")],
        contents: vec![
            piece(
                "Prelude in C-sharp minor",
                "Sergei Rachmaninoff",
                "Op. 3 No. 2",
            ),
            WorkRawInput {
                title: "Nocturne".into(),
                contributors: vec![contributor("Sergei Rachmaninoff", "composer")],
                ..WorkRawInput::default()
            },
        ],
        ..PublicationRawInput::default()
    };
    library
        .create_publication(&rachmaninoff.parse().unwrap())
        .await
        .unwrap();

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
    let suggest = async |typed: &str, composer: Option<&str>| {
        library
            .suggest(
                SuggestField::WorkNumber {
                    composer: composer.map(str::to_string),
                },
                typed,
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
    // A composer nobody answers to narrows nothing
    assert_eq!(
        numbers(&suggest("no 2", Some("rachmaninof")).await).len(),
        3
    );
    let narrowed: Vec<String> = numbers(&suggest("no 2", Some("sergei rachmaninoff")).await)
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

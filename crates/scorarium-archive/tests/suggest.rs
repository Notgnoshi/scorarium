use scorarium_archive::{
    Archive, ContributorInput, HoldingKind, HoldingRawInput, PublicationRawInput, SuggestField,
    Suggested, Suggestion, WorkRawInput,
};

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
    }
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

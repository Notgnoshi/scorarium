#![expect(deprecated)]

use scorarium_engine::legacy::archive::Archive;

async fn demo() -> Archive {
    let archive = Archive::in_memory().await.unwrap();
    archive.populate_demo().await.unwrap();
    archive
}

/// Run a script, returning what it printed and how it ended.
async fn run(archive: &Archive, script: &str) -> (String, eyre::Result<()>) {
    let mut out = Vec::new();
    let result = scorarium_cli::run_script(archive, script.as_bytes(), &mut out).await;
    (String::from_utf8(out).unwrap(), result)
}

#[tokio::test]
async fn a_script_runs_until_quit() {
    let script =
        "# the demo libraries\n\nlibrary books show --help\nlibrary list\nexit\nlibrary list\n";
    let (out, result) = run(&demo().await, script).await;
    result.unwrap();
    assert!(
        out.starts_with("Show the contents of a library\n\nUsage: library <library> show\n"),
        "{out}"
    );
    assert!(
        out.ends_with("1\tBooks      \tprivate\n2\tSheet music\tpublic\n"),
        "{out}"
    );
}

#[tokio::test]
async fn a_failing_script_stops_at_the_failing_line() {
    let (out, result) = run(
        &demo().await,
        "library list\nlibrary \"list\nlibrary list\n",
    )
    .await;
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.starts_with("line 2: "), "{error}");
    assert_eq!(out.lines().count(), 2);
}

#[tokio::test]
async fn library_show_rejects_an_ambiguous_reference() {
    let archive = demo().await;
    // A library named after another library's id, and a second "Books" differing only in case
    let numeric = archive.create_library("1", false).await.unwrap();
    let duplicate = archive.create_library("books", false).await.unwrap();

    let (out, result) = run(&archive, "library 'sheet music' show\n").await;
    result.unwrap();
    assert_eq!(
        out,
        "id          \t2\nname        \tSheet music\nvisibility  \tpublic\npublications\t4\n"
    );

    let (out, result) = run(&archive, "library 2 show\n").await;
    result.unwrap();
    assert!(out.contains("name        \tSheet music\n"), "{out}");

    // The library named "1" and the library with id 1 both match, in name order
    let (_, result) = run(&archive, "library 1 show\n").await;
    let error = format!("{:#}", result.unwrap_err());
    assert_eq!(
        error,
        format!(
            "line 1: '1' names 2 libraries:\n  {}  1\n  1  Books",
            numeric.id
        )
    );

    let (_, result) = run(&archive, "library Books show\n").await;
    let error = format!("{:#}", result.unwrap_err());
    assert_eq!(
        error,
        format!(
            "line 1: 'Books' names 2 libraries:\n  1  Books\n  {}  books",
            duplicate.id
        )
    );
}

#[tokio::test]
async fn library_create_rename_delete() {
    let archive = demo().await;
    let script = "library create Magazines public\n\
                  library create 'Liner notes'\n\
                  library list\n\
                  library Magazines rename Periodicals\n\
                  library 'Liner notes' delete\n\
                  library list\n";
    let (out, result) = run(&archive, script).await;
    result.unwrap();
    assert_eq!(
        out,
        "1\tBooks      \tprivate\n\
         4\tLiner notes\tprivate\n\
         3\tMagazines  \tpublic\n\
         2\tSheet music\tpublic\n\
         1\tBooks      \tprivate\n\
         3\tPeriodicals\tpublic\n\
         2\tSheet music\tpublic\n"
    );

    let (_, result) = run(&archive, "library create '  '\n").await;
    assert_eq!(
        format!("{:#}", result.unwrap_err()),
        "line 1: library name is empty"
    );
}

#[tokio::test]
async fn the_selected_library_fills_the_subject_slot() {
    let archive = demo().await;
    let script = "library books select\nshow\nrename Novels\nshow\nlibrary deselect\nshow\n";
    let (out, result) = run(&archive, script).await;
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.starts_with("line 6: "), "{error}");
    assert!(
        error.ends_with("select a library to run its commands directly"),
        "{error}"
    );
    assert_eq!(
        out,
        "id          \t1\nname        \tBooks\nvisibility  \tprivate\npublications\t3\n\
         id          \t1\nname        \tNovels\nvisibility  \tprivate\npublications\t3\n"
    );
}

#[tokio::test]
async fn publication_show_resolves_by_title() {
    let (out, result) = run(&demo().await, "library books publication 'pro git' show\n").await;
    result.unwrap();
    assert_eq!(
        out,
        "id         \t2\t\n\
         title      \t \tPro Git\n\
         publisher  \t \tApress\n\
         year       \t \t2014\n\
         stars      \t \t3\n\
         identifier \t \tisbn 978-1-4842-0077-3\n\
         contributor\t \tScott Chacon (author)\n\
         contributor\t \tBen Straub (author)\n\
         holding    \t \tphysical\n\
         holding    \t \tdigital: pro-git.pdf\n\
         link       \t \thttps://openlibrary.org/books/OL26372169M/Pro_Git\n\
         link       \t \thttps://git-scm.com/book/en/v2\n"
    );
}

#[tokio::test]
async fn work_show_resolves_by_title() {
    let (out, result) = run(
        &demo().await,
        "library 'sheet music' work 'the isle of the dead' show\n",
    )
    .await;
    result.unwrap();
    assert_eq!(
        out,
        "id             \t7\t\n\
         title          \t \tThe Isle of the Dead\n\
         key            \t \tA minor\n\
         time signature \t \t5/8\n\
         instrumentation\t \tpiano\n\
         stars          \t \t4\n\
         tags           \t \ttranscription, want-to-learn\n\
         catalog number \t \tOp. 29\n\
         contributor    \t \tSergei Rachmaninoff (composer)\n\
         contributor    \t \tGeorgy Kirkor (arranger)\n\
         publication    \t6\tThe Isle of the Dead\n\
         link           \t \thttps://imslp.org/wiki/Isle_of_the_Dead,_Op.29_(Rachmaninoff,_Sergei)\n\
         link           \t \thttps://musicbrainz.org/work/ab65bc19-0079-31a9-9521-5f6ea4c1c637\n\
         link           \t \thttps://en.wikipedia.org/wiki/Isle_of_the_Dead_(Rachmaninoff)\n\
         link           \t \thttps://www.wikidata.org/wiki/Q629711\n"
    );
}

#[tokio::test]
async fn person_list_shows_everyone_credited() {
    let archive = demo().await;
    let (out, result) = run(&archive, "library books person list\n").await;
    result.unwrap();
    assert_eq!(
        out,
        "4\tAmbrose Bierce\tThe Collected Writings of Ambrose Bierce +3\n\
         2\tScott Chacon  \tPro Git\n\
         1\tDrew Neil     \tPractical Vim\n\
         3\tBen Straub    \tPro Git\n"
    );

    let (out, result) = run(&archive, "library books person list --role composer\n").await;
    result.unwrap();
    assert_eq!(out, "");
}

#[tokio::test]
async fn person_show_lists_each_credit_once() {
    let (out, result) = run(
        &demo().await,
        "library 'sheet music' person 'sergei rachmaninoff' show\n",
    )
    .await;
    result.unwrap();
    assert_eq!(
        out,
        "id         \t10\t\n\
         name       \t  \tSergei Rachmaninoff\n\
         sort name  \t  \tRachmaninoff, Sergei\n\
         link       \t  \thttps://imslp.org/wiki/Category:Rachmaninoff,_Sergei\n\
         link       \t  \thttps://en.wikipedia.org/wiki/Sergei_Rachmaninoff\n\
         link       \t  \thttps://www.wikidata.org/wiki/Q131861\n\
         publication\t5 \tRachmaninoff masterpieces for solo piano (composer)\n\
         publication\t4 \tRussian piano album (composer)\n\
         publication\t6 \tThe Isle of the Dead (composer)\n\
         work       \t6 \tPolichinelle (composer)\n\
         work       \t4 \tPrelude in C-sharp minor (composer)\n\
         work       \t5 \tEtude-Tableau (composer)\n\
         work       \t7 \tThe Isle of the Dead (composer)\n"
    );
}

#[tokio::test]
async fn tag_list_orders_by_use_count() {
    let (out, result) = run(&demo().await, "library 'sheet music' tag list\n").await;
    result.unwrap();
    assert_eq!(
        out,
        "want-to-learn\t3\n\
         russian      \t2\n\
         anthology    \t1\n\
         learning     \t1\n\
         memorized    \t1\n\
         transcription\t1\n"
    );
}

#[tokio::test]
async fn search_prints_the_same_columns_for_every_kind() {
    let (out, result) = run(
        &demo().await,
        "search polichinelle\nsearch 'pro git'\nsearch kirkor\n",
    )
    .await;
    result.unwrap();
    assert_eq!(
        out,
        "work\t2\tSheet music\t6\tPolichinelle\tOp. 3 No. 4 by Sergei Rachmaninoff\n\
         publication\t1\tBooks\t2\tPro Git\tScott Chacon, Ben Straub\n\
         person     \t2\tSheet music\t11\tGeorgy Kirkor       \tThe Isle of the Dead +1\n\
         publication\t2\tSheet music\t6 \tThe Isle of the Dead\tSergei Rachmaninoff, Georgy Kirkor\n\
         work       \t2\tSheet music\t7 \tThe Isle of the Dead\tOp. 29 by Sergei Rachmaninoff\n"
    );
}

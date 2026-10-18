#![expect(deprecated)]

use scorarium_engine::archive::Archive;

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

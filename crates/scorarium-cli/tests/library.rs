use scorarium_archive::Archive;

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
    let script = "# the demo libraries\n\nhelp library list\nlibrary list\nexit\nlibrary list\n";
    let (out, result) = run(&demo().await, script).await;
    result.unwrap();
    assert!(out.starts_with("List every library\n"), "{out}");
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
async fn library_show_resolves_by_name_before_id() {
    let archive = demo().await;
    // A library named after another library's id, and a second "Books" differing only in case
    let numeric = archive.create_library("1", false).await.unwrap();
    let duplicate = archive.create_library("books", false).await.unwrap();

    let (out, result) = run(&archive, "library show 'sheet music'\n").await;
    result.unwrap();
    assert_eq!(
        out,
        "id          \t2\nname        \tSheet music\nvisibility  \tpublic\npublications\t4\n"
    );

    let (out, result) = run(&archive, "library show 2\n").await;
    result.unwrap();
    assert!(out.contains("name        \tSheet music\n"), "{out}");

    // The name "1" wins over the id 1
    let (out, result) = run(&archive, "library show 1\n").await;
    result.unwrap();
    assert!(
        out.contains(&format!("id          \t{}\n", numeric.id)),
        "{out}"
    );

    let (_, result) = run(&archive, "library show Books\n").await;
    let error = format!("{:#}", result.unwrap_err());
    assert_eq!(
        error,
        format!(
            "line 1: 2 libraries are named 'Books': ids 1, {}",
            duplicate.id
        )
    );
}

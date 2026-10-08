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
    let script = "# the demo libraries\n\nhelp library list\nlibrary list\nquit\nlibrary list\n";
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

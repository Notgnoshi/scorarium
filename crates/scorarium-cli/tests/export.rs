use scorarium_cli::{ExportLevel, export_script, run_script};
use scorarium_engine::Engine;
use scorarium_engine::library::Library;

async fn library(engine: &Engine, name: &str) -> Library {
    let libraries = engine.libraries().await.unwrap();
    libraries.into_iter().find(|l| l.name() == name).unwrap()
}

async fn export(library: &Library, level: ExportLevel) -> String {
    let mut out = Vec::new();
    export_script(library, level, &mut out).await.unwrap();
    String::from_utf8(out).unwrap()
}

#[tokio::test]
async fn round_trip_exports() {
    let (demo, _demo_dir) = scorarium_test::demo_engine().await;
    let source = library(&demo, "Sheet music").await;
    let exported = export(&source, ExportLevel::Publications).await;

    let (fresh, _fresh_dir) = scorarium_test::empty_engine().await;
    run_script(&fresh, exported.as_bytes(), &mut Vec::new(), false)
        .await
        .unwrap();
    let replayed = library(&fresh, "Sheet music").await;

    assert_eq!(export(&replayed, ExportLevel::Publications).await, exported);
    assert_eq!(
        replayed.publication_count(false).await.unwrap(),
        source.publication_count(false).await.unwrap()
    );
}

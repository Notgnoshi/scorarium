use scorarium_cli::{ExportLevel, export_script, run_script};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
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

async fn committed_works(library: &Library) -> usize {
    let works = library.works().await.unwrap();
    works.iter().filter(|w| !w.is_draft()).count()
}

#[tokio::test]
async fn round_trip_exports() {
    let (demo, _demo_dir) = scorarium_test::demo_engine().await;
    let source = library(&demo, "Sheet music").await;
    for level in [ExportLevel::Publications, ExportLevel::Works] {
        let exported = export(&source, level).await;

        let (fresh, _fresh_dir) = scorarium_test::empty_engine().await;
        run_script(&fresh, exported.as_bytes(), &mut Vec::new(), false)
            .await
            .unwrap();
        let replayed = library(&fresh, "Sheet music").await;

        assert_eq!(export(&replayed, level).await, exported);
        assert_eq!(
            replayed.publication_count(false).await.unwrap(),
            source.publication_count(false).await.unwrap()
        );
        let works = match level {
            ExportLevel::Publications => 0,
            ExportLevel::Works => committed_works(&source).await,
        };
        assert_eq!(committed_works(&replayed).await, works);
    }
}

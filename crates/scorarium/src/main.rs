use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use scorarium::{AppState, db, demo, router};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// A physical and digital sheet music library.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Address and port to serve on.
    #[arg(short, long, env = "SCORARIUM_BIND", default_value = "0.0.0.0:3000")]
    bind: SocketAddr,

    /// Default log level. RUST_LOG overrides this when set.
    #[arg(short, long, default_value = "debug")]
    log_level: LevelFilter,

    /// Directory holding the database and managed files. Created if missing.
    #[arg(short, long, env = "SCORARIUM_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    /// Serve an in-memory demo library instead of the data directory.
    #[arg(long)]
    demo: bool,

    /// Allow the login cookie over plain HTTP
    #[arg(long, env = "SCORARIUM_INSECURE_COOKIES")]
    insecure_cookies: bool,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(args.log_level.into())
                .from_env_lossy(),
        )
        .init();

    let state = if args.demo {
        let pool = db::connect_in_memory().await?;
        demo::populate(&pool).await?;
        tracing::info!(bind = %args.bind, "starting scorarium with in-memory demo data");
        AppState::demo(pool)
    } else {
        tracing::info!(
            bind = %args.bind,
            data_dir = %args.data_dir.display(),
            "starting scorarium"
        );
        let pool = db::connect(&args.data_dir).await?;
        AppState::new(pool, !args.insecure_cookies)
    };
    let app = router(Arc::new(state));
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
    let mut terminate = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    tokio::select! {
        _ = interrupt.recv() => {},
        _ = terminate.recv() => {},
    }
}

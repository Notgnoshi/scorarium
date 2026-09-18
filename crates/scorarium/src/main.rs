use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use scorarium::{AppState, router};
use scorarium_archive::Archive;
use scorarium_client::{Client, UserAgent};
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

    /// Contact email used in the User-Agent used for open metadata APIs
    ///
    /// Some APIs allow higher request rates if you specify a contact email.
    #[arg(long, env = "SCORARIUM_CONTACT")]
    contact: Option<String>,
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

    let archive = if args.demo {
        let archive = Archive::in_memory().await?;
        archive.populate_demo().await?;
        tracing::info!(bind = %args.bind, "starting scorarium with in-memory demo data");
        archive
    } else {
        tracing::info!(
            bind = %args.bind,
            data_dir = %args.data_dir.display(),
            "starting scorarium"
        );
        Archive::open(&args.data_dir).await?
    };

    if args.contact.is_none() {
        tracing::warn!(
            "No contact set; set SCORARIUM_CONTACT to get faster metadata API rate limits"
        );
    }
    let sources = Client::new(UserAgent::new(args.contact.as_deref()))?;

    let state = if args.demo {
        AppState::demo(archive, sources)
    } else {
        AppState::new(archive, sources, !args.insecure_cookies)
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

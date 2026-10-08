use std::net::SocketAddr;
use std::sync::Arc;

use scorarium_archive::Archive;
use scorarium_client::{Client, UserAgent};

use crate::{AppState, router};

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Address and port to serve on.
    #[arg(short, long, env = "SCORARIUM_BIND", default_value = "0.0.0.0:3000")]
    pub bind: SocketAddr,

    /// Allow the login cookie over plain HTTP
    #[arg(long, env = "SCORARIUM_INSECURE_COOKIES")]
    pub insecure_cookies: bool,
}

/// Serve the given archive until interrupted.
pub async fn serve(
    args: ServeArgs,
    archive: Archive,
    demo: bool,
    contact: Option<&str>,
) -> eyre::Result<()> {
    tracing::info!(bind = %args.bind, "starting scorarium");

    if contact.is_none() {
        tracing::warn!(
            "No contact set; set SCORARIUM_CONTACT to get faster metadata API rate limits"
        );
    }
    let sources = Client::new(UserAgent::new(contact))?;

    let state = if demo {
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

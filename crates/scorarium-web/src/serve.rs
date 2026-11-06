use std::net::SocketAddr;

use scorarium_engine::Engine;
use tokio::signal::unix::{SignalKind, signal};

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Address and port to serve on.
    #[arg(short, long, env = "SCORARIUM_BIND", default_value = "0.0.0.0:3000")]
    pub bind: SocketAddr,

    /// Allow the login cookie over plain HTTP
    #[arg(long, env = "SCORARIUM_INSECURE_COOKIES")]
    pub insecure_cookies: bool,
}

pub async fn serve(args: ServeArgs, engine: Engine) -> eyre::Result<()> {
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(bind = %args.bind, "serving");
    axum::serve(listener, crate::router::router(engine))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let mut terminate = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

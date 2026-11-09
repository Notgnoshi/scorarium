use std::net::SocketAddr;

use scorarium_engine::Engine;
use tokio::signal::unix::{SignalKind, signal};

use crate::router::{Settings, router};

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Address and port to serve on.
    #[arg(short, long, env = "SCORARIUM_BIND", default_value = "0.0.0.0:3000")]
    pub bind: SocketAddr,

    /// Allow the login cookie over plain HTTP
    #[arg(long, env = "SCORARIUM_INSECURE_COOKIES")]
    pub insecure_cookies: bool,

    /// Let visitors to a demo upload files and fetch them from URLs
    ///
    /// Has no effect without --demo.
    #[arg(long, env = "SCORARIUM_DEMO_FILES")]
    pub demo_files: bool,
}

pub async fn serve(args: ServeArgs, engine: Engine) -> eyre::Result<()> {
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(bind = %args.bind, "serving");
    let settings = Settings {
        secure_cookies: !args.insecure_cookies,
        demo_files: args.demo_files,
    };
    if engine.is_demo() && args.demo_files {
        tracing::warn!(
            "demo files are enabled; anonymous users can upload files and make the server fetch \
             any URL"
        );
    }
    axum::serve(listener, router(engine, settings))
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

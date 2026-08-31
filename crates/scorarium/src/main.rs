use std::net::SocketAddr;

use clap::Parser;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// A physical and digital sheet music library.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Address and port to serve on.
    #[arg(short, long, env = "SCORARIUM_BIND", default_value = "127.0.0.1:3000")]
    bind: SocketAddr,

    /// Default log level. RUST_LOG overrides this when set.
    #[arg(short, long, default_value = "debug")]
    log_level: LevelFilter,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(args.log_level.into())
                .from_env_lossy(),
        )
        .init();
    tracing::info!(bind = %args.bind, "starting scorarium");

    Ok(())
}

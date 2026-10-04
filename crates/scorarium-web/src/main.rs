use std::path::PathBuf;

use clap::Parser;
use scorarium_web::ServeArgs;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// A physical and digital sheet music library.
#[derive(Debug, Parser)]
#[command(name = "scorarium", version)]
struct Args {
    /// Default log level. RUST_LOG overrides this when set.
    #[arg(short, long, default_value = "debug")]
    log_level: LevelFilter,

    /// Directory holding the database and managed files. Created if missing.
    #[arg(short, long, env = "SCORARIUM_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    /// Serve an in-memory demo library instead of the data directory.
    #[arg(long)]
    demo: bool,

    /// Contact email used in the User-Agent used for open metadata APIs
    ///
    /// Some APIs allow higher request rates if you specify a contact email.
    #[arg(long, env = "SCORARIUM_CONTACT")]
    contact: Option<String>,

    #[command(flatten)]
    serve: ServeArgs,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(args.log_level.into())
                .from_env_lossy(),
        )
        .init();

    scorarium_web::serve(
        args.serve,
        &args.data_dir,
        args.demo,
        args.contact.as_deref(),
    )
    .await
}

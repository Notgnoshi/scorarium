use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use scorarium_archive::Archive;
use scorarium_web::ServeArgs;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// A physical and digital sheet music library.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Default log level. RUST_LOG overrides this when set.
    #[arg(short, long, global = true, default_value = "debug")]
    log_level: LevelFilter,

    /// Directory holding the database and managed files. Created if missing.
    #[arg(
        short,
        long,
        global = true,
        env = "SCORARIUM_DATA_DIR",
        default_value = "./data"
    )]
    data_dir: PathBuf,

    /// Use an in-memory demo library instead of the data directory.
    #[arg(long, global = true)]
    demo: bool,

    /// Contact email used in the User-Agent used for open metadata APIs
    ///
    /// Some APIs allow higher request rates if you specify a contact email.
    #[arg(long, global = true, env = "SCORARIUM_CONTACT")]
    contact: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the web server.
    Serve(ServeArgs),
}

/// Open the archive the global flags point at.
async fn open_archive(data_dir: &Path, demo: bool, migrate: bool) -> color_eyre::Result<Archive> {
    if demo {
        tracing::info!("using an in-memory demo library");
        let archive = Archive::in_memory().await?;
        archive.populate_demo().await?;
        Ok(archive)
    } else {
        tracing::info!(data_dir = %data_dir.display(), "opening library");
        Ok(Archive::open(data_dir, migrate).await?)
    }
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

    match args.command {
        Command::Serve(serve) => {
            let archive = open_archive(&args.data_dir, args.demo, true).await?;
            scorarium_web::serve(serve, archive, args.demo, args.contact.as_deref()).await?;
        }
    }
    Ok(())
}

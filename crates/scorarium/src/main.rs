use std::path::PathBuf;

use clap::{Parser, Subcommand};
use scorarium_cli::{LogWriter, ShellArgs};
use scorarium_client::UserAgent;
use scorarium_engine::{Engine, EngineOptions};
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

    /// Run developer shell to interact with the Scorarium database
    Shell(ShellArgs),
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let filter = EnvFilter::builder()
        .with_default_directive(args.log_level.into())
        .from_env_lossy();

    let log = LogWriter::default();

    match args.command {
        Command::Serve(_) => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
        Command::Shell(_) => {
            // Use a custom writer so that the logs don't corrupt the interactive prompt
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer({
                    let log = log.clone();
                    move || log.clone()
                })
                .init();
        }
    }

    if args.contact.is_none() {
        tracing::warn!("some enrichment APIs grant higher rate limits when a contact email is set");
    }

    if matches!(args.command, Command::Shell(_)) && !args.demo && !args.data_dir.is_dir() {
        color_eyre::eyre::bail!("data directory {} does not exist", args.data_dir.display());
    }

    // The shell an share the database with a running server, so only the server is allowed to migrate
    let migrate = matches!(args.command, Command::Serve(_));
    let options = EngineOptions::new(UserAgent::new(args.contact.as_deref()))?;
    let engine = if args.demo {
        tracing::info!("using an in-memory demo library");
        Engine::demo(options).await?
    } else {
        tracing::info!(data_dir = %args.data_dir.display(), "opening library");
        Engine::open(&args.data_dir, options.migrate_on_startup(migrate)).await?
    };

    match args.command {
        Command::Serve(serve) => scorarium_web::serve(serve, engine).await?,
        Command::Shell(shell) => {
            // The demo library is throwaway, so its history is too
            let history = (!args.demo).then(|| args.data_dir.join(".shell_history"));
            scorarium_cli::shell(shell, engine, history, log).await?;
        }
    }
    Ok(())
}

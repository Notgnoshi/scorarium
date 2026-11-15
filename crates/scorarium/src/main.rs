use std::path::PathBuf;

use clap::{Parser, Subcommand};
use scorarium_cli::{LogWriter, ShellArgs};
use scorarium_client::{Client, UserAgent};
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

    /// Directory holding the database and managed files. Created if missing. Ignored in --demo mode
    #[arg(
        short,
        long,
        global = true,
        env = "SCORARIUM_DATA_DIR",
        default_value = "./data"
    )]
    data_dir: PathBuf,

    /// Directory holding cached API responses. Created if missing.
    ///
    /// Defaults to `cache` inside the data directory.
    #[arg(long, global = true, env = "SCORARIUM_CACHE_DIR")]
    cache_dir: Option<PathBuf>,

    /// Use a demo library in a temporary data directory. Overrides --data-dir
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

    let temp_dir = args.demo.then(tempfile::tempdir).transpose()?;
    let data_dir = match &temp_dir {
        Some(dir) => dir.path().to_path_buf(),
        None => args.data_dir,
    };

    // The shell an share the database with a running server, so only the server is allowed to migrate
    let migrate = matches!(args.command, Command::Serve(_));
    let user_agent = UserAgent::new(args.contact.as_deref());
    let cache_dir = args.cache_dir.unwrap_or_else(|| data_dir.join("cache"));
    tracing::info!(cache_dir = %cache_dir.display(), "caching API responses");
    let client = Client::builder(user_agent.clone())
        .cache_dir(cache_dir)
        .build()?;
    let options = EngineOptions::new(user_agent)?.client(client);
    let engine = if args.demo {
        tracing::info!(data_dir = %data_dir.display(), "opening a temporary demo library");
        Engine::demo(&data_dir, options).await?
    } else {
        tracing::info!(data_dir = %data_dir.display(), "opening library");
        Engine::open(&data_dir, options.migrate_on_startup(migrate)).await?
    };

    match args.command {
        Command::Serve(serve) => scorarium_web::serve(serve, engine).await?,
        Command::Shell(shell) => {
            let history = Some(data_dir.join(".shell_history"));
            scorarium_cli::shell(shell, engine, history, log).await?;
        }
    }
    Ok(())
}

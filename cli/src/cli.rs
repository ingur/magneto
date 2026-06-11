use anyhow::{Context, Result, bail};
use base64::Engine;
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Parser, Subcommand};

use magneto_core::client;
use magneto_core::config;
use magneto_core::protocol;
use magneto_core::supervisor;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::BrightGreen.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::BrightGreen.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::BrightCyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::BrightRed.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::BrightCyan.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::BrightYellow.on_default().effects(Effects::BOLD));

#[derive(Parser)]
#[command(
    name = "magneto",
    version,
    about = "Media-focused torrent client",
    styles = STYLES,
    arg_required_else_help = true,
)]
struct Cli {
    #[arg(value_name = "SOURCE")]
    sources: Vec<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(hide = true)]
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    Start,
    Stop,
    Restart,
    Status,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Daemon { action }) => match action {
            DaemonAction::Start => daemon_start().await,
            DaemonAction::Stop => daemon_stop().await,
            DaemonAction::Restart => daemon_restart().await,
            DaemonAction::Status => daemon_status().await,
        },
        None => add_sources(cli.sources).await,
    }
}

async fn daemon_start() -> Result<()> {
    let data_dir = config::data_dir()?;
    let (port, started) = supervisor::ensure_running(&data_dir).await?;
    if started {
        println!("daemon started on port {port}");
    } else {
        println!("daemon already running on port {port}");
    }
    Ok(())
}

async fn daemon_stop() -> Result<()> {
    let data_dir = config::data_dir()?;
    supervisor::stop(&data_dir).await?;
    println!("daemon stopped");
    Ok(())
}

async fn daemon_restart() -> Result<()> {
    let data_dir = config::data_dir()?;
    let port = supervisor::restart(&data_dir).await?;
    println!("daemon restarted on port {port}");
    Ok(())
}

async fn daemon_status() -> Result<()> {
    let data_dir = config::data_dir()?;
    match supervisor::discover(&data_dir).await {
        supervisor::Discovery::Running { port } => {
            println!("running on port {port}");
            Ok(())
        }
        _ => bail!("daemon not running"),
    }
}

async fn add_sources(sources: Vec<String>) -> Result<()> {
    if sources.is_empty() {
        bail!("no sources provided");
    }
    let data_dir = config::data_dir()?;
    let (port, _started) = supervisor::ensure_running(&data_dir).await?;
    let token = supervisor::read_descriptor(&data_dir).and_then(|d| d.control_token);
    let total = sources.len();
    let mut failures = 0u32;
    for source in sources {
        let prepared = match prepare_source(&source) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{source}: error: {e}");
                failures += 1;
                continue;
            }
        };
        match client::run_raw(
            port,
            "add_torrent",
            serde_json::json!({ "source": prepared }),
            token.as_deref(),
        )
        .await
        {
            Ok(result) => println!(
                "{source} → {}",
                serde_json::to_string(&result).unwrap_or_else(|_| "{}".into())
            ),
            Err(e) => {
                eprintln!("{source}: error: {e}");
                failures += 1;
            }
        }
    }
    if failures > 0 {
        bail!("{failures} of {total} source(s) failed");
    }
    Ok(())
}

fn prepare_source(input: &str) -> Result<String> {
    if protocol::is_direct_source(input) {
        return Ok(input.to_string());
    }
    let bytes =
        std::fs::read(input).with_context(|| format!("reading torrent file {input}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

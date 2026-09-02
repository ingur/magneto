use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

use magneto_daemon::daemon::bootstrap;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let config_path = magneto_core::config::config_path()?;
    let data_dir = magneto_core::config::data_dir()?;
    let metadata_path = data_dir.join("metadata.json");
    init_tracing(&data_dir);
    #[cfg(feature = "deadlock-detection")]
    spawn_deadlock_checker();

    // The engine holds one descriptor per file of every torrent for as long as
    // it is loaded, so a soft limit of 1024 runs out on a library of packs.
    match librqbit::try_increase_nofile_limit() {
        Ok(limit) => tracing::debug!(limit, "file descriptor limit"),
        Err(e) => tracing::warn!(error = %e, "could not raise the file descriptor limit"),
    }

    let kind = bootstrap::run(config_path, data_dir, metadata_path).await?;
    tracing::info!(?kind, "daemon exited");
    Ok(())
}

#[cfg(feature = "deadlock-detection")]
fn spawn_deadlock_checker() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let deadlocks = parking_lot::deadlock::check_deadlock();
            if deadlocks.is_empty() {
                continue;
            }
            tracing::error!("{} parking_lot deadlock(s) detected", deadlocks.len());
            for (i, threads) in deadlocks.iter().enumerate() {
                for t in threads {
                    tracing::error!(
                        "deadlock #{i}, thread {:?}:\n{:?}",
                        t.thread_id(),
                        t.backtrace()
                    );
                }
            }
        }
    });
}

fn init_tracing(data_dir: &Path) {
    let _ = std::fs::create_dir_all(data_dir);
    let stdout_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("magneto_daemon=info,magneto_core=info,librqbit=warn"));
    let ansi = std::io::stdout().is_terminal();
    let stdout = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(ansi)
        .with_filter(stdout_filter);

    let log_path = data_dir.join("magneto.log");
    let file_writer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();
    let file_layer = file_writer.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(f)
            .with_filter(EnvFilter::new("magneto_daemon=debug,magneto_core=debug,librqbit=warn"))
    });

    tracing_subscriber::registry().with(stdout).with(file_layer).init();
}

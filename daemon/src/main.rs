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

// Peer chatter and cold DHT lookups warn per chunk and per round, and say
// nothing actionable, so they stay out of the log at warn level.
const LIBRQBIT_FILTER: &str =
    "librqbit=warn,librqbit::torrent_state::live=error,librqbit_dht::dht=error";

// The log is append-only across runs, so the previous file is kept and the
// current one is started over once it outgrows this.
const LOG_ROTATE_BYTES: u64 = 8 * 1024 * 1024;

async fn run() -> Result<()> {
    let config_path = magneto_core::config::config_path()?;
    let data_dir = magneto_core::config::data_dir()?;
    let metadata_path = data_dir.join("metadata.json");
    let rotated = init_tracing(&data_dir);
    if rotated {
        tracing::debug!("rotated magneto.log");
    }
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

fn init_tracing(data_dir: &Path) -> bool {
    let _ = std::fs::create_dir_all(data_dir);
    let stdout_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("magneto_daemon=info,magneto_core=info,{LIBRQBIT_FILTER}"))
    });
    let ansi = std::io::stdout().is_terminal();
    let stdout = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(ansi)
        .with_filter(stdout_filter);

    let log_path = data_dir.join("magneto.log");
    let rotated = std::fs::metadata(&log_path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES)
        && std::fs::rename(&log_path, data_dir.join("magneto.log.1")).is_ok();
    let file_writer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();
    let file_layer = file_writer.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(f)
            .with_filter(EnvFilter::new(format!(
                "magneto_daemon=debug,magneto_core=debug,{LIBRQBIT_FILTER}"
            )))
    });

    tracing_subscriber::registry().with(stdout).with(file_layer).init();
    rotated
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, Layer};

    use super::*;

    struct Targets(Arc<Mutex<Vec<&'static str>>>);

    impl<S: Subscriber> Layer<S> for Targets {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            self.0.lock().push(event.metadata().target());
        }
    }

    #[test]
    fn librqbit_noise_is_dropped_at_warn_only() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let filter = EnvFilter::new(format!("magneto_daemon=debug,{LIBRQBIT_FILTER}"));
        let subscriber =
            tracing_subscriber::registry().with(Targets(seen.clone()).with_filter(filter));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "librqbit::torrent_state::live", "chunk chatter");
            tracing::warn!(target: "librqbit_dht::dht", "no successful lookups");
            tracing::error!(target: "librqbit::torrent_state::live", "kept");
            tracing::warn!(target: "librqbit::session", "kept");
            tracing::debug!(target: "magneto_daemon::daemon", "kept");
            tracing::debug!(target: "librqbit::session", "dropped");
        });
        assert_eq!(
            *seen.lock(),
            ["librqbit::torrent_state::live", "librqbit::session", "magneto_daemon::daemon"]
        );
    }
}

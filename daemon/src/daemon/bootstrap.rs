use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use magneto_core::config::Config;
use crate::daemon::{
    Daemon, DaemonEvent, ShutdownKind, commands, control, install_shutdown_listeners, lan,
    preflight, stats, upnp,
};
use magneto_core::supervisor;

pub async fn run(config_path: PathBuf, data_dir: PathBuf, metadata_path: PathBuf) -> Result<ShutdownKind> {
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;

    // Refuse a second instance: two daemons over one session dir corrupt state.
    if let supervisor::Discovery::Running { port } = supervisor::discover(&data_dir).await {
        return Err(anyhow!("daemon already running on port {port}"));
    }
    // The descriptor check above is the friendly diagnostic; this lock is the
    // actual mutual exclusion (the descriptor only lands at commit, so two
    // starts can race past the check). Held for the process lifetime and
    // released by the OS on any exit, including a kill. The retry window
    // covers a stop/restart overlap: the outgoing daemon holds the lock
    // through cleanup (≤10s) and task joins (≤5s).
    let _lock = lock_data_dir(&data_dir).await?;

    // An unusable config.toml (a hand-edit gone wrong) must not dead-end the
    // daemon: fall back exactly like a failed start does. The UI then shows
    // the running config, and the next settings save rewrites the file.
    let desired = match Config::load_or_create(&config_path) {
        Ok(config) => config,
        Err(e) => {
            warn!(error = %e, "config.toml unusable; starting from last-known-good");
            load_last_good(&data_dir).unwrap_or_default()
        }
    };

    // Installed once, above the config choice, so a failed attempt never
    // orphans signal listeners.
    let (inbox_tx, inbox_rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();
    install_shutdown_listeners(inbox_tx.clone());

    // Only the config-dependent checks get a second candidate. Everything past
    // them owns the session directory, and two engines over one session
    // directory corrupt it.
    let mut started = None;
    let mut last_err = None;
    for (idx, candidate) in candidate_configs(&desired, &data_dir).into_iter().enumerate() {
        match preflight_config(&candidate) {
            Ok(()) => {
                started = Some(candidate);
                break;
            }
            Err(e) => {
                if idx == 0 {
                    warn!(error = %e, "primary config unusable; trying last-known-good");
                }
                last_err = Some(e);
            }
        }
    }
    let started = match started {
        Some(config) => config,
        None => return Err(last_err.unwrap_or_else(|| anyhow!("daemon failed to start"))),
    };

    let daemon = start_once(&desired, started, &config_path, &data_dir, &metadata_path, &inbox_tx, &cancel)
        .await?;
    daemon.run(inbox_rx).await
}

/// The checks that depend on the config: a contested port or an unwritable
/// downloads dir means this candidate cannot run.
fn preflight_config(config: &Config) -> Result<()> {
    preflight::probe_bind([127, 0, 0, 1], config.network.control_port)
        .with_context(|| format!("control port {} unavailable", config.network.control_port))?;
    preflight::probe_dir(&config.downloads.dir)
        .with_context(|| format!("downloads dir {} not writable", config.downloads.dir.display()))
}

async fn start_once(
    desired: &Config,
    started: Config,
    config_path: &Path,
    data_dir: &Path,
    metadata_path: &Path,
    inbox_tx: &mpsc::Sender<DaemonEvent>,
    cancel: &CancellationToken,
) -> Result<Daemon> {
    let mut daemon = Daemon::new(
        desired.clone(),
        started.clone(),
        config_path.to_path_buf(),
        data_dir.to_path_buf(),
        metadata_path.to_path_buf(),
        inbox_tx.clone(),
        cancel.clone(),
    )
    .await?;

    // Per-run token guarding the control WebSocket. Handed to the listener (to
    // validate the ?token= query) and written into daemon.json by commit() so
    // local clients can read it.
    let control_token = generate_token();
    let control_task = control::spawn(
        cancel.clone(),
        inbox_tx.clone(),
        daemon.session.clone(),
        daemon.metadata.clone(),
        started.network.control_port,
        control_token.clone(),
    )
    .await?;
    daemon.control_task = Some(control_task);

    daemon.stats_task = Some(stats::spawn(
        cancel.clone(),
        inbox_tx.clone(),
        daemon.session.clone(),
        daemon.metadata.clone(),
        daemon.config_tx.subscribe(),
    ));

    // DLNA is optional: a contested LAN/SSDP port degrades to control-only rather
    // than failing the start.
    if started.network.upnp_enabled {
        match upnp::spawn(cancel.clone(), daemon.session.clone(), daemon.metadata.clone(), &started).await {
            Ok((ssdp_task, upnp_router)) => {
                match lan::spawn(
                    cancel.clone(),
                    inbox_tx.clone(),
                    daemon.session.clone(),
                    daemon.metadata.clone(),
                    upnp_router,
                    started.network.lan_port,
                )
                .await
                {
                    Ok(lan_task) => {
                        daemon.upnp_ssdp = Some(ssdp_task);
                        daemon.lan_task = Some(lan_task);
                        daemon.upnp_active = true;
                    }
                    Err(e) => {
                        warn!(error = %e, "LAN listener failed to bind; continuing control-only");
                        ssdp_task.abort();
                    }
                }
            }
            Err(e) => warn!(error = %e, "UPnP server failed to start; continuing control-only"),
        }
    }

    commit(&mut daemon, &started, &control_token).await;
    Ok(daemon)
}

/// Exclusive lock on `{data_dir}/daemon.lock`. The file stays open (and so
/// locked) for the daemon's whole life; the content is irrelevant.
async fn lock_data_dir(data_dir: &Path) -> Result<std::fs::File> {
    let path = data_dir.join("daemon.lock");
    let file = std::fs::File::create(&path)
        .with_context(|| format!("creating {}", path.display()))?;
    for _ in 0..60 {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("locking {}", path.display()));
            }
        }
    }
    Err(anyhow!("another magneto-daemon holds the data directory lock"))
}

/// Random 256-bit control token, hex-encoded. `rand::rng()` is a CSPRNG, so the
/// token is unguessable by anything that can't read the descriptor file.
fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// All persistent side effects happen here, once, after a fully successful start.
async fn commit(daemon: &mut Daemon, started: &Config, control_token: &str) {
    commands::reconcile(daemon).await;
    if let Err(e) = supervisor::write_descriptor(
        &daemon.data_dir,
        started.network.control_port,
        Some(control_token),
    ) {
        warn!(error = %e, "failed to write runtime descriptor");
    }
    if let Err(e) = write_last_good(&daemon.data_dir, started) {
        warn!(error = %e, "failed to write last-good config");
    }
}

fn candidate_configs(desired: &Config, data_dir: &Path) -> Vec<Config> {
    let mut candidates = vec![desired.clone()];
    if let Some(last_good) = load_last_good(data_dir)
        && &last_good != desired
    {
        candidates.push(last_good);
    }
    candidates
}

fn last_good_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.last-good.toml")
}

fn load_last_good(data_dir: &Path) -> Option<Config> {
    let text = std::fs::read_to_string(last_good_path(data_dir)).ok()?;
    toml::from_str(&text).ok()
}

fn write_last_good(data_dir: &Path, config: &Config) -> Result<()> {
    config.save(&last_good_path(data_dir))
}

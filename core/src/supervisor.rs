//! Subprocess supervision shared by the CLI and any host (e.g. a Tauri app).
//!
//! The daemon publishes `{data_dir}/daemon.json` with the control port it bound.
//! Discovery reads it and pings for liveness; a port that does not answer is
//! stale. Control-plane operations target the descriptor's port, which may differ
//! from the desired config after a saved-but-not-yet-applied port change.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};

const PING_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDescriptor {
    pub control_port: u16,
    // Per-run token guarding the control WebSocket. Absent in descriptors
    // written before token auth existed; an unauthenticated daemon reads as
    // `None` and is reached without a token.
    #[serde(default)]
    pub control_token: Option<String>,
    // Pid of the daemon that wrote the descriptor, so a wedged daemon can be
    // identity-checked before being killed. Absent in older descriptors.
    #[serde(default)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discovery {
    Running { port: u16 },
    Stale,
    NotRunning,
}

pub fn descriptor_path(data_dir: &Path) -> PathBuf {
    data_dir.join("daemon.json")
}

pub fn write_descriptor(
    data_dir: &Path,
    control_port: u16,
    control_token: Option<&str>,
) -> std::io::Result<()> {
    let path = descriptor_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    let descriptor = RuntimeDescriptor {
        control_port,
        control_token: control_token.map(str::to_owned),
        pid: Some(std::process::id()),
    };
    let text = serde_json::to_string(&descriptor).expect("serializing RuntimeDescriptor");
    std::fs::write(&tmp, text)?;
    // The descriptor holds the control token in plaintext; keep it owner-only so
    // a co-located user can't read the token. Best-effort (no-op off Unix).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &path)
}

pub fn read_descriptor(data_dir: &Path) -> Option<RuntimeDescriptor> {
    let text = std::fs::read_to_string(descriptor_path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn remove_descriptor(data_dir: &Path) {
    let _ = std::fs::remove_file(descriptor_path(data_dir));
}

pub async fn discover(data_dir: &Path) -> Discovery {
    let Some(desc) = read_descriptor(data_dir) else {
        return Discovery::NotRunning;
    };
    if ping(desc.control_port, desc.control_token.as_deref()).await {
        Discovery::Running { port: desc.control_port }
    } else {
        Discovery::Stale
    }
}

/// Return the effective control port of a running daemon, spawning one if none is
/// running. The bool is true when this call started a new process.
pub async fn ensure_running(data_dir: &Path) -> Result<(u16, bool)> {
    let exe = daemon_exe().context("locating the magneto-daemon binary")?;
    ensure_running_with_exe(data_dir, &exe).await
}

/// Like [`ensure_running`], but spawns a specific daemon executable. The Tauri app
/// passes its bundled daemon sidecar; pass a path when the sibling default is wrong.
pub async fn ensure_running_with_exe(data_dir: &Path, exe: &Path) -> Result<(u16, bool)> {
    match discover(data_dir).await {
        Discovery::Running { port } => return Ok((port, false)),
        // A descriptor nobody answers on: crash leftovers, or a daemon that
        // is starting, stopping, busy, or wedged. Watch it before spawning;
        // a confirmed wedge is killed so the spawn below isn't doomed.
        Discovery::Stale => {
            if let Some(port) = resolve_stale(data_dir).await? {
                return Ok((port, false));
            }
        }
        Discovery::NotRunning => {}
    }
    start_detached_exe(exe)?;
    match wait_for_any_daemon(data_dir).await {
        Ok(port) => Ok((port, true)),
        Err(e) => {
            // Best-effort hint over the generic timeout: a foreign process
            // squatting the configured port dooms its bind. Only checkable
            // when the config parses; the daemon's own last-good fallback
            // may still have come up elsewhere, so this fires only when
            // nothing answered at all.
            if let Ok(port) = desired_port()
                && port_is_bound(port)
            {
                bail!("daemon did not start; another process is holding port {port}");
            }
            Err(e)
        }
    }
}

fn port_is_bound(port: u16) -> bool {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err()
}

/// A bound port whose descriptor nobody answers is one of four things: a
/// daemon that is starting (event loop not yet running), stopping (teardown
/// holds the port through up to ~10s of cleanup), busy (one on-loop operation
/// outlasting the 2s ping timeout), or actually wedged. Only the last one
/// may be killed, so watch the descriptor for a grace window first. Full
/// re-discovery each pass, not a bare ping: a starting daemon rewrites the
/// descriptor (fresh token, possibly another port) when it commits.
///
/// Returns the port when a live daemon emerged (it was starting or busy),
/// or None when the way is clear to spawn (port released, or wedge killed).
async fn resolve_stale(data_dir: &Path) -> Result<Option<u16>> {
    let Some(desc) = read_descriptor(data_dir) else {
        return Ok(None);
    };
    if !port_is_bound(desc.control_port) {
        // Crash leftovers; the next daemon overwrites the descriptor.
        return Ok(None);
    }
    // Deadline, not an attempt count: a wedged socket still accepts TCP, so
    // every discovery ping inside the loop burns its full 2s timeout.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match discover(data_dir).await {
            Discovery::Running { port } => return Ok(Some(port)),
            // Released: it was a daemon on its way out.
            _ if !port_is_bound(desc.control_port) => return Ok(None),
            _ => {}
        }
    }
    // Bound and unanswering through the whole window: wedged. Lost teardown
    // is what the next start's cleanup pass exists for.
    kill_wedged(&desc)?;
    wait_for_port_free(desc.control_port).await;
    Ok(None)
}

/// Kill a wedged daemon, but only with its identity confirmed, never blind.
fn kill_wedged(desc: &RuntimeDescriptor) -> Result<()> {
    let port = desc.control_port;
    let Some(pid) = desc.pid else {
        bail!(
            "a magneto-daemon is holding port {port} but not responding, and its \
             descriptor predates pid tracking; kill the process bound to that port and retry"
        );
    };
    confirm_and_kill(pid).with_context(|| {
        format!("a magneto-daemon (pid {pid}) is holding port {port} but not responding")
    })
}

#[cfg(target_os = "linux")]
fn confirm_and_kill(pid: u32) -> Result<()> {
    // The comm check guards against the pid having been recycled by another
    // process since the descriptor was written.
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .context("reading the process name; kill it manually and retry")?;
    if comm.trim() != "magneto-daemon" {
        bail!("pid {pid} is now \"{}\", not magneto-daemon; kill the port holder manually", comm.trim());
    }
    // SIGKILL: the daemon is unresponsive by definition, and a graceful
    // signal needs the very event loop that stopped answering.
    if unsafe { libc::kill(pid as i32, libc::SIGKILL) } != 0 {
        return Err(std::io::Error::last_os_error()).context("killing the wedged daemon");
    }
    Ok(())
}

#[cfg(windows)]
fn confirm_and_kill(pid: u32) -> Result<()> {
    // The image-name filter makes taskkill verify identity itself: a recycled
    // pid that is not magneto-daemon.exe matches nothing and nothing dies.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string(), "/FI", "IMAGENAME eq magneto-daemon.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("running taskkill")?;
    if !status.success() {
        bail!("taskkill did not confirm pid {pid} as magneto-daemon.exe; kill the port holder manually");
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn confirm_and_kill(pid: u32) -> Result<()> {
    // No /proc here to confirm what the pid is now; never kill blind.
    bail!("cannot verify what pid {pid} is on this platform; kill it manually and retry");
}

pub async fn stop(data_dir: &Path) -> Result<()> {
    let port = request_stop(data_dir).await?;
    wait_for_port_free(port).await;
    Ok(())
}

/// Ask a running daemon to shut down without waiting for its listener to be
/// released. Enough for a caller that is itself exiting and will never
/// rebind the port. Returns the port the shutdown was sent to.
pub async fn request_stop(data_dir: &Path) -> Result<u16> {
    match discover(data_dir).await {
        Discovery::Running { port } => {
            let token = read_descriptor(data_dir).and_then(|d| d.control_token);
            client_call(port, "shutdown", token.as_deref()).await?;
            Ok(port)
        }
        _ => bail!("daemon not running"),
    }
}

/// Restart over WebSocket: reach the daemon on its effective port, then respawn and
/// wait for the desired port to come up. Returns the desired port.
pub async fn restart(data_dir: &Path) -> Result<u16> {
    let desired = desired_port()?;
    if let Discovery::Running { port } = discover(data_dir).await {
        let token = read_descriptor(data_dir).and_then(|d| d.control_token);
        client_call(port, "restart", token.as_deref()).await?;
        wait_for_port_free(port).await;
    }
    start_detached()?;
    if wait_for_daemon_ready(data_dir, desired).await.is_ok() {
        return Ok(desired);
    }
    // The respawn may have fallen back to last-good on a different port; trust the descriptor.
    if let Discovery::Running { port } = discover(data_dir).await {
        return Ok(port);
    }
    bail!("daemon did not come back up")
}

/// Path to the daemon binary that ships beside the caller. In a dev workspace
/// build and in a packaged bundle, `magneto-daemon` sits next to the invoking
/// executable (the CLI, or the app with its bundled sidecar).
pub fn daemon_exe() -> Option<PathBuf> {
    let mut path = std::env::current_exe().ok()?;
    path.pop();
    path.push(if cfg!(windows) { "magneto-daemon.exe" } else { "magneto-daemon" });
    Some(path)
}

pub fn start_detached() -> Result<()> {
    let exe = daemon_exe().context("locating the magneto-daemon binary")?;
    start_detached_exe(&exe)
}

/// Spawn the daemon binary detached in its own session. Running `magneto-daemon`
/// with no arguments starts the daemon in the foreground; here we background it.
pub fn start_detached_exe(exe: &Path) -> Result<()> {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Inside an AppImage the runtime injects loader paths that point into its
    // FUSE mount. The daemon outlives the app and later spawns host binaries
    // (player, fallback app), so it must not inherit them.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        for var in ["APPIMAGE", "APPDIR", "OWD", "LD_LIBRARY_PATH"] {
            cmd.env_remove(var);
        }
    }
    // The daemon is a console-subsystem binary; spawned from the GUI app,
    // Windows would otherwise allocate a visible console window for it.
    // CREATE_NO_WINDOW keeps a hidden console, which console-control
    // shutdown/logoff events still reach (DETACHED_PROCESS would drop them,
    // costing the daemon its graceful teardown at OS shutdown).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn().context("spawning daemon")?;
    // Reap the child so a long-lived host (the Tauri app, respawning the daemon
    // across restarts) doesn't accumulate zombies. setsid already detaches it;
    // this thread only collects the eventual exit status.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

// One outer deadline for both ready-waits, not an attempt count: each ping can
// itself burn the 2s ping timeout, which would let a counted loop run far past
// its nominal budget. 20s, because a spawn behind a stop/restart overlap
// legitimately waits up to ~15s on the daemon's data-dir lock first.
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// Wait for a daemon to answer on whatever port its descriptor announces.
/// Port-agnostic, because a fresh daemon may commit on a last-good port (or
/// the configured one may not even be knowable when config.toml is broken).
/// A leftover stale descriptor can't satisfy this: its port is dead, or its
/// per-run token is rejected, until the new daemon overwrites it.
pub async fn wait_for_any_daemon(data_dir: &Path) -> Result<u16> {
    let ready = async {
        loop {
            if let Discovery::Running { port } = discover(data_dir).await {
                return port;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    };
    match tokio::time::timeout(READY_TIMEOUT, ready).await {
        Ok(port) => Ok(port),
        Err(_) => bail!("daemon did not become ready within 20s"),
    }
}

pub async fn wait_for_daemon_ready(data_dir: &Path, port: u16) -> Result<()> {
    // Re-read the descriptor each attempt: the daemon binds the control port
    // before it writes daemon.json, so the token only becomes available once
    // the descriptor lands. A leftover descriptor for a different port is
    // ignored until the freshly spawned daemon overwrites it.
    let ready = async {
        loop {
            if let Some(desc) = read_descriptor(data_dir)
                && desc.control_port == port
                && ping(port, desc.control_token.as_deref()).await
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    };
    if tokio::time::timeout(READY_TIMEOUT, ready).await.is_err() {
        bail!("daemon did not become ready within 20s");
    }
    Ok(())
}

/// Bind-probe (not connect-probe) so we only return once the listener FD is freed.
pub async fn wait_for_port_free(port: u16) {
    for _ in 0..50 {
        if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn ping(port: u16, token: Option<&str>) -> bool {
    matches!(
        tokio::time::timeout(PING_TIMEOUT, client_call(port, "ping", token)).await,
        Ok(Ok(_))
    )
}

async fn client_call(port: u16, command: &str, token: Option<&str>) -> Result<serde_json::Value> {
    crate::client::run_raw(port, command, serde_json::json!({}), token).await
}

fn desired_port() -> Result<u16> {
    let path = config::config_path()?;
    Ok(Config::load_or_create(&path)?.network.control_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_round_trips_with_token_and_pid() {
        let d = RuntimeDescriptor {
            control_port: 61481,
            control_token: Some("deadbeef".into()),
            pid: Some(4242),
        };
        let back: RuntimeDescriptor = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.control_port, 61481);
        assert_eq!(back.control_token.as_deref(), Some("deadbeef"));
        assert_eq!(back.pid, Some(4242));
    }

    #[test]
    fn descriptor_without_optional_fields_reads_as_none() {
        // A daemon.json written before token auth / pid tracking must still parse.
        let back: RuntimeDescriptor = serde_json::from_str(r#"{"control_port":61481}"#).unwrap();
        assert_eq!(back.control_port, 61481);
        assert_eq!(back.control_token, None);
        assert_eq!(back.pid, None);
    }
}

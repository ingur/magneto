use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use magneto_core::config::PlayerConfig;

pub fn launch_player(cfg: &PlayerConfig, uris: &[String]) -> Result<()> {
    if cfg.command.trim().is_empty() {
        bail!("player command is not configured");
    }
    let mut args = cfg.args.clone();
    args.extend(uris.iter().cloned());
    spawn(&cfg.command, &args)
}

pub fn spawn(command: &str, args: &[String]) -> Result<()> {
    // Players and fallback handlers are host programs; hand them an
    // environment scrubbed of AppImage bundle paths (core::spawn_env).
    // PATH is among the scrubbed values and Command resolves `command`
    // through the child's PATH, so a bundled tool (like AppImage's own
    // xdg-open) can't shadow the system one.
    let child = Command::new(command)
        .args(args)
        .env_clear()
        .envs(magneto_core::spawn_env::sanitized())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {command}"))?;
    // std children aren't auto-reaped; a waiter thread keeps a long-running
    // daemon from accumulating zombie player processes.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

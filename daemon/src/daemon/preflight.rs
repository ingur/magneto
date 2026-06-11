use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

use anyhow::{Context, Result};

/// A pass is a fast-reject aid, not a guarantee. The real bind is authoritative.
pub fn probe_bind(ip: [u8; 4], port: u16) -> Result<()> {
    let addr = (Ipv4Addr::from(ip), port);
    let listener =
        TcpListener::bind(addr).with_context(|| format!("binding {}:{port}", Ipv4Addr::from(ip)))?;
    drop(listener);
    Ok(())
}

pub fn probe_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let probe = dir.join(".magneto-write-probe");
    std::fs::write(&probe, b"").with_context(|| format!("writing under {}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

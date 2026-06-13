use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};

const APP_QUALIFIER: &str = "";
const APP_ORG: &str = "";
const APP_NAME: &str = "magneto";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub network: NetworkConfig,
    pub downloads: DownloadsConfig,
    pub media: MediaConfig,
    pub player: PlayerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkConfig {
    pub control_port: u16,
    pub lan_port: u16,
    pub upnp_enabled: bool,
    pub server_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadsConfig {
    pub dir: PathBuf,
    pub fallback_app: String,
    pub fallback_args: Vec<String>,
    pub auto_download: bool,
    pub persist_by_default: bool,
    pub share_by_default: bool,
    pub autoplay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaConfig {
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerConfig {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: NetworkConfig {
                control_port: 61481,
                lan_port: 61482,
                upnp_enabled: true,
                server_name: "magneto".into(),
            },
            downloads: DownloadsConfig {
                dir: default_downloads_dir(),
                fallback_app: default_fallback_app().into(),
                fallback_args: default_fallback_args(),
                auto_download: true,
                persist_by_default: false,
                share_by_default: false,
                autoplay: false,
            },
            media: MediaConfig {
                extensions: vec![
                    "mkv", "mp4", "avi", "webm", "mov", "flv", "wmv", "m4v", "ts", "mpg", "mpeg",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            },
            player: PlayerConfig { command: String::new(), args: Vec::new() },
        }
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME)
        .context("could not determine platform project directories")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.toml"))
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().to_path_buf())
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.data_dir().to_path_buf())
}

fn default_downloads_dir() -> PathBuf {
    if let Some(user) = UserDirs::new()
        && let Some(d) = user.download_dir()
    {
        return d.join("magneto");
    }
    dirs_home().join("Downloads").join("magneto")
}

fn dirs_home() -> PathBuf {
    UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

// All platforms hand the source to a direct-exec opener as a single argv element.
#[cfg(target_os = "linux")]
fn default_fallback_app() -> &'static str { "xdg-open" }
#[cfg(target_os = "macos")]
fn default_fallback_app() -> &'static str { "open" }
#[cfg(target_os = "windows")]
fn default_fallback_app() -> &'static str { "rundll32.exe" }
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn default_fallback_app() -> &'static str { "" }

#[cfg(target_os = "windows")]
fn default_fallback_args() -> Vec<String> {
    vec!["url.dll,FileProtocolHandler".into()]
}
#[cfg(not(target_os = "windows"))]
fn default_fallback_args() -> Vec<String> { Vec::new() }

#[derive(Debug, Clone)]
pub struct ConfigDiff {
    pub merged: Config,
}

impl Config {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut cfg: Self = toml::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?;
            cfg.expand_paths();
            cfg.validate()?;
            Ok(cfg)
        } else {
            let cfg = Self::default();
            cfg.save(path)?;
            Ok(cfg)
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing config")?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, &text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("renaming {}", path.display()))
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.downloads.dir).with_context(|| {
            format!("creating downloads.dir {}", self.downloads.dir.display())
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.network.control_port == 0 {
            bail!("network.control_port must be in 1..=65535");
        }
        if self.network.lan_port == 0 {
            bail!("network.lan_port must be in 1..=65535");
        }
        if self.network.upnp_enabled && self.network.control_port == self.network.lan_port {
            bail!("network.control_port and network.lan_port must differ when upnp_enabled");
        }
        if self.media.extensions.is_empty() {
            bail!("media.extensions must contain at least one entry");
        }
        for ext in &self.media.extensions {
            if ext.is_empty() {
                bail!("media.extensions contains an empty entry");
            }
            if ext.chars().any(|c| c == '.' || c == '/' || c == '\\' || c.is_whitespace()) {
                bail!("media.extensions entry {ext:?} contains '.', '/', '\\\\', or whitespace");
            }
            if ext.chars().any(|c| c.is_ascii_uppercase()) {
                bail!("media.extensions entry {ext:?} must be lowercase");
            }
        }
        Ok(())
    }

    fn expand_paths(&mut self) {
        self.downloads.dir = expand_tilde(&self.downloads.dir);
    }

    pub fn diff(&self, patch: &serde_json::Value) -> Result<ConfigDiff> {
        let mut value = serde_json::to_value(self).context("serializing current config")?;
        let mut touched = Vec::new();
        let mut unknown = Vec::new();
        merge_into(&mut value, patch, "", &mut touched, &mut unknown);
        if !unknown.is_empty() {
            bail!("unknown config field(s): {}", unknown.join(", "));
        }
        let mut merged: Self = serde_json::from_value(value).context("merging config patch")?;
        merged.expand_paths();
        merged.validate()?;
        Ok(ConfigDiff { merged })
    }
}

fn expand_tilde(p: &Path) -> PathBuf {
    let Some(rest) = p.strip_prefix("~").ok() else {
        return p.to_path_buf();
    };
    let home = UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    if rest.as_os_str().is_empty() { home } else { home.join(rest) }
}

fn merge_into(
    base: &mut serde_json::Value,
    patch: &serde_json::Value,
    prefix: &str,
    touched: &mut Vec<String>,
    unknown: &mut Vec<String>,
) {
    let (serde_json::Value::Object(b), serde_json::Value::Object(p)) = (base, patch) else {
        return;
    };
    for (k, v) in p {
        let dotted = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match (b.get_mut(k), v) {
            (Some(existing @ serde_json::Value::Object(_)), serde_json::Value::Object(_)) => {
                merge_into(existing, v, &dotted, touched, unknown);
            }
            (Some(_), _) => {
                b.insert(k.clone(), v.clone());
                touched.push(dotted);
            }
            (None, _) => unknown.push(dotted),
        }
    }
}

pub const RESTART_REQUIRED_FIELDS: &[&str] = &[
    "network.control_port",
    "network.lan_port",
    "network.upnp_enabled",
    "network.server_name",
    "downloads.dir",
];

/// Restart-required fields whose value differs between the running and candidate configs.
pub fn pending_restart(running: &Config, candidate: &Config) -> Vec<String> {
    RESTART_REQUIRED_FIELDS
        .iter()
        .filter(|f| restart_field_differs(running, candidate, f))
        .map(|f| f.to_string())
        .collect()
}

fn restart_field_differs(a: &Config, b: &Config, field: &str) -> bool {
    match field {
        "network.control_port" => a.network.control_port != b.network.control_port,
        "network.lan_port" => a.network.lan_port != b.network.lan_port,
        "network.upnp_enabled" => a.network.upnp_enabled != b.network.upnp_enabled,
        "network.server_name" => a.network.server_name != b.network.server_name,
        "downloads.dir" => a.downloads.dir != b.downloads.dir,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_hot_only_patch_has_no_restart_required() {
        let cfg = Config::default();
        let patch = serde_json::json!({ "downloads": { "autoplay": true } });
        let diff = cfg.diff(&patch).unwrap();
        assert!(pending_restart(&cfg, &diff.merged).is_empty());
        assert!(diff.merged.downloads.autoplay);
    }

    #[test]
    fn pending_restart_lists_only_changed_restart_fields() {
        let cfg = Config::default();
        let patch = serde_json::json!({
            "network": { "control_port": 50000 },
            "downloads": { "autoplay": true }
        });
        let diff = cfg.diff(&patch).unwrap();
        assert_eq!(pending_restart(&cfg, &diff.merged), vec!["network.control_port".to_string()]);
    }

    #[test]
    fn pending_restart_ignores_resent_unchanged_restart_fields() {
        let cfg = Config::default();
        let port = cfg.network.control_port;
        let patch = serde_json::json!({
            "network": { "control_port": port },
            "downloads": { "autoplay": false }
        });
        let diff = cfg.diff(&patch).unwrap();
        assert!(pending_restart(&cfg, &diff.merged).is_empty());
    }

    #[test]
    fn diff_invalid_patch_errors_without_mutating_self() {
        let cfg = Config::default();
        let patch = serde_json::json!({ "network": { "control_port": 0 } });
        assert!(cfg.diff(&patch).is_err());
    }

    #[test]
    fn diff_unknown_top_level_key_errors() {
        let cfg = Config::default();
        let patch = serde_json::json!({ "bogus": true });
        assert!(cfg.diff(&patch).is_err());
    }

    #[test]
    fn diff_unknown_nested_key_errors() {
        let cfg = Config::default();
        let patch = serde_json::json!({ "network": { "bogus": 1 } });
        assert!(cfg.diff(&patch).is_err());
    }

    #[test]
    fn diff_mixed_valid_and_unknown_errors_atomically() {
        let cfg = Config::default();
        let patch = serde_json::json!({ "downloads": { "autoplay": false, "bogus": 1 } });
        assert!(cfg.diff(&patch).is_err());
    }
}

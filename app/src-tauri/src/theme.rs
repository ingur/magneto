//! User-editable theme. The app owns a `theme.toml` in the config dir: it
//! generates the file with built-in defaults if missing, validates it, and
//! hot-reloads it. The frontend pulls both resolved palettes via `get_theme`
//! and receives `theme://changed` / `theme://error` on file edits; it owns the
//! choice of variant (System/Dark/Light) and writes the CSS variables.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Theme {
    pub dark: Palette,
    pub light: Palette,
}

/// One semantic role per field. `kebab-case` so the wire/file keys match the
/// `--t-<role>` CSS variables the frontend writes (e.g. `on_accent` ⇒
/// `on-accent` ⇒ `--t-on-accent`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Palette {
    // surface
    pub bg: String,
    pub panel: String,
    pub raised: String,
    pub overlay: String,
    // text
    pub fg: String,
    pub muted: String,
    pub subtle: String,
    pub disabled: String,
    pub on_accent: String,
    // border
    pub border: String,
    // state
    pub hover: String,
    pub cursor: String,
    pub marked: String,
    // accent
    pub accent: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub info: String,
    pub link: String,
    // progress
    pub progress_active: String,
    pub progress_done: String,
    pub progress_idle: String,
    pub progress_error: String,
    // chrome
    pub scrollbar: String,
    pub scrollbar_active: String,
    pub backdrop: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self { dark: Palette::dark(), light: Palette::light() }
    }
}

impl Palette {
    fn dark() -> Self {
        Self {
            bg: "#161819".into(), panel: "#1d2021".into(), raised: "#3f3f3f".into(), overlay: "#161819".into(),
            fg: "#d1c6b1".into(), muted: "#7d786e".into(), subtle: "#5a564e".into(),
            disabled: "#3f3f3f".into(), on_accent: "#161819".into(),
            border: "#1d2021".into(),
            hover: "#1f2223".into(), cursor: "#7daea3".into(), marked: "#d8a657".into(),
            accent: "#89b482".into(), success: "#89b482".into(), warning: "#d8a657".into(),
            danger: "#ea6962".into(), info: "#7daea3".into(), link: "#a09ace".into(),
            progress_active: "#7daea3".into(), progress_done: "#89b482".into(),
            progress_idle: "#7d786e".into(), progress_error: "#ea6962".into(),
            scrollbar: "#3f3f3f".into(), scrollbar_active: "#5a564e".into(), backdrop: "#16181999".into(),
        }
    }

    // "Porcelain": neutral warm-gray paper mirroring the dark variant's
    // desaturated character. Accents reuse the dark variant's hues, darkened
    // for light paper and pinned to OKLCH lightness 0.53 (≥4:1 WCAG on bg);
    // amber sits higher (0.57) because darker yellow reads as brown. Backdrop
    // follows the same rule as dark (bg at 60% alpha) so modals wash
    // content toward the paper rather than shadowing it.
    fn light() -> Self {
        Self {
            bg: "#f3f1ea".into(), panel: "#e9e6dd".into(), raised: "#c9c5b8".into(), overlay: "#f3f1ea".into(),
            fg: "#4d473d".into(), muted: "#7c756a".into(), subtle: "#9d968a".into(),
            disabled: "#bcb5a8".into(), on_accent: "#f3f1ea".into(),
            border: "#e9e6dd".into(),
            hover: "#eae7df".into(), cursor: "#217693".into(), marked: "#a66700".into(),
            accent: "#377d4b".into(), success: "#377d4b".into(), warning: "#a66700".into(),
            danger: "#b23f40".into(), info: "#217693".into(), link: "#705ea2".into(),
            progress_active: "#217693".into(), progress_done: "#377d4b".into(),
            progress_idle: "#7c756a".into(), progress_error: "#b23f40".into(),
            scrollbar: "#c9c5b8".into(), scrollbar_active: "#a39e90".into(), backdrop: "#f3f1ea99".into(),
        }
    }

    /// Apply a variant table from the file over these defaults. An omitted key
    /// keeps its default; an unknown key or non-string value is a hard error so
    /// typos surface instead of silently doing nothing.
    fn overlay(self, variant: &str, table: toml::Table) -> Result<Self> {
        let toml::Value::Table(mut map) = toml::Value::try_from(&self)? else {
            unreachable!("a Palette serializes to a table")
        };
        for (key, value) in table {
            if !map.contains_key(&key) {
                bail!("[{variant}] unknown theme key {key:?}");
            }
            if !value.is_str() {
                bail!("[{variant}] {key} must be a string color");
            }
            map.insert(key, value);
        }
        Ok(toml::Value::Table(map).try_into()?)
    }

    fn validate(&self, variant: &str) -> Result<()> {
        let toml::Value::Table(map) = toml::Value::try_from(self)? else { unreachable!() };
        for (key, value) in &map {
            let color = value.as_str().context("color must be a string")?;
            if !is_hex(color) {
                bail!("[{variant}] {key} = {color:?} is not a valid #rrggbb or #rrggbbaa color");
            }
        }
        Ok(())
    }
}

fn is_hex(s: &str) -> bool {
    matches!(s.strip_prefix('#'),
        Some(h) if (h.len() == 6 || h.len() == 8) && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Theme {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if !path.exists() {
            let theme = Self::default();
            theme.save(path)?;
            return Ok(theme);
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let theme = Self::parse(&text).with_context(|| format!("parsing {}", path.display()))?;
        theme.validate()?;
        Ok(theme)
    }

    fn parse(text: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            dark: toml::Table,
            #[serde(default)]
            light: toml::Table,
        }
        let raw: Raw = toml::from_str(text)?;
        Ok(Self {
            dark: Palette::dark().overlay("dark", raw.dark)?,
            light: Palette::light().overlay("light", raw.light)?,
        })
    }

    fn validate(&self) -> Result<()> {
        self.dark.validate("dark")?;
        self.light.validate("light")
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let body = toml::to_string_pretty(self).context("serializing theme")?;
        std::fs::write(path, format!("{HEADER}{body}")).with_context(|| format!("writing {}", path.display()))
    }
}

const HEADER: &str = "\
# Magneto theme. Two variants: [dark] and [light]. Mode (System/Dark/Light) is
# chosen in the app. Each key is a semantic UI role; reuse a color across roles
# freely. Colors are \"#rrggbb\" or \"#rrggbbaa\". Saving hot-reloads the app.
# Omit a key to use its built-in default; an unknown key reports an error.
# Reserved roles (no UI yet, safe to set): link, progress-error.

";

pub struct ThemeState(pub Mutex<Theme>);

#[tauri::command]
pub fn get_theme(state: tauri::State<ThemeState>) -> Theme {
    state.0.lock().map(|guard| guard.clone()).unwrap_or_else(|_| Theme::default())
}

/// Load the theme, expose it via `get_theme`, and watch the file. Theme
/// problems never block startup: on any failure the app falls back to built-in
/// defaults and (when possible) emits `theme://error`.
pub fn init(app: &AppHandle) {
    let path = match magneto_core::config::config_dir() {
        Ok(dir) => dir.join("theme.toml"),
        Err(e) => {
            eprintln!("theme: cannot resolve config dir ({e:#}); using built-in defaults");
            app.manage(ThemeState(Mutex::new(Theme::default())));
            return;
        }
    };
    let theme = Theme::load_or_create(&path).unwrap_or_else(|e| {
        eprintln!("theme: load failed ({e:#}); using built-in defaults");
        let _ = app.emit("theme://error", e.to_string());
        Theme::default()
    });
    app.manage(ThemeState(Mutex::new(theme)));
    if let Err(e) = spawn_watcher(app, path) {
        eprintln!("theme: hot-reload disabled ({e:#}); get_theme still works");
    }
}

fn spawn_watcher(app: &AppHandle, path: PathBuf) -> Result<()> {
    use notify_debouncer_full::{new_debouncer, notify::RecursiveMode, DebounceEventResult};

    let dir = path.parent().context("theme path has no parent")?.to_path_buf();
    let handle = app.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(250),
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            // Watch the parent dir (survives editor tmp+rename saves); act only
            // on events touching theme.toml itself.
            if !events.iter().any(|ev| ev.paths.iter().any(|p| p == &path)) {
                return;
            }
            match Theme::load_or_create(&path) {
                Ok(theme) => {
                    if let Ok(mut guard) = handle.state::<ThemeState>().0.lock() {
                        *guard = theme.clone();
                    }
                    let _ = handle.emit("theme://changed", theme);
                }
                Err(e) => {
                    let _ = handle.emit("theme://error", e.to_string());
                }
            }
        },
    )?;
    debouncer.watch(&dir, RecursiveMode::NonRecursive)?;
    app.manage(Mutex::new(debouncer)); // keep the watcher alive for the app's lifetime
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Theme::default().validate().unwrap();
    }

    #[test]
    fn full_file_roundtrips() {
        let text = toml::to_string_pretty(&Theme::default()).unwrap();
        assert_eq!(Theme::parse(&text).unwrap(), Theme::default());
    }

    #[test]
    fn omitted_keys_fall_back_per_variant() {
        let theme = Theme::parse("[dark]\nbg = \"#010203\"\n").unwrap();
        assert_eq!(theme.dark.bg, "#010203");
        assert_eq!(theme.dark.fg, Palette::dark().fg); // untouched key keeps its default
        assert_eq!(theme.light, Palette::light()); // untouched variant is fully default
    }

    #[test]
    fn unknown_key_errors() {
        assert!(Theme::parse("[dark]\nnope = \"#010203\"\n").is_err());
    }

    #[test]
    fn non_string_value_errors() {
        assert!(Theme::parse("[dark]\nbg = 123\n").is_err());
    }

    #[test]
    fn bad_hex_fails_validate() {
        let theme = Theme::parse("[dark]\nbg = \"nothex\"\n").unwrap();
        assert!(theme.validate().is_err());
    }
}

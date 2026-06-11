//! OS-handed add sources: magnet links and `.torrent` files arriving through
//! the handler registrations: cold-start argv, a second launch forwarded by
//! single-instance (Windows/Linux), or open events (macOS).
//!
//! The queue here is the single buffer: sources stay in it until the frontend
//! has a live daemon connection and drains via `take_pending_sources`, so a
//! source survives webview reloads and a daemon that is still spawning.
//! Every arrival also emits `sources-ready`; a missed ping is harmless
//! because every (re)connect snapshot triggers a drain too.

use std::path::Path;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct Pending(Mutex<Vec<String>>);

/// Filter raw process arguments down to add sources: magnet URIs and HTTP(S)
/// URLs (passed through verbatim, the daemon's grammar), `file://` URLs
/// pointing at `.torrent` files (the `%U` desktop-entry field code hands
/// local files as URLs), and bare `.torrent` paths. Flags and anything else
/// are dropped. Relative paths from a second launch must be resolved with
/// [`absolutize`], they would otherwise resolve against this process's cwd
/// at read time.
pub fn parse_args<I>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    args.into_iter().filter_map(|a| normalize(&a)).collect()
}

fn normalize(arg: &str) -> Option<String> {
    if arg.starts_with('-') {
        return None;
    }
    if magneto_core::protocol::is_direct_source(arg) {
        return Some(arg.to_owned());
    }
    if arg.to_ascii_lowercase().starts_with("file://") {
        let path = tauri::Url::parse(arg).ok()?.to_file_path().ok()?;
        return is_torrent_file(&path).then(|| path.to_string_lossy().into_owned());
    }
    is_torrent_file(Path::new(arg)).then(|| arg.to_owned())
}

/// One predicate for "is this a `.torrent` file path", shared with
/// `read_torrent_file`'s fence so the queue can't admit a path the read
/// would reject.
pub fn is_torrent_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
}

/// Resolve a relative `.torrent` path from a second launch against that
/// launch's cwd. Direct sources and absolute paths pass through.
pub fn absolutize(source: String, cwd: &str) -> String {
    if magneto_core::protocol::is_direct_source(&source) {
        return source;
    }
    let path = Path::new(&source);
    if path.is_absolute() {
        source
    } else {
        Path::new(cwd).join(path).to_string_lossy().into_owned()
    }
}

/// Queue sources for the frontend and ping it. Safe to call before the
/// webview is listening; the queue holds until the next drain.
pub fn queue(app: &AppHandle, sources: Vec<String>) {
    if sources.is_empty() {
        return;
    }
    app.state::<Pending>().0.lock().unwrap().extend(sources);
    let _ = app.emit("sources-ready", ());
}

/// Drain the pending queue (the frontend's half of the handshake; it only
/// drains while its daemon connection is live).
#[tauri::command]
pub fn take_pending_sources(state: tauri::State<'_, Pending>) -> Vec<String> {
    std::mem::take(&mut *state.0.lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Vec<String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn accepts_magnet_uris_case_insensitively() {
        let got = parse(&["MAGNET:?xt=urn:btih:abc", "magnet:?xt=urn:btih:def"]);
        assert_eq!(got, vec!["MAGNET:?xt=urn:btih:abc", "magnet:?xt=urn:btih:def"]);
    }

    #[test]
    fn accepts_torrent_paths_and_drops_other_files() {
        let got = parse(&["/tmp/a.TORRENT", "/tmp/movie.mkv", "/tmp/b.torrent"]);
        assert_eq!(got, vec!["/tmp/a.TORRENT", "/tmp/b.torrent"]);
    }

    #[test]
    fn converts_file_urls_to_paths() {
        let got = parse(&["file:///tmp/space%20name.torrent", "file:///tmp/other.iso"]);
        assert_eq!(got, vec!["/tmp/space name.torrent"]);
    }

    #[test]
    fn drops_flags_junk_and_extensionless_dotfiles() {
        // A file literally named ".torrent" has no extension; the read fence
        // would reject it, so the queue must too.
        let got = parse(&["--flag", "-v", "not-a-source", "/tmp/movie.mkv", "/tmp/.torrent"]);
        assert!(got.is_empty());
    }

    #[test]
    fn passes_http_sources_through() {
        let got = parse(&["https://example.com/file.torrent"]);
        assert_eq!(got, vec!["https://example.com/file.torrent"]);
    }

    #[test]
    fn absolutize_joins_relative_paths_against_the_forwarded_cwd() {
        assert_eq!(
            absolutize("downloads/x.torrent".into(), "/home/u"),
            "/home/u/downloads/x.torrent"
        );
        assert_eq!(absolutize("/tmp/x.torrent".into(), "/home/u"), "/tmp/x.torrent");
        assert_eq!(absolutize("magnet:?xt=a".into(), "/home/u"), "magnet:?xt=a");
    }
}

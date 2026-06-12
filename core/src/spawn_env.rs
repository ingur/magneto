// The environment handed to spawned host programs (players, fallback
// handlers, system openers). Inside an AppImage, AppRun exports bundle
// paths (PATH and LD_LIBRARY_PATH prefixes, GTK/GLib module caches) so the
// bundled app finds its own libraries; host programs inheriting them load
// mismatched libraries and crash. Sanitizing strips everything pointing
// into $APPDIR; without $APPDIR set (dev, deb/rpm, Windows, macOS) it is
// the identity.

use std::ffi::OsString;

/// Snapshot of the process environment with AppImage-internal entries
/// removed. Colon-separated values lose their $APPDIR components; variables
/// whose value lives entirely under $APPDIR (module caches, schema dirs,
/// APPDIR itself) are dropped.
pub fn sanitized() -> Vec<(OsString, OsString)> {
    let appdir = std::env::var("APPDIR").ok().filter(|dir| !dir.is_empty());
    sanitize(appdir.as_deref(), std::env::vars_os())
}

fn sanitize(
    appdir: Option<&str>,
    vars: impl Iterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    vars.filter_map(|(key, value)| {
        let Some(appdir) = appdir else {
            return Some((key, value));
        };
        if key == "APPDIR" {
            return None;
        }
        match value.into_string() {
            // AppRun-derived values are plain text; anything non-unicode
            // came from elsewhere and passes through untouched.
            Err(raw) => Some((key, raw)),
            Ok(text) => {
                if !text.contains(appdir) {
                    return Some((key, text.into()));
                }
                let kept: Vec<&str> = text
                    .split(':')
                    .filter(|part| !part.is_empty() && !part.starts_with(appdir))
                    .collect();
                if kept.is_empty() {
                    None
                } else {
                    Some((key, kept.join(":").into()))
                }
            }
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize;
    use std::ffi::OsString;

    fn vars(list: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        list.iter().map(|(k, v)| (k.into(), v.into())).collect()
    }

    #[test]
    fn identity_without_appdir() {
        let input = vars(&[("PATH", "/usr/bin:/bin"), ("HOME", "/home/u")]);
        assert_eq!(sanitize(None, input.clone().into_iter()), input);
    }

    #[test]
    fn strips_appdir_components_from_path_lists() {
        let out = sanitize(
            Some("/tmp/.mount_app"),
            vars(&[("PATH", "/tmp/.mount_app/usr/bin/:/usr/bin:/bin")]).into_iter(),
        );
        assert_eq!(out, vars(&[("PATH", "/usr/bin:/bin")]));
    }

    #[test]
    fn drops_vars_living_entirely_under_appdir() {
        let out = sanitize(
            Some("/tmp/.mount_app"),
            vars(&[
                ("GDK_PIXBUF_MODULE_FILE", "/tmp/.mount_app/usr/lib/loaders.cache"),
                ("LD_LIBRARY_PATH", "/tmp/.mount_app/usr/lib/:/tmp/.mount_app/lib/"),
                ("APPDIR", "/tmp/.mount_app"),
            ])
            .into_iter(),
        );
        assert_eq!(out, vec![]);
    }

    #[test]
    fn keeps_unrelated_values_untouched() {
        let input = vars(&[("GTK_THEME", "Adwaita:dark"), ("DISPLAY", ":0")]);
        let out = sanitize(Some("/tmp/.mount_app"), input.clone().into_iter());
        assert_eq!(out, input);
    }
}

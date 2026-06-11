use std::path::Path;

pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "idx"];

pub fn is_media(filename: &str, extensions: &[String]) -> bool {
    extension_of(filename)
        .is_some_and(|ext| extensions.iter().any(|m| m.eq_ignore_ascii_case(&ext)))
}

pub fn is_subtitle(filename: &str) -> bool {
    extension_of(filename)
        .is_some_and(|ext| SUBTITLE_EXTENSIONS.iter().any(|s| s.eq_ignore_ascii_case(&ext)))
}

pub fn mime_for(filename: &str) -> &'static str {
    let Some(ext) = extension_of(filename) else {
        return "application/octet-stream";
    };
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "flv" => "video/x-flv",
        "wmv" => "video/x-ms-wmv",
        "ts" => "video/mp2t",
        "mpg" | "mpeg" => "video/mpeg",
        _ => "application/octet-stream",
    }
}

fn extension_of(filename: &str) -> Option<String> {
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts() -> Vec<String> {
        vec!["mkv".into(), "mp4".into()]
    }

    #[test]
    fn is_media_case_insensitive() {
        assert!(is_media("Movie.MKV", &exts()));
        assert!(is_media("clip.Mp4", &exts()));
    }

    #[test]
    fn is_media_no_extension() {
        assert!(!is_media("README", &exts()));
        assert!(!is_media("", &exts()));
    }

    #[test]
    fn is_media_multi_dot() {
        assert!(is_media("show.s01e01.foo.mkv", &exts()));
        assert!(!is_media("show.mkv.bak", &exts()));
    }

    #[test]
    fn is_subtitle_recognises_common_formats() {
        assert!(is_subtitle("subs.srt"));
        assert!(is_subtitle("subs.ASS"));
        assert!(is_subtitle("subs.vtt"));
        assert!(!is_subtitle("movie.mkv"));
        assert!(!is_subtitle(""));
    }
}

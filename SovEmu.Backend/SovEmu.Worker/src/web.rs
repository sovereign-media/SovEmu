//! Serving the built web client (`SovEmu.Web`'s `dist/`) as static files.
//!
//! Phase 1 serves it from the worker; Phase 2 moves this, unchanged in behaviour, into
//! `SovEmu.Server`, which serves the same files next to the API.

use std::path::{Component, Path, PathBuf};

/// A file to send back.
#[derive(Debug, PartialEq, Eq)]
pub struct StaticFile {
    pub body: Vec<u8>,
    pub content_type: &'static str,
    pub cache_control: &'static str,
}

/// Shown at `/` when no web client is installed.
pub const NOT_INSTALLED: &str = "<!doctype html><title>SovEmu</title>\
<p>SovEmu is running, but the web client is not installed. Build <code>SovEmu.Web</code> \
(<code>npm run build</code>) and start the server with <code>--web-dir SovEmu.Web/dist</code>.</p>";

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Maps a URL path onto a file under `root`, refusing anything that would leave it.
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let relative = Path::new(url_path.trim_start_matches('/'));
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) || url_path.contains('\\')
    {
        return None;
    }
    let path = root.join(relative);
    if path.is_dir() { Some(path.join("index.html")) } else { Some(path) }
}

/// Looks up `url_path` (no query string) in the web client's directory.
pub fn lookup(root: &Path, url_path: &str) -> Option<StaticFile> {
    let path = resolve(root, url_path)?;
    let body = std::fs::read(&path).ok()?;
    // Astro fingerprints everything under /_astro/, so those never change; pages must revalidate.
    let cache_control = if url_path.starts_with("/_astro/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(StaticFile { body, content_type: content_type(&path), cache_control })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> PathBuf {
        let root = std::env::temp_dir().join(format!("sovemu-web-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("_astro")).unwrap();
        std::fs::create_dir_all(root.join("about")).unwrap();
        std::fs::write(root.join("index.html"), "<h1>home</h1>").unwrap();
        std::fs::write(root.join("about/index.html"), "<h1>about</h1>").unwrap();
        std::fs::write(root.join("_astro/app.abc123.js"), "console.log(1)").unwrap();
        root
    }

    #[test]
    fn serves_pages_and_assets() {
        let root = site();
        let home = lookup(&root, "/").unwrap();
        assert_eq!(home.body, b"<h1>home</h1>");
        assert_eq!(home.content_type, "text/html; charset=utf-8");
        assert_eq!(home.cache_control, "no-cache");
        assert_eq!(lookup(&root, "/about/").unwrap().body, b"<h1>about</h1>");
        assert_eq!(lookup(&root, "/about").unwrap().body, b"<h1>about</h1>");
        let js = lookup(&root, "/_astro/app.abc123.js").unwrap();
        assert_eq!(js.content_type, "text/javascript; charset=utf-8");
        assert!(js.cache_control.contains("immutable"));
        assert_eq!(lookup(&root, "/missing.css"), None);
    }

    #[test]
    fn refuses_to_leave_the_root() {
        let root = site();
        std::fs::write(root.parent().unwrap().join("sovemu-secret.txt"), "secret").ok();
        for path in
            ["/../sovemu-secret.txt", "/_astro/../../sovemu-secret.txt", "/..\\x", "//etc/passwd"]
        {
            assert_eq!(lookup(&root, path), None, "{path}");
        }
    }
}

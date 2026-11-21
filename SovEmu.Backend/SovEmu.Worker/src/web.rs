//! Serving the built web client (`SovEmu.Web`'s `dist/`) as static files.
//!
//! Phase 1 serves it from the worker; Phase 2 moves this, unchanged in behaviour, into
//! `SovEmu.Server`, which serves the same files next to the API.

use std::path::{Component, Path, PathBuf};

/// A file to send back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StaticFile {
    pub body: Vec<u8>,
    pub content_type: &'static str,
    pub cache_control: &'static str,
}

/// Shown at `/` when no web client is installed.
pub(crate) const NOT_INSTALLED: &str = "<!doctype html><title>SovEmu</title>\
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

/// Looks up `url_path` (no query string) in the web client's directory. The client is one page
/// that routes its own screens, so a page path with no file of its own (`/system/`) gets
/// `index.html`; a missing asset (`/assets/…`, anything with an extension) or API path does not.
pub(crate) fn lookup(root: &Path, url_path: &str) -> Option<StaticFile> {
    let path = resolve(root, url_path)?;
    let is_page = !url_path.starts_with("/assets/")
        && !url_path.starts_with("/api/")
        && Path::new(url_path).extension().is_none();
    let (path, body) = match std::fs::read(&path) {
        Ok(body) => (path, body),
        Err(_) if is_page => {
            let index = root.join("index.html");
            let body = std::fs::read(&index).ok()?;
            (index, body)
        }
        Err(_) => return None,
    };
    // Vite fingerprints everything under /assets/, so those never change; pages must revalidate.
    let cache_control = if url_path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(StaticFile { body, content_type: content_type(&path), cache_control })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh site per call: tests run in parallel, and one rewriting files another is
    /// reading made it read them empty.
    fn site() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("sovemu-web-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::create_dir_all(root.join("about")).unwrap();
        std::fs::write(root.join("index.html"), "<h1>home</h1>").unwrap();
        std::fs::write(root.join("about/index.html"), "<h1>about</h1>").unwrap();
        std::fs::write(root.join("assets/app.abc123.js"), "console.log(1)").unwrap();
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
        let js = lookup(&root, "/assets/app.abc123.js").unwrap();
        assert_eq!(js.content_type, "text/javascript; charset=utf-8");
        assert!(js.cache_control.contains("immutable"));
        assert_eq!(lookup(&root, "/missing.css"), None);
        assert_eq!(lookup(&root, "/assets/old.js"), None);
        // A screen the client routes itself: the one page.
        let screen = lookup(&root, "/system/").unwrap();
        assert_eq!(
            (screen.body.as_slice(), screen.cache_control),
            (b"<h1>home</h1>".as_slice(), "no-cache")
        );
        assert_eq!(lookup(&root, "/api/nope"), None);
    }

    #[test]
    fn refuses_to_leave_the_root() {
        let root = site();
        std::fs::write(root.parent().unwrap().join("sovemu-secret.txt"), "secret").ok();
        for path in
            ["/../sovemu-secret.txt", "/assets/../../sovemu-secret.txt", "/..\\x", "//etc/passwd"]
        {
            // Refused, or (`//etc/passwd` is `etc/passwd` under the root, which is missing) the one
            // page: never a file from outside.
            let served = lookup(&root, path).map(|f| f.body);
            assert!(
                served.is_none() || served.as_deref() == Some(b"<h1>home</h1>".as_slice()),
                "{path}"
            );
        }
    }
}

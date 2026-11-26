//! The open-source packages SovEmu ships, with their licences, for the admin About page: the
//! server's crates (`third-party-notices.json`, written by cargo-about from `about.toml` and
//! `about.hbs`: listed again by the image's `notices` stage and put beside the server binary, so
//! the image always carries the current list; a local build falls back to the committed copy,
//! built in) and the web client's npm packages (`third-party-licenses.json` beside its
//! `index.html`, written by its build).
//!
//! Both are the same shape: each distinct licence text, with the packages it covers.

use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

/// One licence text, and the packages it covers.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct Notice {
    /// SPDX (`MIT`).
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) crates: Vec<Package>,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) repository: Option<String>,
}

/// The server's crates, read once: the image's list beside the binary, else the copy built in.
/// The image's list is not compiled in, so the release compile never waits for cargo-about.
pub(crate) fn server() -> &'static [Notice] {
    static NOTICES: OnceLock<Vec<Notice>> = OnceLock::new();
    NOTICES.get_or_init(|| {
        let beside = crate::about::beside_exe("third-party-notices.json")
            .and_then(|file| std::fs::read_to_string(&file).ok().map(|text| (file, text)));
        if let Some((file, text)) = beside {
            match serde_json::from_str(&text) {
                Ok(notices) => return notices,
                Err(e) => tracing::warn!(file = %file.display(), error = %e, "the image's third-party notices cannot be read; using the built-in list"),
            }
        }
        serde_json::from_str(include_str!("../third-party-notices.json")).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "the built-in third-party notices cannot be read");
            Vec::new()
        })
    })
}

/// The web client's packages, from its build in `web_dir`; none if it has no list (a
/// development build of the web client, or none served).
pub(crate) fn web(web_dir: Option<&Path>) -> Vec<Notice> {
    let Some(file) = web_dir.map(|d| d.join("third-party-licenses.json")) else {
        return Vec::new();
    };
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(file = %file.display(), error = %e, "the web client's notices cannot be read");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_list_names_every_crate_with_a_licence_and_its_text() {
        let notices = server();
        assert!(notices.len() > 10, "{} licence texts", notices.len());
        assert!(notices.iter().all(|n| !n.text.trim().is_empty() && !n.crates.is_empty()));
        let names: Vec<&str> =
            notices.iter().flat_map(|n| n.crates.iter().map(|c| c.name.as_str())).collect();
        for shipped in ["axum", "tokio", "sqlx", "str0m"] {
            assert!(names.contains(&shipped), "{shipped} is listed");
        }
        assert!(!names.iter().any(|n| n.starts_with("sovemu-")), "SovEmu's own are not");
    }

    #[test]
    fn the_web_list_is_read_from_its_build_or_is_empty() {
        let dir = std::env::temp_dir().join(format!("sovemu-notices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(web(Some(&dir)).is_empty());
        assert!(web(None).is_empty());
        std::fs::write(
            dir.join("third-party-licenses.json"),
            r#"[{"id":"MIT","name":"MIT License","crates":[{"name":"react","version":"19.0.0","repository":null}],"text":"MIT"}]"#,
        )
        .unwrap();
        assert_eq!(web(Some(&dir))[0].crates[0].name, "react");
    }
}

//! The effective configuration as one list: every setting, its value (redacted where it holds a
//! credential), where the value came from, and its group as the README's settings table has it.
//! The startup log renders it ([`Config::summary`]) and the admin Configuration page shows it
//! (the `configuration` query), so values are formatted and redacted in this one place.
//!
//! The list is built from clap's own description of [`Config`], so a new setting appears here
//! without being listed again; a test fails if it has no group or no row in the README.

use std::collections::BTreeMap;

use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory};

use crate::{Config, LibraryMode, SECRETS, redact};

/// Where a setting's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Default,
    Environment,
    /// A secret read from the file `<VAR>_FILE` names.
    File,
    CommandLine,
}

/// The README settings table's groups, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    Accounts,
    Library,
    GameInformation,
    Streaming,
    Operations,
}

impl Group {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Accounts => "Accounts",
            Self::Library => "Library",
            Self::GameInformation => "Game information",
            Self::Streaming => "Streaming",
            Self::Operations => "Operations",
        }
    }
}

/// One setting as shown.
#[derive(Debug, Clone)]
pub(crate) struct Setting {
    pub(crate) group: Group,
    /// The environment variable (`SOVEMU_HTTP`, `SOVEMU_USERS__0__USERNAME`, `RUST_LOG`).
    pub(crate) name: String,
    /// As given, redacted; none when unset.
    pub(crate) value: Option<String>,
    /// What the server works it out to be, where that differs from the value (an unset
    /// `SOVEMU_ROMS_DIR` is `<data>/roms`).
    pub(crate) effective: Option<String>,
    pub(crate) source: Source,
    /// Holds a secret: the value only says whether it is set.
    pub(crate) secret: bool,
    /// The default, as clap has it.
    pub(crate) default: Option<String>,
    /// The setting's help, from its doc comment.
    pub(crate) description: String,
}

/// What the command line and the environment gave each argument, kept from parsing: clap's
/// [`ArgMatches`] knows where each value came from, which `Config` itself does not.
#[derive(Debug, Clone, Default)]
pub(crate) struct Given {
    /// Argument id → where its value came from, and its values as given.
    args: BTreeMap<String, (Source, Vec<String>)>,
    /// Secrets read from `<VAR>_FILE`, by variable.
    pub(crate) from_files: Vec<&'static str>,
    /// Variables read outside clap: the structured settings (`SOVEMU_USERS__…`,
    /// `SOVEMU_SYSTEMS__…`, `SOVEMU_CONTROLLERS__…`) and the standard ones (`RUST_LOG`, `TZ`,
    /// `OTEL_*`).
    pub(crate) environment: Vec<(String, String)>,
}

impl Given {
    pub(crate) fn from_matches(matches: &ArgMatches) -> Self {
        let mut args = BTreeMap::new();
        for arg in Config::command().get_arguments() {
            let id = arg.get_id().as_str();
            let Some(source) = matches.value_source(id) else { continue };
            let source = match source {
                ValueSource::DefaultValue => Source::Default,
                ValueSource::EnvVariable => Source::Environment,
                _ => Source::CommandLine,
            };
            let values = matches
                .get_raw(id)
                .map(|v| v.map(|s| s.to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            args.insert(id.to_owned(), (source, values));
        }
        Self { args, ..Self::default() }
    }

    /// The variables read outside clap, from the process's environment.
    pub(crate) fn keep_environment(&mut self, vars: &[(String, String)]) {
        self.environment = vars
            .iter()
            .filter(|(k, _)| {
                ["SOVEMU_USERS__", "SOVEMU_SYSTEMS__", "SOVEMU_CONTROLLERS__", "OTEL_"]
                    .iter()
                    .any(|p| k.starts_with(p))
                    || k == "RUST_LOG"
                    || k == "TZ"
            })
            .cloned()
            .collect();
        self.environment.sort();
    }
}

/// The README group of each `SOVEMU_*` setting clap reads.
fn group(env: &str) -> Option<Group> {
    use Group::*;
    Some(match env {
        "SOVEMU_ADMIN_USERNAME"
        | "SOVEMU_ADMIN_PASSWORD"
        | "SOVEMU_USERS_PRUNE"
        | "SOVEMU_SESSION_DAYS"
        | "SOVEMU_ACCESS_TOKEN_MINUTES"
        | "SOVEMU_REFRESH_RETRY_SECONDS"
        | "SOVEMU_PUBLIC_URL"
        | "SOVEMU_PASSWORD_SIGN_IN"
        | "SOVEMU_CLIENT_EVENTS"
        | "SOVEMU_TOKEN_KEY"
        | "SOVEMU_LOGIN_MAX_FAILURES"
        | "SOVEMU_LOGIN_MAX_FAILURES_PER_IP"
        | "SOVEMU_LOGIN_LOCKOUT_MINUTES" => Accounts,
        "SOVEMU_DATA_DIR"
        | "SOVEMU_ROMS_DIR"
        | "SOVEMU_BIOS_DIR"
        | "SOVEMU_LIBRARY_MODE"
        | "SOVEMU_CACHE_GB"
        | "SOVEMU_MIRROR_SYSTEMS"
        | "SOVEMU_SCAN_INTERVAL_MINUTES"
        | "SOVEMU_MIRROR_INTERVAL_MINUTES"
        | "SOVEMU_CORES_DIR"
        | "SOVEMU_CACHE_LARGE_GAME_MB"
        | "SOVEMU_COPY_THREADS" => Library,
        "SOVEMU_METADATA_AUTO"
        | "SOVEMU_LAUNCHBOX"
        | "SOVEMU_LAUNCHBOX_REFRESH_HOURS"
        | "SOVEMU_LAUNCHBOX_URL"
        | "SOVEMU_LAUNCHBOX_IMAGES_URL"
        | "SOVEMU_SCREENSCRAPER_DEVID"
        | "SOVEMU_SCREENSCRAPER_DEVPASSWORD"
        | "SOVEMU_SCREENSCRAPER_USER"
        | "SOVEMU_SCREENSCRAPER_PASSWORD"
        | "SOVEMU_THEGAMESDB_KEY"
        | "SOVEMU_METADATA_REGIONS"
        | "SOVEMU_METADATA_LANGUAGES"
        | "SOVEMU_SCREENSCRAPER_DELAY_MS"
        | "SOVEMU_ARTWORK_DOWNLOADS"
        | "SOVEMU_SCREENSCRAPER_URL"
        | "SOVEMU_THEGAMESDB_URL" => GameInformation,
        "SOVEMU_HTTP"
        | "SOVEMU_UDP_PORTS"
        | "SOVEMU_UDP_BIND"
        | "SOVEMU_ADVERTISE_IP"
        | "SOVEMU_ICE_SERVERS"
        | "SOVEMU_VIDEO_KBPS"
        | "SOVEMU_VIDEO_MIN_KBPS"
        | "SOVEMU_VIDEO_MAX_KBPS"
        | "SOVEMU_VIDEO_CODEC"
        | "SOVEMU_VIDEO_ENCODER"
        | "SOVEMU_AUDIO_KBPS"
        | "SOVEMU_IDLE_TIMEOUT"
        | "SOVEMU_WORKER_START_TIMEOUT"
        | "SOVEMU_WORKER_STOP_TIMEOUT"
        | "SOVEMU_REWIND_SECONDS"
        | "SOVEMU_REWIND_MAX_MB"
        | "SOVEMU_FAST_FORWARD"
        | "SOVEMU_KEYFRAME_INTERVAL"
        | "SOVEMU_SESSION_HEALTH_SECS"
        | "SOVEMU_SESSION_FAILURES"
        | "SOVEMU_HOME_FEATURED"
        | "SOVEMU_HOME_ROW"
        | "SOVEMU_GPU" => Streaming,
        "SOVEMU_SECURE_COOKIES"
        | "SOVEMU_TRUSTED_PROXIES"
        | "SOVEMU_MAINTENANCE_INTERVAL_MINUTES"
        | "SOVEMU_JOB_HISTORY_DAYS"
        | "SOVEMU_CLIENT_EVENT_DAYS"
        | "SOVEMU_ACTIVITY_HISTORY_DAYS"
        | "SOVEMU_RESOURCE_SAMPLE_SECONDS"
        | "SOVEMU_RESOURCE_HISTORY_DAYS"
        | "SOVEMU_ASSET_KEY"
        | "SOVEMU_ASSET_URL_HOURS"
        | "SOVEMU_LOG_FORMAT"
        | "SOVEMU_OTEL_ENDPOINT"
        | "SOVEMU_DECORATION"
        | "SOVEMU_BACKGROUND"
        | "SOVEMU_BACKGROUNDS_DIRS"
        | "SOVEMU_DECORATIONS_DIRS"
        | "SOVEMU_WEB_DIR"
        | "SOVEMU_WORKER"
        | "SOVEMU_DECORATIONS_BUILTIN" => Operations,
        _ => return None,
    })
}

/// A variable read outside clap: its group, and whether it holds a secret.
fn environment_setting(name: &str) -> (Group, bool) {
    if name.starts_with("SOVEMU_USERS__") {
        // `__PASSWORD` is a secret; `__PASSWORD_FILE` is only a path.
        (Group::Accounts, name.ends_with("__PASSWORD"))
    } else if name.starts_with("SOVEMU_SYSTEMS__") || name.starts_with("SOVEMU_CONTROLLERS__") {
        (Group::Library, false)
    } else {
        (Group::Operations, false)
    }
}

/// A value as shown: redacted to fit what it holds.
fn shown(env: &str, values: &[String]) -> Option<String> {
    let joined = values.join(",");
    if joined.is_empty() {
        return None;
    }
    Some(match env {
        "SOVEMU_OTEL_ENDPOINT" => redact::url(&joined),
        "SOVEMU_ICE_SERVERS" => serde_json::from_str(&joined)
            .map_or_else(|_| redact::MASK.to_owned(), |v| redact::ice_servers(&v).to_string()),
        "OTEL_EXPORTER_OTLP_HEADERS" => format!("({})", redact::header_names(&joined).join(",")),
        _ => joined,
    })
}

/// What the server makes of a setting, where that differs from what was given.
fn effective(config: &Config, env: &str) -> Option<String> {
    Some(match env {
        "SOVEMU_ROMS_DIR" => config.roms_dir.display().to_string(),
        "SOVEMU_BIOS_DIR" => config.bios_dir().display().to_string(),
        "SOVEMU_PUBLIC_URL" => config.public_urls().join(","),
        "SOVEMU_DECORATIONS_BUILTIN" => config.builtin_decorations().display().to_string(),
        "SOVEMU_CACHE_GB" if config.library_mode == LibraryMode::Cache => {
            config.cache_gb.unwrap_or(20).to_string()
        }
        _ => return None,
    })
    .filter(|v| !v.is_empty())
}

/// Every setting, grouped as the README's table, in `Config`'s order within each group.
pub(crate) fn settings(config: &Config) -> Vec<Setting> {
    let mut out = Vec::new();
    for arg in Config::command().get_arguments() {
        let Some(env) = arg.get_env().and_then(|e| e.to_str()) else { continue };
        let Some(group) = group(env) else { continue };
        let default: Vec<String> =
            arg.get_default_values().iter().map(|v| v.to_string_lossy().into_owned()).collect();
        let (source, values) =
            config.given.args.get(arg.get_id().as_str()).cloned().unwrap_or_else(|| {
                // Not given and no default: unset.
                (Source::Default, default.clone())
            });
        let secret = SECRETS.contains(&env);
        let (value, source) = if secret {
            // Secrets are read after parsing (`<VAR>_FILE`): ask `Config` itself.
            let set = config.secret(env).is_some();
            let source = if config.given.from_files.contains(&env) { Source::File } else { source };
            (Some(if set { "(set)" } else { "(unset)" }.to_owned()), source)
        } else {
            (shown(env, &values), source)
        };
        let description = arg
            .get_long_help()
            .or_else(|| arg.get_help())
            .map(|h| h.to_string())
            .unwrap_or_default();
        let effective = effective(config, env).filter(|e| Some(e) != value.as_ref());
        out.push(Setting {
            group,
            name: env.to_owned(),
            value,
            effective,
            source,
            secret,
            default: (!default.is_empty()).then(|| default.join(",")),
            description,
        });
    }
    for (name, raw) in &config.given.environment {
        let (group, secret) = environment_setting(name);
        out.push(Setting {
            group,
            value: if secret {
                Some("(set)".to_owned())
            } else {
                shown(name, std::slice::from_ref(raw))
            },
            name: name.clone(),
            effective: None,
            source: Source::Environment,
            secret,
            default: None,
            description: String::new(),
        });
    }
    // Stable: `Config`'s order within each group.
    out.sort_by_key(|s| s.group);
    out
}

/// The settings for the startup log: a heading per group, then `NAME=value` lines, with the
/// effective value and the source where they say something.
/// The settings whose names `keep` accepts, as [`settings`] lists them: an admin page's slice.
pub(crate) fn settings_where(config: &Config, keep: impl Fn(&str) -> bool) -> Vec<Setting> {
    settings(config).into_iter().filter(|s| keep(&s.name)).collect()
}

pub(crate) fn summary(config: &Config) -> String {
    let mut lines = Vec::new();
    let mut current = None;
    for s in settings(config) {
        if current != Some(s.group) {
            lines.push(format!("[{}]", s.group.label()));
            current = Some(s.group);
        }
        let mut line = format!("{}={}", s.name, s.value.as_deref().unwrap_or("(unset)"));
        if let Some(effective) = &s.effective {
            line.push_str(&format!(" → {effective}"));
        }
        match s.source {
            Source::File => line.push_str(" (from file)"),
            Source::CommandLine => line.push_str(" (command line)"),
            Source::Default | Source::Environment => {}
        }
        lines.push(line);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The README, whose settings table documents every setting.
    const README: &str = include_str!("../../../README.md");

    #[test]
    fn every_setting_has_a_group_and_a_row_in_the_readme() {
        for arg in Config::command().get_arguments() {
            let Some(env) = arg.get_env().and_then(|e| e.to_str()) else { continue };
            assert!(group(env).is_some(), "{env} has no group in configuration.rs");
            assert!(README.contains(&format!("`{env}`")), "{env} has no row in README.md");
        }
    }

    #[test]
    fn values_say_where_they_came_from() {
        let config = Config::parse_with_sources([
            "sovemu-server",
            "--udp-ports",
            "6000-6001",
            "--otel-endpoint",
            "https://u:t0k3n@collector/v1?key=k3y",
        ]);
        let all = settings(&config);
        let get = |name: &str| all.iter().find(|s| s.name == name).unwrap().clone();
        let ports = get("SOVEMU_UDP_PORTS");
        assert_eq!(
            (ports.value.as_deref(), ports.source),
            (Some("6000-6001"), Source::CommandLine)
        );
        assert_eq!(ports.default.as_deref(), Some("50000-50009"));
        assert_eq!(ports.group, Group::Streaming);
        let http = get("SOVEMU_HTTP");
        assert_eq!((http.value.as_deref(), http.source), (Some("0.0.0.0:8080"), Source::Default));
        assert!(http.description.contains("Address for the web client"));
        let otel = get("SOVEMU_OTEL_ENDPOINT");
        assert_eq!(otel.value.as_deref(), Some("https://***@collector/v1?***"));
        let roms = get("SOVEMU_ROMS_DIR");
        assert_eq!(roms.value, None);
        assert_eq!(roms.effective, None, "resolved as the state is built, not by parsing");
        let password = get("SOVEMU_ADMIN_PASSWORD");
        assert_eq!((password.value.as_deref(), password.secret), (Some("(unset)"), true));
        assert!(all.windows(2).all(|w| w[0].group <= w[1].group), "grouped");
    }

    #[test]
    fn structured_and_standard_variables_are_listed_and_redacted() {
        let mut config = Config::parse_with_sources(["sovemu-server"]);
        config.given.keep_environment(&[
            ("SOVEMU_USERS__0__USERNAME".into(), "kid".into()),
            ("SOVEMU_USERS__0__PASSWORD".into(), "hunter22".into()),
            ("SOVEMU_SYSTEMS__psx__CORE".into(), "pcsx_rearmed".into()),
            ("OTEL_EXPORTER_OTLP_HEADERS".into(), "authorization=Bearer s3cr3t".into()),
            ("RUST_LOG".into(), "debug".into()),
            ("HOME".into(), "/root".into()),
        ]);
        let all = settings(&config);
        let get = |name: &str| all.iter().find(|s| s.name == name).map(|s| s.value.clone());
        assert_eq!(get("SOVEMU_USERS__0__USERNAME"), Some(Some("kid".into())));
        assert_eq!(get("SOVEMU_USERS__0__PASSWORD"), Some(Some("(set)".into())));
        assert_eq!(get("SOVEMU_SYSTEMS__psx__CORE"), Some(Some("pcsx_rearmed".into())));
        assert_eq!(get("OTEL_EXPORTER_OTLP_HEADERS"), Some(Some("(authorization)".into())));
        assert_eq!(get("RUST_LOG"), Some(Some("debug".into())));
        assert_eq!(get("HOME"), None, "only the settings SovEmu reads");
        let summary = summary(&config);
        assert!(summary.contains("[Accounts]") && summary.contains("RUST_LOG=debug"), "{summary}");
        for secret in ["hunter22", "s3cr3t"] {
            assert!(!summary.contains(secret), "{secret} in {summary}");
        }
    }
}

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
    /// The environment variable (`SovEmu__Http__Listen`, `SovEmu__Users__0__Username`, `RUST_LOG`).
    pub(crate) name: String,
    /// As given, redacted; none when unset.
    pub(crate) value: Option<String>,
    /// What the server works it out to be, where that differs from the value (an unset
    /// `SovEmu__Paths__Library` is `<data>/roms`).
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
    /// Variables read outside clap: the structured settings (`SovEmu__Users__…`,
    /// `SovEmu__Systems__…`, `SovEmu__Controllers__…`) and the standard ones (`RUST_LOG`, `TZ`,
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
                [
                    "SovEmu__Users__",
                    "SovEmu__Systems__",
                    "SovEmu__Controllers__",
                    "SovEmu__Collections__",
                ]
                .iter()
                .any(|p| starts_with_any_case(k, p))
                    || k.starts_with("OTEL_")
                    || k == "RUST_LOG"
                    || k == "TZ"
            })
            .cloned()
            .collect();
        self.environment.sort();
    }
}

/// The README group of each `SovEmu__*` setting clap reads.
fn group(env: &str) -> Option<Group> {
    use Group::*;
    Some(match env {
        "SovEmu__Authentication__AdminUsername"
        | "SovEmu__Authentication__AdminPassword"
        | "SovEmu__Users__Prune"
        | "SovEmu__Authentication__SessionDays"
        | "SovEmu__Authentication__Tokens__AccessTokenMinutes"
        | "SovEmu__Authentication__Tokens__RefreshRetrySeconds"
        | "SovEmu__Http__PublicUrl"
        | "SovEmu__Authentication__PasswordSignIn"
        | "SovEmu__ClientErrors__Enabled"
        | "SovEmu__Authentication__TokenSigningKey"
        | "SovEmu__Authentication__Lockout__MaxFailures"
        | "SovEmu__Authentication__Lockout__MaxFailuresPerAddress"
        | "SovEmu__Authentication__Lockout__Minutes" => Accounts,
        "SovEmu__Paths__Data"
        | "SovEmu__Paths__Library"
        | "SovEmu__Paths__Bios"
        | "SovEmu__Library__Mode"
        | "SovEmu__Library__Cache__MaxGigabytes"
        | "SovEmu__Library__Mirror__Systems"
        | "SovEmu__Library__ScanIntervalMinutes"
        | "SovEmu__Library__Mirror__IntervalMinutes"
        | "SovEmu__Paths__Cores"
        | "SovEmu__Library__Cache__LargeGameMegabytes"
        | "SovEmu__Library__CopyThreads" => Library,
        "SovEmu__Metadata__AutoLookup"
        | "SovEmu__Metadata__Sources"
        | "SovEmu__Metadata__RefreshDays"
        | "SovEmu__Metadata__ScreenScraper__DevId"
        | "SovEmu__Metadata__ScreenScraper__DevPassword"
        | "SovEmu__Metadata__ScreenScraper__Username"
        | "SovEmu__Metadata__ScreenScraper__Password"
        | "SovEmu__Metadata__Regions"
        | "SovEmu__Metadata__Languages"
        | "SovEmu__Metadata__ScreenScraper__DelayMilliseconds"
        | "SovEmu__Metadata__ScreenScraper__Threads"
        | "SovEmu__Artwork__MaxConcurrentDownloads"
        | "SovEmu__Artwork__MaxPixels"
        | "SovEmu__Artwork__WebpQuality"
        | "SovEmu__Metadata__Videos"
        | "SovEmu__Metadata__ScreenScraper__Url" => GameInformation,
        "SovEmu__Http__Listen"
        | "SovEmu__Streaming__Udp__Ports"
        | "SovEmu__Streaming__Udp__Bind"
        | "SovEmu__Streaming__Udp__Dscp"
        | "SovEmu__Streaming__AdvertiseAddress"
        | "SovEmu__Streaming__Video__Kbps"
        | "SovEmu__Streaming__Video__MinKbps"
        | "SovEmu__Streaming__Video__MaxKbps"
        | "SovEmu__Streaming__Video__Codec"
        | "SovEmu__Streaming__Video__Encoder"
        | "SovEmu__Streaming__Video__Prescale"
        | "SovEmu__Streaming__Video__ScreenContent"
        | "SovEmu__Streaming__Audio__Kbps"
        | "SovEmu__Sessions__IdleTimeoutSeconds"
        | "SovEmu__Sessions__StartTimeoutSeconds"
        | "SovEmu__Sessions__StopTimeoutSeconds"
        | "SovEmu__Sessions__Rewind__Seconds"
        | "SovEmu__Sessions__Rewind__MaxMegabytes"
        | "SovEmu__Sessions__FastForward"
        | "SovEmu__Streaming__Video__KeyframeInterval"
        | "SovEmu__Streaming__Debug__VideoDropPercent"
        | "SovEmu__Streaming__Debug__VideoDelayMilliseconds"
        | "SovEmu__Streaming__Debug__VideoDropBurstMilliseconds"
        | "SovEmu__Streaming__Debug__VideoLimitKbps"
        | "SovEmu__Streaming__Debug__ReplayFrames"
        | "SovEmu__Streaming__Debug__VideoJitterMilliseconds"
        | "SovEmu__Streaming__Video__DropoutKeyframeMilliseconds"
        | "SovEmu__Streaming__Probe__Milliseconds"
        | "SovEmu__Streaming__Probe__LanMilliseconds"
        | "SovEmu__Streaming__Input__LapseMilliseconds"
        | "SovEmu__Sessions__HealthSeconds"
        | "SovEmu__Sessions__FailuresKept"
        | "SovEmu__Home__FeaturedCount"
        | "SovEmu__Home__RowLength"
        | "SovEmu__Rendering__Gpu" => Streaming,
        "SovEmu__Http__SecureCookies"
        | "SovEmu__Network__TrustedProxyNetwork"
        | "SovEmu__Jobs__MaintenanceIntervalMinutes"
        | "SovEmu__Backups__Directory"
        | "SovEmu__Backups__IntervalHours"
        | "SovEmu__Backups__Keep"
        | "SovEmu__Backups__RestoreFrom"
        | "SovEmu__Jobs__RetentionDays"
        | "SovEmu__ClientErrors__RetentionDays"
        | "SovEmu__ActivityLog__RetentionDays"
        | "SovEmu__Logging__BufferLines"
        | "SovEmu__SystemResources__SampleSeconds"
        | "SovEmu__SystemResources__RetentionDays"
        | "SovEmu__Assets__SecretKey"
        | "SovEmu__Assets__UrlHours"
        | "SovEmu__Logging__Format"
        | "SovEmu__Telemetry__Otlp__Endpoint"
        | "SovEmu__Decorations__Set"
        | "SovEmu__Appearance__Background"
        | "SovEmu__Appearance__BackgroundDirectories"
        | "SovEmu__Decorations__Directories"
        | "SovEmu__Paths__Web"
        | "SovEmu__Paths__Worker"
        | "SovEmu__Paths__BuiltInArtwork" => Operations,
        _ => return None,
    })
}

/// Whether `name` starts with `prefix`, in any letter case (as settings are read).
fn starts_with_any_case(name: &str, prefix: &str) -> bool {
    name.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

/// A variable read outside clap: its group, and whether it holds a secret.
fn environment_setting(name: &str) -> (Group, bool) {
    if starts_with_any_case(name, "SovEmu__Users__") {
        (Group::Accounts, name.to_ascii_lowercase().ends_with("__password"))
    } else if ["SovEmu__Systems__", "SovEmu__Controllers__", "SovEmu__Collections__"]
        .iter()
        .any(|p| starts_with_any_case(name, p))
    {
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
        "SovEmu__Telemetry__Otlp__Endpoint" => redact::url(&joined),
        "OTEL_EXPORTER_OTLP_HEADERS" => format!("({})", redact::header_names(&joined).join(",")),
        _ => joined,
    })
}

/// What the server makes of a setting, where that differs from what was given.
fn effective(config: &Config, env: &str) -> Option<String> {
    Some(match env {
        "SovEmu__Paths__Library" => config.roms_dir.display().to_string(),
        "SovEmu__Paths__Bios" => config.bios_dir().display().to_string(),
        "SovEmu__Http__PublicUrl" => config.public_urls().join(","),
        "SovEmu__Paths__BuiltInArtwork" => config.builtin_artwork().root().display().to_string(),
        "SovEmu__Library__Cache__MaxGigabytes" if config.library_mode == LibraryMode::Cache => {
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
            // Empty secrets are cleared after parsing: ask `Config` itself.
            let set = config.secret(env).is_some();
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
            // A list's row shows how its entries are given: `SovEmu__…__<n>`.
            let documented = [format!("`{env}`"), format!("`{env}__<n>`")];
            assert!(documented.iter().any(|d| README.contains(d)), "{env} has no row in README.md");
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
        let ports = get("SovEmu__Streaming__Udp__Ports");
        assert_eq!(
            (ports.value.as_deref(), ports.source),
            (Some("6000-6001"), Source::CommandLine)
        );
        assert_eq!(ports.default.as_deref(), Some("50000-50009"));
        assert_eq!(ports.group, Group::Streaming);
        let http = get("SovEmu__Http__Listen");
        assert_eq!((http.value.as_deref(), http.source), (Some("0.0.0.0:8080"), Source::Default));
        assert!(http.description.contains("Address for the web client"));
        let otel = get("SovEmu__Telemetry__Otlp__Endpoint");
        assert_eq!(otel.value.as_deref(), Some("https://***@collector/v1?***"));
        let roms = get("SovEmu__Paths__Library");
        assert_eq!(roms.value, None);
        assert_eq!(roms.effective, None, "resolved as the state is built, not by parsing");
        let password = get("SovEmu__Authentication__AdminPassword");
        assert_eq!((password.value.as_deref(), password.secret), (Some("(unset)"), true));
        assert!(all.windows(2).all(|w| w[0].group <= w[1].group), "grouped");
    }

    #[test]
    fn structured_and_standard_variables_are_listed_and_redacted() {
        let mut config = Config::parse_with_sources(["sovemu-server"]);
        config.given.keep_environment(&[
            ("SovEmu__Users__0__Username".into(), "kid".into()),
            ("SovEmu__Users__0__Password".into(), "hunter22".into()),
            ("SovEmu__Systems__psx__Core".into(), "pcsx_rearmed".into()),
            ("OTEL_EXPORTER_OTLP_HEADERS".into(), "authorization=Bearer s3cr3t".into()),
            ("RUST_LOG".into(), "debug".into()),
            ("HOME".into(), "/root".into()),
        ]);
        let all = settings(&config);
        let get = |name: &str| all.iter().find(|s| s.name == name).map(|s| s.value.clone());
        assert_eq!(get("SovEmu__Users__0__Username"), Some(Some("kid".into())));
        assert_eq!(get("SovEmu__Users__0__Password"), Some(Some("(set)".into())));
        assert_eq!(get("SovEmu__Systems__psx__Core"), Some(Some("pcsx_rearmed".into())));
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

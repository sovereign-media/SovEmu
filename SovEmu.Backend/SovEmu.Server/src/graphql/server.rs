//! The admin's server pages: configuration, network, storage, telemetry, rendering, cores and attributions.

use super::*;

/// A service SovEmu looks games up in.
#[derive(SimpleObject)]
pub(super) struct AttributedService {
    pub(super) name: String,
    /// What SovEmu takes from it.
    pub(super) provides: String,
    pub(super) url: String,
    /// The data's licence, where the service states one.
    pub(super) licence: Option<String>,
    pub(super) licence_url: Option<String>,
    /// What its terms ask of an app, or that they state nothing.
    pub(super) terms: String,
}

/// A component the image ships besides the cores.
#[derive(SimpleObject)]
pub(super) struct BundledComponent {
    pub(super) name: String,
    pub(super) what: String,
    pub(super) licence: String,
    pub(super) url: String,
    /// The commit it is taken from, when pinned.
    pub(super) commit: Option<String>,
}

/// What the server draws on and ships with (the cores are `cores`).
#[derive(SimpleObject)]
pub(super) struct Attributions {
    pub(super) services: Vec<AttributedService>,
    pub(super) components: Vec<BundledComponent>,
    /// The FinalBurn Neo commit whose DAT files name arcade games.
    pub(super) fbneo_commit: String,
}

/// OpenTelemetry export: whether it is on, where to, and how it has gone.
#[derive(SimpleObject)]
pub(super) struct TelemetryReport {
    /// `SOVEMU_OTEL_ENDPOINT` is set: traces and metrics are exported.
    pub(super) enabled: bool,
    /// The collector, credentials masked.
    pub(super) endpoint: Option<String>,
    /// `text` or `json` (`SOVEMU_LOG_FORMAT`).
    pub(super) log_format: String,
    /// `OTEL_SERVICE_NAME`, else SovEmu's.
    pub(super) service_name: String,
    /// How often metrics are sent (`OTEL_METRIC_EXPORT_INTERVAL`), milliseconds.
    pub(super) metric_interval_ms: f64,
    /// The headers sent with each export (`OTEL_EXPORTER_OTLP_HEADERS`), by name only.
    pub(super) header_names: Vec<String>,
    pub(super) traces: ExportResult,
    pub(super) metrics: ExportResult,
    /// The settings this page is about, as the Configuration page shows them.
    pub(super) settings: Vec<ConfigSetting>,
}

/// How one signal's exports have gone since the server started.
#[derive(SimpleObject)]
pub(super) struct ExportResult {
    /// When the last export finished (Unix seconds); null before the first.
    pub(super) last_at: Option<f64>,
    pub(super) ok: bool,
    /// The last export's error, URLs' credentials masked.
    pub(super) error: Option<String>,
    pub(super) exports: f64,
    pub(super) failures: f64,
}

impl From<crate::telemetry::ExportStatus> for ExportResult {
    fn from(s: crate::telemetry::ExportStatus) -> Self {
        Self {
            last_at: s
                .last_at
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64()),
            ok: s.ok,
            error: s.error,
            exports: s.exports as f64,
            failures: s.failures as f64,
        }
    }
}

/// Where the data folder's space goes.
#[derive(SimpleObject)]
pub(super) struct StorageReport {
    /// When it was measured (Unix seconds): measurements are kept a few minutes.
    pub(super) measured_at: f64,
    pub(super) data_dir: String,
    /// The parts of the data folder, largest first. `other` is what no part claims.
    pub(super) areas: Vec<StorageArea>,
    /// Each user's saves, largest first.
    pub(super) saves: Vec<UserStorage>,
    /// The data folder's filesystem, in bytes.
    pub(super) total_bytes: Option<f64>,
    pub(super) free_bytes: Option<f64>,
    /// `cache` mode: the cache's cap, in bytes (`SOVEMU_CACHE_GB`).
    pub(super) cache_limit_bytes: Option<f64>,
}

#[derive(SimpleObject)]
pub(super) struct StorageArea {
    /// `database`, `saves`, `cache`, `discs`, `mirror`, `artwork`, `metadata`,
    /// `decorations`, `builtin_decorations`, `roms`, `bios` or `other`.
    pub(super) id: String,
    pub(super) path: String,
    pub(super) bytes: f64,
    pub(super) files: f64,
}

#[derive(SimpleObject)]
pub(super) struct UserStorage {
    pub(super) user_id: i64,
    /// Null for a user that has been deleted (their saves are left behind).
    pub(super) username: Option<String>,
    pub(super) bytes: f64,
    pub(super) files: f64,
}

/// How browsers and apps reach the server and its game streams.
#[derive(SimpleObject)]
pub(super) struct NetworkReport {
    /// The web client and API's address (`SOVEMU_HTTP`).
    pub(super) http: String,
    /// The UDP ports for game sessions, one per concurrent session (`SOVEMU_UDP_PORTS`).
    pub(super) udp_first: i32,
    pub(super) udp_last: i32,
    pub(super) port_count: i32,
    /// Ports held: by a session, or by a worker that has not exited yet.
    pub(super) ports_in_use: i32,
    pub(super) sessions: i32,
    /// The address session ports bind to (`SOVEMU_UDP_BIND`).
    pub(super) udp_bind: String,
    /// The address browsers send media to, as workers advertise it.
    pub(super) advertised_ip: String,
    /// It was worked out (the machine's primary address), not set (`SOVEMU_ADVERTISE_IP`). In
    /// a container that is the container's own address, which browsers cannot reach.
    pub(super) advertised_ip_detected: bool,
    /// The addresses sign-in works at: `SOVEMU_PUBLIC_URL`, else this host's own.
    public_urls: Vec<String>,
    public_urls_configured: bool,
    /// Sign-in works at the address this request was made to.
    pub(super) sign_in_available_here: bool,
    /// The address this request was made to (its `Host`).
    pub(super) request_host: String,
    /// This request's client, as the server resolved it (behind trusted proxies, the client).
    pub(super) client_ip: Option<String>,
    /// This request's `X-Forwarded-For`, for checking a proxy.
    pub(super) forwarded_for: Option<String>,
    pub(super) trusted_proxies: Vec<String>,
    pub(super) secure_cookies: bool,
    /// STUN/TURN servers handed to browsers.
    pub(super) ice_servers: Vec<IceServerSummary>,
    /// The settings this page is about, as the Configuration page shows them.
    pub(super) settings: Vec<ConfigSetting>,
}

/// An ICE server, without its credentials.
#[derive(SimpleObject)]
pub(super) struct IceServerSummary {
    pub(super) urls: Vec<String>,
    /// It has a username and credential (a TURN server).
    pub(super) has_credential: bool,
}

/// The settings the Network page shows.
pub(super) const NETWORK_SETTINGS: &[&str] = &[
    "SOVEMU_HTTP",
    "SOVEMU_UDP_PORTS",
    "SOVEMU_UDP_BIND",
    "SOVEMU_ADVERTISE_IP",
    "SOVEMU_ICE_SERVERS",
    "SOVEMU_PUBLIC_URL",
    "SOVEMU_TRUSTED_PROXIES",
    "SOVEMU_SECURE_COOKIES",
];

/// A group of settings, as the README's settings table has them.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigGroup {
    Accounts,
    Library,
    GameInformation,
    Streaming,
    Operations,
}

/// Where a setting's value comes from.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigSource {
    /// Not set: the default.
    Default,
    Environment,
    /// A secret read from the file `<VAR>_FILE` names.
    File,
    CommandLine,
}

/// A setting of the running server.
#[derive(SimpleObject)]
pub(super) struct ConfigSetting {
    pub(super) group: ConfigGroup,
    /// The environment variable that sets it.
    pub(super) name: String,
    /// As given, with credentials masked; null when unset. A secret only says `(set)` or
    /// `(unset)`.
    pub(super) value: Option<String>,
    /// What the server makes of it, where that differs from the value (an unset
    /// `SOVEMU_ROMS_DIR` is `<data>/roms`).
    pub(super) effective: Option<String>,
    pub(super) source: ConfigSource,
    pub(super) secret: bool,
    pub(super) default: Option<String>,
    pub(super) description: String,
}

impl From<crate::configuration::Setting> for ConfigSetting {
    fn from(s: crate::configuration::Setting) -> Self {
        use crate::configuration::{Group, Source};
        Self {
            group: match s.group {
                Group::Accounts => ConfigGroup::Accounts,
                Group::Library => ConfigGroup::Library,
                Group::GameInformation => ConfigGroup::GameInformation,
                Group::Streaming => ConfigGroup::Streaming,
                Group::Operations => ConfigGroup::Operations,
            },
            source: match s.source {
                Source::Default => ConfigSource::Default,
                Source::Environment => ConfigSource::Environment,
                Source::File => ConfigSource::File,
                Source::CommandLine => ConfigSource::CommandLine,
            },
            name: s.name,
            value: s.value,
            effective: s.effective,
            secret: s.secret,
            default: s.default,
            description: s.description,
        }
    }
}

/// A libretro core: shipped in the image, or found in the cores folder.
#[derive(SimpleObject)]
pub(super) struct CoreReport {
    /// Its file name without `_libretro.<ext>` (`snes9x`).
    pub(super) name: String,
    /// What the core calls itself: from its probe, else the licence table's name for it.
    pub(super) label: Option<String>,
    pub(super) version: Option<String>,
    /// Its file in the cores folder; null when it is shipped but missing here.
    pub(super) file: Option<String>,
    /// It loads (the image's build-time probe, else a probe now).
    pub(super) loaded: bool,
    pub(super) error: Option<String>,
    /// The systems it plays.
    pub(super) systems: Vec<CoreSystem>,
    /// As its core info file states it; null for a core the image doesn't ship.
    pub(super) licence: Option<String>,
    /// Its licence forbids commercial use.
    pub(super) non_commercial: bool,
    /// Its source.
    pub(super) source_url: Option<String>,
}

/// A system a core plays.
#[derive(SimpleObject)]
pub(super) struct CoreSystem {
    pub(super) id: String,
    pub(super) name: String,
    /// This system's games start with this core.
    pub(super) chosen: bool,
}

/// A device the startup GPU probe tried.
#[derive(SimpleObject)]
pub(super) struct GpuDevice {
    /// The render node; none for the software renderer.
    pub(super) node: Option<String>,
    pub(super) driver: Option<String>,
    /// `hardware`, `virtual` (a hypervisor's adapter) or `software`.
    pub(super) kind: String,
    pub(super) vendor: Option<String>,
    pub(super) renderer: Option<String>,
    pub(super) version: Option<String>,
    pub(super) works: bool,
    pub(super) error: Option<String>,
    /// Workers render on this one.
    pub(super) chosen: bool,
}

/// Where 3D systems render, decided at startup.
#[derive(SimpleObject)]
pub(super) struct GpuStatus {
    /// `SOVEMU_GPU` as configured.
    pub(super) setting: String,
    pub(super) mode: Rendering,
    /// What 3D games draw with.
    pub(super) renderer: Option<String>,
    pub(super) devices: Vec<GpuDevice>,
    /// Why the setting could not be honoured, or why nothing could be probed.
    pub(super) note: Option<String>,
    /// What workers are started with (`--gpu`): a render node, `auto`, `software` or `off`.
    pub(super) worker_arg: String,
    /// When the probe ran (Unix seconds): once, at startup, so a GPU added later needs a
    /// restart. None when nothing was probed (`SOVEMU_GPU=off`).
    pub(super) probed_at: Option<f64>,
}

/// A package SovEmu ships.
#[derive(SimpleObject)]
pub(super) struct NoticePackage {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) repository: Option<String>,
}

/// One licence text, and the packages it covers.
#[derive(SimpleObject)]
pub(super) struct Notice {
    /// SPDX (`MIT`).
    pub(super) id: String,
    pub(super) name: String,
    pub(super) packages: Vec<NoticePackage>,
    pub(super) text: String,
}

impl From<&crate::notices::Notice> for Notice {
    fn from(n: &crate::notices::Notice) -> Self {
        Self {
            id: n.id.clone(),
            name: n.name.clone(),
            packages: n
                .crates
                .iter()
                .map(|p| NoticePackage {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    repository: p.repository.clone(),
                })
                .collect(),
            text: n.text.clone(),
        }
    }
}

/// The open-source packages SovEmu ships, with their licences.
#[derive(SimpleObject)]
pub(super) struct ThirdPartyNotices {
    /// The server's and the workers' Rust crates (cargo-about, at the image's build).
    pub(super) server: Vec<Notice>,
    /// The web client's npm packages (its build); empty for a development build.
    pub(super) web: Vec<Notice>,
}

#[derive(Default)]
pub(super) struct ServerQuery;

#[Object]
impl ServerQuery {
    /// Admins: the game-information services and the components the image ships, with their
    /// licences and terms.
    async fn attributions(&self, ctx: &Context<'_>) -> Result<Attributions> {
        admin(ctx).await?;
        let own = |s: &str| s.to_owned();
        Ok(Attributions {
            services: crate::about::SERVICES
                .iter()
                .map(|s| AttributedService {
                    name: own(s.name),
                    provides: own(s.provides),
                    url: own(s.url),
                    licence: s.licence.map(own),
                    licence_url: s.licence_url.map(own),
                    terms: own(s.terms),
                })
                .collect(),
            components: crate::about::COMPONENTS
                .iter()
                .map(|c| BundledComponent {
                    name: own(c.name),
                    what: own(c.what),
                    licence: own(c.licence),
                    url: own(c.url),
                    commit: c.commit.map(own),
                })
                .collect(),
            fbneo_commit: own(sovemu_library::info::fbneo::FBNEO_COMMIT),
        })
    }

    /// Admins: every open-source package the server and the web client ship, with its licence
    /// text, grouped by licence.
    async fn third_party_notices(&self, ctx: &Context<'_>) -> Result<ThirdPartyNotices> {
        admin(ctx).await?;
        let web_dir = state(ctx).config.web_dir.as_deref();
        Ok(ThirdPartyNotices {
            server: crate::notices::server().iter().map(Notice::from).collect(),
            web: crate::notices::web(web_dir).iter().map(Notice::from).collect(),
        })
    }

    /// Admins: OpenTelemetry export: whether it is on, where to, and how each signal's last
    /// export went.
    async fn telemetry(&self, ctx: &Context<'_>) -> Result<TelemetryReport> {
        admin(ctx).await?;
        let config = &state(ctx).config;
        let env = |name: &str| {
            config.given.environment.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
        };
        let (traces, metrics) = crate::telemetry::export_status();
        Ok(TelemetryReport {
            enabled: config.otel_endpoint.is_some(),
            endpoint: config.otel_endpoint.as_deref().map(crate::redact::url),
            log_format: config.log_format.as_arg().to_owned(),
            service_name: env("OTEL_SERVICE_NAME")
                .unwrap_or_else(|| crate::telemetry::SERVICE.into()),
            metric_interval_ms: env("OTEL_METRIC_EXPORT_INTERVAL")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(60_000.0),
            header_names: env("OTEL_EXPORTER_OTLP_HEADERS")
                .map(|h| crate::redact::header_names(&h))
                .unwrap_or_default(),
            traces: traces.into(),
            metrics: metrics.into(),
            settings: crate::configuration::settings_where(config, |name| {
                matches!(name, "SOVEMU_OTEL_ENDPOINT" | "SOVEMU_LOG_FORMAT" | "RUST_LOG")
                    || name.starts_with("OTEL_")
            })
            .into_iter()
            .map(Into::into)
            .collect(),
        })
    }

    /// Admins: where the data folder's space goes. Measured at most every few minutes, unless
    /// `refresh` asks for a measurement now.
    async fn storage(
        &self,
        ctx: &Context<'_>,
        #[graphql(default)] refresh: bool,
    ) -> Result<StorageReport> {
        admin(ctx).await?;
        let state = state(ctx);
        let r = crate::storage::report(state, refresh).await.api()?;
        Ok(StorageReport {
            measured_at: r.measured_at,
            data_dir: r.data_dir.display().to_string(),
            areas: r
                .areas
                .into_iter()
                .map(|a| StorageArea {
                    id: a.id.to_owned(),
                    path: a.path.display().to_string(),
                    bytes: a.bytes as f64,
                    files: a.files as f64,
                })
                .collect(),
            saves: r
                .saves
                .into_iter()
                .map(|u| UserStorage {
                    user_id: u.user_id,
                    username: u.username,
                    bytes: u.bytes as f64,
                    files: u.files as f64,
                })
                .collect(),
            total_bytes: r.total_bytes.map(|b| b as f64),
            free_bytes: r.free_bytes.map(|b| b as f64),
            cache_limit_bytes: (state.config.library_mode == crate::LibraryMode::Cache)
                .then(|| state.config.cache_gb.unwrap_or(20).saturating_mul(1 << 30) as f64),
        })
    }

    /// Admins: how browsers and apps reach the server and its game streams.
    async fn network(&self, ctx: &Context<'_>) -> Result<NetworkReport> {
        admin(ctx).await?;
        let state = state(ctx);
        let config = &state.config;
        let headers = &ctx.data_unchecked::<Auth>().headers;
        let header = |name: axum::http::HeaderName| {
            headers.get(name).and_then(|h| h.to_str().ok()).map(str::to_owned)
        };
        let bound = std::net::SocketAddr::new(config.udp_bind, config.udp_ports.0);
        let advertised = sovemu_protocol::net::advertised_addr(config.advertise_ip, bound).ip();
        let ice_servers = config
            .ice_servers
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|s| IceServerSummary {
                        urls: match &s["urls"] {
                            serde_json::Value::String(u) => vec![u.clone()],
                            serde_json::Value::Array(us) => {
                                us.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect()
                            }
                            _ => Vec::new(),
                        },
                        has_credential: s.get("credential").is_some(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(NetworkReport {
            http: config.http.to_string(),
            udp_first: config.udp_ports.0.into(),
            udp_last: config.udp_ports.1.into(),
            port_count: state.workers.port_count() as i32,
            ports_in_use: state.workers.ports_in_use() as i32,
            sessions: state.workers.count() as i32,
            udp_bind: config.udp_bind.to_string(),
            advertised_ip: advertised.to_string(),
            advertised_ip_detected: config.advertise_ip.is_none()
                && config.udp_bind.is_unspecified(),
            public_urls: config.public_urls(),
            public_urls_configured: !config.public_url.is_empty(),
            sign_in_available_here: state.identity.sign_in_config(headers).is_some(),
            request_host: header(axum::http::header::HOST).unwrap_or_default(),
            client_ip: ctx
                .data_opt::<crate::client_ip::ClientIp>()
                .and_then(|c| c.0)
                .map(|ip| ip.to_string()),
            forwarded_for: header(axum::http::HeaderName::from_static("x-forwarded-for")),
            trusted_proxies: config.trusted_proxies.iter().map(ToString::to_string).collect(),
            secure_cookies: config.secure_cookies,
            ice_servers,
            settings: crate::configuration::settings_where(config, |name| {
                NETWORK_SETTINGS.contains(&name)
            })
            .into_iter()
            .map(Into::into)
            .collect(),
        })
    }

    /// Admins: the server's effective configuration, every setting with where its value came
    /// from, credentials masked (as the startup log prints it).
    async fn configuration(&self, ctx: &Context<'_>) -> Result<Vec<ConfigSetting>> {
        admin(ctx).await?;
        Ok(crate::configuration::settings(&state(ctx).config).into_iter().map(Into::into).collect())
    }

    /// Admins: every shipped core, then any other in the cores folder: version, whether it
    /// loads, the systems it plays, and its licence.
    async fn cores(&self, ctx: &Context<'_>) -> Result<Vec<CoreReport>> {
        admin(ctx).await?;
        Ok(crate::cores::report(state(ctx))
            .await
            .into_iter()
            .map(|c| CoreReport {
                label: c.label.or_else(|| c.licence.map(|l| l.label.to_owned())),
                name: c.name,
                version: c.version,
                file: c.file.map(|f| f.display().to_string()),
                loaded: c.loaded,
                error: c.error,
                systems: c
                    .systems
                    .into_iter()
                    .map(|s| CoreSystem {
                        id: s.system.id.to_owned(),
                        name: s.system.name.to_owned(),
                        chosen: s.chosen,
                    })
                    .collect(),
                licence: c.licence.map(|l| l.licence.to_owned()),
                non_commercial: c.licence.is_some_and(|l| l.non_commercial),
                source_url: c.licence.map(|l| l.source.to_owned()),
            })
            .collect())
    }

    /// Admins: the libretro stable release the image's cores come from.
    async fn libretro_release(&self, ctx: &Context<'_>) -> Result<&'static str> {
        admin(ctx).await?;
        Ok(sovemu_library::cores::LIBRETRO_RELEASE)
    }

    /// Admins: the BIOS folder on the server (`SOVEMU_BIOS_DIR`): each system's files go in
    /// the subfolder its `bios.folder` names.
    async fn bios_dir(&self, ctx: &Context<'_>) -> Result<String> {
        admin(ctx).await?;
        Ok(state(ctx).config.bios_dir().display().to_string())
    }

    /// Admins: where 3D systems render, and every device the startup probe tried.
    async fn gpu(&self, ctx: &Context<'_>) -> Result<GpuStatus> {
        admin(ctx).await?;
        let g = &state(ctx).gpu;
        Ok(GpuStatus {
            setting: g.setting.clone(),
            mode: g.mode.into(),
            renderer: g.renderer.clone(),
            devices: g
                .devices
                .iter()
                .map(|d| GpuDevice {
                    node: d.node.clone(),
                    driver: d.driver.clone(),
                    kind: d.kind.clone(),
                    vendor: d.vendor.clone(),
                    renderer: d.renderer.clone(),
                    version: d.version.clone(),
                    works: d.works,
                    error: d.error.clone(),
                    chosen: g.chosen(d),
                })
                .collect(),
            note: g.note.clone(),
            worker_arg: g.worker_arg.clone(),
            probed_at: g
                .probed_at
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64()),
        })
    }
}

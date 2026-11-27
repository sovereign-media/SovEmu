//! Emulator settings: the libretro core options of each system's core, which an admin can
//! change in the web app.
//!
//! What a core offers is only known by loading it, and the server never loads a core: it asks a
//! worker (`sovemu-worker probe <core>`) once per core and keeps the answer. Some cores declare
//! their options only once a game is loaded, which the probe cannot see; for those, the options
//! the core's last game session reported (`core_options`) are used, and until a game has run
//! the page says to start one. A game starts with
//! each option taken from, last one winning: the core's default, SovEmu's own default for the
//! system ([`sovemu_library::systems`]), the admin's choice (the `system_settings` table), and the
//! environment (`SovEmu__Systems__<id>__Options__<name>`). The environment always wins and is shown
//! as locked, so the app stays reproducible from configuration; the page offers the admin's
//! choices as environment lines to move there.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::AppState;
use crate::error::{ApiError, ApiResult};
use sovemu_library::systems::System;

pub(crate) use sovemu_protocol::ipc::{CoreCategory, CoreOption};

/// What the probe says about a core.
#[derive(Debug, Clone, Deserialize)]
pub struct CoreInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub variables: Vec<CoreOption>,
    #[serde(default)]
    pub categories: Vec<CoreCategory>,
}

/// Probe results, per core library, for the life of the process.
#[derive(Debug, Default)]
pub struct CoreInfoCache(Mutex<HashMap<PathBuf, Arc<CoreInfo>>>);

/// One core in the image's build-time probe (`<cores>/manifest.json`, `sovemu-worker probe`).
#[derive(Debug, Deserialize)]
struct ManifestEntry {
    path: PathBuf,
    #[serde(flatten)]
    info: CoreInfo,
}

impl CoreInfoCache {
    /// Starts from the image's build-time probe of every core it ships
    /// (`<cores_dir>/manifest.json`), so no core is probed again at run time; empty without
    /// one (a local build), when cores are probed as they are first asked about.
    pub fn from_manifest(cores_dir: &Path) -> Self {
        let path = cores_dir.join("manifest.json");
        let Ok(bytes) = std::fs::read(&path) else { return Self::default() };
        match serde_json::from_slice::<Vec<ManifestEntry>>(&bytes) {
            Ok(entries) => {
                let known = entries
                    .into_iter()
                    .filter_map(|e| {
                        // Keyed as `find_core` names them, wherever the probe ran.
                        let file = e.path.file_name()?;
                        Some((cores_dir.join(file), Arc::new(e.info)))
                    })
                    .collect::<HashMap<_, _>>();
                tracing::info!(cores = known.len(), path = %path.display(), "core manifest read");
                Self(Mutex::new(known))
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "core manifest unreadable");
                Self::default()
            }
        }
    }

    /// What `core` declares, probing it in a worker the first time.
    pub async fn get(&self, worker: &Path, core: &Path) -> ApiResult<Arc<CoreInfo>> {
        let known = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(core).cloned();
        if let Some(info) = known {
            return match &info.error {
                // The image's probe could not load it.
                Some(e) => Err(ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("the core did not load: {e}"),
                )),
                None => Ok(info),
            };
        }
        let mut command = tokio::process::Command::new(worker);
        command
            .arg("probe")
            .arg(core)
            .env_clear()
            .envs(sovemu_sessions::workers::worker_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let unavailable = |e: String| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e);
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .map_err(|_| unavailable("reading the core's settings timed out".into()))?
            .map_err(|e| unavailable(format!("reading the core's settings: {e}")))?;
        let mut found: Vec<CoreInfo> = serde_json::from_slice(&output.stdout)
            .map_err(|e| unavailable(format!("reading the core's settings: {e}")))?;
        let info = Arc::new(found.pop().ok_or_else(|| unavailable("no probe result".into()))?);
        if let Some(e) = &info.error {
            return Err(unavailable(format!("the core did not load: {e}")));
        }
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(core.to_owned(), info.clone());
        Ok(info)
    }
}

/// The file name a core is remembered by (`fceumm_libretro.so`).
fn core_name(core: &Path) -> String {
    core.file_name().map_or_else(|| core.display().to_string(), |n| n.to_string_lossy().into())
}

/// Keeps the options a core declared in a game session, for cores the probe cannot see them
/// in. Failing to keep them only costs the Settings page, so it is logged, not returned.
pub(crate) async fn remember(
    db: &SqlitePool,
    core: &Path,
    (options, categories): &sovemu_sessions::workers::Declared,
) {
    let result = sqlx::query(
        "INSERT INTO core_options (core, options, categories) VALUES (?, ?, ?)
         ON CONFLICT(core) DO UPDATE SET options = excluded.options,
           categories = excluded.categories, updated_at = datetime('now')",
    )
    .bind(core_name(core))
    .bind(serde_json::to_string(options).unwrap_or_default())
    .bind(serde_json::to_string(categories).unwrap_or_default())
    .execute(db)
    .await;
    if let Err(e) = result {
        tracing::warn!(core = %core.display(), error = %e, "remembering core options failed");
    }
}

/// The options a core's last session reported, if any.
async fn remembered(
    db: &SqlitePool,
    core: &Path,
) -> sqlx::Result<Option<sovemu_sessions::workers::Declared>> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT options, categories FROM core_options WHERE core = ?")
            .bind(core_name(core))
            .fetch_optional(db)
            .await?;
    Ok(row
        .and_then(|(o, c)| Some((serde_json::from_str(&o).ok()?, serde_json::from_str(&c).ok()?))))
}

/// What a core offers: the probe's options, or where the probe saw none (the core declares
/// them with a game), the last session's. The flag says neither has any yet.
fn offered(
    probe: &CoreInfo,
    remembered: Option<sovemu_sessions::workers::Declared>,
) -> (Vec<CoreOption>, Vec<CoreCategory>, bool) {
    if !probe.variables.is_empty() {
        return (probe.variables.clone(), probe.categories.clone(), false);
    }
    match remembered {
        Some((options, categories)) if !options.is_empty() => (options, categories, false),
        _ => (Vec::new(), Vec::new(), true),
    }
}

/// Where an option's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    Core,
    Sovemu,
    Admin,
    Environment,
}

/// The admin's choices for a system.
pub(crate) async fn admin_values(
    db: &SqlitePool,
    system: &str,
) -> sqlx::Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM system_settings WHERE system = ?")
            .bind(system)
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().collect())
}

/// The options a game of `system` starts with, beyond the core's own defaults, and where each
/// comes from. SovEmu's own include its GPU enhancements (`System::gpu_options`) when
/// `hardware`: the 3D systems render on a hardware GPU here.
pub(crate) fn layered(
    system: &System,
    hardware: bool,
    admin: &BTreeMap<String, String>,
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, (String, Source)> {
    let gpu_options = if hardware { system.gpu_options } else { &[] };
    let mut out: BTreeMap<String, (String, Source)> = system
        .options
        .iter()
        .chain(gpu_options)
        .map(|(k, v)| ((*k).to_owned(), ((*v).to_owned(), Source::Sovemu)))
        .collect();
    for (k, v) in admin {
        out.insert(k.clone(), (v.clone(), Source::Admin));
    }
    for (k, v) in environment {
        out.insert(k.clone(), (v.clone(), Source::Environment));
    }
    out
}

/// The options to start a game of `system` with.
pub(crate) async fn for_launch(
    state: &AppState,
    system: &System,
) -> ApiResult<Vec<(String, String)>> {
    let admin = admin_values(&state.db, system.id).await?;
    Ok(layered(system, state.gpu.renders_on_hardware(), &admin, &environment(state, system))
        .into_iter()
        .map(|(k, (v, _))| (k, v))
        .collect())
}

fn environment(state: &AppState, system: &System) -> BTreeMap<String, String> {
    state.config.systems.get(system.id).map(|s| s.options.clone()).unwrap_or_default()
}

#[derive(Debug, Serialize)]
pub(crate) struct Setting {
    pub(crate) key: String,
    pub(crate) description: String,
    /// What it does, in the core's words (empty where the core does not say).
    pub(crate) info: String,
    /// The key of the core's category for it; the web client groups the rest itself.
    pub(crate) category: Option<String>,
    pub(crate) choices: Vec<String>,
    /// A display label per choice.
    pub(crate) labels: Vec<String>,
    /// The core's own default.
    pub(crate) default: String,
    pub(crate) value: String,
    pub(crate) source: Source,
}

#[derive(Debug, Serialize)]
pub(crate) struct SystemSettings {
    pub(crate) system: String,
    pub(crate) core: String,
    pub(crate) settings: Vec<Setting>,
    /// The core's own option categories, in its order.
    pub(crate) categories: Vec<CoreCategory>,
    /// The core lists its settings only with a game loaded, and none has run yet.
    pub(crate) needs_game: bool,
    /// The admin's choices as environment lines, to make them part of the configuration.
    pub(crate) environment: Vec<String>,
    pub(crate) run_ahead: RunAhead,
}

/// How many frames a system's pictures run ahead of its games, hiding their own input lag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct RunAhead {
    /// 0: off.
    pub(crate) frames: u32,
    /// `Sovemu` when nothing sets it (the default), else the admin or the environment (locked).
    pub(crate) source: Source,
    /// Whether the system can run ahead at all (the 2D systems).
    pub(crate) supported: bool,
}

/// The run-ahead a game of `system` starts with: the environment's, else the admin's, else
/// SovEmu's default (`System::default_run_ahead`: on for the 2D systems).
pub(crate) fn layered_run_ahead(
    system: &System,
    admin: Option<u32>,
    environment: Option<u32>,
) -> RunAhead {
    let supported = system.run_ahead();
    let (frames, source) = match (environment, admin) {
        (Some(frames), _) => (frames, Source::Environment),
        (None, Some(frames)) => (frames, Source::Admin),
        (None, None) => (system.default_run_ahead(), Source::Sovemu),
    };
    RunAhead { frames: if supported { frames } else { 0 }, source, supported }
}

async fn admin_run_ahead(db: &SqlitePool, system: &str) -> sqlx::Result<Option<u32>> {
    let frames: Option<i64> =
        sqlx::query_scalar("SELECT frames FROM system_run_ahead WHERE system = ?")
            .bind(system)
            .fetch_optional(db)
            .await?;
    Ok(frames.map(|f| f.clamp(0, i64::from(sovemu_protocol::ipc::MAX_RUN_AHEAD)) as u32))
}

fn environment_run_ahead(state: &AppState, system: &System) -> Option<u32> {
    state.config.systems.get(system.id).and_then(|s| s.run_ahead)
}

/// The frames a game of `system` runs ahead (0: off).
pub(crate) async fn run_ahead_for_launch(state: &AppState, system: &System) -> ApiResult<u32> {
    let admin = admin_run_ahead(&state.db, system.id).await?;
    Ok(layered_run_ahead(system, admin, environment_run_ahead(state, system)).frames)
}

/// Sets how many frames `id`'s games run ahead, or with `None` goes back to off.
pub(crate) async fn set_run_ahead(
    state: &AppState,
    id: &str,
    frames: Option<u32>,
) -> ApiResult<SystemSettings> {
    let system = system(id)?;
    if environment_run_ahead(state, system).is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("run-ahead is set by SovEmu__Systems__{}__RunAhead", system.id),
        ));
    }
    match frames {
        Some(frames) => {
            let max = sovemu_protocol::ipc::MAX_RUN_AHEAD;
            if frames > max {
                return Err(ApiError::bad_request(format!("run-ahead: at most {max} frames")));
            }
            if frames > 0 && !system.run_ahead() {
                return Err(ApiError::bad_request(format!(
                    "{} is too costly to run ahead",
                    system.name
                )));
            }
            sqlx::query(
                "INSERT INTO system_run_ahead (system, frames) VALUES (?, ?)
                 ON CONFLICT(system) DO UPDATE SET frames = excluded.frames,
                   updated_at = datetime('now')",
            )
            .bind(system.id)
            .bind(i64::from(frames))
            .execute(&state.db)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM system_run_ahead WHERE system = ?")
                .bind(system.id)
                .execute(&state.db)
                .await?;
        }
    }
    describe(state, system).await
}

async fn describe(state: &AppState, system: &'static System) -> ApiResult<SystemSettings> {
    let core = crate::api::find_core(state, system).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("no core installed for {}", system.name),
        )
    })?;
    let worker = state.workers.worker_bin();
    let info = state.core_info.get(&worker, &core).await?;
    let (options, categories, needs_game) = offered(&info, remembered(&state.db, &core).await?);
    let admin = admin_values(&state.db, system.id).await?;
    let mut layers =
        layered(system, state.gpu.renders_on_hardware(), &admin, &environment(state, system));
    let mut settings: Vec<Setting> = options
        .iter()
        .map(|o| {
            let (value, source) =
                layers.remove(&o.key).unwrap_or_else(|| (o.default.clone(), Source::Core));
            Setting {
                key: o.key.clone(),
                description: if o.description.is_empty() {
                    o.key.clone()
                } else {
                    o.description.clone()
                },
                info: o.info.clone(),
                category: o.category.clone(),
                choices: o.choices.clone(),
                labels: if o.labels.len() == o.choices.len() {
                    o.labels.clone()
                } else {
                    o.choices.clone()
                },
                default: o.default.clone(),
                value,
                source,
            }
        })
        .collect();
    // Options set for the system that this core does not declare (another core's, or a typo):
    // shown so they are not invisible, with no choices.
    settings.extend(layers.into_iter().map(|(key, (value, source))| Setting {
        description: key.clone(),
        key,
        info: String::new(),
        category: None,
        choices: Vec::new(),
        labels: Vec::new(),
        default: String::new(),
        value,
        source,
    }));
    let admin_run_ahead = admin_run_ahead(&state.db, system.id).await?;
    let run_ahead =
        layered_run_ahead(system, admin_run_ahead, environment_run_ahead(state, system));
    let mut environment: Vec<String> = admin
        .iter()
        .map(|(k, v)| format!("SovEmu__Systems__{}__Options__{k}={v}", system.id))
        .collect();
    if let (Some(frames), Source::Admin) = (admin_run_ahead, run_ahead.source) {
        environment.push(format!("SovEmu__Systems__{}__RunAhead={frames}", system.id));
    }
    let core = [info.name.clone(), info.version.clone()].into_iter().flatten().collect::<Vec<_>>();
    Ok(SystemSettings {
        system: system.id.to_owned(),
        core: core.join(" "),
        settings,
        categories,
        needs_game,
        environment,
        run_ahead,
    })
}

fn system(id: &str) -> ApiResult<&'static System> {
    sovemu_library::systems::by_id(id).ok_or_else(|| ApiError::not_found("system"))
}

pub(crate) async fn get_settings(state: &AppState, id: &str) -> ApiResult<SystemSettings> {
    describe(state, system(id)?).await
}

/// Sets options (`key` → `Some(value)`), or with `None` goes back to the default.
pub(crate) async fn put_settings(
    state: &AppState,
    id: &str,
    changes: BTreeMap<String, Option<String>>,
) -> ApiResult<SystemSettings> {
    let system = system(id)?;
    let core = crate::api::find_core(state, system).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("no core installed for {}", system.name),
        )
    })?;
    let info = state.core_info.get(&state.workers.worker_bin(), &core).await?;
    let (options, _, _) = offered(&info, remembered(&state.db, &core).await?);
    let locked = environment(state, system);
    for (key, value) in &changes {
        if locked.contains_key(key) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("{key} is set by SovEmu__Systems__{}__Options__{key}", system.id),
            ));
        }
        let option = options.iter().find(|o| &o.key == key);
        match (option, value) {
            (None, Some(_)) => {
                return Err(ApiError::bad_request(format!("{} has no option {key}", system.name)));
            }
            (Some(o), Some(v)) if !o.choices.is_empty() && !o.choices.contains(v) => {
                return Err(ApiError::bad_request(format!(
                    "{key}: {v:?} is not one of {}",
                    o.choices.join(", ")
                )));
            }
            _ => {}
        }
    }
    let mut tx = crate::db::write_tx(&state.db).await?;
    for (key, value) in &changes {
        match value {
            Some(v) => {
                sqlx::query(
                    "INSERT INTO system_settings (system, key, value) VALUES (?, ?, ?)
                     ON CONFLICT(system, key) DO UPDATE SET value = excluded.value,
                       updated_at = datetime('now')",
                )
                .bind(system.id)
                .bind(key)
                .bind(v)
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM system_settings WHERE system = ? AND key = ?")
                    .bind(system.id)
                    .bind(key)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await?;
    describe(state, system).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_image_manifest_answers_without_probing() {
        let dir = std::env::temp_dir().join(format!("sovemu-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // As the image's probe wrote it, somewhere else.
        std::fs::write(
            dir.join("manifest.json"),
            r#"[{"path":"/opt/sovemu/cores/good_libretro.so","name":"Good","version":"1.0",
                 "valid_extensions":[],"need_fullpath":false,"supports_no_game":false,
                 "variables":[],"categories":[]},
                {"path":"/opt/sovemu/cores/bad_libretro.so","error":"missing libGL",
                 "valid_extensions":[],"need_fullpath":false,"supports_no_game":false,
                 "variables":[],"categories":[]}]"#,
        )
        .unwrap();
        let cache = CoreInfoCache::from_manifest(&dir);
        // A worker that does not exist: the answers must come from the manifest.
        let worker = Path::new("/nonexistent/sovemu-worker");
        let good = cache.get(worker, &dir.join("good_libretro.so")).await.unwrap();
        assert_eq!((good.name.as_deref(), good.version.as_deref()), (Some("Good"), Some("1.0")));
        let bad = cache.get(worker, &dir.join("bad_libretro.so")).await.unwrap_err();
        assert!(bad.message.contains("missing libGL"), "{}", bad.message);
        assert!(
            CoreInfoCache::from_manifest(Path::new("/nonexistent")).0.lock().unwrap().is_empty()
        );
        std::fs::remove_dir_all(dir).ok();
    }

    fn option(key: &str) -> CoreOption {
        CoreOption {
            key: key.into(),
            description: key.into(),
            info: String::new(),
            category: None,
            choices: vec!["a".into(), "b".into()],
            labels: Vec::new(),
            default: "a".into(),
        }
    }

    #[test]
    fn falls_back_to_what_a_session_reported() {
        let probe = |variables| CoreInfo {
            name: None,
            version: None,
            error: None,
            variables,
            categories: Vec::new(),
        };
        let session = Some((vec![option("from_session")], Vec::new()));
        // The probe saw options: they win.
        let (o, _, needs) = offered(&probe(vec![option("from_probe")]), session.clone());
        assert_eq!((o[0].key.as_str(), needs), ("from_probe", false));
        // It saw none (declared with a game): the last session's.
        let (o, _, needs) = offered(&probe(Vec::new()), session);
        assert_eq!((o[0].key.as_str(), needs), ("from_session", false));
        // Neither yet: start a game first.
        let (o, _, needs) = offered(&probe(Vec::new()), None);
        assert!(o.is_empty() && needs);
    }

    #[test]
    fn run_ahead_comes_from_the_environment_then_the_admin_else_is_one_frame() {
        let snes = sovemu_library::systems::by_id("snes").unwrap();
        let default = layered_run_ahead(snes, None, None);
        assert_eq!((default.frames, default.source, default.supported), (1, Source::Sovemu, true));
        let admin = layered_run_ahead(snes, Some(2), None);
        assert_eq!((admin.frames, admin.source), (2, Source::Admin));
        let off = layered_run_ahead(snes, Some(0), None);
        assert_eq!((off.frames, off.source), (0, Source::Admin), "an admin can turn it off");
        let env = layered_run_ahead(snes, Some(2), Some(1));
        assert_eq!((env.frames, env.source), (1, Source::Environment));
        let n64 = sovemu_library::systems::by_id("n64").unwrap();
        let n64 = layered_run_ahead(n64, Some(2), None);
        assert_eq!((n64.frames, n64.supported), (0, false), "never on a 3D system");
        let ps2 = sovemu_library::systems::by_id("ps2").unwrap();
        assert_eq!(layered_run_ahead(ps2, None, None).frames, 0, "not even by default");
    }

    #[test]
    fn the_environment_wins_then_the_admin_then_sovemu() {
        let psx = sovemu_library::systems::by_id("psx").unwrap();
        let admin = BTreeMap::from([
            ("pcsx_rearmed_cd_readahead".to_owned(), "64".to_owned()),
            ("pcsx_rearmed_frameskip".to_owned(), "1".to_owned()),
        ]);
        let env = BTreeMap::from([("pcsx_rearmed_frameskip".to_owned(), "2".to_owned())]);
        let l = layered(psx, false, &BTreeMap::new(), &BTreeMap::new());
        assert_eq!(l["pcsx_rearmed_cd_readahead"], ("256".into(), Source::Sovemu));
        let l = layered(psx, false, &admin, &env);
        assert_eq!(l["pcsx_rearmed_cd_readahead"], ("64".into(), Source::Admin));
        assert_eq!(l["pcsx_rearmed_frameskip"], ("2".into(), Source::Environment));
    }

    #[test]
    fn the_gpu_enhancements_apply_only_on_a_hardware_gpu_and_still_give_way() {
        let gamecube = sovemu_library::systems::by_id("gamecube").unwrap();
        let none = BTreeMap::new();
        let l = layered(gamecube, true, &none, &none);
        assert_eq!(l["dolphin_max_anisotropy"], ("2".into(), Source::Sovemu));
        let l = layered(gamecube, false, &none, &none);
        assert!(!l.contains_key("dolphin_max_anisotropy"), "the core's own default in software");
        let admin = BTreeMap::from([("dolphin_max_anisotropy".to_owned(), "3".to_owned())]);
        let env = BTreeMap::from([("dolphin_shader_compilation_mode".to_owned(), "0".to_owned())]);
        let l = layered(gamecube, true, &admin, &env);
        assert_eq!(l["dolphin_max_anisotropy"], ("3".into(), Source::Admin));
        assert_eq!(l["dolphin_shader_compilation_mode"], ("0".into(), Source::Environment));
    }
}

//! Emulator settings: the libretro core options of each system's core, which an admin can
//! change in the web app.
//!
//! What a core offers is only known by loading it, and the server never loads a core: it asks a
//! worker (`sovemu-worker probe <core>`) once per core and keeps the answer. A game starts with
//! each option taken from, last one winning: the core's default, SovEmu's own default for the
//! system ([`crate::systems`]), the admin's choice (the `system_settings` table), and the
//! environment (`SOVEMU_SYSTEMS__<id>__OPTIONS__<name>`). The environment always wins and is shown
//! as locked, so the app stays reproducible from configuration; the page offers the admin's
//! choices as environment lines to move there.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::AppState;
use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::systems::System;

/// One option as the core declares it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CoreOption {
    pub key: String,
    pub description: String,
    /// What it does, where the core explains it.
    #[serde(default)]
    pub info: String,
    /// The key of its category, where the core groups its options.
    #[serde(default)]
    pub category: Option<String>,
    pub choices: Vec<String>,
    /// A display label per choice.
    #[serde(default)]
    pub labels: Vec<String>,
    pub default: String,
}

/// A group of options the core declares ("Video", "Audio", …).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CoreCategory {
    pub key: String,
    pub description: String,
    #[serde(default)]
    pub info: String,
}

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

impl CoreInfoCache {
    /// What `core` declares, probing it in a worker the first time.
    pub async fn get(&self, worker: &Path, core: &Path) -> ApiResult<Arc<CoreInfo>> {
        if let Some(info) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(core) {
            return Ok(info.clone());
        }
        let mut command = tokio::process::Command::new(worker);
        command
            .arg("probe")
            .arg(core)
            .env_clear()
            .envs(crate::workers::worker_env())
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

/// Where an option's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Core,
    Sovemu,
    Admin,
    Environment,
}

/// The admin's choices for a system.
pub async fn admin_values(db: &SqlitePool, system: &str) -> sqlx::Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM system_settings WHERE system = ?")
            .bind(system)
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().collect())
}

/// The options a game of `system` starts with, beyond the core's own defaults, and where each
/// comes from.
pub fn layered(
    system: &System,
    admin: &BTreeMap<String, String>,
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, (String, Source)> {
    let mut out: BTreeMap<String, (String, Source)> = system
        .options
        .iter()
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
pub async fn for_launch(state: &AppState, system: &System) -> ApiResult<Vec<(String, String)>> {
    let admin = admin_values(&state.db, system.id).await?;
    Ok(layered(system, &admin, &environment(state, system))
        .into_iter()
        .map(|(k, (v, _))| (k, v))
        .collect())
}

fn environment(state: &AppState, system: &System) -> BTreeMap<String, String> {
    state.config.systems.get(system.id).map(|s| s.options.clone()).unwrap_or_default()
}

#[derive(Debug, Serialize)]
pub struct Setting {
    key: String,
    description: String,
    /// What it does, in the core's words (empty where the core does not say).
    info: String,
    /// The key of the core's category for it; the web client groups the rest itself.
    category: Option<String>,
    choices: Vec<String>,
    /// A display label per choice.
    labels: Vec<String>,
    /// The core's own default.
    default: String,
    value: String,
    source: Source,
}

#[derive(Debug, Serialize)]
pub struct SystemSettings {
    system: String,
    core: String,
    settings: Vec<Setting>,
    /// The core's own option categories, in its order.
    categories: Vec<CoreCategory>,
    /// The admin's choices as environment lines, to make them part of the configuration.
    environment: Vec<String>,
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
    let admin = admin_values(&state.db, system.id).await?;
    let mut layers = layered(system, &admin, &environment(state, system));
    let mut settings: Vec<Setting> = info
        .variables
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
    let environment = admin
        .iter()
        .map(|(k, v)| format!("SOVEMU_SYSTEMS__{}__OPTIONS__{k}={v}", system.id))
        .collect();
    let core = [info.name.clone(), info.version.clone()].into_iter().flatten().collect::<Vec<_>>();
    Ok(SystemSettings {
        system: system.id.to_owned(),
        core: core.join(" "),
        settings,
        categories: info.categories.clone(),
        environment,
    })
}

fn system(id: &str) -> ApiResult<&'static System> {
    crate::systems::by_id(id).ok_or_else(|| ApiError::not_found("system"))
}

pub async fn get_settings(
    State(state): State<AppState>,
    _admin: AdminUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<SystemSettings>> {
    Ok(Json(describe(&state, system(&id)?).await?))
}

/// `{"<key>": "<value>" | null}`: sets an option, or with null goes back to the default.
pub async fn put_settings(
    State(state): State<AppState>,
    _admin: AdminUser,
    UrlPath(id): UrlPath<String>,
    Json(changes): Json<BTreeMap<String, Option<String>>>,
) -> ApiResult<Json<SystemSettings>> {
    let system = system(&id)?;
    let core = crate::api::find_core(&state, system).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("no core installed for {}", system.name),
        )
    })?;
    let info = state.core_info.get(&state.workers.worker_bin(), &core).await?;
    let locked = environment(&state, system);
    for (key, value) in &changes {
        if locked.contains_key(key) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("{key} is set by SOVEMU_SYSTEMS__{}__OPTIONS__{key}", system.id),
            ));
        }
        let option = info.variables.iter().find(|o| &o.key == key);
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
    let mut tx = state.db.begin().await?;
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
    Ok(Json(describe(&state, system).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_wins_then_the_admin_then_sovemu() {
        let psx = crate::systems::by_id("psx").unwrap();
        let admin = BTreeMap::from([
            ("pcsx_rearmed_cd_readahead".to_owned(), "64".to_owned()),
            ("pcsx_rearmed_frameskip".to_owned(), "1".to_owned()),
        ]);
        let env = BTreeMap::from([("pcsx_rearmed_frameskip".to_owned(), "2".to_owned())]);
        let l = layered(psx, &BTreeMap::new(), &BTreeMap::new());
        assert_eq!(l["pcsx_rearmed_cd_readahead"], ("256".into(), Source::Sovemu));
        let l = layered(psx, &admin, &env);
        assert_eq!(l["pcsx_rearmed_cd_readahead"], ("64".into(), Source::Admin));
        assert_eq!(l["pcsx_rearmed_frameskip"], ("2".into(), Source::Environment));
    }
}

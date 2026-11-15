//! Button mappings per controller model, shared by every client. The IDs and input names are
//! in docs/CONTROLLERS.md; this module only stores mappings and checks their shape.
//!
//! A user's mapping for a model is, in order: their own (saved from any client), else the
//! default from the environment (`SOVEMU_CONTROLLERS__<n>__ID` / `__NAME` / `__BINDINGS`), else
//! an admin's default (saved from the admin page), else the clients' built-in defaults (no
//! mapping at all). The environment wins over the admin page, which shows those models locked
//! and offers its own defaults as environment lines.

use std::collections::BTreeMap;

use axum::http::StatusCode;

use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::User;
use crate::error::{ApiError, ApiResult};

/// The inputs a controller (anything but the keyboard) can bind, named by position.
pub const PAD_INPUTS: [&str; 16] = [
    "south", "east", "west", "north", "l1", "r1", "l2", "r2", "select", "start", "l3", "r3", "up",
    "down", "left", "right",
];
/// The keyboard's fast-forward and rewind keys, which cannot be bound.
const RESERVED_KEYS: [&str; 2] = ["Tab", "Backspace"];
const MAX_ID: usize = 64;
const MAX_NAME: usize = 128;
const MAX_KEY: usize = 32;
const MAX_BINDINGS: usize = 64;
/// Mappings a user may keep: far more controller models than anyone owns.
const MAX_MAPPINGS: i64 = 64;

/// A RetroPad button (libretro's RETRO_DEVICE_ID_JOYPAD_*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, async_graphql::Enum)]
#[serde(rename_all = "UPPERCASE")]
#[graphql(rename_items = "UPPERCASE")]
pub enum PadButton {
    B,
    Y,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
    A,
    X,
    L,
    R,
    L2,
    R2,
    L3,
    R3,
}

/// One input driving one button.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    async_graphql::SimpleObject,
    async_graphql::InputObject,
)]
#[graphql(input_name = "BindingInput")]
pub struct Binding {
    /// A controller input (`south`, `l1`, `up` …) or a keyboard's `KeyboardEvent.code`.
    pub input: String,
    pub button: PadButton,
}

/// Where a mapping comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum MappingSource {
    /// The user's own.
    User,
    /// An admin's default, from the admin page.
    Admin,
    /// A default from `SOVEMU_CONTROLLERS__<n>__…`, which an admin cannot change (locked).
    Environment,
}

/// A mapping for one controller model.
#[derive(Debug, Clone, async_graphql::SimpleObject)]
pub struct ControllerMapping {
    /// `keyboard`, a known model (`xbox`, `dualsense` …) or `pad:<vendor>:<product>`.
    pub controller: String,
    /// What the controller called itself when the mapping was saved, for lists.
    pub name: String,
    pub bindings: Vec<Binding>,
    pub source: MappingSource,
}

/// A default mapping declared in the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerSpec {
    pub controller: String,
    pub name: String,
    pub bindings: Vec<Binding>,
}

#[derive(sqlx::FromRow)]
struct Row {
    controller: String,
    name: String,
    bindings: String,
}

impl Row {
    fn into_mapping(self, source: MappingSource) -> ControllerMapping {
        // Only this module writes the column, so it parses; an empty list is the safe fallback.
        let bindings = serde_json::from_str(&self.bindings).unwrap_or_default();
        ControllerMapping { controller: self.controller, name: self.name, bindings, source }
    }
}

impl ControllerSpec {
    fn mapping(&self) -> ControllerMapping {
        ControllerMapping {
            controller: self.controller.clone(),
            name: self.name.clone(),
            bindings: self.bindings.clone(),
            source: MappingSource::Environment,
        }
    }
}

/// A button by its libretro name (`B`, `SELECT`, `L2` …), as the API and the environment write it.
pub fn parse_button(name: &str) -> Option<PadButton> {
    serde_json::from_value(serde_json::Value::String(name.trim().to_ascii_uppercase())).ok()
}

/// `south:B,east:A`, the environment's form of a binding list.
pub fn bindings_line(bindings: &[Binding]) -> String {
    let pairs: Vec<String> = bindings
        .iter()
        .map(|b| {
            let button = serde_json::to_value(b.button).ok();
            format!("{}:{}", b.input, button.as_ref().and_then(|v| v.as_str()).unwrap_or(""))
        })
        .collect();
    pairs.join(",")
}

/// Parses `south:B, east:A` (the environment's `__BINDINGS`).
pub fn parse_bindings(line: &str) -> Result<Vec<Binding>, String> {
    line.split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (input, button) =
                pair.split_once(':').ok_or_else(|| format!("{pair:?} is not input:BUTTON"))?;
            let button = parse_button(button).ok_or_else(|| {
                format!("{button:?} is not a RetroPad button (B, A, SELECT, L2 …)")
            })?;
            Ok(Binding { input: input.trim().to_owned(), button })
        })
        .collect()
}

/// Whether `id` is a controller ID's shape: lowercase ASCII letters, digits, `-` and `:`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b':')
}

/// Checks a mapping against docs/CONTROLLERS.md: known input names for a controller, plain
/// `KeyboardEvent.code` values (not the hotkeys) for the keyboard, each input at most once.
pub fn check(controller: &str, name: &str, bindings: &[Binding]) -> Result<(), String> {
    if !valid_id(controller) {
        return Err("a controller ID is up to 64 lowercase letters, digits, '-' and ':'".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err("a controller name is up to 128 characters".into());
    }
    if bindings.len() > MAX_BINDINGS {
        return Err("a mapping has up to 64 bindings".into());
    }
    let keyboard = controller == "keyboard";
    let mut seen = std::collections::HashSet::new();
    for b in bindings {
        let ok = if keyboard {
            !b.input.is_empty()
                && b.input.len() <= MAX_KEY
                && b.input.bytes().all(|c| c.is_ascii_alphanumeric())
                && !RESERVED_KEYS.contains(&b.input.as_str())
        } else {
            PAD_INPUTS.contains(&b.input.as_str())
        };
        if !ok {
            return Err(format!(
                "'{}' is not an input a {} can bind",
                b.input,
                if keyboard { "keyboard" } else { "controller" }
            ));
        }
        if !seen.insert(b.input.as_str()) {
            return Err(format!("'{}' is bound twice", b.input));
        }
    }
    Ok(())
}

/// The defaults: the environment's, then an admin's for the models it leaves alone.
pub async fn defaults(state: &AppState) -> ApiResult<Vec<ControllerMapping>> {
    let mut out: BTreeMap<String, ControllerMapping> =
        admin_defaults(state).await?.into_iter().map(|m| (m.controller.clone(), m)).collect();
    for spec in &state.config.controllers {
        out.insert(spec.controller.clone(), spec.mapping());
    }
    Ok(out.into_values().collect())
}

async fn admin_defaults(state: &AppState) -> ApiResult<Vec<ControllerMapping>> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT controller, name, bindings FROM controller_defaults ORDER BY controller",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| r.into_mapping(MappingSource::Admin)).collect())
}

/// The mappings in force for a user, by controller ID: their own over the defaults. A model
/// missing here uses the clients' built-in defaults.
pub async fn list(state: &AppState, user: &User) -> ApiResult<Vec<ControllerMapping>> {
    let mut out: BTreeMap<String, ControllerMapping> =
        defaults(state).await?.into_iter().map(|m| (m.controller.clone(), m)).collect();
    let rows = sqlx::query_as::<_, Row>(
        "SELECT controller, name, bindings FROM controller_mappings WHERE user_id = ?",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    for m in rows.into_iter().map(|r| r.into_mapping(MappingSource::User)) {
        out.insert(m.controller.clone(), m);
    }
    Ok(out.into_values().collect())
}

/// The admin's defaults as environment lines, numbered after the ones already there, to make
/// them part of the configuration.
pub async fn environment_lines(state: &AppState) -> ApiResult<Vec<String>> {
    let configured = &state.config.controllers;
    let mut lines = Vec::new();
    let mut n = configured.len();
    for m in admin_defaults(state).await? {
        if configured.iter().any(|c| c.controller == m.controller) {
            continue;
        }
        lines.push(format!("SOVEMU_CONTROLLERS__{n}__ID={}", m.controller));
        if !m.name.is_empty() {
            lines.push(format!("SOVEMU_CONTROLLERS__{n}__NAME={}", m.name));
        }
        lines.push(format!("SOVEMU_CONTROLLERS__{n}__BINDINGS={}", bindings_line(&m.bindings)));
        n += 1;
    }
    Ok(lines)
}

/// Saves (replaces) the default for a controller model, for every user without their own.
pub async fn save_default(
    state: &AppState,
    controller: &str,
    name: &str,
    bindings: Vec<Binding>,
) -> ApiResult<ControllerMapping> {
    let name = name.trim();
    check(controller, name, &bindings).map_err(ApiError::bad_request)?;
    if state.config.controllers.iter().any(|c| c.controller == controller) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("{controller}'s default is set in the environment (SOVEMU_CONTROLLERS)"),
        ));
    }
    let json = serde_json::to_string(&bindings).map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO controller_defaults (controller, name, bindings) VALUES (?, ?, ?)
         ON CONFLICT(controller) DO UPDATE SET name = excluded.name,
           bindings = excluded.bindings, updated_at = datetime('now')",
    )
    .bind(controller)
    .bind(name)
    .bind(&json)
    .execute(&state.db)
    .await?;
    Ok(ControllerMapping {
        controller: controller.to_owned(),
        name: name.to_owned(),
        bindings,
        source: MappingSource::Admin,
    })
}

/// Forgets an admin's default, so the model is back on the built-in one. Returns whether there
/// was one.
pub async fn reset_default(state: &AppState, controller: &str) -> ApiResult<bool> {
    let removed = sqlx::query("DELETE FROM controller_defaults WHERE controller = ?")
        .bind(controller)
        .execute(&state.db)
        .await?
        .rows_affected();
    Ok(removed > 0)
}

/// Saves (replaces) the user's mapping for a controller model.
pub async fn save(
    state: &AppState,
    user: &User,
    controller: &str,
    name: &str,
    bindings: Vec<Binding>,
) -> ApiResult<ControllerMapping> {
    let name = name.trim();
    check(controller, name, &bindings).map_err(ApiError::bad_request)?;
    let json = serde_json::to_string(&bindings).map_err(ApiError::internal)?;
    let mut tx = state.db.begin().await?;
    let (count, exists): (i64, bool) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(MAX(controller = ?), 0) FROM controller_mappings WHERE user_id = ?",
    )
    .bind(controller)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists && count >= MAX_MAPPINGS {
        return Err(ApiError::bad_request("too many controller mappings: reset one first"));
    }
    sqlx::query(
        "INSERT INTO controller_mappings (user_id, controller, name, bindings) VALUES (?, ?, ?, ?)
         ON CONFLICT(user_id, controller) DO UPDATE SET name = excluded.name,
           bindings = excluded.bindings, updated_at = datetime('now')",
    )
    .bind(user.id)
    .bind(controller)
    .bind(name)
    .bind(&json)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ControllerMapping {
        controller: controller.to_owned(),
        name: name.to_owned(),
        bindings,
        source: MappingSource::User,
    })
}

/// Forgets the user's mapping for a controller model, so it is back on the defaults (an
/// admin's or the environment's, else the built-in one). Returns
/// whether there was one.
pub async fn reset(state: &AppState, user: &User, controller: &str) -> ApiResult<bool> {
    let removed =
        sqlx::query("DELETE FROM controller_mappings WHERE user_id = ? AND controller = ?")
            .bind(user.id)
            .bind(controller)
            .execute(&state.db)
            .await?
            .rows_affected();
    Ok(removed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(input: &str, button: PadButton) -> Binding {
        Binding { input: input.to_owned(), button }
    }

    #[test]
    fn checks_ids_and_inputs() {
        assert!(check("xbox", "Xbox Wireless Controller", &[b("south", PadButton::A)]).is_ok());
        assert!(check("pad:2dc8:6101", "", &[b("start", PadButton::Start)]).is_ok());
        assert!(
            check("keyboard", "", &[b("KeyZ", PadButton::B), b("ArrowUp", PadButton::Up)]).is_ok()
        );
        assert!(check("Xbox", "", &[]).is_err(), "IDs are lowercase");
        assert!(check("", "", &[]).is_err());
        assert!(check("xbox", "", &[b("KeyZ", PadButton::B)]).is_err(), "a key on a pad");
        assert!(
            check("keyboard", "", &[b("south", PadButton::B)]).is_ok(),
            "a plain word is a key"
        );
        assert!(check("keyboard", "", &[b("Tab", PadButton::B)]).is_err(), "the hotkeys");
        assert!(check("keyboard", "", &[b("Key Z", PadButton::B)]).is_err());
        assert!(check("xbox", "", &[b("south", PadButton::A), b("south", PadButton::B)]).is_err());
    }

    #[test]
    fn reads_and_writes_the_environment_form() {
        let parsed = parse_bindings(" south:a, east:B ,,l2:L2").unwrap();
        assert_eq!(
            parsed,
            vec![b("south", PadButton::A), b("east", PadButton::B), b("l2", PadButton::L2)]
        );
        assert_eq!(bindings_line(&parsed), "south:A,east:B,l2:L2");
        assert!(parse_bindings("south").is_err());
        assert!(parse_bindings("south:Z").is_err());
        assert_eq!(parse_bindings("").unwrap(), vec![]);
    }

    #[test]
    fn buttons_use_libretro_names() {
        let json = serde_json::to_string(&b("l2", PadButton::L2)).unwrap();
        assert_eq!(json, r#"{"input":"l2","button":"L2"}"#);
        let back: Binding =
            serde_json::from_str(r#"{"input":"select","button":"SELECT"}"#).unwrap();
        assert_eq!(back.button, PadButton::Select);
    }
}

//! Soft patches: the IPS, UPS and BPS files the scan found with each game (`library.rs`), and
//! which one each player plays. A patch beside a game with its name applies unless the player
//! chose otherwise (RetroArch's rule); a player may pick another, or the original.
//!
//! The worker applies the patch to a copy as the game loads (`sovemu-worker`'s `patch.rs`): the
//! library is never written. A patched game keeps its own saves (`<game> [<patch>]`), as it is in
//! effect another game.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::FromRow;

use crate::AppState;
use crate::api::Game;
use crate::auth::User;
use crate::error::{ApiError, ApiResult};

/// A patch for a game.
#[derive(Debug, Clone, Serialize, FromRow)]
pub(crate) struct Patch {
    pub id: i64,
    pub name: String,
    /// Relative to the ROM directory.
    #[serde(skip)]
    pub path: String,
    /// Applied unless the player chose otherwise.
    pub is_default: bool,
}

/// The patch each of a list's games plays for a user: their choice, else the default. Binds the
/// user's id, then the games' ids (`ids` placeholders).
pub(crate) fn chosen_sql(ids: usize) -> String {
    format!(
        "SELECT gp.game_id, gp.name FROM game_patches gp
         LEFT JOIN patch_choices c ON c.game_id = gp.game_id AND c.user_id = ?
         WHERE gp.game_id IN ({})
           AND (c.patch_id = gp.id OR (c.user_id IS NULL AND gp.is_default))",
        vec!["?"; ids].join(", ")
    )
}

/// Sets `patch` on each of `games` that plays one for this user.
pub(crate) async fn mark(state: &AppState, user_id: i64, games: &mut [Game]) -> ApiResult<()> {
    let mut names: HashMap<i64, String> = HashMap::new();
    // SQLite allows thousands of parameters; lists are read in parts well inside that.
    for part in games.chunks(500) {
        let sql = chosen_sql(part.len());
        let mut query = sqlx::query_as::<_, (i64, String)>(&sql).bind(user_id);
        for g in part {
            query = query.bind(g.id);
        }
        names.extend(query.fetch_all(&state.db).await?);
    }
    for g in games {
        g.patch = names.remove(&g.id);
    }
    Ok(())
}

/// A patch as the admin sees it: with its game, and how many players chose it.
#[derive(Debug, Clone, FromRow)]
pub(crate) struct PatchUse {
    pub id: i64,
    pub name: String,
    /// Relative to the ROM directory.
    pub path: String,
    pub is_default: bool,
    pub game_id: i64,
    pub game_title: String,
    pub system: String,
    /// Players who chose this patch.
    pub chosen_by: i64,
    /// Players who chose the original for its game, over the default.
    pub original_by: i64,
}

impl PatchUse {
    /// `IPS`, `UPS` or `BPS`, from the file's extension.
    pub(crate) fn format(&self) -> String {
        std::path::Path::new(&self.path)
            .extension()
            .map_or_else(String::new, |e| e.to_string_lossy().to_ascii_uppercase())
    }
}

/// Every patch in the library, by system, game, then the default first: for the admin.
pub(crate) async fn all(state: &AppState) -> ApiResult<Vec<PatchUse>> {
    Ok(sqlx::query_as(
        "SELECT gp.id, gp.name, gp.path, gp.is_default, g.id AS game_id, g.title AS game_title,
                g.system,
                (SELECT COUNT(*) FROM patch_choices c WHERE c.patch_id = gp.id) AS chosen_by,
                (SELECT COUNT(*) FROM patch_choices c
                  WHERE c.game_id = g.id AND c.patch_id IS NULL) AS original_by
         FROM game_patches gp JOIN games g ON g.id = gp.game_id
         ORDER BY g.system, g.title COLLATE NOCASE, gp.is_default DESC, gp.name COLLATE NOCASE",
    )
    .fetch_all(&state.db)
    .await?)
}

/// A game's patches: the default first, then by name.
pub(crate) async fn list(state: &AppState, game_id: i64) -> ApiResult<Vec<Patch>> {
    Ok(sqlx::query_as(
        "SELECT id, name, path, is_default FROM game_patches WHERE game_id = ?
         ORDER BY is_default DESC, name COLLATE NOCASE",
    )
    .bind(game_id)
    .fetch_all(&state.db)
    .await?)
}

/// The patch this user plays the game with, if any.
pub(crate) async fn effective(
    state: &AppState,
    user_id: i64,
    game_id: i64,
) -> ApiResult<Option<Patch>> {
    Ok(sqlx::query_as(
        "SELECT gp.id, gp.name, gp.path, gp.is_default FROM game_patches gp
         LEFT JOIN patch_choices c ON c.game_id = gp.game_id AND c.user_id = ?
         WHERE gp.game_id = ? AND (c.patch_id = gp.id OR (c.user_id IS NULL AND gp.is_default))",
    )
    .bind(user_id)
    .bind(game_id)
    .fetch_optional(&state.db)
    .await?)
}

/// Plays the game with `patch_id` from now on, or the original (`None`).
pub(crate) async fn choose(
    state: &AppState,
    user: &User,
    game_id: i64,
    patch_id: Option<i64>,
) -> ApiResult<()> {
    crate::api::require_game(state, game_id).await?;
    if let Some(id) = patch_id {
        let belongs: Option<i64> =
            sqlx::query_scalar("SELECT id FROM game_patches WHERE id = ? AND game_id = ?")
                .bind(id)
                .bind(game_id)
                .fetch_optional(&state.db)
                .await?;
        if belongs.is_none() {
            return Err(ApiError::not_found("patch"));
        }
    }
    sqlx::query(
        "INSERT INTO patch_choices (user_id, game_id, patch_id) VALUES (?, ?, ?)
         ON CONFLICT(user_id, game_id) DO UPDATE SET patch_id = excluded.patch_id",
    )
    .bind(user.id)
    .bind(game_id)
    .bind(patch_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// A patch's name as it appears in file names (the worker names the patched copy, and so its
/// battery save, the same way).
pub(crate) fn file_label(name: &str) -> String {
    name.replace(['/', '\\'], "-")
}

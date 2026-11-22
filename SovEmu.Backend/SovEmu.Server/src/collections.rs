//! A user's favourite games and named collections of games, and the collections an admin keeps
//! for everyone (shared: no user), some of them declared in the environment
//! (`SOVEMU_COLLECTIONS__…`, kept in step after each scan and locked on the admin page).
//!
//! A user's and the shared collections are one table and one set of functions, told apart by
//! [`Owner`]: a user reads the shared ones but changes only their own.

use std::collections::HashSet;

use serde::Serialize;

use crate::AppState;
use crate::api::{GAME_COLUMNS, Game};
use crate::auth::User;
use crate::error::{ApiError, ApiResult};
use crate::structured::CollectionSpec;

/// Longest collection name.
const MAX_NAME: usize = 64;
/// Collections a user may keep.
const MAX_COLLECTIONS: i64 = 100;

/// A named list of games.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub(crate) struct Collection {
    pub id: i64,
    pub name: String,
    pub game_count: i64,
    /// Kept by an admin for everyone.
    pub shared: bool,
    /// Declared in the environment: changed only there.
    pub declared: bool,
}

/// Whose collections: a user's, or the server's (shared with everyone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    User(i64),
    Server,
}

impl Owner {
    /// The `user_id` it binds, matched with `IS` (NULL for the server's).
    fn user_id(self) -> Option<i64> {
        match self {
            Self::User(id) => Some(id),
            Self::Server => None,
        }
    }

    fn already(self, name: &str) -> ApiError {
        ApiError::conflict(match self {
            Self::User(_) => format!("you already have a collection called {name}"),
            Self::Server => format!("there is already a shared collection called {name}"),
        })
    }
}

/// A user's favourites: read for every list of games.
pub(crate) const FAVOURITE_IDS: &str = "SELECT game_id FROM favourites WHERE user_id = ?";
/// The user's collections that hold a game: read when its panel opens.
pub(crate) const OF_GAME: &str = "SELECT c.id FROM collections c
     JOIN collection_games cg ON cg.collection_id = c.id
     WHERE c.user_id IS ? AND cg.game_id = ? ORDER BY c.name COLLATE NOCASE";
/// A collection's games, most recently added first (binds: the user's id, the collection's).
pub(crate) fn collection_games_sql() -> String {
    format!(
        "SELECT {GAME_COLUMNS} FROM collection_games cg
         JOIN games g ON g.id = cg.game_id
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE cg.collection_id = ? ORDER BY cg.added_at DESC, g.id DESC"
    )
}

/// Marks the user's favourites among `games`.
pub(crate) async fn mark_favourites(
    state: &AppState,
    user_id: i64,
    games: &mut [Game],
) -> ApiResult<()> {
    if games.is_empty() {
        return Ok(());
    }
    let ids: HashSet<i64> = sqlx::query_scalar(FAVOURITE_IDS)
        .bind(user_id)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect();
    for g in games {
        g.favourite = ids.contains(&g.id);
    }
    Ok(())
}

/// Adds the game to the user's favourites, or takes it out.
pub(crate) async fn set_favourite(
    state: &AppState,
    user: &User,
    game_id: i64,
    favourite: bool,
) -> ApiResult<()> {
    crate::api::require_game(state, game_id).await?;
    let sql = if favourite {
        "INSERT OR IGNORE INTO favourites (user_id, game_id) VALUES (?, ?)"
    } else {
        "DELETE FROM favourites WHERE user_id = ? AND game_id = ?"
    };
    sqlx::query(sql).bind(user.id).bind(game_id).execute(&state.db).await?;
    Ok(())
}

/// A user's or the shared collections, by name.
pub(crate) async fn list(state: &AppState, owner: Owner) -> ApiResult<Vec<Collection>> {
    Ok(sqlx::query_as::<_, Collection>(LIST).bind(owner.user_id()).fetch_all(&state.db).await?)
}

/// The collections of one owner (bound with `IS`: NULL for the shared ones), by name.
pub(crate) const LIST: &str = "SELECT c.id, c.name, COUNT(cg.game_id) AS game_count,
         c.user_id IS NULL AS shared, c.declared
     FROM collections c LEFT JOIN collection_games cg ON cg.collection_id = c.id
     WHERE c.user_id IS ? GROUP BY c.id ORDER BY c.name COLLATE NOCASE";

/// A collection's games, most recently added first: one of the user's own, or a shared one.
pub(crate) async fn games(
    state: &AppState,
    user: &User,
    collection_id: i64,
) -> ApiResult<Vec<Game>> {
    let shared = owned(state, Owner::Server, collection_id).await.is_ok();
    if !shared {
        owned(state, Owner::User(user.id), collection_id).await?;
    }
    let mut games = sqlx::query_as::<_, Game>(&collection_games_sql())
        .bind(user.id)
        .bind(collection_id)
        .fetch_all(&state.db)
        .await?;
    // A hidden system's games are not offered, in a collection as anywhere.
    games.retain(|g| crate::api::enabled(state, &g.system));
    crate::api::annotate(state, user.id, &mut games).await?;
    Ok(games.into_iter().map(Game::with_box_art).collect())
}

/// The ids of the owner's collections that hold `game_id`.
pub(crate) async fn of_game(state: &AppState, owner: Owner, game_id: i64) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar(OF_GAME).bind(owner.user_id()).bind(game_id).fetch_all(&state.db).await?)
}

/// Creates a collection; a name the owner already has is refused.
pub(crate) async fn create(state: &AppState, owner: Owner, name: &str) -> ApiResult<Collection> {
    let name = valid_name(name)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collections WHERE user_id IS ?")
        .bind(owner.user_id())
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_COLLECTIONS {
        return Err(ApiError::bad_request(format!("up to {MAX_COLLECTIONS} collections")));
    }
    let id = sqlx::query("INSERT INTO collections (user_id, name) VALUES (?, ?)")
        .bind(owner.user_id())
        .bind(&name)
        .execute(&state.db)
        .await
        .map_err(|e| unique(e, owner, &name))?
        .last_insert_rowid();
    Ok(Collection { id, name, game_count: 0, shared: owner == Owner::Server, declared: false })
}

pub(crate) async fn rename(
    state: &AppState,
    owner: Owner,
    collection_id: i64,
    name: &str,
) -> ApiResult<()> {
    changeable(state, owner, collection_id).await?;
    let name = valid_name(name)?;
    sqlx::query("UPDATE collections SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(collection_id)
        .execute(&state.db)
        .await
        .map_err(|e| unique(e, owner, &name))?;
    Ok(())
}

pub(crate) async fn delete(state: &AppState, owner: Owner, collection_id: i64) -> ApiResult<()> {
    changeable(state, owner, collection_id).await?;
    sqlx::query("DELETE FROM collections WHERE id = ?")
        .bind(collection_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Puts the game in the collection, or takes it out.
pub(crate) async fn set_member(
    state: &AppState,
    owner: Owner,
    collection_id: i64,
    game_id: i64,
    member: bool,
) -> ApiResult<()> {
    changeable(state, owner, collection_id).await?;
    crate::api::require_game(state, game_id).await?;
    let sql = if member {
        "INSERT OR IGNORE INTO collection_games (collection_id, game_id) VALUES (?, ?)"
    } else {
        "DELETE FROM collection_games WHERE collection_id = ? AND game_id = ?"
    };
    sqlx::query(sql).bind(collection_id).bind(game_id).execute(&state.db).await?;
    Ok(())
}

/// What [`declare`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) collections: usize,
    /// The games named that the library does not have, as written.
    pub(crate) missing: Vec<String>,
}

/// Makes the shared collections the environment declares match it: each by name (taking over a
/// shared one the admin made by that name), holding the games it names that the library has, in
/// order; a collection declared before and no longer is deleted. Run after each scan, as games
/// come and go.
pub(crate) async fn declare(
    db: &sqlx::SqlitePool,
    specs: &[CollectionSpec],
) -> sqlx::Result<Declared> {
    let mut tx = db.begin().await?;
    let mut out = Declared { collections: specs.len(), missing: Vec::new() };
    let mut kept = Vec::with_capacity(specs.len());
    for spec in specs {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO collections (user_id, name, declared) VALUES (NULL, ?, 1)
             ON CONFLICT (name) WHERE user_id IS NULL DO UPDATE SET declared = 1
             RETURNING id",
        )
        .bind(&spec.name)
        .fetch_one(&mut *tx)
        .await?;
        kept.push(id);
        sqlx::query("DELETE FROM collection_games WHERE collection_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        // In order: the first named is the newest added, as a collection lists them.
        for (n, (system, file)) in spec.games.iter().enumerate() {
            let game: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM games WHERE system = ? AND path LIKE ? ESCAPE '\\'
                 ORDER BY id LIMIT 1",
            )
            .bind(system)
            .bind(format!("%/{}", like_escape(file)))
            .fetch_optional(&mut *tx)
            .await?;
            let Some(game) = game else {
                out.missing.push(format!("{system}/{file}"));
                continue;
            };
            sqlx::query(
                "INSERT OR IGNORE INTO collection_games (collection_id, game_id, added_at)
                 VALUES (?, ?, datetime('now', ?))",
            )
            .bind(id)
            .bind(game)
            .bind(format!("-{n} seconds"))
            .execute(&mut *tx)
            .await?;
        }
    }
    let gone: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM collections WHERE user_id IS NULL AND declared = 1")
            .fetch_all(&mut *tx)
            .await?;
    for id in gone.into_iter().filter(|id| !kept.contains(id)) {
        sqlx::query("DELETE FROM collections WHERE id = ?").bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(out)
}

/// `text` matched literally in a `LIKE … ESCAPE '\'`.
fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn valid_name(name: &str) -> ApiResult<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(ApiError::bad_request(format!(
            "a collection's name has 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name)
}

fn unique(e: sqlx::Error, owner: Owner, name: &str) -> ApiError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => owner.already(name),
        e => e.into(),
    }
}

/// Whether the owner's collection was declared in the environment. Another owner's collection
/// is as good as missing.
async fn owned(state: &AppState, owner: Owner, collection_id: i64) -> ApiResult<bool> {
    let declared: Option<bool> =
        sqlx::query_scalar("SELECT declared FROM collections WHERE id = ? AND user_id IS ?")
            .bind(collection_id)
            .bind(owner.user_id())
            .fetch_optional(&state.db)
            .await?;
    declared.ok_or_else(|| ApiError::not_found("collection"))
}

/// The owner's collection, and not one the environment declares.
async fn changeable(state: &AppState, owner: Owner, collection_id: i64) -> ApiResult<()> {
    if owned(state, owner, collection_id).await? {
        return Err(ApiError::conflict(
            "this collection is set in the environment (SOVEMU_COLLECTIONS__…): change it there",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// Migration 0026 rebuilds both tables: every collection and its games come through, with
    /// foreign keys on as the server runs.
    #[tokio::test]
    async fn the_shared_collections_migration_keeps_every_users_collections() {
        let options = SqliteConnectOptions::from_str("sqlite::memory:").unwrap().foreign_keys(true);
        let db = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
        let mut before = sqlx::migrate!("./migrations");
        before.migrations =
            before.migrations.iter().filter(|m| m.version < 26).cloned().collect::<Vec<_>>().into();
        before.run(&db).await.unwrap();
        for sql in [
            "INSERT INTO users (id, username, password_hash) VALUES (1, 'kid', 'x')",
            "INSERT INTO games (id, system, path, title, tags, size, modified)
             VALUES (7, 'nes', 'nes/A.nes', 'A', '', 1, 0)",
            "INSERT INTO collections (id, user_id, name) VALUES (3, 1, 'Mine')",
            "INSERT INTO collection_games (collection_id, game_id) VALUES (3, 7)",
        ] {
            sqlx::query(sql).execute(&db).await.unwrap();
        }

        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        let kept: (i64, Option<i64>, String, bool, i64) = sqlx::query_as(
            "SELECT c.id, c.user_id, c.name, c.declared, cg.game_id
             FROM collections c JOIN collection_games cg ON cg.collection_id = c.id",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(kept, (3, Some(1), "Mine".into(), false, 7));
        // And the foreign keys still hold: removing the game empties the collection.
        sqlx::query("DELETE FROM games WHERE id = 7").execute(&db).await.unwrap();
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collection_games")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(left, 0);
    }
}

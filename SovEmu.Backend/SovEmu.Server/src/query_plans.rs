//! The queries every request or page runs must use an index: this asks SQLite how it would run
//! each (`EXPLAIN QUERY PLAN`, on the migrated schema) and fails on a plan that reads a whole
//! table. A new query on a hot path joins the list, with its index in the same migration.
//!
//! What counts as reading a whole table: a bare `SCAN <table>`, or a scan that is then sorted
//! (`USE TEMP B-TREE FOR ORDER BY`: every row is read before the first is returned). A scan of
//! an index in the order asked for is fine: with a `LIMIT` it stops early.
//!
//! Left out on purpose, each reading every game by design: Home's featured row (a daily
//! shuffle), search (matched in Rust, `search.rs`), and the release labels (worked out after
//! scans and lookups, not per request, `releases.rs`).

use sqlx::Row;

use crate::{api, auth, collections, home};

/// The hot queries, by what they serve.
fn hot_queries() -> Vec<(&'static str, String)> {
    vec![
        ("every signed-in request: the session's user", auth::SESSION_USER.to_owned()),
        ("a system's games", api::games_sql(api::LIST_GAMES)),
        ("a game's panel", api::games_sql(api::ONE_GAME)),
        ("Home: continue playing", home::row_sql(home::RECENT, "")),
        ("Home: recently added", home::row_sql(home::ADDED, "")),
        ("Home: favourites", home::row_sql(home::FAVOURITES, "")),
        ("Home: a collection's row", collections::collection_games_sql()),
        ("every list: the user's favourites", collections::FAVOURITE_IDS.to_owned()),
        ("a game's panel: its collections", collections::OF_GAME.to_owned()),
    ]
}

/// The problems with a plan's steps, if any.
fn whole_table_reads(plan: &[String]) -> Vec<String> {
    let scans: Vec<&String> = plan.iter().filter(|step| step.starts_with("SCAN ")).collect();
    let sorted = plan.iter().any(|step| step.contains("USE TEMP B-TREE FOR ORDER BY"));
    let mut problems: Vec<String> = scans
        .iter()
        .filter(|step| !step.contains(" USING "))
        .map(|step| format!("reads a whole table: {step}"))
        .collect();
    if sorted {
        problems.extend(scans.iter().map(|step| format!("reads every row to sort them: {step}")));
    }
    problems
}

#[tokio::test]
async fn hot_queries_use_indexes() {
    let dir = std::env::temp_dir().join(format!("sovemu-plans-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = crate::db::open(&dir.join("plans.db")).await.unwrap();
    let mut failures = Vec::new();
    for (name, sql) in hot_queries() {
        let explain = format!("EXPLAIN QUERY PLAN {sql}");
        let mut q = sqlx::query(&explain);
        for _ in 0..sql.matches('?').count() {
            q = q.bind(None::<i64>);
        }
        let plan: Vec<String> = q
            .fetch_all(&db)
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect();
        for problem in whole_table_reads(&plan) {
            failures.push(format!("{name}: {problem}\n  plan: {plan:?}"));
        }
    }
    db.close().await;
    std::fs::remove_dir_all(&dir).ok();
    assert!(failures.is_empty(), "hot queries without an index:\n{}", failures.join("\n"));
}

#[test]
fn tells_a_whole_table_read_from_an_index_read() {
    let plan = |steps: &[&str]| steps.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    // Before `games_added`: the whole table read through another index, then sorted.
    assert_eq!(
        whole_table_reads(&plan(&[
            "SCAN g USING INDEX sqlite_autoindex_games_1",
            "USE TEMP B-TREE FOR ORDER BY"
        ]))
        .len(),
        1
    );
    assert_eq!(whole_table_reads(&plan(&["SCAN games"])).len(), 1);
    // After: the index read in order, stopping at the limit.
    assert!(whole_table_reads(&plan(&["SCAN g USING COVERING INDEX games_added"])).is_empty());
    // A lookup, and sorting the few rows it found.
    assert!(
        whole_table_reads(&plan(&[
            "SEARCH h USING INDEX sqlite_autoindex_play_history_1 (user_id=?)",
            "USE TEMP B-TREE FOR ORDER BY"
        ]))
        .is_empty()
    );
}

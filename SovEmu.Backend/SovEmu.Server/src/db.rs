use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Opens (creating if needed) the SQLite database in WAL mode, applies migrations, and brings
/// the stored release labels up to date (a new column starts empty).
pub(crate) async fn open(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    crate::releases::refresh(&pool).await?;
    Ok(pool)
}

/// A transaction that writes. It takes the write lock as it begins (`BEGIN IMMEDIATE`), waiting
/// up to the busy timeout for it. A plain `BEGIN` takes it only at the first write, and if
/// another connection committed since this one first read, SQLite cannot upgrade the stale
/// snapshot and fails at once (`SQLITE_BUSY_SNAPSHOT`, "database is locked", code 517),
/// whatever the timeout: a rescan while a lookup writes, say.
/// Runs a write without waiting for it, for an observer: it is told in code that cannot wait,
/// and must not slow or fail what it observes. A write that fails is logged as `what`. Outside a
/// runtime (a synchronous test) there is nothing to write with, so nothing is written.
pub(crate) fn write_in_background(
    what: &'static str,
    write: impl std::future::Future<Output = sqlx::Result<()>> + Send + 'static,
) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    runtime.spawn(async move {
        if let Err(e) = write.await {
            tracing::warn!(error = %e, what, "a background write failed");
        }
    });
}

pub(crate) async fn write_tx(pool: &SqlitePool) -> sqlx::Result<Transaction<'static, Sqlite>> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read-then-write transaction while another connection commits: `BEGIN` fails as it
    /// writes; `write_tx` waits its turn and succeeds.
    #[tokio::test]
    async fn a_write_transaction_does_not_fail_when_another_wrote_first() {
        let dir = std::env::temp_dir().join(format!("sovemu-db-tx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pool = open(&dir.join("t.db")).await.unwrap();
        sqlx::query("CREATE TABLE t (n INTEGER)").execute(&pool).await.unwrap();

        let race = |immediate: bool| {
            let pool = pool.clone();
            async move {
                let mut tx = if immediate { write_tx(&pool).await? } else { pool.begin().await? };
                // Read first (a deferred transaction now holds a snapshot) …
                let _: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM t").fetch_one(&mut *tx).await?;
                // … another connection commits meanwhile (it waits for us with IMMEDIATE) …
                let other = {
                    let pool = pool.clone();
                    tokio::spawn(async move {
                        sqlx::query("INSERT INTO t VALUES (1)").execute(&pool).await
                    })
                };
                tokio::time::sleep(Duration::from_millis(100)).await;
                // … then write.
                sqlx::query("INSERT INTO t VALUES (2)").execute(&mut *tx).await?;
                tx.commit().await?;
                other.await.expect("joins")?;
                sqlx::Result::Ok(())
            }
        };
        let deferred = race(false).await;
        assert!(
            deferred.as_ref().is_err_and(|e: &sqlx::Error| e.to_string().contains("locked")),
            "a plain BEGIN fails: {deferred:?}"
        );
        race(true).await.expect("BEGIN IMMEDIATE succeeds");
        pool.close().await;
        std::fs::remove_dir_all(dir).ok();
    }
}

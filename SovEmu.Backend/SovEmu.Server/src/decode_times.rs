//! What each device's video decoder has measured (`device_decode_times`): the mean decode time
//! of its latest session with each codec, kept as its stats come in, and read when it next
//! offers, so a codec is priced for it by what it measured over what it declares (`codec.rs`).

use sqlx::SqlitePool;

/// Keeps what sessions measure. Told by `SovEmu.Sessions` as clients report.
#[derive(Debug, Clone)]
pub(crate) struct DecodeTimes {
    db: SqlitePool,
}

impl DecodeTimes {
    pub(crate) fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl sovemu_sessions::Observer for DecodeTimes {
    fn decode_measured(&self, measured: &sovemu_sessions::Decoded<'_>) {
        let (device, codec) = (measured.device.to_owned(), measured.codec.to_owned());
        let (mean_ms, samples) = (measured.mean_ms, i64::from(measured.samples));
        let db = self.db.clone();
        crate::db::write_in_background("decode time", async move {
            keep(&db, &device, &codec, mean_ms, samples).await
        });
    }
}

/// Keeps a device's mean decode time for a codec, replacing its last, and forgets the times of
/// sign-ins that have ended.
pub(crate) async fn keep(
    db: &SqlitePool,
    device: &str,
    codec: &str,
    mean_ms: f64,
    samples: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO device_decode_times (family_id, codec, decode_ms, samples) VALUES (?, ?, ?, ?)
         ON CONFLICT (family_id, codec) DO UPDATE SET decode_ms = excluded.decode_ms,
           samples = excluded.samples, measured_at = datetime('now')",
    )
    .bind(device)
    .bind(codec)
    .bind(mean_ms)
    .bind(samples)
    .execute(db)
    .await?;
    sqlx::query(
        "DELETE FROM device_decode_times WHERE family_id NOT IN
           (SELECT family_id FROM oauth_refresh_tokens)",
    )
    .execute(db)
    .await?;
    Ok(())
}

/// What a device has measured, by codec name, in ms a frame.
pub(crate) async fn of(db: &SqlitePool, device: &str) -> sqlx::Result<Vec<(String, f64)>> {
    sqlx::query_as("SELECT codec, decode_ms FROM device_decode_times WHERE family_id = ?")
        .bind(device)
        .fetch_all(db)
        .await
}

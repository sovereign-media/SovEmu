//! oauth-as's [`Storage`] over SQLite: the tables are in the server's migration
//! `0016_oidc.sql`, one per kind of record, each record kept whole as JSON beside the columns it
//! is looked up, cascaded and swept by.
//!
//! Atomicity comes from the database, never from a lock in this process: a take is one
//! `DELETE … RETURNING`, a state compare-and-swap one conditional `UPDATE`, and everything that
//! reads before it writes (a barrier check, a whole-record comparison, a cascade) runs in a
//! `BEGIN IMMEDIATE` transaction, which takes SQLite's write lock before its first read, so no
//! other writer can come between the read and the write.
//!
//! Access tokens, refresh tokens and authorization codes are keyed by the SHA-256 of the
//! credential, and the credential is left out of the stored JSON: the caller hands it back on
//! every lookup, and it is put back into the record read.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oauth_as::authorization::{AuthorizationCodeRecord, AuthorizationCodeState};
use oauth_as::client::{Client, ClientId};
use oauth_as::device::{DeviceGrant, DeviceGrantState, normalize_user_code};
use oauth_as::store::{RevocationWindow, Storage, StorageError, WriteOutcome};
use oauth_as::token::{IssuedToken, RefreshTokenRecord};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};

/// A live sign-in, as [`SqliteStorage::sign_ins`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredSignIn {
    pub(crate) family_id: String,
    pub(crate) client_id: String,
    pub(crate) subject: Option<String>,
    pub(crate) signed_in_at: SystemTime,
    pub(crate) last_used_at: SystemTime,
    pub(crate) expires_at: Option<SystemTime>,
}

/// The JSON member each credential-keyed record carries its own credential in.
const ACCESS_KEY: &str = "access_token";
const REFRESH_KEY: &str = "refresh_token";
const CODE_KEY: &str = "code";

/// Whether a barrier stands over a write for this client, family and subject, whose grant was
/// established at `?4` (nanoseconds). The same predicate as `MemoryStorage::is_revoked`, plus
/// SovEmu's `subject` scope ([`SqliteStorage::revoke_subject`]):
///
/// - `client` refuses a grant established at or before the deletion, or any grant while the
///   client is still gone (a deletion no re-provisioning followed);
/// - `consent` and `subject` refuse a grant established at or before the revocation, so signing
///   in again afterwards is served;
/// - `family` refuses unconditionally.
///
/// A NULL family or subject matches nothing.
const IS_REVOKED: &str = "SELECT EXISTS (SELECT 1 FROM oauth_revocation_barriers WHERE
       (kind = 'client' AND scope = ?1 AND subject = ''
            AND (?4 <= recorded_at
                 OR NOT EXISTS (SELECT 1 FROM oauth_clients WHERE client_id = ?1)))
    OR (kind = 'consent' AND scope = ?1 AND subject = ?3 AND ?4 <= recorded_at)
    OR (kind = 'subject' AND scope = ?3 AND subject = '' AND ?4 <= recorded_at)
    OR (kind = 'family' AND scope = ?2 AND subject = ''))";

/// Records a barrier, or folds a repeat revocation of the same scope into the one recorded:
/// both instants only ever move forward (`BarrierTimes::merge` in oauth-as).
const RECORD_BARRIER: &str =
    "INSERT INTO oauth_revocation_barriers (kind, scope, subject, recorded_at, until)
     VALUES (?, ?, ?, ?, ?)
     ON CONFLICT (kind, scope, subject) DO UPDATE SET
         recorded_at = max(recorded_at, excluded.recorded_at),
         until = max(until, excluded.until)";

const UPSERT_ACCESS: &str =
    "INSERT INTO oauth_access_tokens (token_hash, client_id, subject, family_id, expires_at, record)
     VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT (token_hash) DO UPDATE SET client_id = excluded.client_id,
         subject = excluded.subject, family_id = excluded.family_id,
         expires_at = excluded.expires_at, record = excluded.record";

const UPSERT_REFRESH: &str =
    "INSERT INTO oauth_refresh_tokens (token_hash, client_id, subject, family_id, expires_at, record)
     VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT (token_hash) DO UPDATE SET client_id = excluded.client_id,
         subject = excluded.subject, family_id = excluded.family_id,
         expires_at = excluded.expires_at, record = excluded.record";

/// oauth-as's storage in SovEmu's SQLite database. Cheap to clone (it is the pool), so several
/// authorization servers can share one.
#[derive(Debug, Clone)]
pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// A store over `pool`, which must have had the server's migrations applied.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Every client registration, read back as oauth-as wrote it.
    pub(crate) async fn clients(&self) -> Result<Vec<Client>, StorageError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT record FROM oauth_clients")
            .fetch_all(&self.pool)
            .await
            .map_err(db("clients"))?;
        rows.iter().map(|json| decode(json, None)).collect()
    }

    /// Every sign-in still live, or one user's (`subject`): a refresh family with an active
    /// token, its client, when its grant was made, when it last had an access token, and when it
    /// lapses. Read back into oauth-as's own records (the credential, left out of the stored
    /// JSON, as an empty placeholder), so no column of theirs is parsed here.
    pub(crate) async fn sign_ins(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<StoredSignIn>, StorageError> {
        let op = "sign_ins";
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT record FROM oauth_refresh_tokens WHERE ?1 IS NULL OR subject = ?1",
        )
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(db(op))?;
        let mut out = Vec::new();
        for json in rows {
            let record: RefreshTokenRecord = decode(&json, Some((REFRESH_KEY, "")))?;
            if record.state != oauth_as::token::RefreshTokenState::Active {
                continue;
            }
            let issued: Vec<String> =
                sqlx::query_scalar("SELECT record FROM oauth_access_tokens WHERE family_id = ?")
                    .bind(&record.family_id)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(db(op))?;
            let mut last_used_at = record.grant_established_at;
            for json in issued {
                let token: IssuedToken = decode(&json, Some((ACCESS_KEY, "")))?;
                last_used_at = last_used_at.max(token.issued_at);
            }
            out.push(StoredSignIn {
                family_id: record.family_id,
                client_id: record.client_id.as_str().to_owned(),
                subject: record.subject,
                signed_in_at: record.grant_established_at,
                last_used_at,
                expires_at: record.expires_at,
            });
        }
        out.sort_by_key(|s| std::cmp::Reverse(s.last_used_at));
        Ok(out)
    }

    /// Ends one sign-in (a refresh family) through oauth-as's family revocation: its tokens go,
    /// and a barrier over `window` refuses a rotation already in flight. Whether there was one.
    pub(crate) async fn end_sign_in(
        &self,
        family_id: &str,
        window: RevocationWindow,
    ) -> Result<bool, StorageError> {
        Ok(self.revoke_token_family(family_id, window).await? > 0)
    }

    /// Ends every sign-in of one user (`subject`, the user id as oauth-as records it): deletes
    /// their access and refresh tokens, authorization codes and approved device grants, and
    /// records a `subject` barrier over `window` in the same transaction, so a token issuance
    /// already in flight for them is refused when it writes. A grant established after
    /// `window.recorded_at` (signing in again) is served. Returns how many records went.
    pub(crate) async fn revoke_subject(
        &self,
        subject: &str,
        window: RevocationWindow,
    ) -> Result<u64, StorageError> {
        reject_empty("subject", subject)?;
        let op = "revoke_subject";
        let mut tx = self.immediate(op).await?;
        record_barrier(&mut tx, "subject", subject, "", window, op).await?;
        let mut removed = 0;
        for table in [
            "oauth_access_tokens",
            "oauth_refresh_tokens",
            "oauth_authorization_codes",
            "oauth_device_grants",
        ] {
            removed += sqlx::query(&format!("DELETE FROM {table} WHERE subject = ?"))
                .bind(subject)
                .execute(&mut *tx)
                .await
                .map_err(db(op))?
                .rows_affected();
        }
        tx.commit().await.map_err(db(op))?;
        Ok(removed)
    }

    /// A transaction holding SQLite's write lock from its first statement, so that what it reads
    /// cannot change before it writes.
    async fn immediate(
        &self,
        op: &'static str,
    ) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, StorageError> {
        self.pool.begin_with("BEGIN IMMEDIATE").await.map_err(db(op))
    }
}

impl Storage for SqliteStorage {
    async fn get_client(&self, client_id: &ClientId) -> Result<Option<Arc<Client>>, StorageError> {
        let row: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_clients WHERE client_id = ?")
                .bind(client_id.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(db("get_client"))?;
        row.map(|json| decode(&json, None).map(Arc::new)).transpose()
    }

    async fn put_client(&self, client: Client) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO oauth_clients (client_id, record) VALUES (?, ?)
             ON CONFLICT (client_id) DO UPDATE SET record = excluded.record",
        )
        .bind(client.client_id.as_str())
        .bind(encode(&client, None)?)
        .execute(&self.pool)
        .await
        .map_err(db("put_client"))?;
        Ok(())
    }

    async fn compare_and_swap_client(
        &self,
        expected: &Client,
        updated: Client,
    ) -> Result<bool, StorageError> {
        let op = "compare_and_swap_client";
        let mut tx = self.immediate(op).await?;
        let current: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_clients WHERE client_id = ?")
                .bind(updated.client_id.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(db(op))?;
        // Absent (deleted) or changed: refuse, and never insert.
        match current.map(|json| decode::<Client>(&json, None)).transpose()? {
            Some(current) if current == *expected => {}
            _ => return Ok(false),
        }
        sqlx::query("UPDATE oauth_clients SET record = ? WHERE client_id = ?")
            .bind(encode(&updated, None)?)
            .bind(updated.client_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(db(op))?;
        tx.commit().await.map_err(db(op))?;
        Ok(true)
    }

    async fn delete_client(
        &self,
        client_id: &ClientId,
        window: RevocationWindow,
    ) -> Result<bool, StorageError> {
        reject_empty("client_id", client_id.as_str())?;
        let op = "delete_client";
        let mut tx = self.immediate(op).await?;
        let existed = sqlx::query("DELETE FROM oauth_clients WHERE client_id = ?")
            .bind(client_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(db(op))?
            .rows_affected()
            > 0;
        // Recorded whether or not the registration was still there: an issuance in flight for
        // a client deleted twice still has to be refused.
        record_barrier(&mut tx, "client", client_id.as_str(), "", window, op).await?;
        for table in [
            "oauth_access_tokens",
            "oauth_refresh_tokens",
            "oauth_authorization_codes",
            "oauth_device_grants",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE client_id = ?"))
                .bind(client_id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(db(op))?;
        }
        tx.commit().await.map_err(db(op))?;
        Ok(existed)
    }

    async fn put_device_grant(&self, grant: DeviceGrant) -> Result<(), StorageError> {
        // One upsert keyed by `device_code`. `user_code` is a UNIQUE column, so a changed code
        // retires the old one with the row, and a code another device holds aborts the statement
        // before it writes anything.
        sqlx::query(
            "INSERT INTO oauth_device_grants
                 (device_code, user_code, client_id, subject, state, expires_at, record)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (device_code) DO UPDATE SET user_code = excluded.user_code,
                 client_id = excluded.client_id, subject = excluded.subject,
                 state = excluded.state, expires_at = excluded.expires_at,
                 record = excluded.record",
        )
        .bind(&grant.device_code)
        .bind(normalize_user_code(&grant.user_code))
        .bind(grant.client_id.as_str())
        .bind(approved_subject(&grant.state))
        .bind(encode(&grant.state, None)?)
        .bind(nanos(grant.expires_at))
        .bind(encode(&grant, None)?)
        .execute(&self.pool)
        .await
        .map_err(device_write("put_device_grant"))?;
        Ok(())
    }

    async fn get_device_grant(
        &self,
        device_code: &str,
    ) -> Result<Option<DeviceGrant>, StorageError> {
        let row: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_device_grants WHERE device_code = ?")
                .bind(device_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(db("get_device_grant"))?;
        row.map(|json| decode(&json, None)).transpose()
    }

    async fn find_device_grant_by_user_code(
        &self,
        normalized_user_code: &str,
    ) -> Result<Option<DeviceGrant>, StorageError> {
        // The exact key: the store normalizes what it is given, never the query.
        let row: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_device_grants WHERE user_code = ?")
                .bind(normalized_user_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(db("find_device_grant_by_user_code"))?;
        row.map(|json| decode(&json, None)).transpose()
    }

    async fn take_device_grant(
        &self,
        device_code: &str,
    ) -> Result<Option<DeviceGrant>, StorageError> {
        let row: Option<String> = sqlx::query_scalar(
            "DELETE FROM oauth_device_grants WHERE device_code = ? RETURNING record",
        )
        .bind(device_code)
        .fetch_optional(&self.pool)
        .await
        .map_err(db("take_device_grant"))?;
        row.map(|json| decode(&json, None)).transpose()
    }

    async fn compare_and_swap_device_grant(
        &self,
        expected: &DeviceGrantState,
        updated: DeviceGrant,
    ) -> Result<bool, StorageError> {
        // One conditional UPDATE: it cannot create a row, so a redeemed grant stays gone, and the
        // UNIQUE user code refuses a code another device holds exactly as the put does.
        let changed = sqlx::query(
            "UPDATE oauth_device_grants SET user_code = ?, client_id = ?, subject = ?, state = ?,
                 expires_at = ?, record = ?
             WHERE device_code = ? AND state = ?",
        )
        .bind(normalize_user_code(&updated.user_code))
        .bind(updated.client_id.as_str())
        .bind(approved_subject(&updated.state))
        .bind(encode(&updated.state, None)?)
        .bind(nanos(updated.expires_at))
        .bind(encode(&updated, None)?)
        .bind(&updated.device_code)
        .bind(encode(expected, None)?)
        .execute(&self.pool)
        .await
        .map_err(device_write("compare_and_swap_device_grant"))?
        .rows_affected();
        Ok(changed == 1)
    }

    async fn put_authorization_code(
        &self,
        record: AuthorizationCodeRecord,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO oauth_authorization_codes
                 (code_hash, client_id, subject, state_digest, expires_at, record)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (code_hash) DO UPDATE SET client_id = excluded.client_id,
                 subject = excluded.subject, state_digest = excluded.state_digest,
                 expires_at = excluded.expires_at, record = excluded.record",
        )
        .bind(digest(&record.code))
        .bind(record.client_id.as_str())
        .bind(&record.subject)
        .bind(state_digest(&record.state)?)
        .bind(nanos(record.expires_at))
        .bind(encode(&record, Some(CODE_KEY))?)
        .execute(&self.pool)
        .await
        .map_err(db("put_authorization_code"))?;
        Ok(())
    }

    async fn compare_and_swap_authorization_code(
        &self,
        expected: &AuthorizationCodeState,
        updated: AuthorizationCodeRecord,
    ) -> Result<bool, StorageError> {
        let changed = sqlx::query(
            "UPDATE oauth_authorization_codes SET client_id = ?, subject = ?, state_digest = ?,
                 expires_at = ?, record = ?
             WHERE code_hash = ? AND state_digest = ?",
        )
        .bind(updated.client_id.as_str())
        .bind(&updated.subject)
        .bind(state_digest(&updated.state)?)
        .bind(nanos(updated.expires_at))
        .bind(encode(&updated, Some(CODE_KEY))?)
        .bind(digest(&updated.code))
        .bind(state_digest(expected)?)
        .execute(&self.pool)
        .await
        .map_err(db("compare_and_swap_authorization_code"))?
        .rows_affected();
        Ok(changed == 1)
    }

    async fn take_authorization_code(
        &self,
        code: &str,
    ) -> Result<Option<AuthorizationCodeRecord>, StorageError> {
        let row: Option<String> = sqlx::query_scalar(
            "DELETE FROM oauth_authorization_codes WHERE code_hash = ? RETURNING record",
        )
        .bind(digest(code))
        .fetch_optional(&self.pool)
        .await
        .map_err(db("take_authorization_code"))?;
        row.map(|json| decode(&json, Some((CODE_KEY, code)))).transpose()
    }

    async fn put_token(&self, token: IssuedToken) -> Result<WriteOutcome, StorageError> {
        let op = "put_token";
        let mut tx = self.immediate(op).await?;
        if is_revoked(
            &mut tx,
            token.client_id.as_str(),
            token.family_id.as_deref(),
            token.subject.as_deref(),
            token.grant_established_at,
            op,
        )
        .await?
        {
            return Ok(WriteOutcome::RefusedRevoked);
        }
        write_access(&mut tx, &token, op).await?;
        tx.commit().await.map_err(db(op))?;
        Ok(WriteOutcome::Applied)
    }

    async fn get_token(
        &self,
        access_token: &str,
    ) -> Result<Option<Arc<IssuedToken>>, StorageError> {
        let row: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_access_tokens WHERE token_hash = ?")
                .bind(digest(access_token))
                .fetch_optional(&self.pool)
                .await
                .map_err(db("get_token"))?;
        row.map(|json| decode(&json, Some((ACCESS_KEY, access_token))).map(Arc::new)).transpose()
    }

    async fn delete_token(&self, access_token: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM oauth_access_tokens WHERE token_hash = ?")
            .bind(digest(access_token))
            .execute(&self.pool)
            .await
            .map_err(db("delete_token"))?;
        Ok(())
    }

    async fn put_refresh_token(
        &self,
        record: RefreshTokenRecord,
    ) -> Result<WriteOutcome, StorageError> {
        let op = "put_refresh_token";
        let mut tx = self.immediate(op).await?;
        if is_revoked(
            &mut tx,
            record.client_id.as_str(),
            Some(&record.family_id),
            record.subject.as_deref(),
            record.grant_established_at,
            op,
        )
        .await?
        {
            return Ok(WriteOutcome::RefusedRevoked);
        }
        write_refresh(&mut tx, &record, op).await?;
        tx.commit().await.map_err(db(op))?;
        Ok(WriteOutcome::Applied)
    }

    async fn get_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<Arc<RefreshTokenRecord>>, StorageError> {
        let row: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_refresh_tokens WHERE token_hash = ?")
                .bind(digest(refresh_token))
                .fetch_optional(&self.pool)
                .await
                .map_err(db("get_refresh_token"))?;
        row.map(|json| decode(&json, Some((REFRESH_KEY, refresh_token))).map(Arc::new)).transpose()
    }

    async fn rotate_refresh_token(
        &self,
        expected: &RefreshTokenRecord,
        spent: &RefreshTokenRecord,
        access: &IssuedToken,
        next: Option<&RefreshTokenRecord>,
    ) -> Result<bool, StorageError> {
        let op = "rotate_refresh_token";
        let mut tx = self.immediate(op).await?;
        let current: Option<String> =
            sqlx::query_scalar("SELECT record FROM oauth_refresh_tokens WHERE token_hash = ?")
                .bind(digest(&expected.refresh_token))
                .fetch_optional(&mut *tx)
                .await
                .map_err(db(op))?;
        let current = current
            .map(|json| {
                decode::<RefreshTokenRecord>(&json, Some((REFRESH_KEY, &expected.refresh_token)))
            })
            .transpose()?;
        if current.as_ref() != Some(expected)
            || is_revoked(
                &mut tx,
                expected.client_id.as_str(),
                Some(&expected.family_id),
                expected.subject.as_deref(),
                expected.grant_established_at,
                op,
            )
            .await?
        {
            return Ok(false);
        }
        write_refresh(&mut tx, spent, op).await?;
        write_access(&mut tx, access, op).await?;
        if let Some(next) = next {
            write_refresh(&mut tx, next, op).await?;
        }
        tx.commit().await.map_err(db(op))?;
        Ok(true)
    }

    async fn take_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<RefreshTokenRecord>, StorageError> {
        let row: Option<String> = sqlx::query_scalar(
            "DELETE FROM oauth_refresh_tokens WHERE token_hash = ? RETURNING record",
        )
        .bind(digest(refresh_token))
        .fetch_optional(&self.pool)
        .await
        .map_err(db("take_refresh_token"))?;
        row.map(|json| decode(&json, Some((REFRESH_KEY, refresh_token)))).transpose()
    }

    async fn revoke_token_family(
        &self,
        family_id: &str,
        window: RevocationWindow,
    ) -> Result<u64, StorageError> {
        reject_empty("family_id", family_id)?;
        let op = "revoke_token_family";
        let mut tx = self.immediate(op).await?;
        // The barrier first, in the same transaction: a rotation that has already taken its
        // refresh record (so the deletes below cannot see it) is refused when it writes back.
        record_barrier(&mut tx, "family", family_id, "", window, op).await?;
        let mut removed = 0;
        for table in ["oauth_access_tokens", "oauth_refresh_tokens"] {
            removed += sqlx::query(&format!("DELETE FROM {table} WHERE family_id = ?"))
                .bind(family_id)
                .execute(&mut *tx)
                .await
                .map_err(db(op))?
                .rows_affected();
        }
        tx.commit().await.map_err(db(op))?;
        Ok(removed)
    }

    async fn sweep_expired(&self, now: SystemTime) -> Result<u64, StorageError> {
        let op = "sweep_expired";
        let now = nanos(now);
        let mut tx = self.immediate(op).await?;
        let mut removed = 0;
        // Dead at `now` means at or before it; a refresh chain with no absolute lifetime (NULL)
        // never is. A barrier is counted like a record.
        for (table, column) in [
            ("oauth_device_grants", "expires_at"),
            ("oauth_authorization_codes", "expires_at"),
            ("oauth_access_tokens", "expires_at"),
            ("oauth_refresh_tokens", "expires_at"),
            ("oauth_revocation_barriers", "until"),
        ] {
            removed += sqlx::query(&format!("DELETE FROM {table} WHERE {column} <= ?"))
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(db(op))?
                .rows_affected();
        }
        tx.commit().await.map_err(db(op))?;
        Ok(removed)
    }
}

/// See [`IS_REVOKED`]. Runs inside the caller's write transaction, so no revocation can land
/// between this answer and the write it guards.
async fn is_revoked(
    conn: &mut SqliteConnection,
    client_id: &str,
    family_id: Option<&str>,
    subject: Option<&str>,
    grant_established_at: SystemTime,
    op: &'static str,
) -> Result<bool, StorageError> {
    sqlx::query_scalar(IS_REVOKED)
        .bind(client_id)
        .bind(family_id)
        .bind(subject)
        .bind(nanos(grant_established_at))
        .fetch_one(conn)
        .await
        .map_err(db(op))
}

async fn record_barrier(
    conn: &mut SqliteConnection,
    kind: &str,
    scope: &str,
    subject: &str,
    window: RevocationWindow,
    op: &'static str,
) -> Result<(), StorageError> {
    sqlx::query(RECORD_BARRIER)
        .bind(kind)
        .bind(scope)
        .bind(subject)
        .bind(nanos(window.recorded_at))
        .bind(nanos(window.until))
        .execute(conn)
        .await
        .map_err(db(op))?;
    Ok(())
}

async fn write_access(
    conn: &mut SqliteConnection,
    token: &IssuedToken,
    op: &'static str,
) -> Result<(), StorageError> {
    sqlx::query(UPSERT_ACCESS)
        .bind(digest(&token.access_token))
        .bind(token.client_id.as_str())
        .bind(token.subject.as_deref())
        .bind(token.family_id.as_deref())
        .bind(nanos(token.expires_at))
        .bind(encode(token, Some(ACCESS_KEY))?)
        .execute(conn)
        .await
        .map_err(db(op))?;
    Ok(())
}

async fn write_refresh(
    conn: &mut SqliteConnection,
    record: &RefreshTokenRecord,
    op: &'static str,
) -> Result<(), StorageError> {
    sqlx::query(UPSERT_REFRESH)
        .bind(digest(&record.refresh_token))
        .bind(record.client_id.as_str())
        .bind(record.subject.as_deref())
        .bind(&record.family_id)
        .bind(record.expires_at.map(nanos))
        .bind(encode(record, Some(REFRESH_KEY))?)
        .execute(conn)
        .await
        .map_err(db(op))?;
    Ok(())
}

/// An empty identifier names nothing a barrier could be recorded for: refused before anything
/// is removed, as oauth-as requires of every store.
fn reject_empty(what: &'static str, value: &str) -> Result<(), StorageError> {
    if value.is_empty() {
        return Err(StorageError::new(format!("a revocation needs a non-empty {what}")));
    }
    Ok(())
}

/// The user an approved device grant now acts for: what [`SqliteStorage::revoke_subject`]
/// reaches. A pending or denied grant acts for nobody yet.
fn approved_subject(state: &DeviceGrantState) -> Option<&str> {
    match state {
        DeviceGrantState::Approved { subject } => Some(subject),
        _ => None,
    }
}

/// An instant as Unix nanoseconds, saturating at the ends of `i64` (years 1678 to 2262).
fn nanos(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => i64::try_from(e.duration().as_nanos()).map_or(i64::MIN, |n| -n),
    }
}

/// SHA-256 of a credential, hex: the key a credential-keyed table stores instead of it.
fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// What a code compare-and-swap compares: the digest of the state's JSON, which is
/// deterministic for this enum (strings and options only).
fn state_digest(state: &AuthorizationCodeState) -> Result<String, StorageError> {
    Ok(digest(&encode(state, None)?))
}

/// A record as stored: its JSON, without the credential it is keyed by (`strip`).
fn encode<T: Serialize>(value: &T, strip: Option<&str>) -> Result<String, StorageError> {
    let mut json = serde_json::to_value(value).map_err(json_error)?;
    if let (Some(key), Some(object)) = (strip, json.as_object_mut()) {
        object.remove(key);
    }
    serde_json::to_string(&json).map_err(json_error)
}

/// A stored record back, with the credential it was looked up by put back as `key.0`.
fn decode<T: DeserializeOwned>(json: &str, key: Option<(&str, &str)>) -> Result<T, StorageError> {
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(json_error)?;
    if let (Some((name, credential)), Some(object)) = (key, value.as_object_mut()) {
        object.insert(name.to_owned(), serde_json::Value::String(credential.to_owned()));
    }
    serde_json::from_value(value).map_err(json_error)
}

fn json_error(e: serde_json::Error) -> StorageError {
    tracing::warn!(error = %e, "oauth storage record does not (de)serialize");
    StorageError::new(format!("record does not (de)serialize: {e}"))
}

fn db(op: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |e| {
        tracing::warn!(operation = op, error = %e, "oauth storage query failed");
        StorageError::new(format!("{op}: {e}"))
    }
}

/// A device grant write's error: the UNIQUE user code turns a code another device holds into a
/// refusal (the contract's `StorageError`, but an expected one, not a database fault).
fn device_write(op: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |e| {
        if e.as_database_error().is_some_and(|d| d.is_unique_violation()) {
            tracing::debug!(operation = op, "device grant user code already taken");
            StorageError::new("user code is already indexed for a different device_code")
        } else {
            db(op)(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use oauth_as::authorization::AuthorizationCodeRecord;
    use oauth_as::scope::ScopeSet;
    use oauth_as::storage_conformance::StorageConformance;
    use sqlx::sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
    };

    use super::*;

    /// A fresh database file (not `:memory:`, so every pool connection shares it), opened as the
    /// server opens its own (`SovEmu.Server/src/db.rs`) and migrated with the server's migrations.
    async fn fresh_pool() -> SqlitePool {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir: PathBuf = std::env::temp_dir().join("sovemu-identity-store");
        std::fs::create_dir_all(&dir).unwrap();
        let path =
            dir.join(format!("{}-{}.db", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
            .unwrap()
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().max_connections(8).connect_with(options).await.unwrap();
        sqlx::migrate!("../SovEmu.Server/migrations").run(&pool).await.unwrap();
        pool
    }

    async fn fresh_store() -> SqliteStorage {
        SqliteStorage::new(fresh_pool().await)
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn window(recorded_at: SystemTime) -> RevocationWindow {
        RevocationWindow { recorded_at, until: recorded_at + Duration::from_secs(3600) }
    }

    fn access(token: &str, subject: &str) -> IssuedToken {
        let mut t = IssuedToken::new(
            token,
            ClientId::new("app"),
            Some(subject.to_owned()),
            ScopeSet::empty(),
            at(0),
            at(600),
        );
        t.grant_established_at = at(0);
        t
    }

    fn refresh(token: &str, subject: &str, family: &str) -> RefreshTokenRecord {
        let mut r = RefreshTokenRecord::new(
            token,
            ClientId::new("app"),
            Some(subject.to_owned()),
            ScopeSet::empty(),
            family,
        );
        r.grant_established_at = at(0);
        r
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn passes_the_oauth_as_conformance_harness() {
        let violations = StorageConformance::new(fresh_store)
            .with_spawn(|task| {
                tokio::spawn(task);
            })
            .racers(16)
            .run()
            .await;
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// N tasks redeem one authorization code and N rotate one refresh token: exactly one of each
    /// wins, the rest see nothing (never an error).
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_redemptions_have_exactly_one_winner() {
        const RACERS: usize = 32;
        let store = fresh_store().await;
        store
            .put_authorization_code(AuthorizationCodeRecord::new(
                "the-code",
                ClientId::new("app"),
                "https://app/cb",
                ScopeSet::empty(),
                "7",
                "challenge",
                at(600),
            ))
            .await
            .unwrap();
        let predecessor = refresh("rt-0", "7", "fam");
        assert!(store.put_refresh_token(predecessor.clone()).await.unwrap().is_applied());

        let barrier = Arc::new(tokio::sync::Barrier::new(RACERS * 2));
        let mut takes = Vec::new();
        let mut rotations = Vec::new();
        for i in 0..RACERS {
            let (racer, gate) = (store.clone(), barrier.clone());
            takes.push(tokio::spawn(async move {
                gate.wait().await;
                racer.take_authorization_code("the-code").await
            }));
            let (racer, gate, expected) = (store.clone(), barrier.clone(), predecessor.clone());
            rotations.push(tokio::spawn(async move {
                gate.wait().await;
                let mut spent = expected.clone();
                spent.expires_at = Some(at(900));
                let next = refresh(&format!("rt-1-{i}"), "7", "fam");
                let mut token = access(&format!("at-{i}"), "7");
                token.family_id = Some("fam".into());
                racer.rotate_refresh_token(&expected, &spent, &token, Some(&next)).await
            }));
        }
        let mut code_winners = 0;
        for take in takes {
            if take.await.unwrap().expect("a lost race is Ok(None), never an error").is_some() {
                code_winners += 1;
            }
        }
        let mut rotation_winners = Vec::new();
        for (i, rotation) in rotations.into_iter().enumerate() {
            if rotation.await.unwrap().expect("a lost race is Ok(false), never an error") {
                rotation_winners.push(i);
            }
        }
        assert_eq!(code_winners, 1);
        assert_eq!(rotation_winners.len(), 1, "{rotation_winners:?}");
        let winner = rotation_winners[0];
        // Only the winner's successor and access token were written.
        for i in 0..RACERS {
            let next = store.get_refresh_token(&format!("rt-1-{i}")).await.unwrap();
            let token = store.get_token(&format!("at-{i}")).await.unwrap();
            assert_eq!(next.is_some(), i == winner);
            assert_eq!(token.is_some(), i == winner);
        }
        assert_eq!(
            store.get_refresh_token("rt-0").await.unwrap().unwrap().expires_at,
            Some(at(900))
        );
    }

    #[tokio::test]
    async fn revoke_subject_ends_one_users_sign_ins_and_refuses_in_flight_issuance() {
        let store = fresh_store().await;
        for (token, subject) in [("at-a", "1"), ("at-b", "2")] {
            assert!(store.put_token(access(token, subject)).await.unwrap().is_applied());
        }
        for (token, subject, family) in [("rt-a", "1", "fa"), ("rt-b", "2", "fb")] {
            let record = refresh(token, subject, family);
            assert!(store.put_refresh_token(record).await.unwrap().is_applied());
        }
        store
            .put_authorization_code(AuthorizationCodeRecord::new(
                "code-a",
                ClientId::new("app"),
                "https://app/cb",
                ScopeSet::empty(),
                "1",
                "challenge",
                at(600),
            ))
            .await
            .unwrap();

        assert_eq!(store.revoke_subject("1", window(at(10))).await.unwrap(), 3);
        assert!(store.get_token("at-a").await.unwrap().is_none());
        assert!(store.get_refresh_token("rt-a").await.unwrap().is_none());
        assert!(store.take_authorization_code("code-a").await.unwrap().is_none());
        // Another user's sign-ins stand.
        assert!(store.get_token("at-b").await.unwrap().is_some());
        assert!(store.get_refresh_token("rt-b").await.unwrap().is_some());

        // An issuance in flight for a grant made before the revocation is refused...
        let late = access("at-late", "1");
        assert!(store.put_token(late).await.unwrap().is_refused());
        let late = refresh("rt-late", "1", "fa");
        assert!(store.put_refresh_token(late).await.unwrap().is_refused());
        // ...while signing in again afterwards is served, and other users never were affected.
        let mut again = access("at-again", "1");
        again.grant_established_at = at(11);
        assert!(store.put_token(again).await.unwrap().is_applied());
        assert!(store.put_token(access("at-c", "2")).await.unwrap().is_applied());

        assert!(store.revoke_subject("", window(at(10))).await.is_err());
    }

    #[tokio::test]
    async fn sign_ins_are_live_refresh_families_with_their_last_use() {
        let store = fresh_store().await;
        for (token, subject, family) in [("rt-a", "1", "fa"), ("rt-b", "2", "fb")] {
            assert!(
                store
                    .put_refresh_token(refresh(token, subject, family))
                    .await
                    .unwrap()
                    .is_applied()
            );
        }
        // A rotation left a spent token in family a; its access tokens were issued later.
        let mut spent = refresh("rt-a-old", "1", "fa");
        spent.state = oauth_as::token::RefreshTokenState::Spent;
        assert!(store.put_refresh_token(spent).await.unwrap().is_applied());
        for (token, issued) in [("at-a1", 100), ("at-a2", 300)] {
            let mut t = access(token, "1");
            t.family_id = Some("fa".into());
            t.issued_at = at(issued);
            assert!(store.put_token(t).await.unwrap().is_applied());
        }

        let all = store.sign_ins(None).await.unwrap();
        assert_eq!(all.len(), 2, "one per live family: {all:?}");
        assert_eq!(all[0].family_id, "fa", "the most recently used first");
        assert_eq!((all[0].signed_in_at, all[0].last_used_at), (at(0), at(300)));
        assert_eq!(all[1].last_used_at, at(0), "no access token since: its grant");
        assert_eq!(store.sign_ins(Some("2")).await.unwrap().len(), 1);

        assert!(store.end_sign_in("fa", window(at(400))).await.unwrap());
        assert!(!store.end_sign_in("fa", window(at(400))).await.unwrap(), "already ended");
        assert!(store.get_refresh_token("rt-a").await.unwrap().is_none());
        assert_eq!(store.sign_ins(None).await.unwrap().len(), 1);
        // A rotation in flight for the ended family is refused.
        assert!(
            store.put_refresh_token(refresh("rt-a-new", "1", "fa")).await.unwrap().is_refused()
        );
    }

    #[tokio::test]
    async fn credentials_are_stored_only_as_digests() {
        let store = fresh_store().await;
        assert!(store.put_token(access("secret-access", "1")).await.unwrap().is_applied());
        let record = refresh("secret-refresh", "1", "f");
        assert!(store.put_refresh_token(record).await.unwrap().is_applied());
        let dump: Vec<String> = sqlx::query_scalar(
            "SELECT token_hash || record FROM oauth_access_tokens
             UNION ALL SELECT token_hash || record FROM oauth_refresh_tokens",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        assert_eq!(dump.len(), 2);
        assert!(dump.iter().all(|row| !row.contains("secret-")), "{dump:?}");
        let token = store.get_token("secret-access").await.unwrap().unwrap();
        assert_eq!(token.access_token, "secret-access");
    }
}

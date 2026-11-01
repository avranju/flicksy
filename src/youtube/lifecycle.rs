use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests;

const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const REVOCATION_RETENTION: i64 = 7 * 24 * 60 * 60;
const AUTHORIZATION_RETENTION: i64 = 30 * 24 * 60 * 60;
const REVOCATION_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS youtube_revocations (username TEXT PRIMARY KEY, token BLOB NOT NULL, requested_at INTEGER NOT NULL)";

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// No user foreign key: deleting an account must not destroy a pending revocation.
pub(crate) fn queue_account_revocation(db: &Connection, user: &str) -> io::Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='youtube_connections')",
        [], |r| r.get(0),
    ).map_err(ioerr)?;
    if exists {
        db.execute_batch(REVOCATION_SCHEMA).map_err(ioerr)?;
        db.execute(
            "INSERT OR IGNORE INTO youtube_revocations SELECT username,token,?2 FROM youtube_connections WHERE username=?1",
            params![user, now()],
        ).map_err(ioerr)?;
    }
    Ok(())
}

impl YouTube {
    pub(super) fn initialize_lifecycle_schema(&self) -> io::Result<()> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(ioerr)?;
        tx.execute_batch(REVOCATION_SCHEMA).map_err(ioerr)?;
        let columns = tx
            .prepare("PRAGMA table_info(youtube_connections)")
            .map_err(ioerr)?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(ioerr)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ioerr)?;
        if !columns.iter().any(|c| c == "last_verified_at") {
            tx.execute_batch("ALTER TABLE youtube_connections ADD COLUMN last_verified_at INTEGER NOT NULL DEFAULT 0").map_err(ioerr)?;
            // Legacy records have no verification timestamp. Check them at startup;
            // allow a bounded grace period if Google's service is temporarily down.
            tx.execute(
                "UPDATE youtube_connections SET last_verified_at=?1",
                [now()],
            )
            .map_err(ioerr)?;
        }
        tx.commit().map_err(ioerr)
    }

    pub(super) fn account_lock(&self, user: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.inner
            .account_locks
            .lock()
            .unwrap()
            .entry(user.to_owned())
            .or_default()
            .clone()
    }

    pub(super) fn revocation_pending(&self, user: &str) -> io::Result<bool> {
        self.db()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM youtube_revocations WHERE username=?1)",
                [user],
                |r| r.get(0),
            )
            .map_err(ioerr)
    }

    // Short, synchronous critical section shared with upload creation and OAuth
    // commits. Deleted uploads cannot be recreated by an in-flight worker (its
    // writes are UPDATEs); aborting the worker also drops in-memory credentials.
    pub(super) fn erase_youtube_data(
        &self,
        user: &str,
        expected_token: Option<&[u8]>,
        revoke: bool,
    ) -> io::Result<bool> {
        let mut active = self.inner.active.lock().unwrap();
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(ioerr)?;
        if let Some(expected) = expected_token {
            let token: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT token FROM youtube_connections WHERE username=?1",
                    [user],
                    |r| r.get(0),
                )
                .optional()
                .map_err(ioerr)?;
            if token.as_deref() != Some(expected) {
                return Ok(false); // A stale refresh must not erase a newer connection.
            }
        }
        if revoke {
            queue_account_revocation(&tx, user)?;
        }
        tx.execute("DELETE FROM youtube_uploads WHERE username=?1", [user])
            .map_err(ioerr)?;
        tx.execute("DELETE FROM youtube_connections WHERE username=?1", [user])
            .map_err(ioerr)?;
        tx.commit().map_err(ioerr)?;
        let mut generations = self.inner.connection_generation.lock().unwrap();
        let generation = generations.entry(user.to_owned()).or_default();
        *generation = generation.wrapping_add(1);
        self.inner
            .flows
            .lock()
            .unwrap()
            .retain(|_, flow| flow.user != user);
        if let Some(upload) = active.remove(user) {
            upload.task.abort();
        }
        Ok(true)
    }

    pub(super) async fn disconnect_user(&self, user: &str) -> io::Result<bool> {
        // Serialize token exchanges and revocation: an OAuth callback cannot
        // reconnect an account while its previous consent is being revoked.
        let _guard = self.account_lock(user).lock_owned().await;
        self.erase_youtube_data(user, None, true)?;
        self.revoke_pending_locked(user).await
    }

    pub(super) async fn revoke_pending_locked(&self, user: &str) -> io::Result<bool> {
        let row: Option<(Vec<u8>, i64)> = self
            .db()?
            .query_row(
                "SELECT token,requested_at FROM youtube_revocations WHERE username=?1",
                [user],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(ioerr)?;
        let Some((token, requested_at)) = row else {
            return Ok(true);
        };
        if now().saturating_sub(requested_at) >= REVOCATION_RETENTION {
            self.db()?
                .execute("DELETE FROM youtube_revocations WHERE username=?1", [user])
                .map_err(ioerr)?;
            tracing::warn!(
                "Expired a pending YouTube revocation without Google confirmation; revoke access in Google account permissions"
            );
            return Ok(false);
        }
        let Ok(refresh) = self.open(user, &token) else {
            tracing::warn!(
                "Cannot decrypt pending YouTube revocation; restore the configured encryption key"
            );
            return Ok(false);
        };
        let result = self
            .inner
            .client
            .post(self.endpoint("/revoke", "https://oauth2.googleapis.com/revoke"))
            .timeout(Duration::from_secs(30))
            .form(&[("token", refresh.as_str())])
            .send()
            .await;
        let confirmed = match result {
            Ok(response) if response.status() == StatusCode::OK => true,
            Ok(response) if response.status() == StatusCode::BAD_REQUEST => {
                // An already invalid token is safe to forget. Other 400s (bad
                // requests/client configuration) must not be reported as success.
                response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .is_some_and(|v| v["error"].as_str() == Some("invalid_token"))
            }
            _ => false,
        };
        if confirmed {
            self.db()?
                .execute(
                    "DELETE FROM youtube_revocations WHERE username=?1 AND token=?2",
                    params![user, token],
                )
                .map_err(ioerr)?;
        } else {
            tracing::warn!(
                "YouTube revocation not confirmed; encrypted credential retained for retry"
            );
        }
        Ok(confirmed)
    }

    pub fn start_maintenance(&self) -> tokio::task::AbortHandle {
        let yt = self.clone();
        tokio::spawn(async move {
            let mut checks = tokio::time::interval(CHECK_INTERVAL);
            checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                checks.tick().await;
                if yt.maintain_once().await.is_err() {
                    // Do not log HTTP response bodies, tokens, or session URLs.
                    tracing::warn!("YouTube authorization maintenance failed; will retry");
                }
            }
        })
        .abort_handle()
    }

    async fn maintain_once(&self) -> io::Result<()> {
        // Also clean up legacy upload history belonging to an already-disconnected
        // account. Never touch the projects table or media files.
        {
            let _active = self.inner.active.lock().unwrap();
            self.db()?.execute(
                "DELETE FROM youtube_uploads WHERE NOT EXISTS (SELECT 1 FROM youtube_connections c WHERE c.username=youtube_uploads.username)", [],
            ).map_err(ioerr)?;
        }
        let pending =
            self.usernames("SELECT username FROM youtube_revocations ORDER BY username")?;
        for user in pending {
            let _guard = self.account_lock(&user).lock_owned().await;
            if self.revoke_pending_locked(&user).await.is_err() {
                tracing::warn!("Could not process pending YouTube revocation; will retry");
            }
        }
        for user in self.usernames("SELECT username FROM youtube_connections ORDER BY username")? {
            let _guard = self.account_lock(&user).lock_owned().await;
            let verified: Option<i64> = self
                .db()?
                .query_row(
                    "SELECT last_verified_at FROM youtube_connections WHERE username=?1",
                    [&user],
                    |r| r.get(0),
                )
                .optional()
                .map_err(ioerr)?;
            let Some(verified) = verified else {
                continue;
            };
            if now().saturating_sub(verified) >= AUTHORIZATION_RETENTION {
                // Even prolonged network/configuration failures must not cause
                // indefinite retention of unverified authorized data.
                self.erase_youtube_data(&user, None, true)?;
                let confirmed = self.revoke_pending_locked(&user).await?;
                // The unverified credential is already at its retention limit;
                // do not extend it by another seven days of retries.
                if !confirmed {
                    self.db()?
                        .execute("DELETE FROM youtube_revocations WHERE username=?1", [&user])
                        .map_err(ioerr)?;
                    tracing::warn!(
                        "Deleted expired unverified YouTube credentials without Google revocation confirmation"
                    );
                }
            } else if self.inner.config.is_some() && self.access_token(&user).await.is_err() {
                tracing::warn!(
                    "YouTube authorization check failed; revoked consent is removed, temporary failures are retried"
                );
            }
        }
        Ok(())
    }

    pub(super) fn queue_unused_token(&self, user: &str, token: &[u8]) -> io::Result<()> {
        self.db()?.execute(
            "INSERT OR IGNORE INTO youtube_revocations SELECT ?1,?2,?3 WHERE NOT EXISTS (SELECT 1 FROM youtube_connections WHERE username=?1)",
            params![user, token, now()],
        ).map_err(ioerr)?;
        Ok(())
    }

    fn usernames(&self, sql: &str) -> io::Result<Vec<String>> {
        self.db()?
            .prepare(sql)
            .map_err(ioerr)?
            .query_map([], |r| r.get(0))
            .map_err(ioerr)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ioerr)
    }
}

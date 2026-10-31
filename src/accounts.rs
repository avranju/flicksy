use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{generate_salt, phc::PasswordHash},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Accounts {
    path: PathBuf,
    pub secure_cookie: bool,
}

fn db_error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

impl Accounts {
    pub fn open(path: &Path, secure_cookie: bool) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let this = Self {
            path: path.to_owned(),
            secure_cookie,
        };
        this.connect()?.execute_batch("CREATE TABLE IF NOT EXISTS users (username TEXT PRIMARY KEY, password_hash TEXT NOT NULL); CREATE TABLE IF NOT EXISTS sessions (token_hash TEXT PRIMARY KEY, username TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE, credential_hash TEXT NOT NULL);").map_err(db_error)?;
        Ok(this)
    }
    fn connect(&self) -> io::Result<Connection> {
        let c = Connection::open(&self.path).map_err(db_error)?;
        c.busy_timeout(Duration::from_secs(5)).map_err(db_error)?;
        c.pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        Ok(c)
    }
    pub fn add(&self, username: &str, password: &str) -> io::Result<()> {
        if !valid(username, password) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Username must be nonblank and at most 128 bytes; password must be 1–1024 bytes",
            ));
        }
        let hash = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(db_error)?
            .to_string();
        self.connect()?
            .execute(
                "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
                params![username, hash],
            )
            .map_err(db_error)?;
        Ok(())
    }
    pub fn reset_password(&self, username: &str, password: &str) -> io::Result<()> {
        if !valid(username, password) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Username must be nonblank and at most 128 bytes; password must be 1–1024 bytes",
            ));
        }
        let hash = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(db_error)?
            .to_string();
        let mut connection = self.connect()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let updated = transaction
            .execute(
                "UPDATE users SET password_hash = ?1 WHERE username = ?2",
                params![hash, username],
            )
            .map_err(db_error)?;
        if updated == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "User does not exist",
            ));
        }
        transaction
            .execute("DELETE FROM sessions WHERE username = ?1", [username])
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }
    pub fn list(&self) -> io::Result<Vec<String>> {
        let c = self.connect()?;
        let mut statement = c
            .prepare("SELECT username FROM users ORDER BY username")
            .map_err(db_error)?;
        let names = statement
            .query_map([], |row| row.get(0))
            .map_err(db_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?;
        Ok(names)
    }
    pub fn remove(&self, username: &str) -> io::Result<()> {
        if self
            .connect()?
            .execute("DELETE FROM users WHERE username = ?1", [username])
            .map_err(db_error)?
            == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "User does not exist",
            ));
        }
        Ok(())
    }
    pub fn login(&self, username: &str, password: &str) -> io::Result<Option<String>> {
        if !valid(username, password) {
            return Ok(None);
        }
        let c = self.connect()?;
        let hash: Option<String> = c
            .query_row(
                "SELECT password_hash FROM users WHERE username = ?1",
                [username],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| {
            DUMMY
                .get_or_init(|| {
                    Argon2::default()
                        .hash_password(b"flicksy-invalid-user-dummy")
                        .expect("dummy hash")
                        .to_string()
                })
                .clone()
        });
        let valid = PasswordHash::new(&hash).ok().is_some_and(|parsed| {
            parsed.algorithm.as_str() == "argon2id"
                && Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
        });
        if !valid || !known {
            return Ok(None);
        }
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(&generate_salt());
        bytes[16..].copy_from_slice(&generate_salt());
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let changed = c.execute("INSERT INTO sessions (token_hash, username, credential_hash) SELECT ?1, username, ?2 FROM users WHERE username = ?3 AND password_hash = ?4", params![fingerprint(&token), fingerprint(&hash), username, hash]).map_err(db_error)?;
        Ok((changed == 1).then_some(token))
    }
    pub fn session(&self, token: &str) -> io::Result<Option<String>> {
        let c = self.connect()?;
        let pair: Option<(String, String, String)> = c.query_row("SELECT s.credential_hash, u.password_hash, u.username FROM sessions s JOIN users u ON u.username = s.username WHERE s.token_hash = ?1", [fingerprint(token)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(db_error)?;
        Ok(pair.and_then(|(stored, hash, username)| {
            (stored == fingerprint(&hash)).then_some(username)
        }))
    }
    pub fn logout(&self, token: &str) -> io::Result<()> {
        self.connect()?
            .execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                [fingerprint(token)],
            )
            .map_err(db_error)?;
        Ok(())
    }
}
fn valid(username: &str, password: &str) -> bool {
    !username.trim().is_empty()
        && username.len() <= 128
        && !username.chars().any(char::is_control)
        && !password.is_empty()
        && password.len() <= 1024
}
fn fingerprint(s: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(s.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_reset_replaces_credentials_and_revokes_only_that_users_sessions() {
        let path = std::env::temp_dir().join(format!(
            "flicksy-password-reset-{}-{}.sqlite",
            std::process::id(),
            URL_SAFE_NO_PAD.encode(generate_salt())
        ));
        let accounts = Accounts::open(&path, false).unwrap();
        accounts.add("alice", "old password").unwrap();
        accounts.add("bob", "bob password").unwrap();
        let alice_session = accounts.login("alice", "old password").unwrap().unwrap();
        let bob_session = accounts.login("bob", "bob password").unwrap().unwrap();
        accounts.reset_password("alice", "new password").unwrap();
        assert!(accounts.session(&alice_session).unwrap().is_none());
        assert_eq!(
            accounts.session(&bob_session).unwrap().as_deref(),
            Some("bob")
        );
        assert!(accounts.login("alice", "old password").unwrap().is_none());
        assert!(accounts.login("alice", "new password").unwrap().is_some());
        assert_eq!(
            accounts
                .reset_password("missing", "password")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            accounts.reset_password("alice", "").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(accounts.login("alice", "new password").unwrap().is_some());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn accounts_and_sessions_survive_reopen_and_are_revoked() {
        let path = std::env::temp_dir().join(format!(
            "flicksy-auth-test-{}-{}.sqlite",
            std::process::id(),
            URL_SAFE_NO_PAD.encode(generate_salt())
        ));
        let accounts = Accounts::open(&path, false).unwrap();
        accounts.add("alice", "correct horse").unwrap();
        assert!(accounts.add("alice", "other").is_err());
        assert_eq!(accounts.list().unwrap(), ["alice"]);
        assert!(accounts.login("alice", "wrong").unwrap().is_none());
        assert!(accounts.login("nobody", "wrong").unwrap().is_none());
        let token = accounts.login("alice", "correct horse").unwrap().unwrap();
        let db = accounts.connect().unwrap();
        let stored: String = db
            .query_row("SELECT token_hash FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, token);
        let reopened = Accounts::open(&path, false).unwrap();
        assert_eq!(reopened.session(&token).unwrap().as_deref(), Some("alice"));
        reopened.logout(&token).unwrap();
        assert!(accounts.session(&token).unwrap().is_none());
        let token = accounts.login("alice", "correct horse").unwrap().unwrap();
        reopened.remove("alice").unwrap();
        reopened.add("alice", "correct horse").unwrap();
        assert!(accounts.session(&token).unwrap().is_none());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
